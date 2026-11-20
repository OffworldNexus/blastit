//! Header validation, hop-by-hop stripping and the few lookups the front end
//! needs (ARCHITECTURE.md §2.3).
//!
//! Header names travel as bytes on `http.chan`, so every comparison here is
//! ASCII case-insensitive and works on `&[u8]`, never on `&str`.

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
    has_token_bytes(headers, name, token.as_bytes())
}

/// [`has_token`] over bytes, for a caller whose candidate is itself a header
/// name rather than a string literal.
fn has_token_bytes(headers: &[(Vec<u8>, Vec<u8>)], name: &str, token: &[u8]) -> bool {
    tokens(headers, name).any(|value| value.eq_ignore_ascii_case(token))
}

/// Whether `name` is one of the fields that must not cross a hop.
///
/// This is exactly the set [`strip_hop_by_hop`] removes — the fixed list plus
/// every field name the `Connection` header lists — but decided for one name at
/// a time, so filtering a header list costs no allocation at all. That matters
/// on the response path, where the filter is applied while the head is written
/// and nothing is cloned to do it.
pub(crate) fn is_hop_by_hop(headers: &[(Vec<u8>, Vec<u8>)], name: &[u8]) -> bool {
    HOP_BY_HOP.iter().any(|hop| is(name, hop)) || has_token_bytes(headers, "connection", name)
}

/// The headers that may cross a hop, in the order they arrived.
pub(crate) fn strip_hop_by_hop(headers: &[(Vec<u8>, Vec<u8>)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    headers
        .iter()
        .filter(|(name, _)| !is_hop_by_hop(headers, name))
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
