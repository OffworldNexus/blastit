//! The one lexer shared by normalisation and statement counting.
//!
//! Both operations must agree on what is a quote, what is a comment and where
//! each ends, so there is exactly one scanner here ([`lex`]) and both consume
//! its regions. It is still not a SQL parser: it knows only the lexical
//! skeleton, and classifies everything else as code.

/// What a region of the text is.
///
/// Everything but [`RegionKind::Code`] and [`RegionKind::Comment`] is quoted
/// text: its contents are opaque, so whitespace and `;` inside must never be
/// interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionKind {
    /// Unquoted text: keywords, identifiers, operators, `$1` parameters.
    Code,
    /// `'…'`, with `''` as an escaped quote; backslashes are ordinary.
    SingleQuoted,
    /// `E'…'` / `e'…'`; a backslash escapes the next character, and `''` is an
    /// escaped quote too.
    EString,
    /// `"…"`, with `""` as an escaped quote.
    QuotedIdent,
    /// `$$…$$` or `$tag$…$tag$`.
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
        let previous_is_ident = i > 0 && is_ident_char(chars[i - 1]);
        match special_at(chars, i, previous_is_ident) {
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
            None => i += 1,
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

/// Character that can appear in a PostgreSQL identifier, used to decide whether
/// an `E` really is the prefix of an E-string (`name E'x'` yes, `nameE'x'` no).
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// If a quote, dollar quote or comment starts at `chars[i]`, returns its kind
/// and the index just past it.
fn special_at(chars: &[char], i: usize, previous_is_ident: bool) -> Option<(RegionKind, usize)> {
    let c = *chars.get(i)?;
    match c {
        '\'' => Some((RegionKind::SingleQuoted, skip_single_quoted(chars, i))),
        '"' => Some((RegionKind::QuotedIdent, skip_quoted_ident(chars, i))),
        'e' | 'E' if chars.get(i + 1) == Some(&'\'') && !previous_is_ident => {
            Some((RegionKind::EString, skip_e_string(chars, i)))
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

/// Skips `'…'`, starting at the opening quote.
fn skip_single_quoted(chars: &[char], mut i: usize) -> usize {
    i += 1;
    while i < chars.len() {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Skips `"…"`, starting at the opening quote.
fn skip_quoted_ident(chars: &[char], mut i: usize) -> usize {
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' {
            if chars.get(i + 1) == Some(&'"') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Skips `E'…'`, starting at the `E`.
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
/// newline that ends it.
fn skip_line_comment(chars: &[char], mut i: usize) -> usize {
    i += 2;
    while i < chars.len() && chars[i] != '\n' {
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
/// The tag follows PostgreSQL's identifier rules: a letter (including
/// non-ASCII) or `_` first, then letters, digits or `_`. `$1` and a bare `$`
/// are therefore not dollar quotes.
pub(crate) fn dollar_tag_at(chars: &[char], i: usize) -> Option<(usize, String)> {
    if chars.get(i) != Some(&'$') {
        return None;
    }
    let mut j = i + 1;
    if chars.get(j) == Some(&'$') {
        return Some((2, String::new()));
    }
    let start = j;
    if !matches!(chars.get(j), Some(c) if c.is_alphabetic() || *c == '_') {
        return None;
    }
    j += 1;
    while matches!(chars.get(j), Some(c) if c.is_alphanumeric() || *c == '_') {
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

    #[test]
    fn regions_are_contiguous_and_cover_everything() {
        let sql = "SELECT 'a''b', \"c\"\"d\", $t$e$t$, E'f\\'g', 1 -- h\n/* i */ /* j /* k */ l */";
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
            kinds("x--c\n"),
            vec![
                (RegionKind::Code, "x".to_string()),
                (RegionKind::Comment, "--c\n".to_string()),
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
    fn e_strings_need_a_prefix_that_is_not_an_identifier() {
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
        // `WHERE'x'` and `xE'y'`: the E belongs to an identifier.
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
    fn dollar_quote_tags_use_the_postgresql_alphabet() {
        assert_eq!(
            kinds("$é$a$é$"),
            vec![(RegionKind::DollarQuoted, "$é$a$é$".to_string())]
        );
        assert_eq!(
            kinds("$_t1$a$_t1$"),
            vec![(RegionKind::DollarQuoted, "$_t1$a$_t1$".to_string())]
        );
        // Digits may not start a tag: `$1$` stays code.
        assert_eq!(kinds("$1$"), vec![(RegionKind::Code, "$1$".to_string())]);
    }

    #[test]
    fn unterminated_constructs_run_to_the_end() {
        for sql in [
            "'a",
            "\"a",
            "E'a",
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
