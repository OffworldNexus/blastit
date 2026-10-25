//! The one lexer shared by normalisation and statement counting.
//!
//! It follows PostgreSQL 18's `src/backend/parser/scan.l` for the region
//! boundaries a policy check depends on: where a string, an identifier or a
//! comment starts and ends. It is still not a SQL parser — keywords, operators
//! and everything else stay inside [`RegionKind::Code`].
//!
//! What matters is that a character that PostgreSQL would scan as *code* is
//! never mistaken for part of a quoted region, and vice versa. Two rules are
//! easy to get wrong and are implemented explicitly here:
//!
//! * `ident_cont` includes `$`, so `a$b$c` is one identifier and the `$` does
//!   not open a dollar quote; likewise scan.l's `{integer,numeric,real}_junk`
//!   and `param_junk` rules glue an identifier onto a preceding number or
//!   parameter, so `1E'x'` is *not* an E-string.
//! * A quoted region only starts where scan.l's longest-match rule starts it,
//!   which is why [`lex`] walks code token by token ([`code_token_end`])
//!   instead of character by character.
//!
//! Two divergences are deliberate and cannot hide code or quoted data:
//!
//! * scan.l's string continuation (`'a'` + whitespace containing a newline +
//!   `'b'` is a single literal) is not modelled as one region. The text between
//!   the two quotes can only be whitespace and comments, which this lexer
//!   already removes or preserves as a space, so the difference cannot expose a
//!   `;` or quoted data — it only means such a literal normalises to two
//!   regions rather than one.
//! * scan.l's `*_junk`/`*fail` error tokens (`1e+`, `0x_`) are not reproduced
//!   character for character; only their effect on the *following* token
//!   boundary is, because PostgreSQL rejects such a statement with a syntax
//!   error before it can execute anything.

/// What a region of the text is.
///
/// Everything but [`RegionKind::Code`] and [`RegionKind::Comment`] is quoted
/// text: its contents are opaque, so whitespace and `;` inside must never be
/// interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionKind {
    /// Unquoted text: keywords, identifiers, numbers, operators, `$1`.
    Code,
    /// `'…'` (scan.l `xq`), with `''` as an escaped quote and no backslash
    /// escapes — this is the `standard_conforming_strings = on` behaviour.
    SingleQuoted,
    /// `E'…'` / `e'…'` (`xe`): a backslash escapes the next character, and `''`
    /// is an escaped quote too.
    EString,
    /// `B'…'` / `b'…'` (`xb`): only 0 and 1 are meaningful, and there is **no**
    /// escape of any kind, not even `''`.
    BitString,
    /// `X'…'` / `x'…'` (`xh`): hexadecimal bytes, with **no** escape of any
    /// kind either.
    HexString,
    /// `"…"` (`xd`), with `""` as an escaped quote.
    QuotedIdent,
    /// `$$…$$` or `$tag$…$tag$` (`xdolq`): completely opaque.
    DollarQuoted,
    /// `--` to the end of the line, or `/* … */` (nestable).
    Comment,
}

/// A half-open range of characters of one [`RegionKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) kind: RegionKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Whether `c` is whitespace in PostgreSQL: scan.l's `space` = `[ \t\n\r\f\v]`.
///
/// This is deliberately *not* `char::is_whitespace`: a no-break space or a line
/// separator is ordinary text to PostgreSQL, and collapsing it would make two
/// different statements compare equal.
pub(crate) fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// Splits `chars` into contiguous regions that cover all of `chars`.
///
/// Unterminated constructs run to the end of the text: nothing here panics or
/// fails, because a hostile or truncated statement only has to be *denied*, not
/// understood.
pub(crate) fn lex(chars: &[char]) -> Vec<Region> {
    let mut regions = Vec::new();
    let mut code_start = 0;
    let mut i = 0;

    while i < chars.len() {
        match construct_at(chars, i) {
            Some((kind, end)) => {
                if i > code_start {
                    regions.push(Region {
                        kind: RegionKind::Code,
                        start: code_start,
                        end: i,
                    });
                }
                regions.push(Region {
                    kind,
                    start: i,
                    end,
                });
                i = end;
                code_start = end;
            }
            None => i = code_token_end(chars, i),
        }
    }

    if code_start < chars.len() {
        regions.push(Region {
            kind: RegionKind::Code,
            start: code_start,
            end: chars.len(),
        });
    }
    regions
}

/// If a quoted region or a comment starts at `chars[i]`, returns its kind and
/// the index just past it.
///
/// `i` must be the start of a token, which [`lex`] guarantees: that is what
/// makes `E'…'` an E-string here and part of an identifier in `nameE'…'`.
fn construct_at(chars: &[char], i: usize) -> Option<(RegionKind, usize)> {
    let c = *chars.get(i)?;
    match c {
        '\'' => Some((RegionKind::SingleQuoted, skip_single_quoted(chars, i))),
        '"' => Some((RegionKind::QuotedIdent, skip_quoted_ident(chars, i))),
        'e' | 'E' if chars.get(i + 1) == Some(&'\'') => {
            Some((RegionKind::EString, skip_e_string(chars, i)))
        }
        'b' | 'B' if chars.get(i + 1) == Some(&'\'') => {
            Some((RegionKind::BitString, skip_escapeless(chars, i + 1)))
        }
        'x' | 'X' if chars.get(i + 1) == Some(&'\'') => {
            Some((RegionKind::HexString, skip_escapeless(chars, i + 1)))
        }
        '$' => dollar_tag_at(chars, i).map(|(open_len, tag)| {
            let end = match find_dollar_close(chars, i + open_len, &tag) {
                Some(close) => close + open_len,
                None => chars.len(),
            };
            (RegionKind::DollarQuoted, end)
        }),
        '-' if chars.get(i + 1) == Some(&'-') => {
            Some((RegionKind::Comment, skip_line_comment(chars, i)))
        }
        '/' if chars.get(i + 1) == Some(&'*') => {
            Some((RegionKind::Comment, skip_block_comment(chars, i)))
        }
        _ => None,
    }
}

/// Length of the code token starting at `chars[i]`.
///
/// Only identifiers and numeric literals are scanned exactly, because they are
/// the tokens that can swallow a following `E`/`B`/`X` or `$`. Every other
/// token — an operator, a `self` character, `other` — can be walked one
/// character at a time, which also keeps comment detection (`--`, `/*`) and the
/// operator rule's back-up behaviour equivalent.
fn code_token_end(chars: &[char], i: usize) -> usize {
    if matches!(chars.get(i), Some(c) if is_ident_start(*c)) {
        let mut j = i + 1;
        while matches!(chars.get(j), Some(c) if is_ident_cont(*c)) {
            j += 1;
        }
        return j;
    }

    if let Some(end) = number_end(chars, i) {
        let mut j = end;
        // scan.l `{integer,numeric,real}_junk`: an identifier glued to a number
        // is a single (error) token, so the identifier cannot start a new one.
        if matches!(chars.get(j), Some(c) if is_ident_start(*c)) {
            while matches!(chars.get(j), Some(c) if is_ident_cont(*c)) {
                j += 1;
            }
        }
        return j;
    }

    i + 1
}

/// End of a numeric literal starting at `chars[i]`, or `None` when none starts
/// there. Follows scan.l's `decinteger`, `hexinteger`, `octinteger`,
/// `bininteger`, `numeric`, `real` and their `…fail` fallbacks.
fn number_end(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) == Some(&'0') {
        match chars.get(i + 1) {
            Some('x' | 'X') => {
                return Some(radix_end(chars, i + 2, |c: char| c.is_ascii_hexdigit()));
            }
            Some('o' | 'O') => {
                return Some(radix_end(chars, i + 2, |c: char| ('0'..='7').contains(&c)));
            }
            Some('b' | 'B') => {
                return Some(radix_end(chars, i + 2, |c: char| c == '0' || c == '1'));
            }
            _ => {}
        }
    }

    let mut end = if chars.get(i) == Some(&'.') {
        // `{numeric}` = `.` `{decinteger}`
        decimal_end(chars, i + 1)?
    } else {
        let integer = decimal_end(chars, i)?;
        if chars.get(integer) == Some(&'.') {
            if chars.get(integer + 1) == Some(&'.') {
                // `{numericfail}`: `1..2` is `1`, `..`, `2`.
                return Some(integer);
            }
            // `{decinteger}\.{decinteger}?`
            decimal_end(chars, integer + 1).unwrap_or(integer + 1)
        } else {
            integer
        }
    };

    // `{real}` = `({decinteger}|{numeric})[Ee][-+]?{decinteger}`
    if matches!(chars.get(end), Some('e' | 'E')) {
        let sign = if matches!(chars.get(end + 1), Some('+' | '-')) {
            end + 2
        } else {
            end + 1
        };
        if let Some(exp_end) = decimal_end(chars, sign) {
            end = exp_end;
        }
    }

    Some(end)
}

/// End of `{decinteger}` = `{decdigit}(_?{decdigit})*` at `chars[i]`, or `None`
/// when `chars[i]` is not a digit.
fn decimal_end(chars: &[char], i: usize) -> Option<usize> {
    if !matches!(chars.get(i), Some(c) if c.is_ascii_digit()) {
        return None;
    }
    let mut k = i + 1;
    loop {
        let mut next = k;
        if chars.get(next) == Some(&'_') {
            next += 1;
        }
        if matches!(chars.get(next), Some(c) if c.is_ascii_digit()) {
            k = next + 1;
        } else {
            break;
        }
    }
    Some(k)
}

/// End of a `0x`/`0o`/`0b` literal: scan.l's `{hex,oct,bin}integer`
/// (`( _? digit )+`), or its `…fail` form `0[xob]_?` when no digit follows.
fn radix_end(chars: &[char], start: usize, is_digit: impl Fn(char) -> bool) -> usize {
    let mut k = start;
    let mut matched = false;
    loop {
        let mut next = k;
        if chars.get(next) == Some(&'_') {
            next += 1;
        }
        if matches!(chars.get(next), Some(c) if is_digit(*c)) {
            k = next + 1;
            matched = true;
        } else {
            break;
        }
    }
    if matched {
        k
    } else if chars.get(start) == Some(&'_') {
        start + 1
    } else {
        start
    }
}

/// Skips `'…'` (scan.l `xq`), starting at the opening quote: `''` is an escaped
/// quote, and a backslash is an ordinary character.
fn skip_single_quoted(chars: &[char], i: usize) -> usize {
    skip_doubled(chars, i, '\'')
}

/// Skips `"…"` (scan.l `xd`), starting at the opening quote: `""` is an escaped
/// quote.
fn skip_quoted_ident(chars: &[char], i: usize) -> usize {
    skip_doubled(chars, i, '"')
}

/// Skips a `''`-or-`""`-style quoted region, starting at its opening quote.
fn skip_doubled(chars: &[char], mut i: usize, quote: char) -> usize {
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

/// Skips `B'…'` or `X'…'` (scan.l `xb`/`xh`), starting at the opening quote:
/// the contents are `[^']*` and there is no escape at all.
fn skip_escapeless(chars: &[char], quote_index: usize) -> usize {
    let mut i = quote_index + 1;
    while i < chars.len() && chars[i] != '\'' {
        i += 1;
    }
    if i < chars.len() { i + 1 } else { i }
}

/// Skips `E'…'` (scan.l `xe`), starting at the `E`. A backslash escapes the
/// next character (whatever it is) and `''` is an escaped quote.
fn skip_e_string(chars: &[char], mut i: usize) -> usize {
    i += 2; // the E and the opening quote
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                if chars.get(i + 1) == Some(&'\'') {
                    i += 2;
                    continue;
                }
                return i + 1;
            }
            '\\' => {
                if i + 1 >= chars.len() {
                    return chars.len();
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    i
}

/// Skips a `--` comment, starting at the first `-`, up to and including the
/// newline that ends it. scan.l's `comment` is `--{non_newline}*`, so either
/// `\n` or `\r` ends it.
fn skip_line_comment(chars: &[char], mut i: usize) -> usize {
    i += 2;
    while i < chars.len() && !matches!(chars[i], '\n' | '\r') {
        i += 1;
    }
    if i < chars.len() {
        i += 1;
    }
    i
}

/// Skips a `/* … */` comment, starting at the `/`, honouring PostgreSQL's
/// nesting. An unterminated comment runs to the end of the text.
fn skip_block_comment(chars: &[char], mut i: usize) -> usize {
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
    i
}

/// If a dollar-quote opening delimiter starts at `chars[i]` (`$$`, `$tag$`),
/// returns its length and tag.
///
/// scan.l's `dolqdelim` is `\$({dolq_start}{dolq_cont}*)?\$`, with
/// `dolq_start` = `[A-Za-z\200-\377_]` and `dolq_cont` = `dolq_start` plus
/// digits: any character with a code point of 0x80 or more may be part of a
/// tag, but `$1$` is the parameter `$1` followed by `$`.
pub(crate) fn dollar_tag_at(chars: &[char], i: usize) -> Option<(usize, String)> {
    if chars.get(i) != Some(&'$') {
        return None;
    }
    let mut j = i + 1;
    if chars.get(j) == Some(&'$') {
        return Some((2, String::new()));
    }
    let start = j;
    if !matches!(chars.get(j), Some(c) if is_dolq_start(*c)) {
        return None;
    }
    j += 1;
    while matches!(chars.get(j), Some(c) if is_dolq_cont(*c)) {
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

/// scan.l `ident_start` / `dolq_start`: `[A-Za-z\200-\377_]`, that is ASCII
/// letters, `_`, and every character with a code point of 0x80 or more.
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c >= '\u{80}' || c == '_'
}

/// scan.l `ident_cont`: `[A-Za-z\200-\377_0-9$]`, which is why `$` inside an
/// identifier never opens a dollar quote.
fn is_ident_cont(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '$'
}

/// scan.l `dolq_cont`: `[A-Za-z\200-\377_0-9]`, i.e. `ident_cont` without `$`.
fn is_dolq_cont(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// scan.l `dolq_start` is the same character class as `ident_start`.
fn is_dolq_start(c: char) -> bool {
    is_ident_start(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<(RegionKind, String)> {
        let chars: Vec<char> = sql.chars().collect();
        lex(&chars)
            .into_iter()
            .map(|r| (r.kind, chars[r.start..r.end].iter().collect()))
            .collect()
    }

    fn code(sql: &str) -> Vec<(RegionKind, String)> {
        vec![(RegionKind::Code, sql.to_string())]
    }

    #[test]
    fn regions_are_contiguous_and_cover_everything() {
        let sql = "SELECT 'a''b', \"c\"\"d\", $t$e$t$, E'f\\'g', B'1', X'f', 1 -- h\n/* i */ /* j /* k */ l */ €€";
        let chars: Vec<char> = sql.chars().collect();
        let regions = lex(&chars);
        assert_eq!(regions.first().map(|r| r.start), Some(0));
        assert_eq!(regions.last().map(|r| r.end), Some(chars.len()));
        for pair in regions.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
    }

    #[test]
    fn classifies_constructs() {
        assert_eq!(
            kinds("'a'\"b\"$$c$$"),
            vec![
                (RegionKind::SingleQuoted, "'a'".to_string()),
                (RegionKind::QuotedIdent, "\"b\"".to_string()),
                (RegionKind::DollarQuoted, "$$c$$".to_string()),
            ]
        );
        assert_eq!(
            kinds("x/* c /* d */ e */y"),
            vec![
                (RegionKind::Code, "x".to_string()),
                (RegionKind::Comment, "/* c /* d */ e */".to_string()),
                (RegionKind::Code, "y".to_string()),
            ]
        );
    }

    #[test]
    fn whitespace_is_exactly_scan_l_space() {
        for c in [' ', '\t', '\n', '\r', '\x0b', '\x0c'] {
            assert!(is_space(c), "{c:?} is scan.l space");
        }
        for c in ['\u{a0}', '\u{85}', '\u{2000}', '\u{2028}', '\u{3000}'] {
            assert!(!is_space(c), "{c:?} is not scan.l space");
        }
    }

    #[test]
    fn line_comments_end_at_cr_or_lf() {
        assert_eq!(
            kinds("q--c\rx"),
            vec![
                (RegionKind::Code, "q".to_string()),
                (RegionKind::Comment, "--c\r".to_string()),
                (RegionKind::Code, "x".to_string()),
            ]
        );
        assert_eq!(
            kinds("q--c\nx"),
            vec![
                (RegionKind::Code, "q".to_string()),
                (RegionKind::Comment, "--c\n".to_string()),
                (RegionKind::Code, "x".to_string()),
            ]
        );
        assert_eq!(
            kinds("q--c"),
            vec![
                (RegionKind::Code, "q".to_string()),
                (RegionKind::Comment, "--c".to_string()),
            ]
        );
    }

    #[test]
    fn dollar_quote_tags_use_the_scan_l_alphabet() {
        assert_eq!(
            kinds("$é$a$é$"),
            vec![(RegionKind::DollarQuoted, "$é$a$é$".to_string())]
        );
        assert_eq!(
            kinds("$€$a$€$"),
            vec![(RegionKind::DollarQuoted, "$€$a$€$".to_string())]
        );
        assert_eq!(
            kinds("$€1_$a$€1_$"),
            vec![(RegionKind::DollarQuoted, "$€1_$a$€1_$".to_string())]
        );
        assert_eq!(
            kinds("$_t1$a$_t1$"),
            vec![(RegionKind::DollarQuoted, "$_t1$a$_t1$".to_string())]
        );
        // A digit may not start a tag, so `$1$` is the parameter `$1` and `$`.
        assert_eq!(kinds("$1$"), code("$1$"));
        // ... and a second `$` after the parameter does open an (unterminated)
        // dollar quote, exactly as scan.l scans `$1` then `$$`.
        assert_eq!(
            kinds("$1$$"),
            vec![
                (RegionKind::Code, "$1".to_string()),
                (RegionKind::DollarQuoted, "$$".to_string()),
            ]
        );
    }

    #[test]
    fn identifiers_swallow_dollar_signs() {
        assert_eq!(kinds("a$b$c"), code("a$b$c"));
        assert_eq!(kinds("a$b$x   y$b$z"), code("a$b$x   y$b$z"));
        assert_eq!(kinds("a$$b$$c"), code("a$$b$$c"));
        assert_eq!(
            kinds("$tag$a$tag$"),
            vec![(RegionKind::DollarQuoted, "$tag$a$tag$".to_string())]
        );
        // After a number the `$` is a fresh token, so the later `$a$` opens a
        // quote — while `$1$` itself stays code.
        assert_eq!(
            kinds("1$1$a$1$"),
            vec![
                (RegionKind::Code, "1$1".to_string()),
                (RegionKind::DollarQuoted, "$a$1$".to_string()),
            ]
        );
    }

    #[test]
    fn bit_and_hex_strings_have_no_escape() {
        assert_eq!(
            kinds("B'a''b'"),
            vec![
                (RegionKind::BitString, "B'a'".to_string()),
                (RegionKind::SingleQuoted, "'b'".to_string()),
            ]
        );
        assert_eq!(
            kinds("X'a''b'"),
            vec![
                (RegionKind::HexString, "X'a'".to_string()),
                (RegionKind::SingleQuoted, "'b'".to_string()),
            ]
        );
        assert_eq!(
            kinds("b'1010'x'1F'"),
            vec![
                (RegionKind::BitString, "b'1010'".to_string()),
                (RegionKind::HexString, "x'1F'".to_string()),
            ]
        );
        // A backslash is ordinary, so the literal ends at the next quote; the
        // `b` after it is then a bit-string prefix of its own, not an
        // identifier, because `b'` matches scan.l's `xbstart` (longest match).
        assert_eq!(
            kinds(r"B'a\'b'"),
            vec![
                (RegionKind::BitString, r"B'a\'".to_string()),
                (RegionKind::BitString, "b'".to_string()),
            ]
        );
    }

    #[test]
    fn e_strings_need_a_token_start() {
        assert_eq!(
            kinds("E'x'"),
            vec![(RegionKind::EString, "E'x'".to_string())]
        );
        assert_eq!(
            kinds("e'x'"),
            vec![(RegionKind::EString, "e'x'".to_string())]
        );
        assert_eq!(
            kinds("(E'x')"),
            vec![
                (RegionKind::Code, "(".to_string()),
                (RegionKind::EString, "E'x'".to_string()),
                (RegionKind::Code, ")".to_string()),
            ]
        );
        assert_eq!(
            kinds("SELECT 1 E'x'"),
            vec![
                (RegionKind::Code, "SELECT 1 ".to_string()),
                (RegionKind::EString, "E'x'".to_string()),
            ]
        );
        // `WHERE'x'`, `xE'y'`: the E belongs to an identifier.
        assert_eq!(
            kinds("WHERE'x'"),
            vec![
                (RegionKind::Code, "WHERE".to_string()),
                (RegionKind::SingleQuoted, "'x'".to_string()),
            ]
        );
        assert_eq!(
            kinds("xE'y'"),
            vec![
                (RegionKind::Code, "xE".to_string()),
                (RegionKind::SingleQuoted, "'y'".to_string()),
            ]
        );
        assert_eq!(
            kinds("x$E'y'"),
            vec![
                (RegionKind::Code, "x$E".to_string()),
                (RegionKind::SingleQuoted, "'y'".to_string()),
            ]
        );
        // A lone `$` is its own token, so the E does start one.
        assert_eq!(
            kinds("$E'x'"),
            vec![
                (RegionKind::Code, "$".to_string()),
                (RegionKind::EString, "E'x'".to_string()),
            ]
        );
    }

    #[test]
    fn a_number_glues_a_following_identifier_to_itself() {
        // scan.l `integer_junk`: `1E` is one (error) token, so `'x'` is a plain
        // string, not an E-string.
        for sql in ["1E'x'", "1.5E'x'", ".5E'x'", "1_0E'x'", "1e'x'", "1e5'x'"] {
            assert_eq!(
                kinds(sql),
                vec![
                    (RegionKind::Code, sql.trim_end_matches("'x'").to_string()),
                    (RegionKind::SingleQuoted, "'x'".to_string()),
                ],
                "for {sql:?}"
            );
        }
        assert_eq!(
            kinds("0x1E'a'"),
            vec![
                (RegionKind::Code, "0x1E".to_string()),
                (RegionKind::SingleQuoted, "'a'".to_string()),
            ]
        );
        assert_eq!(
            kinds("0x'a'"),
            vec![
                (RegionKind::Code, "0x".to_string()),
                (RegionKind::SingleQuoted, "'a'".to_string()),
            ]
        );
        assert_eq!(
            kinds("0b'1'"),
            vec![
                (RegionKind::Code, "0b".to_string()),
                (RegionKind::SingleQuoted, "'1'".to_string()),
            ]
        );
        assert_eq!(
            kinds("$1E'a'"),
            vec![
                (RegionKind::Code, "$1E".to_string()),
                (RegionKind::SingleQuoted, "'a'".to_string()),
            ]
        );
        // `1..2` is `1`, `..`, `2`, so a following E-string does start there.
        assert_eq!(
            kinds("1..E'x'"),
            vec![
                (RegionKind::Code, "1..".to_string()),
                (RegionKind::EString, "E'x'".to_string()),
            ]
        );
    }

    #[test]
    fn e_string_escapes() {
        assert_eq!(
            kinds(r"E'a\'b'"),
            vec![(RegionKind::EString, r"E'a\'b'".to_string())]
        );
        assert_eq!(
            kinds(r"E'it''s'"),
            vec![(RegionKind::EString, r"E'it''s'".to_string())]
        );
        assert_eq!(
            kinds(r"E'a\'b"),
            vec![(RegionKind::EString, r"E'a\'b".to_string())]
        );
    }

    #[test]
    fn standard_strings_have_no_backslash_escape() {
        // `b'` is scan.l's `xbstart`, so it opens a bit string here rather than
        // continuing an identifier.
        assert_eq!(
            kinds(r"'a\'b'"),
            vec![
                (RegionKind::SingleQuoted, r"'a\'".to_string()),
                (RegionKind::BitString, "b'".to_string()),
            ]
        );
        assert_eq!(
            kinds("'it''s'"),
            vec![(RegionKind::SingleQuoted, "'it''s'".to_string())]
        );
        assert_eq!(
            kinds(r#""a""b""#),
            vec![(RegionKind::QuotedIdent, r#""a""b""#.to_string())]
        );
    }

    #[test]
    fn unterminated_constructs_run_to_the_end() {
        for sql in [
            "'a",
            "\"a",
            "E'a",
            "B'a",
            "X'a",
            "$$a",
            "$t$a",
            "--a",
            "/*a",
            "/* a /* b */ c",
        ] {
            let chars: Vec<char> = sql.chars().collect();
            let regions = lex(&chars);
            assert_eq!(regions.len(), 1, "{sql:?}");
            assert_eq!(regions[0].start, 0, "{sql:?}");
            assert_eq!(regions[0].end, chars.len(), "{sql:?}");
        }
    }
}
