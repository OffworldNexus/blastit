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
//!
//! A keyword is only read as one where a keyword can be: `in` is reserved in
//! PostgreSQL, so a bare `in` in front of a list is the `IN` keyword, while
//! `s.in(…)` and `s.values(…)` are qualified names whose parentheses are an
//! argument list. Apart from the inside of an `IN` list that collapses — whose
//! whole point is that any list of the same shape matches — the pass only
//! deletes what it replaces and never re-spaces what it leaves alone: the
//! whitespace in front of a `(` or a `)` stays where it was written.
//!
//! Neither rule makes the pass cost more than the text it reads: it walks the
//! text once, keeps one frame per open parenthesis, and writes the result once,
//! however deeply the statement nests.

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
///   subquery, a nested tuple) is left alone, and so is a qualified name that
///   merely looks like one — `s.in(…)` is the function `in` of schema `s`, whose
///   argument list is an argument list;
/// * after `VALUES`, a run of two or more *identical* rows `(…), (…), …`
///   becomes the single row followed by `, ...`, because `bulk_create` sends a
///   varying number of identical rows. A single row is left as it is, rows that
///   differ from each other do not collapse, and neither a function call's
///   argument list nor a qualified `s.values(…)` is ever read as a row list —
///   their arity picks the overload;
/// * quoted regions are kept verbatim: single-quoted strings (`'it''s'`),
///   E-strings (`E'a\'b  c'`, where a backslash escapes the next character),
///   bit and hex strings (`B'1010'`, `X'1F'`, with no escape at all), national
///   strings (`N'it''s'`), Unicode escape strings (`U&'\0041'`), double-quoted
///   identifiers (`"a  b"`) and dollar-quoted strings (`$$…$$`,
///   `$tag$…$tag$`);
/// * `a$b$c` is one identifier and does not open a dollar quote. A bare `$` is
///   ordinary text;
/// * an unterminated quote is kept verbatim to the end of the text; the text is
///   trimmed before scanning, so trailing whitespace is gone even inside an
///   unterminated quote. This function never panics;
/// * nothing else changes: the whitespace in front of a `(` or a `)` is kept,
///   and the pass costs one walk over the text whatever it contains. Only the
///   contents of an `IN` list that collapses are rewritten as a whole, because
///   the point of that rule is that any list of the same shape matches.
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
                // The scan visits every character of the region in order, so
                // the *step* is the position the iterator reports and never a
                // counter this loop adds to: a step that fails to advance is
                // the one way the pass could stop terminating, and a counter
                // increment is exactly the arithmetic a mutation can leave
                // standing (MB.1.4). `resume` only says how many of the
                // characters the iterator is about to reach are already in
                // `out`, because one token can be several characters long
                // (`a$1`, `f(`, `::`).
                let mut resume = 0;
                for (i, &c) in text.iter().enumerate() {
                    if i < resume {
                        continue;
                    }
                    if is_space(c) {
                        pending_space = true;
                        continue;
                    }
                    out.flush_space(&mut pending_space);
                    if let Some(end) = placeholder_end(text, i) {
                        // `$?` is stable: it holds no digits, so normalising
                        // the result again changes nothing.
                        out.push('$', true);
                        out.push('?', true);
                        resume = end;
                        continue;
                    }
                    // One whole code token, copied verbatim: `a$1` is a single
                    // identifier and keeps its `$`.
                    let end = code_token_end(text, i);
                    for c in &text[i..end] {
                        out.push(*c, true);
                    }
                    resume = end;
                }
            }
            RegionKind::SingleQuoted
            | RegionKind::EString
            | RegionKind::BitString
            | RegionKind::HexString
            | RegionKind::NationalString
            | RegionKind::UnicodeString
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

/// Rewrites the *observation* spelling of a statement back into one PostgreSQL
/// parses, of the same shape.
///
/// [`normalize`] writes every bind placeholder as `$?` and
/// [`crate::redact_literals`] replaces every quoted literal by `…`. Both keep a
/// statement safe to log and to paste into a manifest, and neither is valid SQL:
/// the table-grant analysis ([`crate::analyze`]) parses the text it is handed.
/// This function gives back a statement of the same shape that a parser accepts,
/// so an observed statement can be analysed against a grant:
///
/// * every `$?` in code becomes `$1` — a parameter's number says nothing about
///   which tables, columns and functions a statement touches, so the analysis
///   needs the placeholder token, not its ordinal;
/// * every `…` in code becomes `''` — the empty literal that stands where the
///   redacted one was, which keeps the access set identical without ever
///   copying a value;
/// * a shape tail — normalize's `, ...` after a placeholder list (`IN ($?, ...)`)
///   or after a row of `VALUES` — becomes one more element or one more row, so
///   the text still parses and the analysis sees the same tables and columns;
/// * comments and quoted regions are copied verbatim, through the one
///   `crate::lexer`, so a `$?` inside a comment, a string, a quoted identifier
///   or a dollar quote is data and is never rewritten.
///
/// The result is only for analysis: it is never compared with a grant (an exact
/// match runs on [`normalize`]'s spelling), never logged and never written to a
/// manifest.
///
/// ```
/// # use blastit_sqlpolicy::restore_parameters;
/// assert_eq!(
///     restore_parameters(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#),
///     r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $1"#
/// );
/// assert_eq!(restore_parameters("SELECT $?, … FROM t"), "SELECT $1, '' FROM t");
/// assert_eq!(
///     restore_parameters("WHERE id IN ($?, ...)"),
///     "WHERE id IN ($1, $1)"
/// );
/// assert_eq!(
///     restore_parameters("INSERT INTO t (a) VALUES ($?), ..."),
///     "INSERT INTO t (a) VALUES ($1), ($1)"
/// );
/// assert_eq!(
///     restore_parameters(r#"SELECT "a$?" FROM "t" -- $?"#),
///     r#"SELECT "a$?" FROM "t" -- $?"#
/// );
/// ```
pub fn restore_parameters(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    for region in lex(&chars) {
        let text = &chars[region.start..region.end];
        if region.kind != RegionKind::Code {
            out.extend(text);
            continue;
        }
        let mut code: Vec<char> = Vec::with_capacity(text.len());
        let mut at = 0;
        while let Some(&character) = text.get(at) {
            if character == '$' && text.get(at + 1) == Some(&'?') {
                code.push('$');
                code.push('1');
                at += 2;
                continue;
            }
            if character == '\u{2026}' {
                code.push('\'');
                code.push('\'');
                at += 1;
                continue;
            }
            code.push(character);
            at += 1;
        }
        out.extend(expand_tails(&code));
    }
    out
}

/// Expands normalize's shape tails into one more element or row.
///
/// `normalize` writes a varying `IN` list as `IN ($?, ...)` and a varying run of
/// identical `VALUES` rows as `($?, $?), ...`. Neither parses, and both are what
/// a Django `__in`, `prefetch_related` or `bulk_create` sends every request, so
/// a table grant would otherwise deny them (`MB.40.2`). The tail is `, ...` in
/// code: after a placeholder it gains one more placeholder, and after a `)` it
/// repeats the group that closes there. Any other `...` is left as it was.
fn expand_tails(code: &[char]) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(code.len() + 8);
    let mut at = 0;
    while at < code.len() {
        if code[at] == '.'
            && code.get(at + 1) == Some(&'.')
            && code.get(at + 2) == Some(&'.')
            && let Some(repetition) = tail_repetition(&mut out)
        {
            out.push(' ');
            out.extend(repetition);
            at += 3;
            continue;
        }
        out.push(code[at]);
        at += 1;
    }
    out
}

/// The element a `, ...` tail repeats, trimming the tail off `out` first.
///
/// `None` when what precedes the ellipsis is not a shape tail; `out` is then
/// left exactly as it was, whitespace included.
fn tail_repetition(out: &mut Vec<char>) -> Option<Vec<char>> {
    let mut length = out.len();
    while length > 0 && matches!(out[length - 1], ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') {
        length -= 1;
    }
    let comma = length.checked_sub(1)?;
    if out.get(comma) != Some(&',') {
        return None;
    }
    let last = comma.checked_sub(1)?;
    let repetition = match out.get(last) {
        // `VALUES ($?, $?), ...`: one more row, so the list still parses.
        Some(&')') => {
            let start = matching_open(&out[..=last])?;
            out[start..=last].to_vec()
        }
        // `IN ($?, ...)`: one more placeholder; a parameter names no column.
        Some(&'1') if out.get(last.checked_sub(1)?) == Some(&'$') => vec!['$', '1'],
        _ => return None,
    };
    out.truncate(length);
    Some(repetition)
}

/// The `(` matching the `)` that ends `text`.
fn matching_open(text: &[char]) -> Option<usize> {
    let mut depth = 0usize;
    for (at, character) in text.iter().enumerate().rev() {
        match character {
            ')' => depth += 1,
            '(' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The shape rules, applied to the already-normalised text: `IN` lists of
/// placeholders and `VALUES` row runs, as the module documentation describes.
///
/// The text is built once, as a flat list of [`Piece`]s in output order, and a
/// rule that rewrites a group *deletes* the pieces it replaces instead of
/// copying them anywhere, so the pass is linear in the length of the text
/// however deeply the parentheses nest: `SELECT ` followed by a million `(`
/// and a million `)` is one pass over two million pieces, not a million
/// rebuilds of the text inside them.
///
/// The pieces are walked with one frame per open parenthesis and the frames
/// live on a `Vec`, so the pass needs no recursion either: a hostile statement
/// cannot exhaust the stack, and it cannot stall `dbproxy` for long enough to
/// matter. Parentheses that never close are left as ordinary text, exactly as
/// the lexer leaves an unterminated quote alone.
fn reshape(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut outer = Frame::new(0);
    let mut stack: Vec<Frame> = Vec::new();

    for token in tokenize(&chars) {
        match token.kind {
            Kind::Comma => {
                let frame = current(&mut outer, &mut stack);
                push_piece(&mut pieces, frame, Kind::Comma, token.text);
            }
            Kind::Open => {
                let start = pieces.len();
                pieces.push(Piece::new(Kind::Other, token.text));
                stack.push(Frame::new(start));
            }
            Kind::Close if !stack.is_empty() => {
                if let Some(child) = stack.pop() {
                    // The `)` keeps the whitespace in front of it: the shape
                    // rules never touch the whitespace of a text they leave
                    // alone.
                    pieces.push(Piece::new(Kind::Other, token.text));
                    let parent = current(&mut outer, &mut stack);
                    close_group(&mut pieces, parent, child, true);
                }
            }
            // A `)` with no `(` in front of it is ordinary text.
            _ => {
                let frame = current(&mut outer, &mut stack);
                push_token(&mut pieces, frame, token);
            }
        }
    }

    // An unterminated group runs to the end of the text.
    while let Some(child) = stack.pop() {
        let parent = current(&mut outer, &mut stack);
        close_group(&mut pieces, parent, child, false);
    }

    let mut text = String::with_capacity(sql.len());
    for piece in &pieces {
        text.push_str(&piece.text);
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

/// One piece of the text being built, with the whitespace that precedes it.
///
/// The pieces render in order, one after the other: a rule that rewrites a
/// group deletes the pieces it replaces and writes its own, so no text is ever
/// copied from a group to its parent.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    /// What the piece is. An item is read through the kind of its first piece,
    /// so this is the token's kind for a token, and [`Kind::Other`] for
    /// punctuation and for text a rule wrote.
    kind: Kind,
    /// The text, with the whitespace that precedes it.
    text: String,
}

impl Piece {
    /// A piece of kind `kind` that stands for `text`.
    fn new(kind: Kind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }

    /// The piece's text without the whitespace in front of it.
    fn word(&self) -> &str {
        leading_word(&self.text)
    }
}

/// One top-level item of an open parenthesis: a token, or a whole group.
///
/// Items are contiguous ranges of [`Piece`]s, in output order, which is what
/// lets a rule replace one by truncating the pieces and writing its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    /// Index of the item's first piece.
    start: usize,
    /// Index just past the item's last piece. An item is never empty.
    end: usize,
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
                // The same shape as the scan in `normalize`: the iterator is
                // the step, `resume` is only the end of the token just taken,
                // so a token that spans several characters cannot keep the
                // loop on one position (MB.1.4).
                let mut resume = 0;
                for (i, &c) in text.iter().enumerate() {
                    if i < resume {
                        continue;
                    }
                    if is_space(c) {
                        pending_space = true;
                        continue;
                    }
                    let (kind, end) = code_token(text, i);
                    tokens.push(Token::new(kind, &text[i..end], &mut pending_space));
                    resume = end;
                }
            }
            RegionKind::SingleQuoted
            | RegionKind::EString
            | RegionKind::BitString
            | RegionKind::HexString
            | RegionKind::NationalString
            | RegionKind::UnicodeString
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
        leading_word(&self.text)
    }
}

/// One open parenthesis level, while [`reshape`] walks the tokens.
struct Frame {
    /// Index in the pieces of the `(` that opened this group: where a rewrite of
    /// the whole group starts truncating.
    open: usize,
    /// The group's top-level items, in order.
    items: Vec<Item>,
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
}

impl Frame {
    /// An empty frame whose group was opened by the piece at `open`.
    fn new(open: usize) -> Self {
        Self {
            open,
            items: Vec::new(),
            rows: None,
            after_values: false,
            separator: false,
        }
    }
}

/// The rows after a `VALUES` keyword, as [`reshape`] sees them.
///
/// Only the last row is remembered, because only a row that repeats it can
/// join it: two rows that are identical collapse into one, and the rows that
/// follow them may still repeat the last one in their turn.
struct Rows {
    /// The row as it is written out: a row only joins the list when its own
    /// text is this one.
    row: String,
    /// The pieces of the list's last row, or of the `...` that stands for the
    /// rows repeating it. A comma continues the list only right after this.
    tail: Item,
    /// Whether the list's last row has already been written as `row, ...`.
    collapsed: bool,
}

/// Places one piece at the end of the text and records it as an item of
/// `frame`.
fn push_piece(pieces: &mut Vec<Piece>, frame: &mut Frame, kind: Kind, text: String) {
    let item = Item {
        start: pieces.len(),
        end: pieces.len() + 1,
    };
    pieces.push(Piece::new(kind, text));
    frame.items.push(item);
}

/// Places one ordinary token: a word, a placeholder, an operator, a stray `)`.
fn push_token(pieces: &mut Vec<Piece>, frame: &mut Frame, token: Token) {
    // `values` is the keyword only where it is not part of a qualified name:
    // `SELECT s.values(1), (2)` calls the function `values` of schema `s`, and
    // its argument list must not be read as a list of rows.
    frame.after_values = token.kind == Kind::Word
        && token.word().eq_ignore_ascii_case("values")
        && !is_dot(pieces, frame.items.last());
    // Anything but another row ends the `VALUES` row list.
    frame.rows = None;
    if token.kind == Kind::Other && token.word() == ";" {
        frame.separator = true;
    }
    push_piece(pieces, frame, token.kind, token.text);
}

/// Places a group in its parent frame, applying the shape rules.
///
/// `closed` is false for a `(` that never closed; such a group is ordinary
/// text, because the text after it is not the group the rules would have seen.
fn close_group(pieces: &mut Vec<Piece>, parent: &mut Frame, child: Frame, closed: bool) {
    let group = Item {
        start: child.open,
        end: pieces.len(),
    };
    parent.separator |= child.separator;

    if !closed {
        parent.after_values = false;
        parent.rows = None;
        parent.items.push(group);
        return;
    }

    // The row right after `VALUES`: it opens the row list.
    if parent.after_values {
        parent.after_values = false;
        parent.items.push(group);
        parent.rows = Some(Rows {
            row: group_text(pieces, group),
            tail: group,
            collapsed: false,
        });
        return;
    }

    match joined_row(pieces, parent, group, child.separator) {
        // The row repeats the list's last row: the two are written as
        // `row, ...`, and every row after them simply joins the `...`.
        Joined::Repeat(comma) => {
            let marker = format!(
                "{}{}...",
                pieces[comma.start].text,
                group_lead(&pieces[group.start].text)
            );
            pieces.truncate(comma.start);
            pieces.push(Piece::new(Kind::Other, marker));
            let marker = Item {
                start: comma.start,
                end: pieces.len(),
            };
            parent.items.pop();
            parent.items.push(marker);
            if let Some(rows) = parent.rows.as_mut() {
                rows.tail = marker;
                rows.collapsed = true;
            }
            return;
        }
        // ... and the list is already written as `row, ...`: the comma and the
        // row go away with it.
        Joined::RepeatCollapsed(comma) => {
            pieces.truncate(comma.start);
            parent.items.pop();
            return;
        }
        // A different row: it becomes the list's new last row, and a later
        // repeat of *it* collapses in its own turn.
        Joined::Other => {
            parent.items.push(group);
            let row = group_text(pieces, group);
            if let Some(rows) = parent.rows.as_mut() {
                rows.row = row;
                rows.tail = group;
                rows.collapsed = false;
            }
            return;
        }
        Joined::None => {}
    }

    // A list of placeholders right after the keyword `IN` or `NOT IN`, whatever
    // its length: `prefetch_related` and `__in` grow it per request.
    if let Some((item, replacement)) = in_list(pieces, parent, &child.items) {
        pieces.truncate(item.start);
        pieces.push(Piece::new(Kind::Other, replacement));
        parent.items.pop();
        parent.items.push(Item {
            start: item.start,
            end: pieces.len(),
        });
        return;
    }

    // Anything else: the group as written.
    parent.after_values = false;
    parent.rows = None;
    parent.items.push(group);
}

/// What an open `VALUES` row list makes of a row that has just closed.
enum Joined {
    /// Not a row of any open list.
    None,
    /// The row repeats the list's last row, which is still written out in full.
    Repeat(Item),
    /// The row repeats the list's last row, already written as `row, ...`.
    RepeatCollapsed(Item),
    /// The row differs from the list's last one, so it becomes the new last row.
    Other,
}

/// How the open row list of `parent` reads the row that has just closed.
fn joined_row(pieces: &[Piece], parent: &Frame, group: Item, separator: bool) -> Joined {
    let Some(rows) = parent.rows.as_ref() else {
        return Joined::None;
    };
    let Some(comma) = trailing_comma(pieces, parent, rows) else {
        return Joined::None;
    };
    if separator || group_text(pieces, group) != rows.row {
        return Joined::Other;
    }
    if rows.collapsed {
        Joined::RepeatCollapsed(comma)
    } else {
        Joined::Repeat(comma)
    }
}

/// The `IN` keyword and the canonical list that replaces it, when the group
/// that has just closed is a list of placeholders right after `IN`/`NOT IN`.
///
/// `in` is a reserved word in PostgreSQL, so a bare `in` in front of a
/// parenthesised list is the keyword — except after a `.`, where it names a
/// member of a qualified name: `s.in(…)` is the function `in` of the schema
/// `s`, and its argument list is not a list of values. (`"s"."in"(…)` is
/// spelled with quoted identifiers, which are never bare words, so it could
/// not match in the first place.)
fn in_list(pieces: &[Piece], parent: &Frame, items: &[Item]) -> Option<(Item, String)> {
    if !is_placeholder_list(pieces, items) || is_dot(pieces, before_last(&parent.items)) {
        return None;
    }
    let (item, word) = trailing_word(pieces, parent)?;
    if !word.eq_ignore_ascii_case("in") {
        return None;
    }
    let lead = leading_space(&pieces[item.start].text);
    Some((item, format!("{lead}{word} ($?, ...)")))
}

/// The last item of `parent`, when it is a bare word.
fn trailing_word<'a>(pieces: &'a [Piece], parent: &Frame) -> Option<(Item, &'a str)> {
    let item = *parent.items.last()?;
    let piece = pieces.get(item.start)?;
    (item.end == item.start + 1 && piece.kind == Kind::Word).then(|| (item, piece.word()))
}

/// The comma right in front of the row that has just closed, when it also
/// follows the row list's last row: `VALUES (…), x, (…)` is not a list of rows.
fn trailing_comma(pieces: &[Piece], parent: &Frame, rows: &Rows) -> Option<Item> {
    let comma = *parent.items.last()?;
    if kind_of(pieces, comma) != Kind::Comma {
        return None;
    }
    (before_last(&parent.items) == Some(&rows.tail)).then_some(comma)
}

/// The item before the last one, when there is one.
fn before_last(items: &[Item]) -> Option<&Item> {
    items
        .len()
        .checked_sub(2)
        .and_then(|index| items.get(index))
}

/// Whether an item is a `.`, which makes the word after it part of a qualified
/// name rather than a keyword.
fn is_dot(pieces: &[Piece], item: Option<&Item>) -> bool {
    item.and_then(|item| pieces.get(item.start))
        .is_some_and(|piece| piece.kind == Kind::Other && piece.word() == ".")
}

/// The kind of an item: the kind of its first piece, because a group starts
/// with its `(` and a token is one piece.
fn kind_of(pieces: &[Piece], item: Item) -> Kind {
    pieces
        .get(item.start)
        .map_or(Kind::Other, |piece| piece.kind)
}

/// The text of a group the row rules compare: the `(` and what is inside it,
/// without the whitespace in front of the `(`.
///
/// Two rows of a `VALUES` list are the same row even when the first one carries
/// the space that separates it from the keyword and the next one follows a
/// comma instead: `VALUES (…), (…)` is one row repeated. The whitespace itself
/// is not lost — it stays where it was written, in the first row and in the
/// marker of a collapsed one.
fn group_text(pieces: &[Piece], group: Item) -> String {
    let mut text = String::new();
    for (index, piece) in pieces
        .get(group.start..group.end)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if index == 0 {
            text.push_str(piece.word());
        } else {
            text.push_str(&piece.text);
        }
    }
    text
}

/// The whitespace in front of a group's `(`, which the marker of a collapsed
/// row keeps.
fn group_lead(text: &str) -> &str {
    text.strip_suffix('(').unwrap_or(text)
}

/// Whether a group holds placeholders separated by commas and nothing else:
/// `($?)`, `($?, $?)`, … A trailing comma, anything that is not a placeholder,
/// and an empty list all fail.
fn is_placeholder_list(pieces: &[Piece], items: &[Item]) -> bool {
    items
        .last()
        .is_some_and(|item| kind_of(pieces, *item) == Kind::Placeholder)
        && items.iter().enumerate().all(|(index, item)| {
            kind_of(pieces, *item)
                == if index % 2 == 0 {
                    Kind::Placeholder
                } else {
                    Kind::Comma
                }
        })
}

/// `text` without the whitespace in front of it.
fn leading_word(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

/// The whitespace in front of `text`.
fn leading_space(text: &str) -> &str {
    let rest = leading_word(text);
    &text[..text.len() - rest.len()]
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
