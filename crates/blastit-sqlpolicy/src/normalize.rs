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

use crate::lexer::{RegionKind, is_space, lex};

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
/// * quoted regions are kept verbatim: single-quoted strings (`'it''s'`),
///   E-strings (`E'a\'b  c'`, where a backslash escapes the next character),
///   bit and hex strings (`B'1010'`, `X'1F'`, with no escape at all),
///   double-quoted identifiers (`"a  b"`) and dollar-quoted strings (`$$…$$`,
///   `$tag$…$tag$`);
/// * `$1` is a parameter and `a$b$c` is one identifier: neither opens a dollar
///   quote. A bare `$` is ordinary text;
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
                for c in text {
                    if is_space(*c) {
                        pending_space = true;
                    } else {
                        out.flush_space(&mut pending_space);
                        out.push(*c, true);
                    }
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

    out.finish()
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
    fn positional_parameters_are_not_dollar_quotes() {
        case("WHERE a = $1", "WHERE a = $1");
        case("WHERE a = $1  AND b = $2", "WHERE a = $1 AND b = $2");
        case("SELECT $", "SELECT $");
        case("SELECT $foo", "SELECT $foo");
        case("SELECT $1$", "SELECT $1$");
        case("SELECT $1 ;", "SELECT $1");
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
}
