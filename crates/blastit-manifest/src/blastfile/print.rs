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
/// The canonical text is **read back before it is returned** (R25): `fmt` never
/// hands a caller — or writes — a file the one reader refuses. The re-parse runs
/// under the same budgets as any other file, so a printer regression is a
/// refusal at the byte it broke, not a file that fails to boot later.
///
/// # Errors
///
/// Everything the parser refuses, [`ManifestError::TooLarge`] when the canonical
/// text would exceed `MAX_SOURCE_BYTES`: a file that formatting would grow past
/// the reader's own cap is refused rather than written (R25).
pub fn fmt(text: &str) -> Result<String, ManifestErrors> {
    format_with(text, write)
}

/// [`fmt`], with the layout step injected so a test can hand the postcondition a
/// printer that wrote the wrong bytes.
///
/// The production caller is [`fmt`] with [`write`]; the only other caller is the
/// module's own regression test, which proves that deleting the re-parse lets a
/// printer regression through. Nothing outside this module can reach it.
fn format_with(text: &str, layout: impl Fn(&Cst) -> String) -> Result<String, ManifestErrors> {
    let cst = parse(text)?;
    let formatted = layout(&cst);
    if formatted.len() > MAX_SOURCE_BYTES {
        return Err(ManifestError::TooLarge {
            bytes: formatted.len() as u64,
        }
        .into());
    }
    // R25: refuse canonical text the one parser cannot read back. A printer
    // regression is exactly this: text this module wrote that the reader
    // refuses. The check is a plain `parse`, so it is the same budget table
    // (`MAX_TOKENS`, `MAX_TRIVIA`, `MAX_BLOCK_DEPTH`) every other file goes
    // through.
    parse(&formatted)?;
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
