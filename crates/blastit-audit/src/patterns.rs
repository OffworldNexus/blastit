//! The `Blastfile` pattern one requested path is written as (`§8.1`–`§8.3`).
//!
//! A quoted pattern matches its own bytes, so only numeric and UUID-shaped
//! segments become parameters; everything else is copied verbatim. A path that
//! cannot be written as a quoted pattern at all — a `{`, a `}`, a `"`, a `\` or
//! a control byte — proposes nothing, because one unroutable request must not
//! make the whole edit refuse.
/// Whether a requested path can be written as a quoted `Blastfile` pattern at
/// all (§8.3).
///
/// A `{` or `}` would be read as a parameter, a `"` or `\` would end the
/// string, and a control byte has no place in a quoted pattern. Such a path is
/// not proposed: one unroutable request must not make the writer refuse the
/// whole document.
pub fn pattern_is_writable(path: &str) -> bool {
    !path
        .chars()
        .any(|character| matches!(character, '{' | '}' | '"' | '\\') || character.is_control())
}

/// One observed path as the `Blastfile` pattern that matches it (§8.1).
///
/// Numeric and UUID-shaped segments become unique `{idN:int}`/`{idN:uuid}`
/// parameters; every other segment is copied verbatim, because a quoted pattern
/// matches its own bytes. `None` when the path cannot be written as one
/// ([`pattern_is_writable`]).
pub(crate) fn writable_pattern(path: &str) -> Option<String> {
    if !pattern_is_writable(path) {
        return None;
    }
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if segments.is_empty() {
        return Some("/".to_string());
    }
    let mut pattern = String::from("/");
    let mut parameters = 0u32;
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            pattern.push('/');
        }
        if segment.chars().all(|character| character.is_ascii_digit()) {
            parameters = parameters.saturating_add(1);
            pattern.push_str(&format!("{{id{parameters}:int}}"));
        } else if is_uuid(segment) {
            parameters = parameters.saturating_add(1);
            pattern.push_str(&format!("{{id{parameters}:uuid}}"));
        } else {
            pattern.push_str(segment);
        }
    }
    if path.ends_with('/') {
        pattern.push('/');
    }
    Some(pattern)
}

/// Whether `segment` is shaped like a UUID (`8-4-4-4-12` hex digits).
fn is_uuid(segment: &str) -> bool {
    segment.len() == 36
        && segment.char_indices().all(|(at, character)| match at {
            8 | 13 | 18 | 23 => character == '-',
            _ => character.is_ascii_hexdigit(),
        })
}
