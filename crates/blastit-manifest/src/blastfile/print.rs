//! The canonical layout (§14.2): one walk of the tree, no width computation.
//!
//! [`fmt`] is the formatter `blastit fmt` runs. It parses the source with the
//! one parser, walks the resulting tree once, and writes every token back with
//! canonical spacing: one directive per line, four spaces per block level, one
//! blank line between top-level sections, `, ` between list items, and a
//! newline where the author wrote `;` (§6.6). Nothing here computes a column or
//! wraps a line, and there is no second serialiser: the tree `lower` reads is
//! the tree this module writes (§4.2).
//!
//! Comments are elements of the tree, not text, so they survive (P2b): one that
//! stands on its own line is indented to the line it precedes, and one that
//! trails a directive stays on that line with one space before its `#`
//! (§14.2). A comment written inside a list, which the canonical layout never
//! wraps, has no line of its own; it is moved to the line above the statement
//! it belongs to, so it stays attached to the same statement and the output
//! stays a fixed point.
//!
//! A file that is already canonical comes back byte for byte (P2), and
//! `lower(parse(fmt(t)))` is `lower(parse(t))` (P3): the layout is the only
//! thing this module changes.

use super::MAX_SOURCE_BYTES;
use super::parser::parse;
use super::syntax::{Cst, Element, NodeId, NodeKind, TokenId, TriviaId};
use super::tokens::{TokenKind, TriviaKind};
use crate::error::{ManifestError, ManifestErrors};

/// Four spaces per block level (§14.2).
const INDENT: &str = "    ";

/// Formats one `Blastfile` into the canonical layout (§14.2).
///
/// The source is parsed with the one parser, so every refusal is the ordinary
/// `line:col` one, and the output is written from the tree. The result is a
/// fixed point (`fmt(fmt(t)) == fmt(t)`) and parses to the same manifest
/// (`lower(fmt(t))` equals `lower(t)`), which is what makes
/// `blastit fmt --check` a CI gate rather than a diff generator.
///
/// # Errors
///
/// Everything the parser refuses, and [`ManifestError::TooLarge`] when the
/// canonical text would exceed `MAX_SOURCE_BYTES`: a file that formatting would
/// grow past the reader's own cap is refused rather than written (R25).
pub fn fmt(text: &str) -> Result<String, ManifestErrors> {
    let cst = parse(text)?;
    let formatted = write(&cst);
    if formatted.len() > MAX_SOURCE_BYTES {
        return Err(ManifestError::TooLarge {
            bytes: formatted.len() as u64,
        }
        .into());
    }
    Ok(formatted)
}

/// The canonical text of one parsed document.
fn write(cst: &Cst) -> String {
    let mut out = String::with_capacity(cst.text().len().saturating_add(16));
    write_file(cst, &mut out);
    out
}

// ---- the file and its statements ----------------------------------------

/// Writes every top-level section, one blank line apart (§14.2).
///
/// A comment that stands between two sections belongs to the section after it,
/// so the blank line goes above the comment and the section follows it; a
/// comment after the last section keeps its own line, with no blank line of its
/// own, because nothing follows it to separate.
fn write_file(cst: &Cst, out: &mut String) {
    let lines = split_lines(cst, cst.child_elements(cst.root()));
    let last_section = lines.iter().rposition(|line| line.single_node().is_some());
    let mut blank_due = false;
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let section = line.single_node().is_some();
        let before_a_section = last_section.is_some_and(|last| index < last);
        if blank_due && (section || before_a_section) {
            out.push('\n');
        }
        write_line(cst, line, 0, out);
        blank_due = section;
    }
}

/// Writes one statement at `indent`, without its trailing newline.
///
/// The statement's own leading comments are written first, on their own lines,
/// and then the statement itself: its header, and its block when it has one.
fn write_statement(cst: &Cst, node: NodeId, indent: usize, out: &mut String) {
    write_leading_comments(cst, node, indent, out);
    let (text, hoisted) = statement_text(cst, node, indent);
    for comment in hoisted {
        push_indent(out, indent);
        out.push_str(comment);
        out.push('\n');
    }
    out.push_str(&text);
}

/// The canonical text of one statement, starting at its own indentation.
///
/// The second half of the pair is every comment that had no line of its own
/// inside the statement's header (a list the canonical layout never wraps); the
/// caller writes those above the statement.
fn statement_text(cst: &Cst, node: NodeId, indent: usize) -> (String, Vec<&str>) {
    let mut text = String::new();
    let mut hoisted = Vec::new();
    match split_block(cst, node) {
        Some((header, block)) => {
            push_indent(&mut text, indent);
            let (header_text, comments) = inline_text(cst, &header);
            hoisted.extend(comments);
            text.push_str(&header_text);
            text.push_str(" {");
            write_block_body(cst, &block, indent, &mut text);
        }
        None => {
            push_indent(&mut text, indent);
            let elements: Vec<(NodeId, Element)> = cst.child_elements(node).collect();
            let (line, comments) = inline_text(cst, &elements);
            hoisted.extend(comments);
            text.push_str(&line);
        }
    }
    (text, hoisted)
}

/// Writes the comments a node carries before its first token, one per line.
///
/// A comment in that position always stands on its own line: a comment ends the
/// line it is on, so the token after it is on a later one.
fn write_leading_comments(cst: &Cst, node: NodeId, indent: usize, out: &mut String) {
    for (_, element) in cst.child_elements(node) {
        match element {
            Element::Trivia(id) if cst.trivia_kind(id) == Some(TriviaKind::Comment) => {
                push_indent(out, indent);
                out.push_str(cst.trivia_text(id));
                out.push('\n');
            }
            Element::Trivia(_) => {}
            _ => return,
        }
    }
}

/// Writes one line of a body: its comment, its elements, and its trailing
/// comment, each on the line §14.2 gives it.
///
/// A comment that cannot stay on the statement's line — one that stood on a
/// line of its own, or one written inside a list the layout unwraps — goes
/// above the statement it belongs to, which keeps it attached to that statement
/// and keeps the output a fixed point.
fn write_line(cst: &Cst, line: &Line, indent: usize, out: &mut String) {
    let trailing = line.comment_trails(cst);
    if let Some(comment) = line.comment
        && !trailing
    {
        push_indent(out, indent);
        out.push_str(cst.trivia_text(comment));
        out.push('\n');
    }
    if line.elements.is_empty() {
        return;
    }
    if let Some(node) = line.single_node() {
        write_statement(cst, node, indent, out);
    } else {
        let (text, hoisted) = inline_text(cst, &line.elements);
        for comment in hoisted {
            push_indent(out, indent);
            out.push_str(comment);
            out.push('\n');
        }
        push_indent(out, indent);
        out.push_str(&text);
    }
    if trailing && let Some(comment) = line.comment {
        out.push(' ');
        out.push_str(cst.trivia_text(comment));
    }
    out.push('\n');
}

// ---- blocks --------------------------------------------------------------

/// A node's block: the `{` that opened it and the elements between it and its
/// `}`.
struct Block {
    /// The `{` token, for the comment that shares its line.
    open: TokenId,
    /// The elements between `{` and `}`, in source order.
    body: Vec<(NodeId, Element)>,
}

/// Splits `node` into its header elements and its block, when it has one.
///
/// A block is either a child node — a store, a grant or an endpoint block,
/// whose braces are its own children — or the `{` … `}` token run of a `trait`,
/// a `view`, a `static` or an `entrypoint python` block. The header is
/// everything before it, which is what the canonical layout keeps on one line.
fn split_block(cst: &Cst, node: NodeId) -> Option<(Vec<(NodeId, Element)>, Block)> {
    let elements: Vec<(NodeId, Element)> = cst.child_elements(node).collect();
    let mut found = None;
    for (index, (child, element)) in elements.iter().enumerate() {
        match element {
            Element::Node { kind, .. }
                if matches!(
                    *kind,
                    NodeKind::StoreBlock | NodeKind::GrantBlock | NodeKind::EndpointBlock
                ) =>
            {
                found = Some((index, Some(*child)));
                break;
            }
            Element::Token(id) if cst.token_kind(*id) == Some(TokenKind::LBrace) => {
                found = Some((index, None));
                break;
            }
            _ => {}
        }
    }
    let (index, child) = found?;
    let inner: Vec<(NodeId, Element)> = match child {
        Some(child) => cst.child_elements(child).collect(),
        None => elements.iter().skip(index).cloned().collect(),
    };
    let (open, body) = braces(cst, &inner)?;
    let header = elements.iter().take(index).cloned().collect();
    Some((header, Block { open, body }))
}

/// The `{` token and the elements between it and the matching `}`.
fn braces(cst: &Cst, elements: &[(NodeId, Element)]) -> Option<(TokenId, Vec<(NodeId, Element)>)> {
    let is_brace = |element: &(NodeId, Element), kind: TokenKind| match element {
        (_, Element::Token(id)) => cst.token_kind(*id) == Some(kind),
        _ => false,
    };
    let open_at = elements
        .iter()
        .position(|element| is_brace(element, TokenKind::LBrace))?;
    let close_at = elements
        .iter()
        .rposition(|element| is_brace(element, TokenKind::RBrace))?;
    let open = match elements.get(open_at) {
        Some((_, Element::Token(id))) => *id,
        _ => return None,
    };
    let body = elements
        .iter()
        .skip(open_at.saturating_add(1))
        .take(close_at.saturating_sub(open_at.saturating_add(1)))
        .cloned()
        .collect();
    Some((open, body))
}

/// Writes `{`'s line ending and a block's body at `indent + 1`, then `}` at
/// `indent`.
///
/// The body is one line per statement: blank lines inside a block are dropped,
/// and an empty block is `{` newline `}` (§14.2).
fn write_block_body(cst: &Cst, block: &Block, indent: usize, out: &mut String) {
    let lines = split_lines(cst, block.body.iter().cloned());
    let mut skip = 0;
    if let Some(first) = lines.first()
        && first.elements.is_empty()
        && let Some(comment) = first.comment
        && shares_line(cst, block.open, comment)
    {
        // `view v "/x" { # note`: the comment trails the header line, so it
        // stays on it rather than moving inside the block.
        out.push(' ');
        out.push_str(cst.trivia_text(comment));
        skip = 1;
    }
    out.push('\n');
    for line in lines.iter().skip(skip) {
        if line.is_empty() {
            continue;
        }
        write_line(cst, line, indent.saturating_add(1), out);
    }
    push_indent(out, indent);
    out.push('}');
}

/// True when `comment` starts on the same source line as the token `after`.
fn shares_line(cst: &Cst, after: TokenId, comment: TriviaId) -> bool {
    cst.token_span(after)
        .is_some_and(|span| same_line(cst, span.end, comment))
}

/// True when `comment` starts on the same source line as the byte `end`.
fn same_line(cst: &Cst, end: u32, comment: TriviaId) -> bool {
    match cst.trivia_span(comment) {
        Some(span) if span.start >= end => cst
            .text()
            .as_bytes()
            .get(end as usize..span.start as usize)
            .is_none_or(|between| !between.contains(&b'\n')),
        _ => false,
    }
}

// ---- lines ---------------------------------------------------------------

/// One line of a body: the tokens and nodes of one statement, and the comment
/// that shared the line or stood on it alone.
#[derive(Default)]
struct Line {
    /// The tokens and nodes of the line, in source order, with their arena
    /// index (a node's id, for a node element).
    elements: Vec<(NodeId, Element)>,
    /// The comment on this line, when there is one.
    comment: Option<TriviaId>,
}

impl Line {
    /// True when the line holds neither a token nor a comment.
    fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.comment.is_none()
    }

    /// The line's only element, when it is a node: a statement that may span
    /// lines.
    fn single_node(&self) -> Option<NodeId> {
        match self.elements.as_slice() {
            [(id, Element::Node { .. })] => Some(*id),
            _ => None,
        }
    }

    /// True when the comment starts on the same source line as the line's last
    /// token, which is what keeps a trailing comment on its line.
    fn comment_trails(&self, cst: &Cst) -> bool {
        let Some(comment) = self.comment else {
            return false;
        };
        let end = match self.elements.last() {
            Some((_, Element::Token(token))) => cst.token_span(*token),
            Some((id, Element::Node { .. })) => {
                cst.last_token(*id).and_then(|token| cst.token_span(token))
            }
            _ => None,
        };
        end.is_some_and(|span| same_line(cst, span.end, comment))
    }
}

/// Splits a run of elements into lines at the separators that end a statement
/// (§6.6).
///
/// A `;` always ends one. A `Newline` does not when it follows a `,` or a `{`,
/// or when a `(` is still open: those three continue the statement, which is
/// what keeps `select a,` newline `b` on one line (§14.2's "lists … never
/// wrapped"). A `(` is always inside a `table_grant` node, so the third clause
/// is belt and braces for the grammar as it stands.
///
/// The separators themselves are dropped — the canonical layout writes a
/// newline and never a `;` — and each line keeps the comment that was written
/// on it.
fn split_lines(cst: &Cst, elements: impl Iterator<Item = (NodeId, Element)>) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut current = Line::default();
    // The last token seen, and how many `(` are open: §6.6's two conditions for
    // a `Newline` that continues rather than ends.
    let mut previous = None;
    let mut parentheses: u32 = 0;
    for (child, element) in elements {
        match element {
            Element::Trivia(id) => {
                if cst.trivia_kind(id) == Some(TriviaKind::Comment) {
                    current.comment = Some(id);
                }
            }
            Element::Token(id) => match cst.token_kind(id) {
                Some(TokenKind::Eof) => {}
                Some(TokenKind::Semi) => {
                    lines.push(std::mem::take(&mut current));
                    previous = None;
                }
                Some(TokenKind::Newline) => {
                    let continues = parentheses > 0
                        || matches!(previous, Some(TokenKind::Comma | TokenKind::LBrace));
                    if !continues {
                        lines.push(std::mem::take(&mut current));
                        previous = None;
                    }
                }
                Some(kind) => {
                    match kind {
                        TokenKind::LParen => parentheses = parentheses.saturating_add(1),
                        TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
                        _ => {}
                    }
                    current.elements.push((child, element));
                    previous = Some(kind);
                }
                None => {}
            },
            Element::Node { .. } => {
                // A node's last token decides the continuation, so
                // `select products(id)` followed by a newline ends the line.
                previous = cst
                    .last_token(child)
                    .and_then(|token| cst.token_kind(token))
                    .or(previous);
                current.elements.push((child, element));
            }
        }
    }
    lines.push(current);
    lines
}

// ---- inline text ---------------------------------------------------------

/// The canonical text of a header or a line, plus the comments it had to hoist.
fn inline_text<'a>(cst: &'a Cst, elements: &[(NodeId, Element)]) -> (String, Vec<&'a str>) {
    let mut text = String::new();
    let mut hoisted = Vec::new();
    let mut previous = None;
    write_inline(cst, elements, &mut previous, &mut text, &mut hoisted);
    (text, hoisted)
}

/// Writes elements on one line, one space between tokens except where §14.2's
/// list rules say otherwise.
///
/// `previous` is the kind of the last token written, so a nested node — a
/// pattern, a table grant — continues the spacing of the line it sits in
/// instead of restarting it. Separators are dropped: a list is never wrapped.
fn write_inline<'a>(
    cst: &'a Cst,
    elements: &[(NodeId, Element)],
    previous: &mut Option<TokenKind>,
    out: &mut String,
    hoisted: &mut Vec<&'a str>,
) {
    for (child, element) in elements {
        match element {
            Element::Trivia(id) => {
                if cst.trivia_kind(*id) == Some(TriviaKind::Comment) {
                    hoisted.push(cst.trivia_text(*id));
                }
            }
            Element::Token(id) => {
                let Some(kind) = cst.token_kind(*id) else {
                    continue;
                };
                if is_separator(kind) || kind == TokenKind::Eof {
                    continue;
                }
                if needs_space(*previous, kind) {
                    out.push(' ');
                }
                out.push_str(cst.token_text(*id));
                *previous = Some(kind);
            }
            Element::Node { .. } => {
                let nested: Vec<(NodeId, Element)> = cst.child_elements(*child).collect();
                write_inline(cst, &nested, previous, out, hoisted);
            }
        }
    }
}

/// Whether `current` needs a space before it, given the token before it.
///
/// §14.2's list rules: `, ` between items, no space before a `,` or a `)`, none
/// after a `(` or a `.`, and no space before a `(` — so a table grant reads
/// `products(id, name)` and a qualified function reads `pg_catalog.unaccent`.
fn needs_space(previous: Option<TokenKind>, current: TokenKind) -> bool {
    !matches!(
        (previous, current),
        (None, _)
            | (
                _,
                TokenKind::Comma | TokenKind::RParen | TokenKind::LParen | TokenKind::Dot
            )
            | (Some(TokenKind::LParen | TokenKind::Dot), _)
    )
}

/// True for the two tokens that end a statement (§6.6).
fn is_separator(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Newline | TokenKind::Semi)
}

/// Writes `indent` block levels of four spaces.
fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str(INDENT);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::indexing_slicing
)]
mod tests {
    use super::super::syntax::{Cst, Element, NodeId, NodeKind};
    use super::super::tokens::TriviaKind;
    use super::fmt;

    /// Formats a document that must parse.
    fn formatted(text: &str) -> String {
        fmt(text).expect("the document parses")
    }

    /// P2: the canonical layout is a fixed point, and a canonical file is
    /// returned byte for byte.
    #[test]
    fn p2_canonical_fixed_point() {
        let canonical = concat!(
            "# the whole file\n",
            "app \"shop\"\n",
            "\n",
            "entrypoint python \"shop.wsgi:application\" {\n",
            "    preload \"shop.urls\", \"shop.signals\"\n",
            "}\n",
            "\n",
            "postgres $DATABASE_URL\n",
            "\n",
            "trait session {\n",
            "    postgres {\n",
            "        transactions\n",
            "        query \"SET TIME ZONE 'UTC'\"\n",
            "    }\n",
            "}\n",
            "\n",
            "view products GET, POST \"/products\", \"/products/{id:int}\" {\n",
            "    body json\n",
            "    use session\n",
            "    egress any\n",
            "    prefork\n",
            "    postgres {\n",
            "        functions st_distance, pg_catalog.unaccent\n",
            "        select products(id, name), categories\n",
            "        insert orders(id, total)\n",
            "        delete archived_products\n",
            "    }\n",
            "}\n",
            "\n",
            "view cart {\n",
            "    GET \"/cart\"\n",
            "    POST \"/cart/items\" {\n",
            "        body form\n",
            "    }\n",
            "    postgres analytics unrestricted\n",
            "}\n",
            "\n",
            "static \"/assets\" {\n",
            "    root \"public/assets\"\n",
            "    index \"index.html\"\n",
            "    fallback \"index.html\"\n",
            "    immutable \"/assets/*\", \"/fonts/*\"\n",
            "}\n",
        );
        assert_eq!(formatted(canonical), canonical);
        assert_eq!(formatted(&formatted(canonical)), canonical);
    }

    /// Every rule of §14.2's table, in one document: `;` becomes a newline,
    /// blank lines collapse, indentation is four spaces, a header list stays on
    /// its header line, and the file ends with one newline.
    #[test]
    fn every_layout_rule() {
        let source = "\n\n;app  \"shop\"\n\n\n\nentrypoint python \"a:b\"{preload \"one\",\"two\"}\n\n\
                      view v GET,\n  POST \"/a\",\"/b\" { body json ; use  session }\n\n\n";
        let canonical = concat!(
            "app \"shop\"\n",
            "\n",
            "entrypoint python \"a:b\" {\n",
            "    preload \"one\", \"two\"\n",
            "}\n",
            "\n",
            "view v GET, POST \"/a\", \"/b\" {\n",
            "    body json\n",
            "    use session\n",
            "}\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// An empty trait block is `{` newline `}`, and nothing else may be empty.
    #[test]
    fn an_empty_block_is_two_lines() {
        assert_eq!(formatted("trait t { }"), "trait t {\n}\n");
        assert_eq!(formatted("trait t {\n\n\n}"), "trait t {\n}\n");
        assert_eq!(
            formatted("entrypoint python \"a:b\" { }"),
            "entrypoint python \"a:b\" {\n}\n"
        );
    }

    /// P2b: a comment on its own line, a trailing comment, and a comment as the
    /// last line without a newline all survive exactly once.
    #[test]
    fn comments_survive_once() {
        let source = "# above the view\nview v \"/x\" {   # on the header\n    body json   # trailing\n\n    # above the grant\n    postgres {\n        select t\n    }\n}\n# the last line, no newline";
        let canonical = concat!(
            "# above the view\n",
            "view v \"/x\" { # on the header\n",
            "    body json # trailing\n",
            "    # above the grant\n",
            "    postgres {\n",
            "        select t\n",
            "    }\n",
            "}\n",
            "# the last line, no newline\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// A comment on the line after a statement stays on its own line, and a
    /// comment inside a list the layout never wraps moves above the statement
    /// it belongs to. Both are fixed points.
    #[test]
    fn a_comment_that_has_no_line_of_its_own_moves_above_its_statement() {
        let source = "app \"shop\"\n# after the app\n\nview v GET,\n    # the second method\n    POST \"/x\" {\n    body none\n}\n";
        let canonical = concat!(
            "app \"shop\"\n",
            "\n",
            "# after the app\n",
            "# the second method\n",
            "view v GET, POST \"/x\" {\n",
            "    body none\n",
            "}\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// A comment that trails a directive stays on that line, and one that
    /// trails a block's closing brace stays on the brace's line.
    #[test]
    fn a_trailing_comment_stays_on_its_line() {
        let source = "view v \"/x\" {\n    body json\n    postgres { # the block\n        select t\n    } # after the block\n}\n";
        let canonical = concat!(
            "view v \"/x\" {\n",
            "    body json\n",
            "    postgres { # the block\n",
            "        select t\n",
            "    } # after the block\n",
            "}\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// `;` is never emitted, and a statement written with one is one line.
    #[test]
    fn a_semicolon_becomes_a_newline() {
        assert_eq!(
            formatted("view v \"/x\" { body none; prefork }\n"),
            "view v \"/x\" {\n    body none\n    prefork\n}\n"
        );
    }

    /// A file that is only a comment, and an empty file, both stay minimal.
    #[test]
    fn an_empty_file_stays_empty() {
        assert_eq!(formatted(""), "");
        assert_eq!(formatted("\n\n\n"), "");
        assert_eq!(formatted("# only a comment"), "# only a comment\n");
    }

    /// A CRLF file is written back with LF: the canonical layout has one
    /// newline spelling.
    #[test]
    fn a_crlf_file_is_written_with_lf() {
        assert_eq!(
            formatted("app \"shop\"\r\nview v \"/x\" {\r\n  body json\r\n}\r\n"),
            "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n"
        );
    }

    /// §6.6: a newline after a `,` continues the list, so a wrapped grant list
    /// is written on one line — for every grant that takes a list, and for a
    /// table grant whose columns were wrapped inside its parentheses.
    #[test]
    fn a_wrapped_grant_list_is_written_on_one_line() {
        let source = concat!(
            "view v \"/x\" {\n",
            "    postgres {\n",
            "        functions st_distance,\n",
            "            pg_catalog.unaccent\n",
            "        select products(id, name),\n",
            "            categories,\n",
            "            orders\n",
            "        insert orders(id,\n",
            "            total)\n",
            "        update products(price),\n",
            "            categories\n",
            "        delete archived_products,\n",
            "            orders\n",
            "    }\n",
            "}\n",
        );
        let canonical = concat!(
            "view v \"/x\" {\n",
            "    postgres {\n",
            "        functions st_distance, pg_catalog.unaccent\n",
            "        select products(id, name), categories, orders\n",
            "        insert orders(id, total)\n",
            "        update products(price), categories\n",
            "        delete archived_products, orders\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// A comment written inside a grant list, which the layout unwraps, moves
    /// above the statement and stays there.
    #[test]
    fn a_comment_inside_a_grant_list_moves_above_its_statement() {
        let source = concat!(
            "view v \"/x\" {\n",
            "    postgres {\n",
            "        select products(id),\n",
            "            # and the categories\n",
            "            categories\n",
            "    }\n",
            "}\n",
        );
        let canonical = concat!(
            "view v \"/x\" {\n",
            "    postgres {\n",
            "        # and the categories\n",
            "        select products(id), categories\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(formatted(source), canonical);
        assert_eq!(formatted(canonical), canonical);
    }

    /// A file whose canonical form would grow past the cap is refused, and the
    /// refusal names the cap (R25).
    #[test]
    fn a_file_it_would_grow_past_the_cap_is_refused() {
        let mut source = String::from("view v \"/x\" {\npostgres {\n");
        for _ in 0..80_000 {
            source.push_str("delete t\n");
        }
        source.push_str("}\n}\n");
        assert!(source.len() < 1_048_576, "{} bytes", source.len());
        let error = fmt(&source).expect_err("the canonical form is over the cap");
        assert!(
            error.to_string().contains("1048576"),
            "the refusal names the cap: {error}"
        );
    }

    /// P2b, the owner half: every comment is still a child of the same node
    /// after `fmt`, so no comment drifted into another statement.
    ///
    /// This lives here rather than in `tests/critical_blastfile_print.rs`
    /// because it needs the tree, which is `pub(crate)`; the integration suite
    /// asserts the ordered comment texts and the exact canonical text instead.
    #[test]
    fn p2b_comment_owners_survive() {
        let mut documents = vec![
            "# above the view\nview v \"/x\" { # on the header\n    body json # trailing\n    \
             # above the grant\n    postgres {\n        select t\n    } # after the block\n}\n\
             # the last line\n"
                .to_owned(),
        ];
        documents.extend(fixtures());
        for source in &documents {
            let before = owners(source);
            let after = owners(&formatted(source));
            assert_eq!(after, before, "a comment moved into another node");
            assert!(!before.is_empty() || !source.contains('#'), "{source:?}");
        }
    }

    /// The kind of the node each comment is a child of, in source order.
    fn owners(text: &str) -> Vec<NodeKind> {
        let cst = super::parse(text).expect("the document parses");
        let mut owners = Vec::new();
        collect(&cst, cst.root(), &mut owners);
        owners
    }

    /// Walks the tree in source order, recording each comment's parent.
    fn collect(cst: &Cst, node: NodeId, owners: &mut Vec<NodeKind>) {
        let kind = cst.kind(node).unwrap_or(NodeKind::File);
        for (child, element) in cst.child_elements(node) {
            match element {
                Element::Trivia(id) if cst.trivia_kind(id) == Some(TriviaKind::Comment) => {
                    owners.push(kind);
                }
                Element::Node { .. } => collect(cst, child, owners),
                _ => {}
            }
        }
    }

    /// Every fixture of the corpus (§20's round-trip corpus).
    fn fixtures() -> Vec<String> {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(directory)
            .expect("the fixture directory is readable")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "blastfile")
            })
            .collect();
        files.sort();
        assert_eq!(files.len(), 15, "the corpus contract");
        files
            .into_iter()
            .map(|path| std::fs::read_to_string(&path).expect("the fixture is readable"))
            .collect()
    }
}
