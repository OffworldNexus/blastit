//! The `query` template: one PostgreSQL statement, spelled Blastit's way
//! (§13.4; R31–R33, R36).
//!
//! A `Blastfile`'s `query '…'` is matched as a **template** (§6, "Matching"):
//! `$?` is a bind placeholder the app numbers on the wire, `IN ($?, ...)` binds
//! any number of values with one grant, and `($?, $?), ...` stands for any
//! number of identical `VALUES` rows. None of that is SQL, so the text the
//! developer wrote cannot be handed to PostgreSQL as it stands. [`check_template`]
//! asks exactly one question about one text: **is this one parseable statement
//! once the template is expanded?**
//!
//! It is a parse, not a policy (§13.4.1). `analyze`/[`crate::TableGrants`] stays
//! the table-grant gate, [`crate::has_quoted_literal`] stays the literal gate and
//! the redacted `…` stays its own refusal, so a template that is parseable but
//! dangerous — `pg_read_file($?)`, `dblink($?)` — is **legal here**: an exact
//! `query` is trusted as written and refused, when it carries a capability
//! argument, by the literal gate rather than by a second allowlist (ADR-0003).
//!
//! # The expansion (total, deterministic, one pass)
//!
//! The output is the input's characters, in order, with exactly two deletions
//! and the numbering: no re-spacing, no case folding, no comment removal
//! (§13.4.2).
//!
//! | Written (in code, outside quotes and comments) | Expanded |
//! |---|---|
//! | `$?` | the next `$N`, `N = 1, 2, 3, …` in statement order |
//! | `$1` … `$n` | copied unchanged, so numbering may be reused |
//! | `IN ($?, ...)`, `NOT IN ($1, ...)` | `IN ($1)` — the `, ...` is deleted |
//! | `(<row>), ...` after the keyword `VALUES` | `(<row>)` — the `, ...` is deleted |
//!
//! The deleted span starts at the comma and ends at the last dot, so any
//! whitespace or comment inside it goes with it and nothing else does: a code
//! `;` can never be swallowed by a rewrite.
//!
//! # The refusals
//!
//! [`TemplateKind`] names one defect and [`TemplateKind::sentence`] is the fixed
//! sentence the CLI prints, word for word the reference page's table. Every
//! refusal carries a **character** offset into the decoded statement, and the
//! `Syntax` one never renders `pg_query`'s message: that message quotes the token
//! it stopped at (`syntax error at or near "s3cr3t"`), and a `query` may hold a
//! value. Nothing in this module prints, logs or stores the statement.
//!
//! # Bounds
//!
//! [`MAX_TEMPLATE_CHARS`] bounds the decoded statement *before* the rewrite and
//! [`MAX_TEMPLATE_PARAMS`] bounds the `$?` count; both are refusals, never a
//! wrap. The expanded text is reserved as `4 × MAX_TEMPLATE_CHARS + 8` bytes —
//! four bytes per character is the worst case in UTF-8 — so an input under the
//! cap never grows the buffer, and there is no separate output refusal.
//!
//! # The Quick Fix
//!
//! [`parameterize`] is the inverse direction of [`crate::redact_literals`]: every
//! quoted literal becomes `$?` and [`crate::normalize`] then folds `$n` to `$?`,
//! an `IN` list to `IN ($?, ...)` and repeated `VALUES` rows to `(row), ...`, so
//! the result is the canonical grant text. It returns `None` — that is, the
//! editor offers no Quick Fix — for the statements `--learn` itself refuses
//! (`pg_read_file('…')`, `dblink('…')`, `COPY … PROGRAM`), for a statement whose
//! rewrite would not parse, for the two protocol setup statements and for
//! anything over the cap.
//!
//! ```
//! use blastit_sqlpolicy::{TemplateKind, check_template, parameterize};
//!
//! assert!(check_template("SELECT id FROM products WHERE price > $?").is_ok());
//! assert!(check_template("SELECT id FROM products WHERE id IN ($?, ...)").is_ok());
//! assert!(check_template("SELECT id FROM products WHERE id IN ($1, $2, $3)").is_ok());
//!
//! let refused = check_template("SELCT id FROM products").err();
//! assert_eq!(
//!     refused.map(|error| (error.kind(), error.offset())),
//!     Some((TemplateKind::Syntax, 0)),
//! );
//!
//! assert_eq!(
//!     parameterize("SELECT id FROM t WHERE email = 'a@b.com'"),
//!     Some("SELECT id FROM t WHERE email = $?".to_string()),
//! );
//! assert_eq!(parameterize("SELECT id FROM t WHERE email = $?"), None);
//! ```

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fmt::{self, Write as _};

use crate::learn::{LearnVerdict, check_learnable};
use crate::lexer::{RegionKind, code_token_end, is_ident_start, is_space, lex, placeholder_end};
use crate::literals::KNOWN_STATIC_SETUP_SQL;
use crate::normalize::normalize;

/// The longest decoded statement a `query` may hold, in **characters**.
///
/// It is 1/256 of the document cap (`MAX_SOURCE_BYTES`) and about 64× the
/// largest statement this repository ships. A longer one is refused with
/// [`TemplateKind::TooLong`] before the rewrite, so neither the boot nor the
/// editor's per-keystroke parse is unbounded (§4.5, §13.4.2).
pub const MAX_TEMPLATE_CHARS: usize = 65_536;

/// How many `$?` placeholders one `query` may hold.
///
/// The 1025th is [`TemplateKind::TooManyParameters`] at its own `$?`, so the
/// counter is refused rather than wrapped (§13.4.2).
pub const MAX_TEMPLATE_PARAMS: u32 = 1_024;

/// How many distinct `query` entries one `Blastfile` may have validated.
///
/// The count lives in the manifest's `check_queries`, which parses one
/// statement per distinct entry; the 4097th is [`TemplateKind::TooManyQueries`]
/// at that entry's own site (§13.4.2, [`TemplateError::too_many_queries`]).
pub const MAX_TEMPLATE_PARSES: usize = 4_096;

/// What is wrong with one `query` template.
///
/// One entry yields at most one sentence, and these are the reference page's own
/// seven (§13.4.3). The set is closed by that table, but not marked
/// `#[non_exhaustive]`: a caller matches it to decide whether a site is the
/// statement's own (the first six) or the entry's (§18.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateKind {
    /// Nothing outside space, comments and one trailing `;`.
    Empty,
    /// A code `;` that is not the text's last code character.
    MultipleStatements,
    /// `...` outside `IN`/`NOT IN ($?, ...)` and a `VALUES` row list.
    StrayEllipsis,
    /// The expanded text is not one statement `pg_query 6.2.1` parses.
    Syntax,
    /// The decoded statement is longer than [`MAX_TEMPLATE_CHARS`].
    TooLong,
    /// More than [`MAX_TEMPLATE_PARAMS`] `$?` placeholders.
    TooManyParameters,
    /// More than [`MAX_TEMPLATE_PARSES`] distinct `query` entries in one file.
    TooManyQueries,
}

impl TemplateKind {
    /// The one sentence the CLI prints for this defect, word for word.
    ///
    /// It names the fix, names no table, column or value, and is the only text
    /// a refusal may render: `pg_query`'s own message quotes the token it
    /// stopped at, which can be a value, so it is never printed, stored or
    /// logged (§13.4.3, §13.4.4).
    pub fn sentence(&self) -> &'static str {
        match self {
            Self::Empty => "this `query` is empty; write the one statement it grants",
            Self::MultipleStatements => "a `query` is one statement; this one starts a second",
            Self::StrayEllipsis => {
                "`...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row"
            }
            Self::Syntax => {
                "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`"
            }
            Self::TooLong => {
                "a `query` is 65536 characters at most; split it, or grant the tables and columns \
                 instead"
            }
            Self::TooManyParameters => {
                "a `query` holds 1024 bind placeholders at most; grant the tables and columns \
                 instead"
            }
            Self::TooManyQueries => {
                "a `Blastfile` validates 4096 `query` entries at most; fold the shared statements \
                 into a `trait` that many views `use`"
            }
        }
    }
}

impl fmt::Display for TemplateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.sentence())
    }
}

/// One refused `query` template: the defect and where it starts.
///
/// `offset` is a **character** offset into the decoded statement — the text
/// between the quotes, before [`crate::normalize`] — so `0` is the statement's
/// first character and never a byte position. The manifest turns it into a
/// `line:col` with the site it recorded for the string; this crate knows nothing
/// about the file.
///
/// [`Display`](fmt::Display) renders [`TemplateKind::sentence`] and nothing else:
/// no statement text, no `pg_query` message, no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateError {
    kind: TemplateKind,
    offset: u32,
}

impl TemplateError {
    /// Builds a refusal at a character offset.
    fn new(kind: TemplateKind, offset: usize) -> Self {
        Self {
            kind,
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
        }
    }

    /// A refusal for the entry *after* [`MAX_TEMPLATE_PARSES`], built by the
    /// manifest's `check_queries`, whose count this crate does not hold.
    ///
    /// The offset is `0`: the site is the entry's own, not a character inside
    /// its statement (§13.4.3).
    pub fn too_many_queries() -> Self {
        Self::new(TemplateKind::TooManyQueries, 0)
    }

    /// What is wrong.
    pub fn kind(&self) -> TemplateKind {
        self.kind
    }

    /// The character offset into the decoded statement the defect belongs to.
    pub fn offset(&self) -> u32 {
        self.offset
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.sentence())
    }
}

impl std::error::Error for TemplateError {}

/// Checks that `sql` is one statement PostgreSQL parses once the template is
/// expanded.
///
/// `sql` is the **raw decoded** statement — what the developer wrote between the
/// quotes, not [`crate::normalize`]'s output, whose offsets are not the file's.
///
/// The refusals, in this order (one entry, one sentence):
///
/// 1. [`TemplateKind::TooLong`] at `0`, before anything is tokenised;
/// 2. [`TemplateKind::StrayEllipsis`] at the first `.` of the first `...` that is
///    not the last item of an `IN`/`NOT IN ($?, ...)` list or the item after a
///    `VALUES` row;
/// 3. [`TemplateKind::Empty`] at `0` when nothing but space, comments and one
///    trailing `;` is there;
/// 4. [`TemplateKind::MultipleStatements`] at the first code character after the
///    first code `;` that is not the text's last code character;
/// 5. [`TemplateKind::TooManyParameters`] at the 1025th `$?`;
/// 6. [`TemplateKind::Syntax`] at the statement's first non-trivia code
///    character (space, newlines and comments are skipped), with the fixed
///    sentence of [`TemplateKind::Syntax`].
///
/// Rules 2–4 are decided on the token stream **before** any deletion, so a
/// rewrite can never turn a malformed text into a different, parseable one.
///
/// ```
/// use blastit_sqlpolicy::{TemplateKind, check_template};
///
/// assert!(check_template("SELECT id FROM t WHERE id IN ($?, ...)").is_ok());
/// assert!(check_template("SELECT id FROM t WHERE id IN ($?, $?)").is_ok());
/// assert!(check_template("SELECT 1;").is_ok());
///
/// for sql in ["", "  ", ";", "-- nothing"] {
///     assert_eq!(
///         check_template(sql).map_err(|error| error.kind()),
///         Err(TemplateKind::Empty),
///     );
/// }
/// ```
pub fn check_template(sql: &str) -> Result<(), TemplateError> {
    let (expanded, first) = analyse(sql)?;
    let parsed =
        pg_query::parse(&expanded).map_err(|_| TemplateError::new(TemplateKind::Syntax, first))?;
    match parsed.protobuf.stmts.len() {
        1 => Ok(()),
        // Unreachable behind rules 3 and 4, and asserted anyway: `pg_query`'s
        // empty-statement handling is not ours to inherit.
        0 => Err(TemplateError::new(TemplateKind::Empty, 0)),
        _ => Err(TemplateError::new(TemplateKind::MultipleStatements, 0)),
    }
}

/// The text [`check_template`] hands to PostgreSQL, for this crate's own tests.
///
/// `pub(crate)`: the rewrite is one pass, and a test that re-implemented it
/// would prove a lookalike instead of the shipped pass — so the one-statement
/// property of §13.4.2 is asserted in this module's `tests`, where the seam is
/// reachable, and the crate's public surface stays [`check_template`] (§13.5).
pub(crate) fn expand_template(sql: &str) -> Result<String, TemplateError> {
    analyse(sql).map(|(expanded, _)| expanded)
}

/// The Quick Fix's rewrite: the canonical grant text, or `None` when the editor
/// must offer no action.
///
/// Every quoted literal region becomes `$?` — the whole token, prefix included,
/// exactly as [`crate::redact_literals`] replaces it — and [`crate::normalize`]
/// then folds `$n` to `$?`, a placeholder list after `IN` to `IN ($?, ...)` and
/// repeated `VALUES` rows to `(row), ...`. The result is the bytes `--learn`
/// writes and `check` matches.
///
/// `None` means "nothing to offer", never "the statement is fine":
///
/// * the text is longer than [`MAX_TEMPLATE_CHARS`] (checked before any
///   allocation, so the editor may call this on an untrusted buffer);
/// * [`crate::normalize`]'s text is one of the two protocol setup statements,
///   which keep their literals;
/// * there is nothing to change: no quoted literal and no `$n` placeholder;
/// * the statement is one `--learn` itself refuses — `COPY … PROGRAM`,
///   `pg_read_file`, `dblink`, `lo_import` and the rest of
///   [`crate::DANGEROUS_LEARN_FUNCTIONS`], checked on the **expanded** text, so
///   the fix can never turn a capability argument into a `$?` grant;
/// * the result would not re-parse clean, so a paste with a typo or a second
///   statement is offered no edit that swaps one refusal for another.
///
/// ```
/// use blastit_sqlpolicy::parameterize;
///
/// assert_eq!(
///     parameterize("SELECT id FROM t WHERE email = 'a@b.com'"),
///     Some("SELECT id FROM t WHERE email = $?".to_string()),
/// );
/// assert_eq!(
///     parameterize("SELECT id FROM t WHERE id IN ($1, $2)"),
///     Some("SELECT id FROM t WHERE id IN ($?, ...)".to_string()),
/// );
/// assert_eq!(parameterize("SET TIME ZONE 'UTC'"), None);
/// assert_eq!(parameterize("SELECT pg_read_file('/etc/passwd')"), None);
/// ```
pub fn parameterize(sql: &str) -> Option<String> {
    if sql
        .chars()
        .take(MAX_TEMPLATE_CHARS.saturating_add(1))
        .count()
        > MAX_TEMPLATE_CHARS
    {
        return None;
    }

    let chars: Vec<char> = sql.chars().collect();
    let regions = lex(&chars);

    // The two protocol statements keep their literals (ADR-0003): the Quick Fix
    // must not offer to rewrite what the crate deliberately exempts.
    if KNOWN_STATIC_SETUP_SQL.contains(&normalize(sql).as_str()) {
        return None;
    }

    // Nothing to change: a clean `query` has neither a literal nor a `$n`.
    if !regions.iter().any(|region| region.kind.is_literal())
        && !tokenize(&chars)
            .iter()
            .any(|token| token.kind == Kind::Parameter)
    {
        return None;
    }

    let mut substituted = String::with_capacity(sql.len());
    for region in &regions {
        if region.kind.is_literal() {
            substituted.push_str("$?");
        } else if let Some(text) = chars.get(region.start..region.end) {
            substituted.extend(text);
        }
    }
    let candidate = normalize(&substituted);

    // One parse, two gates: `check_learnable` accepts the expanded text only
    // when it parses to exactly one statement (the `check_template` gate) and
    // calls none of the file/network/server capabilities `--learn` refuses.
    let expanded = expand_template(&candidate).ok()?;
    match check_learnable(&expanded) {
        LearnVerdict::Query { .. } => Some(candidate),
        LearnVerdict::Refused { .. } => None,
    }
}

/// The expansion and the offset a `Syntax` refusal is pinned to.
fn analyse(sql: &str) -> Result<(String, usize), TemplateError> {
    // Characters, not bytes (the cap is the page's and every site in the crate
    // counts `char`s), and counted **before** the `Vec<char>` exists: an
    // over-cap statement is refused after one bounded walk and no allocation,
    // so a caller may hand this a whole untrusted buffer. `take` stops the walk
    // at the first character past the cap, so the cost is `MAX + 1` characters
    // whatever the input's length.
    if sql
        .chars()
        .take(MAX_TEMPLATE_CHARS.saturating_add(1))
        .count()
        > MAX_TEMPLATE_CHARS
    {
        return Err(TemplateError::new(TemplateKind::TooLong, 0));
    }
    let chars: Vec<char> = sql.chars().collect();

    let tokens = tokenize(&chars);
    let first = tokens.first().map_or(0, |token| token.start);

    // Rules 6–8 are decided on the token stream, before any deletion.
    let deletions = ellipsis_spans(&chars, &tokens)?;
    if is_empty(&tokens) {
        return Err(TemplateError::new(TemplateKind::Empty, 0));
    }
    if let Some(offset) = second_statement(&tokens) {
        return Err(TemplateError::new(TemplateKind::MultipleStatements, offset));
    }

    let expanded = expand(&chars, &tokens, &deletions)?;
    Ok((expanded, first))
}

/// Whether nothing but space, comments and one trailing `;` is there.
fn is_empty(tokens: &[Token]) -> bool {
    match tokens.split_last() {
        Some((last, rest)) if last.kind == Kind::Semicolon => rest.is_empty(),
        Some(_) => false,
        None => true,
    }
}

/// The character offset of the text after the first code `;` that is not the
/// text's last code character.
fn second_statement(tokens: &[Token]) -> Option<usize> {
    let last = tokens.len().checked_sub(1)?;
    let index = tokens
        .iter()
        .enumerate()
        .find(|(index, token)| token.kind == Kind::Semicolon && *index < last)
        .map(|(index, _)| index)?;
    let next = index.checked_add(1)?;
    tokens.get(next).map(|token| token.start)
}

/// Builds the expanded text: the input's characters in order, with the `, ...`
/// spans deleted and every `$?` numbered.
///
/// The two facts the walk needs are marked on the characters first — deleted, or
/// a placeholder's first character — so the loop's step is the iterator's own
/// index and nothing in it can stand still.
fn expand(chars: &[char], tokens: &[Token], deletions: &[Span]) -> Result<String, TemplateError> {
    let mut marks = vec![0u8; chars.len()];
    for span in deletions {
        if let Some(flags) = marks.get_mut(span.clone()) {
            for flag in flags.iter_mut() {
                *flag |= DELETED;
            }
        }
    }
    for token in tokens.iter().filter(|token| token.kind == Kind::Question) {
        if let Some(flag) = marks.get_mut(token.start) {
            *flag |= QUESTION;
        }
    }

    // Four bytes per character is UTF-8's worst case and the cap bounds the
    // characters, so an input under the cap never grows this buffer.
    let mut out = String::with_capacity(4 * MAX_TEMPLATE_CHARS + 8);
    let mut number: u32 = 0;
    let mut resume = 0usize;

    for (i, &c) in chars.iter().enumerate() {
        // The iterator is the step: `resume` only says how much of the text the
        // placeholder just numbered already covers.
        if i < resume {
            continue;
        }
        let mark = marks.get(i).copied().unwrap_or_default();
        if mark & DELETED != 0 {
            continue;
        }
        if mark & QUESTION != 0 {
            number = number
                .checked_add(1)
                .ok_or_else(|| TemplateError::new(TemplateKind::TooManyParameters, i))?;
            if number > MAX_TEMPLATE_PARAMS {
                return Err(TemplateError::new(TemplateKind::TooManyParameters, i));
            }
            // Writing into a `String` cannot fail.
            let _ = write!(out, "${number}");
            resume = i.saturating_add(2);
            continue;
        }
        out.push(c);
    }

    Ok(out)
}

/// The `, ...` spans the expansion deletes, or the first stray `...`.
fn ellipsis_spans(chars: &[char], tokens: &[Token]) -> Result<Vec<Span>, TemplateError> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut root = Frame::new(0, false);
    let mut spans: Vec<Span> = Vec::new();
    let mut strays: Vec<usize> = Vec::new();

    for token in tokens {
        match token.kind {
            Kind::Open => {
                let in_keyword = current(&mut root, &mut stack).in_keyword_before_open(chars);
                stack.push(Frame::new(token.start, in_keyword));
            }
            Kind::Close => match stack.pop() {
                Some(child) => {
                    match in_list_span(&child) {
                        Some(span) => spans.push(span),
                        // A `...` left in a list that is not `IN ($?, ...)` is
                        // stray, and it is judged once the list has closed.
                        None => strays.extend(dots_of(&child)),
                    }
                    let parent = current(&mut root, &mut stack);
                    let first_row = std::mem::take(&mut parent.after_values);
                    parent.rows = if first_row || parent.rows == Some(RowState::Comma) {
                        Some(RowState::Row)
                    } else {
                        None
                    };
                    parent.items.push(Item {
                        kind: ItemKind::Group,
                        start: child.open,
                        end: token.end,
                    });
                }
                // A `)` with no `(` in front of it is ordinary text.
                None => current(&mut root, &mut stack).push(Item {
                    kind: ItemKind::Other,
                    start: token.start,
                    end: token.end,
                }),
            },
            Kind::Comma => {
                let frame = current(&mut root, &mut stack);
                frame.after_values = false;
                frame.rows = match frame.rows {
                    Some(RowState::Row) => Some(RowState::Comma),
                    _ => None,
                };
                frame.items.push(Item {
                    kind: ItemKind::Comma,
                    start: token.start,
                    end: token.end,
                });
            }
            Kind::Dots => {
                let frame = current(&mut root, &mut stack);
                let exactly_three = token.end.saturating_sub(token.start) == 3;
                let after_row_comma = frame.rows == Some(RowState::Comma)
                    && frame
                        .items
                        .last()
                        .is_some_and(|item| item.kind == ItemKind::Comma);
                if exactly_three && after_row_comma {
                    // `(<row>), ...`: the comma and the marker are deleted for
                    // the parse, and the rows themselves still reach PostgreSQL
                    // at run time.
                    if let Some(comma) = frame.items.last() {
                        spans.push(comma.start..token.end);
                    }
                    frame.items.push(Item {
                        kind: ItemKind::Marker,
                        start: token.start,
                        end: token.end,
                    });
                } else {
                    // Judged when its group closes, or at the end of the text.
                    frame.items.push(Item {
                        kind: ItemKind::Dots,
                        start: token.start,
                        end: token.end,
                    });
                }
                frame.rows = None;
                frame.after_values = false;
            }
            Kind::Word => {
                let frame = current(&mut root, &mut stack);
                let qualified = frame
                    .items
                    .last()
                    .is_some_and(|item| item.kind == ItemKind::Dot);
                frame.after_values =
                    !qualified && slice_is_word(chars, token.start, token.end, "values");
                frame.rows = None;
                frame.items.push(Item {
                    kind: ItemKind::Word,
                    start: token.start,
                    end: token.end,
                });
            }
            _ => {
                let kind = match token.kind {
                    Kind::Question | Kind::Parameter => ItemKind::Placeholder,
                    Kind::Dot => ItemKind::Dot,
                    _ => ItemKind::Other,
                };
                current(&mut root, &mut stack).push(Item {
                    kind,
                    start: token.start,
                    end: token.end,
                });
            }
        }
    }

    // A `(` that never closed cannot hold a legal `...`: the list it would be
    // does not exist, so the marker it holds is stray like any other.
    for frame in &stack {
        strays.extend(dots_of(frame));
    }
    strays.extend(dots_of(&root));

    match strays.iter().copied().min() {
        Some(offset) => Err(TemplateError::new(TemplateKind::StrayEllipsis, offset)),
        None => Ok(spans),
    }
}

/// The span an `IN`/`NOT IN` list deletes, when `frame` is exactly that list.
///
/// The shape is syntactic and minimal: the last item is a `...` and the two
/// before it are one placeholder and one comma. Anything else — a second
/// placeholder, another item after the marker, a qualified `s.in(…)` — is not
/// the page's shape and is refused as [`TemplateKind::StrayEllipsis`].
fn in_list_span(frame: &Frame) -> Option<Span> {
    if !frame.in_keyword {
        return None;
    }
    match frame.items.as_slice() {
        [first, comma, dots]
            if first.kind == ItemKind::Placeholder
                && comma.kind == ItemKind::Comma
                && dots.kind == ItemKind::Dots
                && dots.end.saturating_sub(dots.start) == 3 =>
        {
            Some(comma.start..dots.end)
        }
        _ => None,
    }
}

/// The start of every `...` a frame never placed legally.
fn dots_of(frame: &Frame) -> impl Iterator<Item = usize> + '_ {
    frame
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Dots)
        .map(|item| item.start)
}

/// The frame the next token belongs to: the innermost open group, or the text
/// itself.
fn current<'a>(root: &'a mut Frame, stack: &'a mut [Frame]) -> &'a mut Frame {
    stack.last_mut().unwrap_or(root)
}

/// A half-open range of characters of the decoded statement.
type Span = std::ops::Range<usize>;

/// The mark on a character the expansion deletes.
const DELETED: u8 = 1;

/// The mark on the `$` of a `$?` placeholder.
const QUESTION: u8 = 2;

/// One code token of the decoded statement, with its character offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

/// What a token is. Only the four structural kinds are named; everything else
/// is copied verbatim and matters only as "not a placeholder, not a comma".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `$?`, the one placeholder Blastit numbers.
    Question,
    /// `$1`, `$23`: PostgreSQL's own numbering, copied unchanged.
    Parameter,
    /// A word: a keyword or an identifier.
    Word,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// `,`.
    Comma,
    /// `;`.
    Semicolon,
    /// A run of three or more dots: Blastit's variadic marker.
    Dots,
    /// A run of one or two dots, or the `.` of a qualified name.
    Dot,
    /// A quoted region: a literal or a quoted identifier.
    Quoted,
    /// Anything else: operators, numbers, a lone `$`.
    Other,
}

/// One top-level item of a parenthesised list, in output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Item {
    kind: ItemKind,
    start: usize,
    end: usize,
}

/// What an item of an open parenthesis is, as the two shape rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    /// `$?` or `$N`.
    Placeholder,
    /// `,`.
    Comma,
    /// `...`.
    Dots,
    /// The `, ...` the parse deleted.
    Marker,
    /// A whole parenthesised group.
    Group,
    /// A bare word.
    Word,
    /// A `.`.
    Dot,
    /// Anything else: a quoted region, a separator, an operator, a number.
    Other,
}

/// How far a `VALUES` row list has got in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    /// The last item is a row the list may still repeat.
    Row,
    /// The last item is the comma after that row: a row or `...` may follow.
    Comma,
}

/// One open parenthesis level, while [`ellipsis_spans`] walks the tokens.
struct Frame {
    /// The character offset of the `(` that opened this frame.
    open: usize,
    /// Whether the `(` directly follows the keyword `IN` (`IN (...)`,
    /// `NOT IN (...)`), which is what makes a placeholder list an `IN` list.
    in_keyword: bool,
    /// The frame's top-level items, in order.
    items: Vec<Item>,
    /// Whether the last item was the `VALUES` keyword, so the next group is the
    /// list's first row.
    after_values: bool,
    /// Where the open `VALUES` row list is, if this frame holds one.
    rows: Option<RowState>,
}

impl Frame {
    /// An empty frame whose group was opened at `open`.
    fn new(open: usize, in_keyword: bool) -> Self {
        Self {
            open,
            in_keyword,
            items: Vec::new(),
            after_values: false,
            rows: None,
        }
    }

    /// Whether a `(` opened now would be an `IN`/`NOT IN` list.
    ///
    /// `in` is reserved in PostgreSQL, so a bare `in` in front of a list is the
    /// keyword — except after a `.`, where it names a member of a qualified
    /// name: `s.in(…)` is the function `in` of the schema `s`, and its argument
    /// list is not a list of values.
    fn in_keyword_before_open(&self, chars: &[char]) -> bool {
        let mut items = self.items.iter().rev();
        let Some(word) = items.next() else {
            return false;
        };
        if word.kind != ItemKind::Word || !slice_is_word(chars, word.start, word.end, "in") {
            return false;
        }
        !items.next().is_some_and(|item| item.kind == ItemKind::Dot)
    }

    /// Places one item and ends any `VALUES` row list, as every token but a
    /// comma or a closing group does.
    fn push(&mut self, item: Item) {
        self.after_values = false;
        self.rows = None;
        self.items.push(item);
    }
}

/// Splits the decoded statement into code tokens, with quoted regions opaque and
/// comments dropped.
///
/// Quoted regions come from the shared lexer, so a `$?` or a `;` inside a string,
/// a quoted identifier or a dollar quote is data and never a token: the text the
/// rewrite touches is exactly the text PostgreSQL scans as code.
fn tokenize(chars: &[char]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for region in lex(chars) {
        match region.kind {
            RegionKind::Comment => {}
            RegionKind::Code => {
                let mut resume = region.start;
                for i in region.start..region.end {
                    if i < resume {
                        continue;
                    }
                    // scan.l whitespace is trivia, exactly as it is for
                    // `normalize`: a no-break space is ordinary text and stays.
                    if chars.get(i).is_some_and(|c| is_space(*c)) {
                        continue;
                    }
                    let (kind, end) = code_token(chars, i);
                    let end = end.min(region.end).max(i.saturating_add(1));
                    tokens.push(Token {
                        kind,
                        start: i,
                        end,
                    });
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
            | RegionKind::DollarQuoted => tokens.push(Token {
                kind: Kind::Quoted,
                start: region.start,
                end: region.end,
            }),
        }
    }
    tokens
}

/// The kind and the end of the code token starting at `chars[i]`.
fn code_token(chars: &[char], i: usize) -> (Kind, usize) {
    match chars.get(i) {
        Some('$') if chars.get(i.saturating_add(1)) == Some(&'?') => {
            (Kind::Question, i.saturating_add(2))
        }
        Some('$') => match placeholder_end(chars, i) {
            Some(end) => (Kind::Parameter, end),
            // A bare `$` is PostgreSQL's business: let the parser refuse it.
            None => (Kind::Other, i.saturating_add(1)),
        },
        Some('(') => (Kind::Open, i.saturating_add(1)),
        Some(')') => (Kind::Close, i.saturating_add(1)),
        Some(',') => (Kind::Comma, i.saturating_add(1)),
        Some(';') => (Kind::Semicolon, i.saturating_add(1)),
        Some('.') => {
            // `.5` is one number to PostgreSQL and to the shared lexer, so a dot
            // only starts a run where the numeric scanner would not take it.
            let numeric = code_token_end(chars, i);
            if numeric > i.saturating_add(1) {
                return (Kind::Other, numeric);
            }
            let end = dots_end(chars, i);
            if end.saturating_sub(i) >= 3 {
                (Kind::Dots, end)
            } else {
                (Kind::Dot, end)
            }
        }
        _ => {
            let end = code_token_end(chars, i);
            let kind = match chars.get(i) {
                Some(c) if is_ident_start(*c) => Kind::Word,
                _ => Kind::Other,
            };
            (kind, end)
        }
    }
}

/// The end of the run of `.` characters starting at `chars[i]`.
fn dots_end(chars: &[char], i: usize) -> usize {
    let rest = chars.get(i..).unwrap_or_default();
    let run = rest.iter().take_while(|c| **c == '.').count();
    i.saturating_add(run)
}

/// Whether `chars[start..end]` spells `word`, ignoring ASCII case.
fn slice_is_word(chars: &[char], start: usize, end: usize, word: &str) -> bool {
    let Some(text) = chars.get(start..end) else {
        return false;
    };
    text.len() == word.chars().count()
        && text
            .iter()
            .zip(word.chars())
            .all(|(c, expected)| c.eq_ignore_ascii_case(&expected))
}
