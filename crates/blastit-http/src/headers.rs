//! Header validation, hop-by-hop stripping and the few lookups the front end
//! needs (ARCHITECTURE.md §2.3).
//!
//! Header names travel as bytes on `http.chan`, so every comparison here is
//! ASCII case-insensitive and works on `&[u8]`, never on `&str`.

use std::collections::HashSet;

/// Field names that belong to one hop and are never forwarded.
///
/// `Connection` and the field names it lists, `Keep-Alive`, `Proxy-Connection`,
/// `TE`, `Trailer`, `Transfer-Encoding` and `Upgrade`, in both directions
/// (§2.3).
const HOP_BY_HOP: [&str; 7] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Most bytes of `Connection`-style header values we are willing to walk.
///
/// The head as a whole is already bounded, so this only keeps one absurdly
/// long list from costing more than it is worth: once the budget is spent, the
/// remaining values are not looked at.
const MAX_TOKEN_BYTES: usize = 8 * 1024;

/// Whether a header name equals `expected`, ignoring ASCII case.
pub(crate) fn is(name: &[u8], expected: &str) -> bool {
    name.eq_ignore_ascii_case(expected.as_bytes())
}

/// Whether `name` is an RFC 7230 `tchar` token, which a header name must be.
///
/// This is the check that stops a view from injecting a header name that would
/// split the response: no `:`, no space, no CR or LF.
pub(crate) fn is_token(name: &[u8]) -> bool {
    !name.is_empty() && name.iter().all(|byte| is_tchar(*byte))
}

/// Whether `value` is a usable header field value.
///
/// Printable bytes, high bytes (obs-text, so UTF-8 survives) and horizontal
/// tabs are fine; CR, LF, NUL and the other control bytes are not, because they
/// would let a view rewrite the response it is supposed to only describe.
pub(crate) fn is_field_value(value: &[u8]) -> bool {
    value
        .iter()
        .all(|byte| *byte == b'\t' || !byte.is_ascii_control() || *byte >= 0x80)
}

/// Whether a byte is a `tchar`.
const fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Strip ASCII whitespace from both ends of a field value.
pub(crate) fn trim(value: &[u8]) -> &[u8] {
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    let end = value
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &value[start..end]
}

/// Every value of the headers called `name`, in the order they arrived.
pub(crate) fn values<'a>(
    headers: &'a [(Vec<u8>, Vec<u8>)],
    name: &'a str,
) -> impl Iterator<Item = &'a [u8]> {
    headers
        .iter()
        .filter(move |(field, _)| is(field, name))
        .map(|(_, value)| value.as_slice())
}

/// The comma-separated tokens of every header called `name`, trimmed.
///
/// `Connection: close, TE` yields `close` and `TE`, which is exactly how RFC
/// 7230 defines the value. At most [`MAX_TOKEN_BYTES`] bytes of values are
/// walked; a list longer than that is truncated rather than followed, and no
/// caller depends on seeing the tail of one.
pub(crate) fn tokens<'a>(
    headers: &'a [(Vec<u8>, Vec<u8>)],
    name: &'a str,
) -> impl Iterator<Item = &'a [u8]> {
    let mut budget = MAX_TOKEN_BYTES;
    values(headers, name)
        .take_while(move |value| {
            let fits = value.len() <= budget;
            budget = budget.saturating_sub(value.len());
            fits
        })
        .flat_map(|value| value.split(|byte| *byte == b','))
        .map(trim)
        .filter(|token| !token.is_empty())
}

/// Whether any header called `name` carries `token` in a comma-separated list.
pub(crate) fn has_token(headers: &[(Vec<u8>, Vec<u8>)], name: &str, token: &str) -> bool {
    tokens(headers, name).any(|value| is(value, token))
}

/// The lower-cased names that must not cross a hop: the fixed list plus every
/// field name the `Connection` header lists.
pub(crate) fn hop_by_hop(headers: &[(Vec<u8>, Vec<u8>)]) -> HashSet<Vec<u8>> {
    let mut names: HashSet<Vec<u8>> = HOP_BY_HOP
        .iter()
        .map(|name| name.as_bytes().to_vec())
        .collect();
    for token in tokens(headers, "connection") {
        names.insert(token.to_ascii_lowercase());
    }
    names
}

/// The headers that may cross a hop, in the order they arrived.
pub(crate) fn strip_hop_by_hop(headers: &[(Vec<u8>, Vec<u8>)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let names = hop_by_hop(headers);
    headers
        .iter()
        .filter(|(name, _)| !names.contains(&name.to_ascii_lowercase()))
        .cloned()
        .collect()
}

/// Why a `Content-Length` header could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LengthError {
    /// The header appeared more than once, so the framing is ambiguous.
    Duplicate,
    /// The value is not a plain, unsigned decimal number.
    Invalid,
}

/// The single valid `Content-Length` of a header list, if there is one.
///
/// RFC 7230 lets a sender split a list into repeated identical fields, but a
/// front end that has to guess is a front end that can be made to disagree with
/// its peer, so anything but exactly one plain number is refused. On the
/// response path a refusal only means "pick the framing ourselves".
pub(crate) fn content_length(headers: &[(Vec<u8>, Vec<u8>)]) -> Result<Option<u64>, LengthError> {
    let mut found: Option<u64> = None;
    let mut seen = false;
    for value in values(headers, "content-length") {
        if seen {
            return Err(LengthError::Duplicate);
        }
        seen = true;
        let value = trim(value);
        if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
            return Err(LengthError::Invalid);
        }
        let text = std::str::from_utf8(value).map_err(|_| LengthError::Invalid)?;
        found = Some(text.parse::<u64>().map_err(|_| LengthError::Invalid)?);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        pairs
            .iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn tokens_are_validated() {
        for good in ["Host", "X-Forwarded-For", "a", "!#$%&'*+-.^_`|~", "0"] {
            assert!(is_token(good.as_bytes()), "{good} is a token");
        }
        for bad in ["", " ", "Host:", "a b", "a\r", "a\n", "\t", "a\x7f", "é"] {
            assert!(!is_token(bad.as_bytes()), "{bad:?} is not a token");
        }
    }

    #[test]
    fn field_values_are_validated() {
        for good in [
            &b""[..],
            b"text/plain",
            b"a\tb",
            b"caf\xc3\xa9",
            b"obs\x80text",
            b"line one two",
        ] {
            assert!(is_field_value(good), "{good:?} is a field value");
        }
        for bad in [&b"\r"[..], b"\n", b"\r\n", b"a\0b", b"a\x01b", b"a\x7fb"] {
            assert!(!is_field_value(bad), "{bad:?} is not a field value");
        }
    }

    #[test]
    fn hop_by_hop_strips_the_fixed_list_and_what_connection_names() {
        let list = headers(&[
            ("Host", "example.com"),
            ("Connection", "close, X-Private"),
            ("Keep-Alive", "timeout=5"),
            ("Proxy-Connection", "keep-alive"),
            ("TE", "trailers"),
            ("Trailer", "Expires"),
            ("Transfer-Encoding", "chunked"),
            ("Upgrade", "h2c"),
            ("x-private", "secret"),
            ("X-Kept", "1"),
        ]);
        let kept = strip_hop_by_hop(&list);
        let names: Vec<String> = kept
            .iter()
            .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
            .collect();
        assert_eq!(names, ["Host", "X-Kept"]);

        // The names are matched case-insensitively, in both directions.
        assert!(has_token(&list, "connection", "CLOSE"));
        assert!(has_token(&list, "connection", "x-private"));
        assert!(!has_token(&list, "connection", "keep-alive"));

        // A `Connection` value that itself lists `connection` changes nothing.
        let odd = headers(&[("Connection", "connection, host"), ("Host", "h")]);
        assert!(strip_hop_by_hop(&odd).is_empty());
    }

    #[test]
    fn content_length_accepts_one_plain_number() {
        let plain = headers(&[("Content-Length", "42")]);
        assert_eq!(content_length(&plain), Ok(Some(42)));
        assert_eq!(content_length(&[]), Ok(None));

        let spaced = headers(&[("content-length", " 0 ")]);
        assert_eq!(content_length(&spaced), Ok(Some(0)));

        let big = headers(&[("Content-Length", "18446744073709551615")]);
        assert_eq!(content_length(&big), Ok(Some(u64::MAX)));

        for bad in ["", " ", "-1", "+1", "1 2", "0x10", "1, 1", "1;2", "abc"] {
            let list = headers(&[("Content-Length", bad)]);
            assert_eq!(
                content_length(&list),
                Err(LengthError::Invalid),
                "{bad:?} must not parse"
            );
        }

        let duplicated = headers(&[("Content-Length", "1"), ("Content-Length", "1")]);
        assert_eq!(content_length(&duplicated), Err(LengthError::Duplicate));

        let too_big = headers(&[("Content-Length", "18446744073709551616")]);
        assert_eq!(content_length(&too_big), Err(LengthError::Invalid));
    }

    #[test]
    fn values_and_tokens_iterate_in_order() {
        let list = headers(&[("X-F", "a, b"), ("X-F", "c"), ("Y", "d")]);
        let seen: Vec<&[u8]> = values(&list, "x-f").collect();
        assert_eq!(seen, [b"a, b".as_slice(), b"c".as_slice()]);
        let seen: Vec<&[u8]> = tokens(&list, "X-F").collect();
        assert_eq!(seen, [b"a".as_slice(), b"b".as_slice(), b"c".as_slice()]);
        assert!(values(&list, "missing").next().is_none());
    }

    #[test]
    fn a_huge_token_list_is_bounded_in_bytes() {
        // One value of half the budget, then two more that cannot fit.
        let filler = "x".repeat(MAX_TOKEN_BYTES / 2);
        let list = headers(&[
            ("Connection", &filler),
            ("Connection", &filler),
            ("Connection", "keep-alive"),
        ]);
        let seen: Vec<&[u8]> = tokens(&list, "connection").collect();
        assert_eq!(seen.len(), 2, "the third value is past the budget");
        assert!(!has_token(&list, "connection", "keep-alive"));

        // A value larger than the whole budget is not walked at all.
        let huge = "y".repeat(MAX_TOKEN_BYTES * 2);
        let list = headers(&[("Connection", &huge)]);
        assert_eq!(tokens(&list, "connection").count(), 0);
    }

    #[test]
    fn trim_handles_empty_and_whitespace_only_values() {
        assert_eq!(trim(b"  x  "), b"x");
        assert_eq!(trim(b""), b"");
        assert_eq!(trim(b"   "), b"");
        assert_eq!(trim(b"\t a b \t"), b"a b");
    }
}
