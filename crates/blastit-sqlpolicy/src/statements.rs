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

    /// Skips scan.l whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.chars.get(self.pos), Some(c) if is_space(*c)) {
            self.pos += 1;
        }
    }

    /// Skips whitespace and reports whether the cursor reached the end.
    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.pos >= self.chars.len()
    }

    /// Consumes a bare identifier (`[A-Za-z_][A-Za-z0-9_]*`) and returns it.
    fn eat_word(&mut self) -> Option<String> {
        self.skip_ws();
        if !matches!(self.chars.get(self.pos), Some(c) if c.is_ascii_alphabetic() || *c == '_') {
            return None;
        }
        let start = self.pos;
        self.pos += 1;
        while matches!(self.chars.get(self.pos), Some(c) if c.is_ascii_alphanumeric() || *c == '_')
        {
            self.pos += 1;
        }
        Some(self.chars[start..self.pos].iter().collect())
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
            let mut i = self.pos + 1;
            let mut len = 0usize;
            while i < self.chars.len() {
                if self.chars[i] == '"' {
                    if self.chars.get(i + 1) == Some(&'"') {
                        len += 1;
                        i += 2;
                        continue;
                    }
                    if len == 0 {
                        // `""` is not a valid identifier.
                        return false;
                    }
                    self.pos = i + 1;
                    return true;
                }
                len += 1;
                i += 1;
            }
            return false;
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
            if !self.parse_mode() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;

    /// The grammar consumes normalised text, which is what `Allowlist::check`
    /// passes it.
    fn tc(sql: &str) -> bool {
        is_transaction_control(&normalize(sql))
    }

    fn count(input: &str, expected: usize) {
        assert_eq!(statement_count_hint(input), expected, "input: {input:?}");
    }

    #[test]
    fn counts_one_statement() {
        count("SELECT 1", 1);
        count("SELECT 1 FROM t", 1);
        count("SELECT ';'", 1);
        count("SELECT $$a;b$$", 1);
        count("SELECT $tag$a;b$tag$", 1);
        count("SELECT $1", 1);
        count("SELECT 1 -- ;\n", 1);
        count("SELECT 1 /* ; */", 1);
        count("SELECT E'a\\';b'", 1);
        count("SELECT B'a;b'", 1);
        count("SELECT X'a;b'", 1);
        count("SELECT a$b;c", 2);
    }

    #[test]
    fn counts_separators() {
        count("SELECT 1; SELECT 2", 2);
        count("SELECT 1;SELECT 2;SELECT 3", 3);
        count("SELECT 1; SELECT 2;", 2);
        count("BEGIN; COMMIT", 2);
        count("BEGIN; BEGIN", 2);
    }

    #[test]
    fn ignores_empty_and_comment_only_segments() {
        count("", 0);
        count("   \t\n", 0);
        count(";", 0);
        count(";;", 0);
        count(" ; ; ", 0);
        count("-- only a comment", 0);
        count("-- only a comment\r", 0);
        count("/* only a comment */", 0);
        count("/* a /* nested */ still */", 0);
        count("/* unterminated", 0);
        count("BEGIN; -- trailing comment", 1);
        count("/* lead */ SELECT 1 /* tail */", 1);
        count("Q --\r; DROP TABLE t", 2);
    }

    #[test]
    fn separators_inside_quotes_and_comments_do_not_split() {
        count("SELECT 'a;b'", 1);
        count("\"a;b\"", 1);
        count("SELECT E'a\\';b'", 1);
        count("SELECT B'a;b'", 1);
        count("SELECT $tag$a; b$tag$", 1);
        count("SELECT 1 -- ;\nDROP TABLE t", 1);
        count("SELECT 1 /* ; */ DROP TABLE t", 1);
        // ... while a separator after a terminated comment does split.
        count("SELECT 1; -- trailing\nSELECT 2", 2);
        count("SELECT 1; -- trailing\rSELECT 2", 2);
        count("SELECT 1 /* c */ ; SELECT 2", 2);
    }

    #[test]
    fn unterminated_quotes_swallow_separators() {
        count("SELECT 'a; b", 1);
        count("SELECT \"a; b", 1);
        count("SELECT E'a; b", 1);
        count("SELECT B'a; b", 1);
        count("SELECT X'a; b", 1);
        count("SELECT $tag$a; b", 1);
    }

    #[test]
    fn accepts_plain_transaction_statements() {
        for sql in [
            "BEGIN",
            "begin",
            "BeGiN",
            "BEGIN WORK",
            "BEGIN TRANSACTION",
            "COMMIT",
            "COMMIT WORK",
            "COMMIT TRANSACTION",
            "END",
            "END WORK",
            "END TRANSACTION",
            "ROLLBACK",
            "ROLLBACK WORK",
            "ROLLBACK TRANSACTION",
        ] {
            assert!(tc(sql), "should accept {sql:?}");
        }
    }

    #[test]
    fn accepts_savepoint_statements() {
        for sql in [
            "SAVEPOINT s1",
            "SAVEPOINT s123_x1",
            "savepoint \"s1_x\"",
            "SAVEPOINT _x",
            "RELEASE s1",
            "RELEASE SAVEPOINT s1",
            "release savepoint \"s1_x\"",
            "ROLLBACK TO s1",
            "ROLLBACK TO SAVEPOINT s1",
            "ROLLBACK WORK TO SAVEPOINT s1",
            "ROLLBACK TRANSACTION TO s1",
        ] {
            assert!(tc(sql), "should accept {sql:?}");
        }
    }

    #[test]
    fn accepts_transaction_modes() {
        for sql in [
            "START TRANSACTION",
            "start transaction read write",
            "BEGIN READ ONLY",
            "BEGIN READ WRITE",
            "BEGIN ISOLATION LEVEL SERIALIZABLE",
            "BEGIN ISOLATION LEVEL REPEATABLE READ",
            "BEGIN ISOLATION LEVEL READ COMMITTED",
            "BEGIN ISOLATION LEVEL READ UNCOMMITTED",
            "BEGIN ISOLATION LEVEL SERIALIZABLE, READ ONLY",
            "START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ WRITE",
            "BEGIN READ ONLY, ISOLATION LEVEL SERIALIZABLE",
            "\tBEGIN\tREAD\tONLY\t",
            "BEGIN\rREAD\rONLY",
            "BEGIN\x0bREAD\x0cONLY",
        ] {
            assert!(tc(sql), "should accept {sql:?}");
        }
    }

    #[test]
    fn ignores_comments_and_a_trailing_separator() {
        for sql in [
            "BEGIN -- go",
            "BEGIN /* go */",
            "BEGIN; -- go",
            "BEGIN;",
            "BEGIN; -- go\r",
            "COMMIT /* go */;",
            "END -- go",
            "ROLLBACK WORK; -- go",
            "SAVEPOINT s1 -- go",
            "START TRANSACTION /* iso */ READ WRITE -- go",
        ] {
            assert!(tc(sql), "should accept {sql:?}");
        }
    }

    #[test]
    fn rejects_other_statements() {
        for sql in [
            "SELECT 1",
            "SET TIME ZONE 'UTC'",
            "BEGIN; SELECT 1",
            "BEGIN; COMMIT",
            "COMMIT; SELECT 1",
            "START",
            "START WORK",
            "COMMIT WORK EXTRA",
            "END WORK EXTRA",
            "BEGIN READ",
            "BEGIN READ SOMETHING",
            "BEGIN ISOLATION",
            "BEGIN ISOLATION LEVEL",
            "BEGIN ISOLATION LEVEL BOGUS",
            "BEGIN DEFERRABLE",
            "BEGIN NOT DEFERRABLE",
            "SAVEPOINT",
            "SAVEPOINT 1",
            "SAVEPOINT s1 extra",
            "SAVEPOINT \"\"",
            "SAVEPOINT \"unterminated",
            "RELEASE",
            "RELEASE SAVEPOINT",
            "RELEASE s1 s2",
            "ROLLBACK TO",
            "ROLLBACK TO SAVEPOINT",
            "ROLLBACK SAVEPOINT s1",
            "ROLLBACK TO 1",
            "DROP TABLE t",
            "VACUUM",
            "",
            ";",
            "  ",
            "'BEGIN'",
            // Not scan.l whitespace, so PostgreSQL does not read it as BEGIN.
            "BEGIN\u{a0}",
            "\u{a0}BEGIN",
            "BEGIN\u{2028}",
            "BEGIN E'x'",
        ] {
            assert!(!tc(sql), "should reject {sql:?}");
        }
    }

    #[test]
    fn rejects_a_transaction_followed_by_a_second_statement() {
        for sql in [
            "BEGIN; DROP TABLE t",
            "COMMIT; BEGIN",
            "SAVEPOINT s1; SELECT 1",
            "BEGIN;; COMMIT",
            "BEGIN; -- c\nDROP TABLE t",
            "BEGIN -- c\r; DROP TABLE t",
            "Q --\r; DROP TABLE t",
            "BEGIN; SELECT 'a; b'",
        ] {
            assert!(!tc(sql), "should reject {sql:?}");
        }
    }
}
