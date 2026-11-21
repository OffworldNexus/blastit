//! Statement counting and the transaction-control grammar (ARCHITECTURE.md
//! §5.2, §6).
//!
//! Neither is a SQL parser. The counter splits on top-level `;` only, using the
//! same lexer as [`crate::normalize`], and the grammar accepts exactly the
//! transaction-control statements a grant may enable with
//! `transaction_control = true`, and nothing else.

use crate::lexer::{RegionKind, is_space, lex};

/// How many statements `sql` contains, counting only segments that hold
/// something other than whitespace and comments.
///
/// Splits on `;` in code: never inside single-quoted strings, E-strings, bit or
/// hex strings, double-quoted identifiers, dollar-quoted strings or comments
/// (`--`, ended by `\n` or `\r`, and nested `/* */`). Used to make sure a
/// transaction-control statement is the *only* statement in the text, so that a
/// separator hidden in a string or a comment cannot smuggle a second statement
/// past the policy.
pub(crate) fn statement_count_hint(sql: &str) -> usize {
    let chars: Vec<char> = sql.chars().collect();
    let mut count = 0;
    let mut has_content = false;

    for region in lex(&chars) {
        match region.kind {
            RegionKind::Comment => {}
            RegionKind::Code => {
                for c in &chars[region.start..region.end] {
                    if *c == ';' {
                        if has_content {
                            count += 1;
                            has_content = false;
                        }
                    } else if !is_space(*c) {
                        has_content = true;
                    }
                }
            }
            RegionKind::SingleQuoted
            | RegionKind::EString
            | RegionKind::BitString
            | RegionKind::HexString
            | RegionKind::NationalString
            | RegionKind::UnicodeString
            | RegionKind::QuotedIdent
            | RegionKind::DollarQuoted => has_content = true,
        }
    }

    if has_content {
        count += 1;
    }
    count
}

/// Whether the already-normalised text `normalized` is a single
/// transaction-control statement: `BEGIN`, `START TRANSACTION`, `COMMIT`,
/// `END`, `ROLLBACK`, `SAVEPOINT <ident>`, `RELEASE [SAVEPOINT] <ident>` or
/// `ROLLBACK [WORK|TRANSACTION] TO [SAVEPOINT] <ident>`.
///
/// The caller passes text from [`crate::normalize`], so comments are gone and a
/// trailing `;` is irrelevant. Keywords are case-insensitive; identifiers are
/// plain `[A-Za-z_][A-Za-z0-9_]*` or double-quoted. `BEGIN`, `COMMIT`, `END`
/// and `ROLLBACK` take an optional `WORK`/`TRANSACTION` suffix, and `BEGIN` and
/// `START TRANSACTION` take an optional transaction mode list
/// (`ISOLATION LEVEL …`, `READ ONLY`, `READ WRITE`, separated by whitespace or
/// commas). Only scan.l's `space` separates tokens, so `BEGIN` followed by a
/// no-break space is not this grammar's `BEGIN` — PostgreSQL does not read it
/// as one either.
///
/// A text holding more than one statement never qualifies, however harmless
/// each statement looks on its own.
pub(crate) fn is_transaction_control(normalized: &str) -> bool {
    if statement_count_hint(normalized) != 1 {
        return false;
    }
    let mut parser = Parser::new(normalized);
    parser.parse_statement() && parser.at_end()
}

/// A cursor over one statement's characters.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(sql: &str) -> Self {
        Self {
            chars: sql.chars().collect(),
            pos: 0,
        }
    }

    /// The characters from the cursor on.
    fn rest(&self) -> &[char] {
        &self.chars[self.pos..]
    }

    /// Skips scan.l whitespace.
    ///
    /// The count comes from `take_while`, not from a hand-stepped index: there
    /// is no loop here whose only exit is the cursor moving, so a step that a
    /// later edit (or a mutation testing exactly that step) turns into a no-op
    /// can only make the count wrong, never make the parser spin.
    fn skip_ws(&mut self) {
        let skipped = self.rest().iter().take_while(|c| is_space(**c)).count();
        self.pos += skipped;
    }

    /// Skips whitespace and reports whether the cursor reached the end.
    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.pos >= self.chars.len()
    }

    /// Consumes a bare identifier (`[A-Za-z_][A-Za-z0-9_]*`) and returns it.
    ///
    /// The run is collected by `take_while` and its length taken from the
    /// result, so no cursor step has to terminate a loop.
    fn eat_word(&mut self) -> Option<String> {
        self.skip_ws();
        let first = *self.rest().first()?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }
        let word: String = self
            .rest()
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .collect();
        self.pos += word.chars().count();
        Some(word)
    }

    /// Consumes `keyword` (case-insensitively) if it comes next.
    fn eat_keyword(&mut self, keyword: &str) -> bool {
        let saved = self.pos;
        match self.eat_word() {
            Some(word) if word.eq_ignore_ascii_case(keyword) => true,
            _ => {
                self.pos = saved;
                false
            }
        }
    }

    /// Consumes a savepoint identifier: a plain word or a double-quoted
    /// identifier (`"s1_x"`, with `""` as an escaped quote).
    fn eat_ident(&mut self) -> bool {
        self.skip_ws();
        if self.chars.get(self.pos) == Some(&'"') {
            return match quoted_ident_end(&self.chars, self.pos) {
                Some(end) => {
                    self.pos = end;
                    true
                }
                None => false,
            };
        }
        self.eat_word().is_some()
    }

    /// Consumes an optional `WORK` or `TRANSACTION` suffix.
    fn eat_scope(&mut self) {
        if !self.eat_keyword("WORK") {
            let _ = self.eat_keyword("TRANSACTION");
        }
    }

    /// Parses one whole transaction-control statement, leaving the cursor after
    /// it. Returns `false` as soon as the text is not one.
    fn parse_statement(&mut self) -> bool {
        let Some(word) = self.eat_word() else {
            return false;
        };
        match word.to_ascii_uppercase().as_str() {
            "BEGIN" => {
                self.eat_scope();
                self.parse_modes()
            }
            "START" => self.eat_keyword("TRANSACTION") && self.parse_modes(),
            "COMMIT" | "END" => {
                self.eat_scope();
                self.at_end()
            }
            "ROLLBACK" => {
                self.eat_scope();
                if self.at_end() {
                    return true;
                }
                if !self.eat_keyword("TO") {
                    return false;
                }
                let _ = self.eat_keyword("SAVEPOINT");
                self.eat_ident() && self.at_end()
            }
            "SAVEPOINT" => self.eat_ident() && self.at_end(),
            "RELEASE" => {
                let _ = self.eat_keyword("SAVEPOINT");
                self.eat_ident() && self.at_end()
            }
            _ => false,
        }
    }

    /// Parses the optional transaction mode list of `BEGIN`/`START TRANSACTION`
    /// (`ISOLATION LEVEL …`, `READ ONLY`, `READ WRITE`), which may be separated
    /// by whitespace or commas. An empty list is valid.
    fn parse_modes(&mut self) -> bool {
        loop {
            if self.at_end() {
                return true;
            }
            let before = self.pos;
            if !self.parse_mode() {
                return false;
            }
            // Every mode starts with a keyword, so a successful `parse_mode`
            // always moves the cursor. A mode that reports success without
            // consuming anything would make this loop spin forever; refusing it
            // makes that a denial instead of a hang.
            if self.pos == before {
                return false;
            }
            self.skip_ws();
            if self.chars.get(self.pos) == Some(&',') {
                self.pos += 1;
            }
        }
    }

    /// Parses one transaction mode.
    fn parse_mode(&mut self) -> bool {
        if self.eat_keyword("ISOLATION") {
            if !self.eat_keyword("LEVEL") {
                return false;
            }
            if self.eat_keyword("SERIALIZABLE") {
                return true;
            }
            if self.eat_keyword("REPEATABLE") {
                return self.eat_keyword("READ");
            }
            if self.eat_keyword("READ") {
                return self.eat_keyword("COMMITTED") || self.eat_keyword("UNCOMMITTED");
            }
            return false;
        }
        if self.eat_keyword("READ") {
            return self.eat_keyword("ONLY") || self.eat_keyword("WRITE");
        }
        false
    }
}

/// The index just past the closing quote of the double-quoted identifier whose
/// opening quote is at `start`, or `None` when the text ends inside it or the
/// identifier is empty (`""` is not a valid identifier).
///
/// The body is walked with an iterator, so the scan ends when the characters
/// do — never because a hand-stepped index happens to move. `""` is one escaped
/// quote, and an identifier of nothing but escaped quotes is a non-empty one.
fn quoted_ident_end(chars: &[char], start: usize) -> Option<usize> {
    let mut body = chars[start + 1..].iter();
    let mut width = 0usize;
    let mut consumed = 0usize;
    while let Some(c) = body.next() {
        consumed += 1;
        if *c != '"' {
            width += 1;
            continue;
        }
        if body.as_slice().first() == Some(&'"') {
            let _ = body.next();
            consumed += 1;
            width += 1;
            continue;
        }
        if width == 0 {
            return None;
        }
        return Some(start + 1 + consumed);
    }
    None
}
