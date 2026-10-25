//! Text normalisation (ARCHITECTURE.md §6, "Matching").
//!
//! The manifest and the wire are normalised with the same rules, so a query
//! written by hand in a grant matches the text libpq sends, as long as it is
//! the same statement with arbitrary whitespace and formatting.

/// Normalises `sql` for exact allowlist comparison.
///
/// The rules (ARCHITECTURE.md §6):
///
/// * the text is trimmed;
/// * trailing `;` separators are removed, including the whitespace around them
///   and repeated separators (`SELECT 1 ;;` → `SELECT 1`); only separators
///   outside quotes are removed, so `SELECT ';'` is unchanged;
/// * every run of whitespace **outside** single-quoted strings (`'it''s'`),
///   double-quoted identifiers (`"a  b"`) and dollar-quoted strings (`$$…$$`,
///   `$tag$…$tag$`) collapses to a single space;
/// * comments are left as ordinary text: `--` and `/* */` get no special
///   treatment here and whitespace inside them collapses like anywhere else;
/// * `$1` and `$` are ordinary text, not dollar quotes;
/// * an unterminated quote (or dollar quote) is kept verbatim to the end of the
///   text; the text is trimmed before scanning, so trailing whitespace is gone
///   even inside an unterminated quote; this function never panics.
pub fn normalize(sql: &str) -> String {
    let chars: Vec<char> = sql.trim().chars().collect();
    let mut out = Output::new(chars.len());
    let mut pending_space = false;
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\'' => {
                out.flush_space(&mut pending_space);
                out.push('\'', true);
                i = copy_quoted(&chars, i + 1, '\'', &mut out);
            }
            '"' => {
                out.flush_space(&mut pending_space);
                out.push('"', true);
                i = copy_quoted(&chars, i + 1, '"', &mut out);
            }
            '$' => {
                out.flush_space(&mut pending_space);
                match dollar_tag_at(&chars, i) {
                    Some((open_len, tag)) => {
                        // Copy the whole literal verbatim; its contents never
                        // take part in whitespace collapsing or in the
                        // trailing-separator strip.
                        let end = match find_dollar_close(&chars, i + open_len, &tag) {
                            Some(close) => close + open_len,
                            None => chars.len(),
                        };
                        for c in &chars[i..end] {
                            out.push(*c, false);
                        }
                        i = end;
                    }
                    None => {
                        out.push('$', true);
                        i += 1;
                    }
                }
            }
            c if c.is_whitespace() => {
                pending_space = true;
                i += 1;
            }
            c => {
                out.flush_space(&mut pending_space);
                out.push(c, true);
                i += 1;
            }
        }
    }

    out.finish()
}

/// Copies a single- or double-quoted region, starting just after its opening
/// quote, and returns the index just past its closing quote. A doubled quote is
/// an escape (`'it''s'`, `"a""b"`); an unterminated region runs to the end of
/// the text, which is then kept verbatim.
fn copy_quoted(chars: &[char], mut i: usize, quote: char, out: &mut Output) -> usize {
    while i < chars.len() {
        if chars[i] == quote {
            if chars.get(i + 1) == Some(&quote) {
                out.push(quote, false);
                out.push(quote, false);
                i += 2;
                continue;
            }
            out.push(quote, true);
            return i + 1;
        }
        out.push(chars[i], false);
        i += 1;
    }
    i
}

/// If a dollar-quote opening delimiter starts at `chars[i]` (`$$`, `$tag$`),
/// returns its length and tag. `$1` and a bare `$` are not dollar quotes.
pub(crate) fn dollar_tag_at(chars: &[char], i: usize) -> Option<(usize, String)> {
    if chars.get(i) != Some(&'$') {
        return None;
    }
    let mut j = i + 1;
    if chars.get(j) == Some(&'$') {
        return Some((2, String::new()));
    }
    let start = j;
    if !matches!(chars.get(j), Some(c) if c.is_ascii_alphabetic() || *c == '_') {
        return None;
    }
    j += 1;
    while matches!(chars.get(j), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
        j += 1;
    }
    if chars.get(j) == Some(&'$') {
        Some((j - i + 1, chars[start..j].iter().collect()))
    } else {
        None
    }
}

/// Index of the `$tag$` that closes a dollar quote opened with `tag`, searching
/// from `from`; `None` when the literal is never closed.
pub(crate) fn find_dollar_close(chars: &[char], from: usize, tag: &str) -> Option<usize> {
    let mut needle = String::with_capacity(tag.len() + 2);
    needle.push('$');
    needle.push_str(tag);
    needle.push('$');
    let needle: Vec<char> = needle.chars().collect();
    let len = needle.len();

    let mut k = from;
    while k + len <= chars.len() {
        if chars[k..k + len] == needle[..] {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// Output under construction, with a parallel flag per character telling
/// whether it was produced outside a quoted region.
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

    /// Emits the single space that stands for a collapsed whitespace run, if one
    /// is pending.
    fn flush_space(&mut self, pending: &mut bool) {
        if *pending {
            if !self.text.is_empty() {
                self.push(' ', true);
            }
            *pending = false;
        }
    }

    /// Drops trailing `;` separators and the whitespace around them, then
    /// returns the normalised text. Only unquoted characters are dropped.
    fn finish(mut self) -> String {
        while let Some(last) = self.text.chars().next_back() {
            let plain = matches!(self.plain.last(), Some(true));
            if plain && (last == ';' || last.is_whitespace()) {
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
        case("SELECT \t\n 1", "SELECT 1");
        case("  SELECT\n\n  1  ", "SELECT 1");
        case("", "");
        case("   \t\n ", "");
        case("SELECT 1 --  a   b", "SELECT 1 -- a b");
        case("SELECT 1 /*  a\n b */", "SELECT 1 /* a b */");
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
    }

    #[test]
    fn unicode_is_preserved() {
        case("SELECT  'héllo  wörld'", "SELECT 'héllo  wörld'");
        case("SELECT 'héllo'  ,  'wörld'", "SELECT 'héllo' , 'wörld'");
        case("SELECT '日本語  テスト'", "SELECT '日本語  テスト'");
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
        ] {
            let once = normalize(input);
            assert_eq!(normalize(&once), once, "input: {input:?}");
        }
    }
}
