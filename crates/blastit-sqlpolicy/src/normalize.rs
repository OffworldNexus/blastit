//! Text normalisation (ARCHITECTURE.md §6, "Matching").
//!
//! The manifest and the wire are normalised with the same rules, so a query
//! written by hand in a grant matches the text libpq sends, as long as it is
//! the same statement with arbitrary whitespace, comments and formatting.
//!
//! Which characters are whitespace, where a string ends and where a comment
//! starts all follow PostgreSQL 18's `src/backend/parser/scan.l`, via the
//! shared [`crate::lexer`]: a no-break space is not whitespace to PostgreSQL,
//! and a comment can never hide part of a statement.
//!
//! On top of the text rules, two *shape* rules make a grant survive the two
//! things a real Django app does that are not a change of statement at all
//! (§6):
//!
//! * a `prefetch_related` or `__in` filter sends a placeholder list of any
//!   length — `IN ($1, $2, …, $n)` — and every extra placeholder renumbers
//!   every placeholder after it, so placeholders are compared as `$?` and any
//!   non-empty list of them is compared as one shape;
//! * `bulk_create` sends `VALUES ($1, $2), ($3, $4), …` with a varying number
//!   of identical rows, which collapses to the first row followed by `, ...`.
//!
//! Both rules are applied to the grant and to the statement, by this one
//! function, so neither side can be shaped differently from the other.

use crate::lexer::{RegionKind, code_token_end, is_ident_start, is_space, lex, placeholder_end};

/// Normalises `sql` for exact allowlist comparison.
///
/// The rules (ARCHITECTURE.md §6):
///
/// * the text is trimmed of scan.l whitespace only, so a no-break space at
///   either end is kept;
/// * every comment is replaced by a single space: `--` up to the end of its
///   line (`\n` **or** `\r`), and `/* … */` including PostgreSQL's nesting. An
///   unterminated block comment swallows the rest of the text;
/// * every run of whitespace left in the code — scan.l's `space`, i.e. space,
///   tab, newline, carriage return, form feed, vertical tab — collapses to one
///   space. Any other character that Rust considers whitespace (no-break space,
///   line separator, …) is ordinary text to PostgreSQL and is kept as-is;
/// * trailing `;` separators are removed, including the whitespace around them
///   and repeated separators (`SELECT 1 ;;` → `SELECT 1`); only separators in
///   code are removed, so `SELECT ';'` is unchanged;
/// * every bind placeholder `$1`, `$23`, … in code becomes `$?`: the numbering
///   carries no meaning for the policy, because values always travel
///   separately, and it shifts whenever a statement is assembled dynamically.
///   A `$` inside a string, a quoted identifier or a dollar quote is data, and
///   a `$` inside an identifier (`a$1`) belongs to that identifier;
/// * a parenthesised list right after `IN` or `NOT IN` that holds nothing but
///   placeholders becomes `IN ($?, ...)`, whatever its length: `prefetch_related`
///   sends `IN ($1, $2, …, $n)`. A list with anything else in it (a string, a
///   subquery, a nested tuple) is left alone;
/// * after `VALUES`, a run of two or more *identical* rows `(…), (…), …`
///   becomes the single row followed by `, ...`, because `bulk_create` sends a
///   varying number of identical rows. A single row is left as it is, rows that
///   differ from each other do not collapse, and a function call's argument
///   list is never collapsed — its arity picks the overload;
/// * quoted regions are kept verbatim: single-quoted strings (`'it''s'`),
///   E-strings (`E'a\'b  c'`, where a backslash escapes the next character),
///   bit and hex strings (`B'1010'`, `X'1F'`, with no escape at all),
///   double-quoted identifiers (`"a  b"`) and dollar-quoted strings (`$$…$$`,
///   `$tag$…$tag$`);
/// * `a$b$c` is one identifier and does not open a dollar quote. A bare `$` is
///   ordinary text;
/// * an unterminated quote is kept verbatim to the end of the text; the text is
///   trimmed before scanning, so trailing whitespace is gone even inside an
///   unterminated quote. This function never panics.
///
/// Strings are read as if `standard_conforming_strings` were `on` (see the
/// crate documentation).
///
/// ```
/// # use blastit_sqlpolicy::normalize;
/// assert_eq!(normalize("SELECT 1 -- ; DROP TABLE t"), "SELECT 1");
/// assert_eq!(normalize("SELECT 1 --\n; DROP TABLE t"), "SELECT 1 ; DROP TABLE t");
/// assert_eq!(normalize("SELECT 1 --\r; DROP TABLE t"), "SELECT 1 ; DROP TABLE t");
/// assert_eq!(normalize("SELECT 1 /* c */ 2 ;"), "SELECT 1 2");
/// assert_eq!(normalize("SELECT 'a  b'"), "SELECT 'a  b'");
/// assert_eq!(normalize("SELECT\x0b1"), "SELECT 1");
/// assert_eq!(normalize("WHERE id = $1"), "WHERE id = $?");
/// assert_eq!(normalize("WHERE id IN ($1, $2, $3)"), "WHERE id IN ($?, ...)");
/// assert_eq!(
///     normalize("INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)"),
///     "INSERT INTO t (a, b) VALUES ($?, $?), ...",
/// );
/// ```
pub fn normalize(sql: &str) -> String {
    let chars: Vec<char> = sql.trim_matches(is_space).chars().collect();
    let mut out = Output::new(chars.len());
    let mut pending_space = false;

    for region in lex(&chars) {
        let text = &chars[region.start..region.end];
        match region.kind {
            RegionKind::Comment => {
                // A comment stands for a single space; the surrounding
                // whitespace collapsing then merges it with its neighbours.
                pending_space = true;
            }
            RegionKind::Code => {
                let mut i = 0;
                while i < text.len() {
                    if is_space(text[i]) {
                        pending_space = true;
                        i += 1;
                        continue;
                    }
                    out.flush_space(&mut pending_space);
                    if let Some(end) = placeholder_end(text, i) {
                        // `$?` is stable: it holds no digits, so normalising
                        // the result again changes nothing.
                        out.push('$', true);
                        out.push('?', true);
                        i = end;
                        continue;
                    }
                    // One whole code token, copied verbatim: `a$1` is a single
                    // identifier and keeps its `$`.
                    let end = code_token_end(text, i);
                    for c in &text[i..end] {
                        out.push(*c, true);
                    }
                    i = end;
                }
            }
            RegionKind::SingleQuoted
            | RegionKind::EString
            | RegionKind::BitString
            | RegionKind::HexString
            | RegionKind::QuotedIdent
            | RegionKind::DollarQuoted => {
                // Quoted: verbatim, so inner whitespace and `;` are preserved.
                out.flush_space(&mut pending_space);
                for c in text {
                    out.push(*c, false);
                }
            }
        }
    }

    reshape(&out.finish())
}

/// The shape rules, applied to the already-normalised text: `IN` lists of
/// placeholders and `VALUES` row runs, as the module documentation describes.
///
/// The text is walked token by token, with one frame per open parenthesis, so
/// the pass is linear in the length of the text and needs no recursion: a
/// hostile statement with thousands of nested parentheses cannot exhaust the
/// stack. Parentheses that never close are left as ordinary text, exactly as
/// the lexer leaves an unterminated quote alone.
fn reshape(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut outer = Frame::new(String::new());
    let mut stack: Vec<Frame> = Vec::new();

    for token in tokenize(&chars) {
        match token.kind {
            Kind::Comma => {
                let frame = current(&mut outer, &mut stack);
                flush_comma(frame);
                frame.comma = Some(token.text);
            }
            Kind::Open => stack.push(Frame::new(token.leading_space().to_string())),
            Kind::Close if !stack.is_empty() => {
                if let Some(child) = stack.pop() {
                    let parent = current(&mut outer, &mut stack);
                    close_group(parent, child, true);
                }
            }
            // A `)` with no `(` in front of it is ordinary text.
            _ => {
                let frame = current(&mut outer, &mut stack);
                push_token(frame, token);
            }
        }
    }

    // An unterminated group runs to the end of the text.
    while let Some(child) = stack.pop() {
        let parent = current(&mut outer, &mut stack);
        close_group(parent, child, false);
    }

    let mut text = String::with_capacity(sql.len());
    for node in outer.nodes {
        text.push_str(&node.text);
    }
    if let Some(comma) = outer.comma {
        text.push_str(&comma);
    }
    text
}

/// The frame the next token belongs to: the innermost open group, or the
/// outermost frame when no group is open.
fn current<'a>(outer: &'a mut Frame, stack: &'a mut [Frame]) -> &'a mut Frame {
    stack.last_mut().unwrap_or(outer)
}

/// What a token of the normalised text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A bare word: keywords and identifiers, `$` included (`a$1` is one).
    Word,
    /// A bind placeholder. [`normalize`] has already rewritten `$1` to `$?`.
    Placeholder,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// `,`.
    Comma,
    /// A quoted region: a string, a quoted identifier or a dollar quote.
    Quoted,
    /// Anything else: operators, numbers, `;`, a lone `$`…
    Other,
}

/// One piece of text, with the whitespace that precedes it, so that
/// concatenating a frame's nodes reproduces the text they were built from.
#[derive(Debug, Clone)]
struct Node {
    kind: Kind,
    text: String,
}

impl Node {
    /// A piece of text the rules themselves produced.
    fn other(text: String) -> Self {
        Self {
            kind: Kind::Other,
            text,
        }
    }
}

/// Splits the normalised text into tokens.
///
/// Quoted regions come from the shared lexer, so their contents are never
/// tokenised; code is split into words, placeholders, brackets, commas and
/// single characters.
fn tokenize(chars: &[char]) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut pending_space = false;

    for region in lex(chars) {
        let text = &chars[region.start..region.end];
        match region.kind {
            RegionKind::Comment => pending_space = true,
            RegionKind::Code => {
                let mut i = 0;
                while i < text.len() {
                    if is_space(text[i]) {
                        pending_space = true;
                        i += 1;
                        continue;
                    }
                    let (kind, end) = code_token(text, i);
                    tokens.push(Token::new(kind, &text[i..end], &mut pending_space));
                    i = end;
                }
            }
            RegionKind::SingleQuoted
            | RegionKind::EString
            | RegionKind::BitString
            | RegionKind::HexString
            | RegionKind::QuotedIdent
            | RegionKind::DollarQuoted => {
                tokens.push(Token::new(Kind::Quoted, text, &mut pending_space));
            }
        }
    }

    tokens
}

/// The kind and the end of the code token starting at `text[i]`.
fn code_token(text: &[char], i: usize) -> (Kind, usize) {
    if let Some(end) = placeholder_end(text, i) {
        return (Kind::Placeholder, end);
    }
    // `$?` is what [`normalize`] writes for a placeholder; anything else that
    // starts with `$` is ordinary text.
    if text[i] == '$' && text.get(i + 1) == Some(&'?') {
        return (Kind::Placeholder, i + 2);
    }
    match text[i] {
        '(' => (Kind::Open, i + 1),
        ')' => (Kind::Close, i + 1),
        ',' => (Kind::Comma, i + 1),
        _ => {
            let end = code_token_end(text, i);
            let kind = if is_ident_start(text[i]) {
                Kind::Word
            } else {
                Kind::Other
            };
            (kind, end)
        }
    }
}

/// One token, with the whitespace that came before it.
#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
}

impl Token {
    /// Builds a token, moving a pending whitespace run into its text.
    fn new(kind: Kind, text: &[char], pending_space: &mut bool) -> Self {
        let mut string = String::with_capacity(text.len() + 1);
        if std::mem::take(pending_space) {
            string.push(' ');
        }
        string.extend(text);
        Self { kind, text: string }
    }

    /// The token's text without the whitespace in front of it.
    fn word(&self) -> &str {
        self.text.trim_start_matches(is_space)
    }

    /// The whitespace in front of the token, which is all a group needs to
    /// keep: the `(` itself is written by the shape rules.
    fn leading_space(&self) -> &str {
        let rest = self.word();
        &self.text[..self.text.len() - rest.len()]
    }
}

/// One open parenthesis level, while [`reshape`] walks the tokens.
struct Frame {
    /// The text placed so far, in order.
    nodes: Vec<Node>,
    /// A comma whose node has not been placed yet: whether it survives depends
    /// on what follows it.
    comma: Option<String>,
    /// The rows after `VALUES` that this frame has placed, while the list is
    /// still open: any other token ends it.
    rows: Option<Rows>,
    /// Whether the last token placed was the `VALUES` keyword.
    after_values: bool,
    /// Whether a code `;` sits somewhere in this frame, directly or inside a
    /// group. A row that holds one is never collapsed: dropping it would change
    /// how many statements the text has, and the transaction-control grammar
    /// counts statements.
    separator: bool,
    /// The whitespace that preceded the `(` that opened this frame.
    lead: String,
}

impl Frame {
    /// An empty frame whose group was opened by the token carrying `lead`.
    fn new(lead: String) -> Self {
        Self {
            nodes: Vec::new(),
            comma: None,
            rows: None,
            after_values: false,
            separator: false,
            lead,
        }
    }
}

/// The rows after a `VALUES` keyword, as [`reshape`] sees them.
///
/// Only the last row is remembered, because only a row that repeats it can
/// join it: two rows that are identical collapse into one, and the rows that
/// follow them may still repeat them in their turn.
struct Rows {
    /// Text of the last row of the list.
    last: String,
    /// Index in the frame's nodes of the last row, or of the `...` that stands
    /// for the rows repeating it.
    at: usize,
    /// Whether the list's last row has already been written as `row, ...`.
    collapsed: bool,
}

/// Places a comma that was held back.
fn flush_comma(frame: &mut Frame) {
    if let Some(comma) = frame.comma.take() {
        frame.nodes.push(Node {
            kind: Kind::Comma,
            text: comma,
        });
    }
}

/// Places one ordinary token: a word, a placeholder, an operator, a stray `)`.
fn push_token(frame: &mut Frame, token: Token) {
    flush_comma(frame);
    frame.after_values = token.kind == Kind::Word && token.word().eq_ignore_ascii_case("values");
    // Anything but another row ends the `VALUES` row list.
    frame.rows = None;
    if token.kind == Kind::Other && token.word() == ";" {
        frame.separator = true;
    }
    frame.nodes.push(Node {
        kind: token.kind,
        text: token.text,
    });
}

/// Places a group in its parent frame, applying the shape rules.
///
/// `closed` is false for a `(` that never closed; such a group is ordinary
/// text, because the text after it is not the group the rules would have seen.
fn close_group(parent: &mut Frame, child: Frame, closed: bool) {
    let Frame {
        mut nodes,
        comma,
        rows: _,
        after_values: _,
        separator,
        lead,
    } = child;
    if let Some(comma) = comma {
        nodes.push(Node {
            kind: Kind::Comma,
            text: comma,
        });
    }
    let list = is_placeholder_list(&nodes);
    let inner: String = nodes.iter().map(|node| node.text.as_str()).collect();
    parent.separator |= separator;

    if !closed {
        flush_comma(parent);
        parent.after_values = false;
        parent.rows = None;
        parent.nodes.push(Node::other(format!("{lead}({inner}")));
        return;
    }

    let text = format!("({inner})");

    // The row tuple right after `VALUES` opens the row list.
    if parent.after_values {
        flush_comma(parent);
        parent.after_values = false;
        parent.nodes.push(Node::other(format!("{lead}{text}")));
        let at = parent.nodes.len() - 1;
        parent.rows = Some(Rows {
            last: text,
            at,
            collapsed: false,
        });
        return;
    }

    // Another row of the row list. It only counts when a comma sits between the
    // two rows and nothing has been placed since the list's last row, so
    // `VALUES (…), x, (…)` is never a list of rows.
    let in_rows = parent.comma.is_some()
        && parent
            .rows
            .as_ref()
            .is_some_and(|rows| rows.at + 1 == parent.nodes.len());
    if in_rows {
        let previous = parent
            .rows
            .as_ref()
            .map(|rows| (rows.last.clone(), rows.collapsed));
        if let Some((last, collapsed)) = previous {
            if last == text && !separator {
                if let Some(comma) = parent.comma.take()
                    && !collapsed
                {
                    // The identical row after the list's first one: write
                    // `row, ...` in its place, and drop the rows that repeat it.
                    let marker = format!("{comma}{lead}...");
                    parent.nodes.push(Node::other(marker));
                    let at = parent.nodes.len() - 1;
                    if let Some(rows) = parent.rows.as_mut() {
                        rows.at = at;
                        rows.collapsed = true;
                    }
                }
                return;
            }
            // A different row: it becomes the row list's new last row, and a
            // later repeat of it collapses in its own turn.
            if let Some(comma) = parent.comma.take() {
                parent.nodes.push(Node::other(comma));
                parent.nodes.push(Node::other(format!("{lead}{text}")));
                let at = parent.nodes.len() - 1;
                if let Some(rows) = parent.rows.as_mut() {
                    rows.last = text;
                    rows.at = at;
                    rows.collapsed = false;
                }
            }
            return;
        }
    }

    // A list of placeholders right after `IN` or `NOT IN`, whatever its length.
    if parent.comma.is_none() && list {
        let keyword = parent.nodes.last().and_then(|node| {
            split_keyword(&node.text, "in").map(|(lead, word)| (lead.to_string(), word.to_string()))
        });
        if let Some((word_lead, word)) = keyword {
            parent.nodes.pop();
            parent
                .nodes
                .push(Node::other(format!("{word_lead}{word} ($?, ...)")));
            return;
        }
    }

    // Anything else: the group as written.
    flush_comma(parent);
    parent.after_values = false;
    parent.rows = None;
    parent.nodes.push(Node::other(format!("{lead}{text}")));
}

/// Whether a group holds placeholders separated by commas and nothing else:
/// `($?)`, `($?, $?)`, … A trailing comma, anything that is not a placeholder,
/// and an empty list all fail.
fn is_placeholder_list(nodes: &[Node]) -> bool {
    nodes
        .last()
        .is_some_and(|last| last.kind == Kind::Placeholder)
        && nodes.iter().enumerate().all(|(index, node)| {
            node.kind
                == if index % 2 == 0 {
                    Kind::Placeholder
                } else {
                    Kind::Comma
                }
        })
}

/// Splits `text` into the whitespace in front of it and its spelling, when the
/// text spells `word` (ASCII case-insensitively) and nothing else.
fn split_keyword<'a>(text: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let rest = text.trim_start_matches(is_space);
    let lead = &text[..text.len() - rest.len()];
    (!rest.is_empty() && rest.eq_ignore_ascii_case(word)).then_some((lead, rest))
}

/// Output under construction, with a parallel flag per character telling
/// whether it came from code (and may therefore take part in the trailing
/// separator strip).
struct Output {
    text: String,
    plain: Vec<bool>,
}

impl Output {
    fn new(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
            plain: Vec::with_capacity(capacity),
        }
    }

    fn push(&mut self, c: char, plain: bool) {
        self.text.push(c);
        self.plain.push(plain);
    }

    /// Emits the single space that stands for a collapsed whitespace run or a
    /// comment, if one is pending.
    fn flush_space(&mut self, pending: &mut bool) {
        if *pending {
            if !self.text.is_empty() {
                self.push(' ', true);
            }
            *pending = false;
        }
    }

    /// Drops trailing `;` separators and the whitespace around them, then
    /// returns the normalised text. Only code characters are dropped.
    fn finish(mut self) -> String {
        while let Some(last) = self.text.chars().next_back() {
            let plain = matches!(self.plain.last(), Some(true));
            if plain && (last == ';' || is_space(last)) {
                self.text.pop();
                self.plain.pop();
            } else {
                break;
            }
        }
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(input: &str, expected: &str) {
        assert_eq!(normalize(input), expected, "input: {input:?}");
    }

    #[test]
    fn collapses_whitespace_and_trims() {
        case("SELECT  1", "SELECT 1");
        case("SELECT\t1", "SELECT 1");
        case("SELECT\n1", "SELECT 1");
        case("SELECT\r1", "SELECT 1");
        case("SELECT \t\n 1", "SELECT 1");
        case("  SELECT\n\n  1  ", "SELECT 1");
        case("", "");
        case("   \t\n ", "");
    }

    #[test]
    fn collapses_exactly_scan_l_whitespace() {
        // scan.l `space` = [ \t\n\r\f\v]
        case("SELECT\x0b1", "SELECT 1");
        case("SELECT\x0c1", "SELECT 1");
        case("SELECT\r\n1", "SELECT 1");
        case("SELECT \x0b\x0c\r\n 1", "SELECT 1");
        // Everything else is ordinary text and must not be touched.
        case("SELECT\u{a0}1", "SELECT\u{a0}1");
        case("SELECT\u{85}1", "SELECT\u{85}1");
        case("SELECT\u{2000}1", "SELECT\u{2000}1");
        case("SELECT\u{2028}1", "SELECT\u{2028}1");
        case("SELECT\u{3000}1", "SELECT\u{3000}1");
    }

    #[test]
    fn replaces_comments_with_one_space() {
        case("SELECT 1 --  a   b", "SELECT 1");
        case("SELECT 1 --  a   b\n", "SELECT 1");
        case("SELECT 1 -- a\nFROM t", "SELECT 1 FROM t");
        case("SELECT 1 /*  a\n b */", "SELECT 1");
        case("SELECT 1/*  a  */2", "SELECT 1 2");
        case("/* lead */SELECT 1", "SELECT 1");
        case("/* unterminated", "");
        case("SELECT 1 /* unterminated", "SELECT 1");
        case("SELECT 1 /* a /* b */ c */", "SELECT 1");
        case("-- only a comment", "");
        case("SELECT 1 --", "SELECT 1");
        case("SELECT 1 ;-- x", "SELECT 1");
        case("SELECT /* x */'a  b'", "SELECT 'a  b'");
    }

    #[test]
    fn a_carriage_return_ends_a_line_comment() {
        case("Q --\r; DROP TABLE t", "Q ; DROP TABLE t");
        case("Q -- c\r; DROP TABLE t", "Q ; DROP TABLE t");
        case("BEGIN -- x\r; DROP TABLE t", "BEGIN ; DROP TABLE t");
        case("SELECT 1 -- c\r\nFROM t", "SELECT 1 FROM t");
        case("SELECT 1 --\r", "SELECT 1");
        // Without a newline of either kind, the comment runs to the end.
        case("Q --\n; DROP TABLE t", "Q ; DROP TABLE t");
        case("Q --; DROP TABLE t", "Q");
    }

    #[test]
    fn comment_markers_inside_quotes_are_not_comments() {
        case("SELECT '-- not a comment'", "SELECT '-- not a comment'");
        case(
            "SELECT '/* not a comment */'",
            "SELECT '/* not a comment */'",
        );
        case("SELECT \"-- not a comment\"", "SELECT \"-- not a comment\"");
        case("SELECT E'-- not a comment'", "SELECT E'-- not a comment'");
        case("SELECT B'-- not a comment'", "SELECT B'-- not a comment'");
        case("SELECT X'-- not a comment'", "SELECT X'-- not a comment'");
        case(
            "SELECT $$--  not  a  comment$$",
            "SELECT $$--  not  a  comment$$",
        );
    }

    #[test]
    fn strips_trailing_separators() {
        case("SELECT 1;", "SELECT 1");
        case("SELECT 1 ;", "SELECT 1");
        case("SELECT 1  ;  ", "SELECT 1");
        case("SELECT 1;;", "SELECT 1");
        case("SELECT 1; ; ;", "SELECT 1");
        case("SELECT 1 ; ;  \n", "SELECT 1");
        case(";", "");
        case(";;", "");
        case("SELECT 1; SELECT 2", "SELECT 1; SELECT 2");
        case("SELECT 1;'x'", "SELECT 1;'x'");
    }

    #[test]
    fn keeps_whitespace_inside_single_quoted_strings() {
        case("SELECT 'a  b'", "SELECT 'a  b'");
        case("SELECT   'a  b'   ", "SELECT 'a  b'");
        case("SELECT 'it''s  ok'", "SELECT 'it''s  ok'");
        case("SELECT ';'", "SELECT ';'");
        case("SELECT ';' ;", "SELECT ';'");
        case("SELECT ''", "SELECT ''");
        case("SET TIME ZONE 'UTC'", "SET TIME ZONE 'UTC'");
    }

    #[test]
    fn keeps_whitespace_inside_quoted_identifiers() {
        case(r#"SELECT "a  b""#, r#"SELECT "a  b""#);
        case(r#"SELECT  "a  b"  FROM  "t""#, r#"SELECT "a  b" FROM "t""#);
        case(r#"SELECT "a""b""#, r#"SELECT "a""b""#);
        case(r#"SELECT ";" "#, r#"SELECT ";""#);
    }

    #[test]
    fn keeps_dollar_quotes_verbatim() {
        case("SELECT $$a  b$$", "SELECT $$a  b$$");
        case("SELECT  $$ a  b $$  ", "SELECT $$ a  b $$");
        case("SELECT $tag$a  b$tag$", "SELECT $tag$a  b$tag$");
        case("SELECT $tag$; $other$ $tag$", "SELECT $tag$; $other$ $tag$");
        case("SELECT $$;$$", "SELECT $$;$$");
        case("SELECT $$", "SELECT $$");
        case("SELECT $$a  b", "SELECT $$a  b");
        case("SELECT $tag$a", "SELECT $tag$a");
        case("SELECT $é$a  b$é$", "SELECT $é$a  b$é$");
        case("SELECT $€$a  b$€$", "SELECT $€$a  b$€$");
        case("SELECT $€1_$a  b$€1_$", "SELECT $€1_$a  b$€1_$");
        case("SELECT $_t1$a  b$_t1$", "SELECT $_t1$a  b$_t1$");
    }

    #[test]
    fn identifiers_keep_their_dollar_signs() {
        // `a$b$c` is one identifier (scan.l `ident_cont` includes `$`), so the
        // spaces around it collapse instead of opening a dollar quote.
        case("SELECT a$b$c", "SELECT a$b$c");
        case("SELECT a$b$x   y$b$z", "SELECT a$b$x y$b$z");
        case("SELECT a$$b$$c", "SELECT a$$b$$c");
        case("SELECT  a$tag$x   y$tag$z  ", "SELECT a$tag$x y$tag$z");
    }

    #[test]
    fn keeps_e_strings_verbatim() {
        case(r"SELECT E'a\'b  c'", r"SELECT E'a\'b  c'");
        case(r"SELECT e'a\'b  c'", r"SELECT e'a\'b  c'");
        case(r"SELECT E'it''s  ok'", r"SELECT E'it''s  ok'");
        case(r"E'a\'b", r"E'a\'b");
        case(r"SELECT (E'a  b'), 'c  d'", r"SELECT (E'a  b'), 'c  d'");
    }

    #[test]
    fn e_strings_after_an_identifier_are_not_e_strings() {
        case(r"WHERE'x  y'", r"WHERE'x  y'");
        case(r"SELECT xE'y  z'", r"SELECT xE'y  z'");
        case(r"SELECT name  E'y  z'", r"SELECT name E'y  z'");
    }

    #[test]
    fn e_strings_keep_their_own_whitespace() {
        let escaped = normalize(r"SELECT E'a\'b  c'");
        assert_eq!(escaped, r"SELECT E'a\'b  c'");
        assert_ne!(escaped, normalize(r"SELECT E'a\'b c'"));
        assert_ne!(escaped, r"SELECT E'a'b c'");
    }

    #[test]
    fn a_number_before_the_e_is_not_a_string_prefix() {
        // scan.l `integer_junk`: `1E` is one (error) token, so the quote after
        // it starts a plain string, where a backslash is not an escape.
        case(r"1E'a\'b  c'", r"1E'a\'b c'");
        case(r".5E'a\'b  c'", r".5E'a\'b c'");
        case(r"0x1E'a\'b  c'", r"0x1E'a\'b c'");
        case("SELECT 1, E'a  b'", "SELECT 1, E'a  b'");
    }

    #[test]
    fn bit_and_hex_strings_have_no_escape() {
        // xb/xh are `[^']*`: no `''`, no backslash.
        case(r"B'a\'b  c'", r"B'a\'b c'");
        case(r"X'a\'b  c'", r"X'a\'b c'");
        case("B'a''b  c'", "B'a''b  c'");
        case("SELECT B'1010 1010'", "SELECT B'1010 1010'");
        case("SELECT X'1F 2A'", "SELECT X'1F 2A'");
    }

    #[test]
    fn unterminated_quotes_are_kept_verbatim() {
        case("SELECT 'a  b", "SELECT 'a  b");
        case("SELECT 'a  b  ", "SELECT 'a  b");
        case(r#"SELECT "a  b"#, r#"SELECT "a  b"#);
        case("SELECT '", "SELECT '");
        case(r#"SELECT """#, r#"SELECT """#);
        case("SELECT 'a' 'b  c", "SELECT 'a' 'b  c");
        case("SELECT 'a  b;", "SELECT 'a  b;");
        case("SELECT 'a  b';", "SELECT 'a  b'");
        case(r"SELECT B'a  b", r"SELECT B'a  b");
    }

    #[test]
    fn strings_separated_by_a_newline_are_not_continued() {
        // scan.l would scan `'a'\n'b'` as one literal; see the lexer's module
        // documentation for why that difference is harmless.
        case("SELECT 'a'\n'b'", "SELECT 'a' 'b'");
        case("SELECT 'a'  'b'", "SELECT 'a' 'b'");
    }

    #[test]
    fn unicode_is_preserved() {
        case("SELECT  'héllo  wörld'", "SELECT 'héllo  wörld'");
        case("SELECT 'héllo'  ,  'wörld'", "SELECT 'héllo' , 'wörld'");
        case("SELECT '日本語  テスト'", "SELECT '日本語  テスト'");
        case("SELECT €€", "SELECT €€");
    }

    #[test]
    fn normalisation_is_idempotent_on_examples() {
        for input in [
            "SELECT 1 ;",
            "SELECT 'a  b'",
            "SELECT $tag$a  b$tag$ ;",
            "SELECT 'a  b",
            "BEGIN; SELECT 1",
            "; ;",
            "SELECT 1 -- don't  collapse",
            r"SELECT E'a\'b  c'",
            "SELECT 1/* c */2",
            "Q --\r; DROP TABLE t",
            "SELECT B'a\\'b  c'",
            "SELECT\u{a0}1",
        ] {
            let once = normalize(input);
            assert_eq!(normalize(&once), once, "input: {input:?}");
        }
    }

    //
    // Placeholders.
    //

    #[test]
    fn every_placeholder_becomes_the_same_shape() {
        case("WHERE a = $1", "WHERE a = $?");
        case("WHERE a = $1  AND b = $2", "WHERE a = $? AND b = $?");
        case("WHERE a = $12", "WHERE a = $?");
        case("SELECT $0", "SELECT $?");
        case("SELECT $007", "SELECT $?");
        case("SELECT $1 ;", "SELECT $?");
        case("SELECT $1$", "SELECT $?$");
        case("SELECT $1$1", "SELECT $?$?");
        // A `$` that opens a dollar quote is not a placeholder, and neither is
        // a `$` inside an identifier.
        case("SELECT $$1", "SELECT $$1");
        case("SELECT $tag$ $1 $tag$", "SELECT $tag$ $1 $tag$");
        case("SELECT a$1", "SELECT a$1");
        case("SELECT a$b$c", "SELECT a$b$c");
        case("SELECT 1$1", "SELECT 1$?"); // scan.l reads `1` then the parameter
        case("SELECT $", "SELECT $");
        case("SELECT $foo", "SELECT $foo");
        case("SELECT $?", "SELECT $?");
    }

    #[test]
    fn placeholders_inside_quoted_regions_are_data() {
        for (input, expected) in [
            ("SELECT '$1'", "SELECT '$1'"),
            ("SELECT \"$1\"", "SELECT \"$1\""),
            ("SELECT E'$1'", "SELECT E'$1'"),
            ("SELECT B'$1'", "SELECT B'$1'"),
            ("SELECT X'$1'", "SELECT X'$1'"),
            ("SELECT $$ $1 $$", "SELECT $$ $1 $$"),
            ("SELECT $tag$ $1 $tag$", "SELECT $tag$ $1 $tag$"),
            ("SELECT 1 -- $1", "SELECT 1"),
            ("SELECT 1 /* $1 */", "SELECT 1"),
        ] {
            case(input, expected);
        }
    }

    #[test]
    fn a_placeholder_matches_whatever_its_number_was() {
        // The whole point: a grant written from one run of the app matches the
        // same statement in another run, where the numbering has shifted.
        let grant = r#"SELECT "n"."id" FROM "n" WHERE "n"."id" = $1 AND "n"."owner_id" = $2"#;
        let statement = r#"SELECT "n"."id" FROM "n" WHERE "n"."id" = $3 AND "n"."owner_id" = $4"#;
        assert_eq!(normalize(grant), normalize(statement));
    }

    //
    // `IN` lists.
    //

    #[test]
    fn an_in_list_of_placeholders_collapses_whatever_its_length() {
        case("WHERE id IN ($1)", "WHERE id IN ($?, ...)");
        case("WHERE id IN ($1, $2)", "WHERE id IN ($?, ...)");
        case("WHERE id IN ($1, $2, $3, $4, $5)", "WHERE id IN ($?, ...)");
        case("WHERE id in ($1, $2)", "WHERE id in ($?, ...)");
        case("WHERE id NOT IN ($1, $2)", "WHERE id NOT IN ($?, ...)");
        case("WHERE id not in ($1, $2)", "WHERE id not in ($?, ...)");
        // Spacing inside and around the list does not matter.
        case("WHERE id IN($1,$2)", "WHERE id IN ($?, ...)");
        case("WHERE id IN  ( $1 , $2 )", "WHERE id IN ($?, ...)");
        // ... and every length matches every other.
        for list in [
            "($1)",
            "($1, $2)",
            "($3, $4, $5)",
            "($1, $2, $3, $4, $5, $6, $7)",
        ] {
            assert_eq!(
                normalize(&format!("WHERE id IN {list}")),
                "WHERE id IN ($?, ...)",
                "{list}"
            );
        }
    }

    #[test]
    fn a_django_prefetch_statement_keeps_matching_its_grant() {
        // What `prefetch_related` sends: the `IN` list grows with the page.
        let grant =
            r#"SELECT "n"."id", "n"."owner_id", "n"."text" FROM "n" WHERE "n"."owner_id" IN ($1)"#;
        for n in 1..=6 {
            let placeholders: Vec<String> = (1..=n).map(|i| format!("${i}")).collect();
            let statement = format!(
                r#"SELECT "n"."id", "n"."owner_id", "n"."text" FROM "n" WHERE "n"."owner_id" IN ({})"#,
                placeholders.join(", ")
            );
            assert_eq!(normalize(grant), normalize(&statement), "n = {n}");
        }
    }

    #[test]
    fn an_in_list_that_is_not_only_placeholders_is_left_alone() {
        // Mixed contents, a subquery, a nested tuple, a string, an empty list.
        case("WHERE id IN ($1, 'x')", "WHERE id IN ($?, 'x')");
        case("WHERE id IN ('x', $1)", "WHERE id IN ('x', $?)");
        case(
            "WHERE id IN (SELECT id FROM t WHERE x = $1)",
            "WHERE id IN (SELECT id FROM t WHERE x = $?)",
        );
        case("WHERE id IN ()", "WHERE id IN ()");
        case("WHERE id IN (($1), ($2))", "WHERE id IN (($?), ($?))");
        case("WHERE id IN ($1::int)", "WHERE id IN ($?::int)");
        case("WHERE id IN (DEFAULT)", "WHERE id IN (DEFAULT)");
        // A trailing comma is not a list of placeholders.
        case("WHERE id IN ($1,)", "WHERE id IN ($?,)");
        // `IN` used as anything but the keyword in front of the list.
        case("SELECT within ($1, $2)", "SELECT within ($?, $?)");
        case("SELECT \"in\" ($1, $2)", "SELECT \"in\" ($?, $?)");
        case("SELECT int_in ($1, $2)", "SELECT int_in ($?, $?)");
    }

    #[test]
    fn a_nested_in_list_inside_a_subquery_collapses_too() {
        let grant =
            "SELECT * FROM a WHERE a.id IN ($1) AND a.x IN (SELECT x FROM b WHERE b.y = $2)";
        let statement = "SELECT * FROM a WHERE a.id IN ($1, $2, $3) AND a.x IN (SELECT x FROM b WHERE b.y = $4)";
        assert_eq!(normalize(grant), normalize(statement));
        assert_eq!(
            normalize(statement),
            "SELECT * FROM a WHERE a.id IN ($?, ...) AND a.x IN (SELECT x FROM b WHERE b.y = $?)"
        );
        // ... and the inner one collapses on its own.
        case(
            "SELECT * FROM b WHERE b.y IN ($1, $2)",
            "SELECT * FROM b WHERE b.y IN ($?, ...)",
        );
        // Two `IN` lists in one statement, each collapsed once.
        case(
            "WHERE a IN ($1, $2) OR b IN ($3)",
            "WHERE a IN ($?, ...) OR b IN ($?, ...)",
        );
    }

    #[test]
    fn an_in_list_that_holds_a_function_call_is_not_collapsed() {
        // The arity of a call picks the overload, so argument lists are never
        // touched — not even inside an `IN` list.
        case("SELECT f($1, $2)", "SELECT f($?, $?)");
        case("WHERE id IN (f($1, $2))", "WHERE id IN (f($?, $?))");
        case(
            "WHERE id IN (lower($1), lower($2))",
            "WHERE id IN (lower($?), lower($?))",
        );
    }

    //
    // `VALUES` runs.
    //

    #[test]
    fn a_single_values_row_is_left_as_it_is() {
        case("VALUES ($1, $2)", "VALUES ($?, $?)");
        case(
            "INSERT INTO t (a, b) VALUES ($1, $2)",
            "INSERT INTO t (a, b) VALUES ($?, $?)",
        );
        case("VALUES (1)", "VALUES (1)");
        case("INSERT INTO t VALUES (1)", "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn a_run_of_identical_values_rows_becomes_one_row_and_an_ellipsis() {
        case(
            "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
            "INSERT INTO t (a, b) VALUES ($?, $?), ...",
        );
        case(
            "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ($5, $6)",
            "INSERT INTO t (a, b) VALUES ($?, $?), ...",
        );
        case(
            "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ($5, $6), ($7, $8)",
            "INSERT INTO t (a, b) VALUES ($?, $?), ...",
        );
        // The separator keeps the spacing it was written with, like every other
        // part of the text.
        case(
            "INSERT INTO t VALUES (1),(1),(1)",
            "INSERT INTO t VALUES (1),...",
        );
        // Identical literal rows collapse too: they are the same row.
        case("VALUES (1), (1)", "VALUES (1), ...");
        case("VALUES ((1)), ((1))", "VALUES ((1)), ...");
        // A single run of two rows in a `SELECT … VALUES` form.
        case(
            "INSERT INTO t SELECT * FROM (VALUES ($1, $2), ($3, $4)) AS v",
            "INSERT INTO t SELECT * FROM (VALUES ($?, $?), ...) AS v",
        );
    }

    #[test]
    fn a_django_bulk_create_statement_keeps_matching_its_grant() {
        // `bulk_create` sends one row per object; the count varies per request.
        let grant = "INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES ($1, $2), ($3, $4) RETURNING \"n\".\"id\"";
        for rows in [2, 3, 4, 17] {
            let mut placeholders = 1;
            let mut tuples = Vec::new();
            for _ in 0..rows {
                tuples.push(format!("(${placeholders}, ${})", placeholders + 1));
                placeholders += 2;
            }
            let statement = format!(
                "INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES {} RETURNING \"n\".\"id\"",
                tuples.join(", ")
            );
            assert_eq!(normalize(grant), normalize(&statement), "rows = {rows}");
        }
    }

    #[test]
    fn values_rows_that_differ_are_not_collapsed() {
        case(
            "VALUES ($1, $2), ($3, $4, $5)",
            "VALUES ($?, $?), ($?, $?, $?)",
        );
        case("VALUES (1, 2), (3, 4)", "VALUES (1, 2), (3, 4)");
        case("VALUES (1), (2)", "VALUES (1), (2)");
        // The same number of columns is not enough: the text has to match.
        case("VALUES (1, 2), (1, 3)", "VALUES (1, 2), (1, 3)");
        // A run that ends and a different row after it.
        case("VALUES (1), (1), (2)", "VALUES (1), ..., (2)");
        case("VALUES (1), (2), (2)", "VALUES (1), (2), ...");
    }

    #[test]
    fn on_conflict_after_a_collapsed_values_keeps_matching() {
        let grant = "INSERT INTO t (a) VALUES ($1), ($2) ON CONFLICT (a) DO NOTHING";
        let statement =
            "INSERT INTO t (a) VALUES ($1), ($2), ($3), ($4) ON CONFLICT (a) DO NOTHING";
        assert_eq!(normalize(grant), normalize(statement));
        assert_eq!(
            normalize(statement),
            "INSERT INTO t (a) VALUES ($?), ... ON CONFLICT (a) DO NOTHING"
        );
        case(
            "INSERT INTO t (a) VALUES ($1) ON CONFLICT (a) DO NOTHING",
            "INSERT INTO t (a) VALUES ($?) ON CONFLICT (a) DO NOTHING",
        );
        // ... and `RETURNING` after it, as Django writes it.
        case(
            "INSERT INTO \"t\" (\"a\") VALUES ($1, $2), ($3, $4) RETURNING \"t\".\"id\"",
            "INSERT INTO \"t\" (\"a\") VALUES ($?, $?), ... RETURNING \"t\".\"id\"",
        );
    }

    #[test]
    fn a_columns_list_is_not_a_row_and_a_call_is_not_a_row() {
        // The parenthesised list a statement starts with is not after `VALUES`.
        case(
            "INSERT INTO t (a, b) VALUES ($1)",
            "INSERT INTO t (a, b) VALUES ($?)",
        );
        case("SELECT f(1), f(1)", "SELECT f(1), f(1)");
        case("SELECT f(1), f(1) FROM t", "SELECT f(1), f(1) FROM t");
        // `VALUES` as a column or an alias name does not start a run.
        case("SELECT values FROM t", "SELECT values FROM t");
        case("SELECT values (1), (2)", "SELECT values (1), (2)");
    }

    #[test]
    fn a_values_run_never_drops_a_statement_separator() {
        // Normalisation may not change how many statements a text has: the
        // transaction-control grammar counts them.
        for input in [
            "VALUES (1;2), (1;2)",
            "VALUES (1; SELECT 2), (1; SELECT 2)",
            "; VALUES (1), (1)",
        ] {
            let once = normalize(input);
            assert_eq!(
                crate::statements::statement_count_hint(&once),
                crate::statements::statement_count_hint(input),
                "input: {input:?} → {once:?}"
            );
        }
        case("VALUES (1;2), (1;2)", "VALUES (1;2), (1;2)");
    }

    #[test]
    fn a_collapsed_shape_stays_put_when_it_is_normalised_again() {
        for input in [
            "WHERE id IN ($1, $2, $3)",
            "WHERE id IN ($1)",
            "WHERE id IN($1,$2)",
            "WHERE id IN (SELECT x FROM t WHERE y IN ($1))",
            "INSERT INTO t VALUES ($1, $2), ($3, $4)",
            "INSERT INTO t VALUES ($1, $2), ($3, $4), ($5, $6)",
            "INSERT INTO t VALUES (1), (1), (2)",
            "INSERT INTO t VALUES ($1) ON CONFLICT (a) DO NOTHING",
            "SELECT f($1, $2), g($1)",
            "VALUES ($1, $2",
            "WHERE id IN ($1",
            "WHERE id IN (",
            "((((",
            "SELECT ((",
        ] {
            let once = normalize(input);
            assert_eq!(normalize(&once), once, "input: {input:?}");
        }
    }

    #[test]
    fn unterminated_groups_are_left_as_ordinary_text() {
        case("SELECT (1", "SELECT (1");
        case("SELECT (1, 1", "SELECT (1, 1");
        case("VALUES (1), (1", "VALUES (1), (1");
        case("WHERE id IN ($1", "WHERE id IN ($?");
        case("SELECT )", "SELECT )");
        case("SELECT ) )", "SELECT ) )");
        case("SELECT (,", "SELECT (,");
        case("SELECT (,((", "SELECT (,((");
    }

    #[test]
    fn a_values_run_is_only_collapsed_after_the_keyword() {
        // A tuple list somewhere else is left alone.
        case("SELECT (1), (1)", "SELECT (1), (1)");
        case(
            "SELECT x FROM t WHERE (a, b) IN ((1, 1), (1, 1))",
            "SELECT x FROM t WHERE (a, b) IN ((1, 1), (1, 1))",
        );
        // `values` spelled in another case is the same keyword.
        case(
            "INSERT INTO t values ($1), ($2)",
            "INSERT INTO t values ($?), ...",
        );
        case(
            "INSERT INTO t Values ($1), ($2)",
            "INSERT INTO t Values ($?), ...",
        );
    }

    #[test]
    fn a_deeply_nested_text_does_not_exhaust_the_stack() {
        let deep = format!("SELECT {}1{}", "(".repeat(5_000), ")".repeat(5_000));
        let once = normalize(&deep);
        assert_eq!(normalize(&once), once);
        assert!(once.contains("SELECT ("));
        // Unbalanced, and still no panic.
        let open = format!("SELECT {}1", "(".repeat(5_000));
        assert_eq!(normalize(&normalize(&open)), normalize(&open));
    }

    #[test]
    fn a_shape_rule_never_creates_or_hides_a_second_statement() {
        for input in [
            "WHERE id IN ($1, $2); DROP TABLE t",
            "SELECT 1; WHERE id IN ($1, $2)",
            "INSERT INTO t VALUES ($1), ($2); DROP TABLE t",
            "SELECT 'a; b'; WHERE id IN ($1)",
        ] {
            let once = normalize(input);
            assert_eq!(
                crate::statements::statement_count_hint(&once),
                crate::statements::statement_count_hint(input),
                "input: {input:?} → {once:?}"
            );
        }
    }

    //
    // The whole function, over hostile input.
    //

    #[test]
    fn every_normalised_text_is_stable() {
        // A corpus that exercises the two shape rules together with the
        // quoting rules.
        for input in [
            "SELECT 1",
            "SELECT $1",
            "SELECT $1$2",
            "SELECT $$ $1 $$",
            "SELECT a$1",
            "SELECT a$b$c",
            "WHERE a IN ($1)",
            "WHERE a IN ($1, $2)",
            "WHERE a IN($1,$2)",
            "WHERE a IN (SELECT 1)",
            "WHERE a IN ($1, 'x')",
            "WHERE a NOT IN ($1, $2)",
            "WHERE a IN ($1) AND b IN ($2, $3)",
            "VALUES ($1, $2)",
            "VALUES ($1, $2), ($3, $4)",
            "VALUES ($1, $2), ($3, $4), ($5, $6)",
            "INSERT INTO t (a) VALUES (1), (1) ON CONFLICT DO NOTHING",
            "VALUES (1), (1), (2)",
            "VALUES ('a;b'), ('a;b')",
            "CREATE TABLE x (a int)",
            "SELECT (((((1)))))",
            "SELECT (( ))) ",
            "SELECT (((((",
            "SELECT 'a''b', $1 IN ($2, $3)",
            "SELECT $tag$ $1 $tag$ IN ($1)",
            ");",
            "(",
            ")",
            ",",
            "((((",
            "SELECT ) )",
            "VALUES",
            "IN",
            "IN ()",
            "IN ($1,",
            "SELECT 1E$1",
            "SELECT $1E",
        ] {
            let once = normalize(input);
            assert_eq!(normalize(&once), once, "input: {input:?}");
            // Normalisation never changes how many statements a text has.
            assert_eq!(
                crate::statements::statement_count_hint(&once),
                crate::statements::statement_count_hint(input),
                "input: {input:?} → {once:?}"
            );
        }
    }

    #[test]
    fn the_shape_rules_do_not_change_unrelated_text() {
        // A statement without a placeholder, an `IN` list or a `VALUES` run is
        // exactly what the text rules alone make of it.
        for input in [
            "SELECT 1",
            "SET TIME ZONE 'UTC'",
            "SELECT count(*) FROM notes_note",
            "CREATE TABLE x (a int, b text)",
            "SELECT f(1, 2), g(3)",
            "BEGIN",
        ] {
            let normalize_case = normalize(input);
            assert_eq!(normalize_case, input, "input: {input:?}");
        }
    }
}
