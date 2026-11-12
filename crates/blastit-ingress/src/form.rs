//! `application/x-www-form-urlencoded` validation and canonical re-encoding.
//!
//! `url::form_urlencoded::parse` is infallible: it passes `%ZZ` through as
//! literal text and turns invalid UTF-8 into U+FFFD, so it cannot produce the
//! `400`s `R15` requires. Validation therefore happens here — every `%` must be
//! followed by two hexadecimal digits, the decoded bytes must be UTF-8, and no
//! decoded byte may be NUL — and `url`'s serializer is used only to write the
//! canonical form back out.

use percent_encoding::percent_decode;
use url::form_urlencoded::Serializer;

use crate::IngressReject;

/// Decodes and validates one form body into its ordered `(name, value)` pairs.
///
/// Empty segments (`a=1&&b=2`, a trailing `&`) are dropped, as Python's
/// `urllib.parse.parse_qsl` does. A segment without `=` is a name with an
/// empty value.
pub(crate) fn decode(raw: &[u8]) -> Result<Vec<(String, String)>, IngressReject> {
    let mut pairs = Vec::new();
    for segment in raw.split(|byte| *byte == b'&') {
        if segment.is_empty() {
            continue;
        }
        let (name, value) = match segment.iter().position(|byte| *byte == b'=') {
            Some(separator) => (&segment[..separator], &segment[separator + 1..]),
            None => (segment, &[] as &[u8]),
        };
        pairs.push((decode_component(name)?, decode_component(value)?));
    }
    Ok(pairs)
}

/// Re-encodes decoded pairs with `url`'s canonical form serializer.
pub(crate) fn encode(pairs: &[(String, String)]) -> String {
    let mut serializer = Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

/// Decodes one `+`-and-`%XX` component, rejecting bad escapes, non-UTF-8 and NUL.
fn decode_component(component: &[u8]) -> Result<String, IngressReject> {
    validate_escapes(component)?;
    let with_spaces: Vec<u8> = component
        .iter()
        .map(|byte| if *byte == b'+' { b' ' } else { *byte })
        .collect();
    let decoded = percent_decode(&with_spaces);
    let text = decoded
        .decode_utf8()
        .map_err(|_| IngressReject::FormNotUtf8)?;
    if text.contains('\0') {
        return Err(IngressReject::FormNul);
    }
    Ok(text.into_owned())
}

/// Requires every `%` to be followed by exactly two hexadecimal digits.
fn validate_escapes(component: &[u8]) -> Result<(), IngressReject> {
    let mut index = 0;
    while let Some(byte) = component.get(index) {
        if *byte == b'%' {
            hex_digit(component.get(index + 1).copied())?;
            hex_digit(component.get(index + 2).copied())?;
            index += 3;
        } else {
            index += 1;
        }
    }
    Ok(())
}

/// One hexadecimal digit, or [`IngressReject::FormBadEscape`].
fn hex_digit(byte: Option<u8>) -> Result<u8, IngressReject> {
    match byte {
        Some(digit @ b'0'..=b'9') => Ok(digit - b'0'),
        Some(digit @ b'a'..=b'f') => Ok(digit - b'a' + 10),
        Some(digit @ b'A'..=b'F') => Ok(digit - b'A' + 10),
        _ => Err(IngressReject::FormBadEscape),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_empty_segments() {
        assert_eq!(
            decode(b"a=1&&b=2&"),
            Ok(vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ])
        );
    }

    #[test]
    fn keeps_duplicate_names_in_order() {
        assert_eq!(
            decode(b"b=1&b=2").map(|pairs| encode(&pairs)),
            Ok("b=1&b=2".to_string())
        );
    }
}
