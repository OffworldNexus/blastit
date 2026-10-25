//! Statement counting and the transaction-control grammar (ARCHITECTURE.md
//! §5.2, §6).
//!
//! Neither is a SQL parser. The counter splits on top-level `;` only, and the
//! grammar accepts exactly the transaction-control statements a grant may
//! enable with `transaction_control = true`, and nothing else.

use crate::normalize::{dollar_tag_at, find_dollar_close};

/// How many statements `sql` contains, counting only segments that hold
/// something other than whitespace and comments.
///
/// Splits on `;` outside single-quoted strings, double-quoted identifiers,
/// dollar-quoted strings, `--` line comments and `/* */` block comments
/// (nested, as PostgreSQL allows). Used to make sure a transaction-control
/// statement is the *only* statement in the text.
pub(crate) fn statement_count_hint(sql: &str) -> usize {
    let chars: Vec<char> = sql.chars().collect();
    let mut count = 0;
    let mut has_content = false;
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\'' => {
                i = skip_quoted(&chars, i, '\'');
                has_content = true;
            }
            '"' => {
                i = skip_quoted(&chars, i, '"');
                has_content = true;
            }
            '$' => {
                match dollar_tag_at(&chars, i) {
                    Some((open_len, tag)) => {
                        i = match find_dollar_close(&chars, i + open_len, &tag) {
                            Some(close) => close + open_len,
                            None => chars.len(),
                        };
                    }
                    None => i += 1,
                }
                has_content = true;
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                i += 2;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            ';' => {
                if has_content {
                    count += 1;
                    has_content = false;
                }
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                has_content = true;
                i += 1;
            }
        }
    }

    if has_content {
        count += 1;
    }
    count
}

/// Skips a quoted region opened by `quote` at `chars[i]`, returning the index
/// just past the closing quote (or the end of the text).
fn skip_quoted(chars: &[char], mut i: usize, quote: char) -> usize {
    i += 1;
    while i < chars.len() {
        if chars[i] == quote {
            if chars.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Whether `sql` is a single transaction-control statement: `BEGIN`,
/// `START TRANSACTION`, `COMMIT`, `END`, `ROLLBACK`, `SAVEPOINT <ident>`,
/// `RELEASE [SAVEPOINT] <ident>` or
/// `ROLLBACK [WORK|TRANSACTION] TO [SAVEPOINT] <ident>`.
///
/// Keywords are case-insensitive; identifiers are plain
/// `[A-Za-z_][A-Za-z0-9_]*` or double-quoted. `BEGIN` and `START TRANSACTION`
/// take an optional transaction mode list (`ISOLATION LEVEL …`, `READ ONLY`,
/// `READ WRITE`, separated by whitespace or commas), and `BEGIN`, `COMMIT` and
/// `ROLLBACK` take an optional `WORK`/`TRANSACTION` suffix.
///
/// Callers pass already-normalised text; a text holding more than one statement
/// never qualifies.
pub(crate) fn is_transaction_control(sql: &str) -> bool {
    if statement_count_hint(sql) != 1 {
        return false;
    }
    let mut parser = Parser::new(sql);
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

    /// Skips whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.chars.get(self.pos), Some(c) if c.is_whitespace()) {
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
            "COMMIT" => {
                self.eat_scope();
                self.at_end()
            }
            "END" => self.at_end(),
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
        count("/* only a comment */", 0);
        count("/* a /* nested */ still */", 0);
        count("/* unterminated", 0);
        count("BEGIN; -- trailing comment", 1);
        count("/* lead */ SELECT 1 /* tail */", 1);
    }

    #[test]
    fn unterminated_quotes_swallow_separators() {
        count("SELECT 'a; b", 1);
        count("SELECT \"a; b", 1);
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
            "ROLLBACK",
            "ROLLBACK WORK",
            "ROLLBACK TRANSACTION",
        ] {
            assert!(is_transaction_control(sql), "should accept {sql:?}");
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
            assert!(is_transaction_control(sql), "should accept {sql:?}");
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
        ] {
            assert!(is_transaction_control(sql), "should accept {sql:?}");
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
            "END WORK",
            "END TRANSACTION",
            "COMMIT WORK EXTRA",
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
        ] {
            assert!(!is_transaction_control(sql), "should reject {sql:?}");
        }
    }

    #[test]
    fn rejects_a_transaction_followed_by_a_second_statement() {
        for sql in [
            "BEGIN; DROP TABLE t",
            "COMMIT; BEGIN",
            "SAVEPOINT s1; SELECT 1",
            "BEGIN;; COMMIT",
        ] {
            assert!(!is_transaction_control(sql), "should reject {sql:?}");
        }
    }
}
