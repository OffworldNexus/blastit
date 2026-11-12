//! `Content-Type` parsing: the lowercase essence and the optional charset.
//!
//! Everything a report carries is capped at [`MAX_MIME_REPORT_BYTES`], because
//! the value comes straight from a request header and ends up in a `blastit
//! check` report and in an [`IngressReject`](crate::IngressReject) message.
//! Parameters other than `charset` (a `boundary=…`, say) are dropped: the
//! essence is what decides which codec may read the body, and a multipart
//! boundary is not a codec this crate implements.

use crate::MAX_MIME_REPORT_BYTES;

/// The essence reported when a request carried no `Content-Type` at all.
pub(crate) const NO_MIME: &str = "none";

/// The `application/x-www-form-urlencoded` essence.
pub(crate) const FORM_ESSENCE: &str = "application/x-www-form-urlencoded";

/// The `application/json` essence.
pub(crate) const JSON_ESSENCE: &str = "application/json";

/// Splits a `Content-Type` into its lowercase essence and its charset.
///
/// The essence is the part before the first `;`, trimmed and ASCII-lowercased.
/// Every parameter is dropped except `charset`, whose first occurrence is
/// returned trimmed, unquoted and ASCII-lowercased. A missing or empty
/// `Content-Type` reports the [`NO_MIME`] essence.
pub(crate) fn parse(content_type: Option<&str>) -> (String, Option<String>) {
    let Some(raw) = content_type else {
        return (NO_MIME.to_string(), None);
    };
    let mut parts = split_parameters(raw);
    let essence = parts.next().map_or("", |part| part.trim());
    let essence = if essence.is_empty() {
        NO_MIME.to_string()
    } else {
        cap(&essence.to_ascii_lowercase())
    };
    let mut charset = None;
    for parameter in parts {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("charset") && charset.is_none() {
            let value = value.trim().trim_matches('"').trim();
            charset = Some(value.to_ascii_lowercase());
        }
    }
    (essence, charset)
}

/// Splits on `;`, ignoring separators inside a quoted parameter value.
///
/// Every split point sits on an ASCII `;`, which is always a `char` boundary,
/// so the slices are valid UTF-8.
fn split_parameters(raw: &str) -> impl Iterator<Item = &str> {
    let mut bounds = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, byte) in raw.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b';' if !quoted => {
                bounds.push((start, index));
                start = index + 1;
            }
            _ => {}
        }
    }
    bounds.push((start, raw.len()));
    bounds.into_iter().map(move |(from, to)| &raw[from..to])
}

/// Cuts `value` to at most [`MAX_MIME_REPORT_BYTES`] bytes on a `char` boundary.
pub(crate) fn cap(value: &str) -> String {
    let mut capped = String::new();
    for (offset, character) in value.char_indices() {
        if offset + character.len_utf8() > MAX_MIME_REPORT_BYTES {
            break;
        }
        capped.push(character);
    }
    capped
}

/// Whether `essence` is `application/json` or `application/*+json`.
pub(crate) fn is_json_essence(essence: &str) -> bool {
    essence == JSON_ESSENCE || (essence.starts_with("application/") && essence.ends_with("+json"))
}

/// Whether `essence` is `application/x-www-form-urlencoded`.
pub(crate) fn is_form_essence(essence: &str) -> bool {
    essence == FORM_ESSENCE
}

/// Whether an optional charset is absent or spells UTF-8.
pub(crate) fn is_utf8_charset(charset: Option<&str>) -> bool {
    match charset {
        None => true,
        Some(charset) => charset == "utf-8" || charset == "utf8",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_parameters() {
        let (essence, charset) = parse(Some(
            "application/json; boundary=\";charset=x\"; charset=UTF-8",
        ));
        assert_eq!(essence, "application/json");
        assert_eq!(charset.as_deref(), Some("utf-8"));
    }

    #[test]
    fn caps_the_essence() {
        let long = "a".repeat(MAX_MIME_REPORT_BYTES * 2);
        assert_eq!(parse(Some(&long)).0.len(), MAX_MIME_REPORT_BYTES);
    }
}
