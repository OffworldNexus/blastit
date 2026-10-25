//! Turning a request target into the `PATH_INFO` the routing table matches
//! (ARCHITECTURE.md §3.2, §6).
//!
//! The steps, in order:
//!
//! 1. cut the target at the first `?`: a query string is never routed on;
//! 2. percent-decode what is left — `%2F` decodes to `/`, exactly like WSGI's
//!    `PATH_INFO`, so `%2F` does *not* create an extra path segment;
//! 3. require the decoded path to start with `/`;
//! 4. strip `BLASTIT_SCRIPT_NAME` at a segment boundary (WSGI's mount prefix).
//!
//! Anything that fails step 2 or 3 is a `400`. The view then rebuilds its own
//! `environ` from the original target, so the scheduler never rewrites the
//! request it forwards.

/// The request target without its query string (§8.1: query strings are never
/// logged, and they are never routed on).
///
/// A `#` is left alone: fragments are not sent in a request line, but if one
/// appears it is part of the path bytes as far as blastit is concerned.
pub(crate) fn path_of_target(target: &[u8]) -> &[u8] {
    match target.iter().position(|byte| *byte == b'?') {
        Some(end) => &target[..end],
        None => target,
    }
}

/// Percent-decodes a request path.
///
/// Returns `None` for a `%` that is not followed by two hexadecimal digits:
/// blastit will not guess at what the client meant (§2.4 answers `400`).
pub(crate) fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let byte = input[index];
        if byte == b'%' {
            let high = hex(input.get(index + 1)?)?;
            let low = hex(input.get(index + 2)?)?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    Some(out)
}

/// The value of one hexadecimal digit.
fn hex(byte: &u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Strips a `SCRIPT_NAME` mount prefix from a path, as WSGI does (§6).
///
/// The prefix has to end on a segment boundary: `/app` matches `/app` and
/// `/app/x`, but not `/application`. `/app` itself becomes `/`, so that a
/// route pattern like `/` can still match the mount point.
///
/// Returns `None` when the path is not inside the mount, which the caller
/// answers with a `404`.
pub(crate) fn strip_script_name<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    // Both an empty prefix and `/` mount at the root: nothing to strip.
    if prefix.is_empty() || prefix == "/" {
        return Some(path);
    }
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() {
        return Some("/");
    }
    if rest.starts_with('/') {
        return Some(rest);
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn target_is_cut_at_the_first_question_mark() {
        assert_eq!(path_of_target(b"/a/b?x=1"), b"/a/b");
        assert_eq!(path_of_target(b"/a/b?"), b"/a/b");
        assert_eq!(path_of_target(b"/a/b"), b"/a/b");
        assert_eq!(path_of_target(b"/a?x=1?y=2"), b"/a");
        assert_eq!(path_of_target(b"?only=1"), b"");
        assert_eq!(path_of_target(b""), b"");
        // A `#` is not a query string.
        assert_eq!(path_of_target(b"/a#b?x=1"), b"/a#b");
    }

    #[test]
    fn percent_decoding_handles_every_case() {
        assert_eq!(percent_decode(b"/a/b").unwrap(), b"/a/b");
        assert_eq!(percent_decode(b"/a%20b").unwrap(), b"/a b");
        assert_eq!(percent_decode(b"/%41%42").unwrap(), b"/AB");
        assert_eq!(percent_decode(b"/%61").unwrap(), b"/a");
        // `%2F` decodes to a slash; it does not hide a segment boundary.
        assert_eq!(percent_decode(b"/a%2Fb").unwrap(), b"/a/b");
        assert_eq!(percent_decode(b"/%00").unwrap(), b"/\0");
        assert_eq!(percent_decode(b"").unwrap(), b"");
        // A literal `%` needs two hex digits.
        assert_eq!(percent_decode(b"/%"), None);
        assert_eq!(percent_decode(b"/%2"), None);
        assert_eq!(percent_decode(b"/%zz"), None);
        assert_eq!(percent_decode(b"%g0"), None);
        // ... and a decoded `%` is fine.
        assert_eq!(percent_decode(b"/%25").unwrap(), b"/%");
        // Non-ASCII passes through untouched.
        assert_eq!(percent_decode(b"/\xc3\xa9").unwrap(), b"/\xc3\xa9");
        assert_eq!(percent_decode(b"/%C3%A9").unwrap(), b"/\xc3\xa9");
    }

    #[test]
    fn script_name_is_stripped_at_a_segment_boundary() {
        assert_eq!(strip_script_name("/app/x", "/app"), Some("/x"));
        assert_eq!(strip_script_name("/app/", "/app"), Some("/"));
        assert_eq!(strip_script_name("/app", "/app"), Some("/"));
        assert_eq!(strip_script_name("/app/x", "/"), Some("/app/x"));
        // Not a boundary.
        assert_eq!(strip_script_name("/application", "/app"), None);
        assert_eq!(strip_script_name("/appendix/x", "/app"), None);
        // Not inside the mount at all.
        assert_eq!(strip_script_name("/other/x", "/app"), None);
        assert_eq!(strip_script_name("/", "/app"), None);
        assert_eq!(strip_script_name("", "/app"), None);
        // An empty prefix strips nothing.
        assert_eq!(strip_script_name("/x", ""), Some("/x"));
    }
}
