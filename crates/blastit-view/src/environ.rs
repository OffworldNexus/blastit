//! The WSGI `environ` of PEP 3333, built from a `REQ_HEAD`.

use blastit_frame::{ReqHead, Scheme};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::input::Input;

/// Build the `environ` dictionary for one request.
///
/// Every value is a `str` (PEP 3333), decoded as latin-1 wherever the input was
/// bytes: a request path or a header value is a byte string on the wire, and
/// latin-1 is the bijection PEP 3333 prescribes for turning it into text.
///
/// # Errors
///
/// Whatever the interpreter raises while the dictionary is built; these are
/// allocation failures and a missing `sys.stderr`, not request errors.
pub(crate) fn build<'py>(
    py: Python<'py>,
    head: &ReqHead,
    route_name: &str,
    script_name: &str,
    input: &Bound<'py, Input>,
) -> PyResult<Bound<'py, PyDict>> {
    let headers = partition_headers(&head.headers);
    let (raw_path, query) = split_target(&head.target);

    let env = PyDict::new(py);
    env.set_item("REQUEST_METHOD", latin1_decode(&head.method))?;
    env.set_item("SCRIPT_NAME", script_name)?;
    env.set_item(
        "PATH_INFO",
        strip_script_name(&latin1_decode(&percent_decode(raw_path)), script_name),
    )?;
    env.set_item("QUERY_STRING", latin1_decode(query))?;
    if let Some(value) = headers.content_type {
        env.set_item("CONTENT_TYPE", latin1_decode(&value))?;
    }
    if let Some(value) = headers.content_length {
        env.set_item("CONTENT_LENGTH", latin1_decode(&value))?;
    }
    env.set_item("SERVER_NAME", &head.server_name)?;
    env.set_item("SERVER_PORT", head.server_port.to_string())?;
    env.set_item("SERVER_PROTOCOL", "HTTP/1.1")?;
    env.set_item("REMOTE_ADDR", &head.remote_addr)?;
    // CGI splits the connection's address in two, and PEP 3333 keeps the split:
    // `REMOTE_ADDR` is the address, `REMOTE_PORT` is its port. The only port a
    // request carries is the connection's — a trusted client-IP chain entry is
    // an address and nothing else — so a peer that has no port (a socket with no
    // name) has no `REMOTE_PORT` either, and the key is absent rather than
    // empty.
    if let Some(port) = remote_port(&head.peer_addr) {
        env.set_item("REMOTE_PORT", port.to_string())?;
    }
    for (name, value) in &headers.http {
        env.set_item(latin1_decode(name), latin1_decode(value))?;
    }

    env.set_item("wsgi.version", PyTuple::new(py, [1i32, 0i32])?)?;
    env.set_item("wsgi.url_scheme", scheme_name(head.scheme))?;
    env.set_item("wsgi.input", input)?;
    env.set_item("wsgi.errors", py.import("sys")?.getattr("stderr")?)?;
    env.set_item("wsgi.multithread", true)?;
    env.set_item("wsgi.multiprocess", true)?;
    env.set_item("wsgi.run_once", false)?;
    env.set_item("wsgi.input_terminated", true)?;
    env.set_item("blastit.route", route_name)?;
    Ok(env)
}

/// `wsgi.url_scheme` for a request's scheme.
fn scheme_name(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Http => "http",
        Scheme::Https => "https",
    }
}

/// The port `REMOTE_PORT` reports: the one the request arrived from.
///
/// CGI and PEP 3333 split a connection's address into `REMOTE_ADDR`, which is
/// the address alone, and `REMOTE_PORT`. The port is [`ReqHead::peer_addr`]'s —
/// the only place in a request that has one, because a client-IP chain entry is
/// an address and nothing else. A peer that is not an address at all is a
/// *connection* with no name, and then the app is told nothing rather than told
/// wrongly.
///
/// When a trusted proxy's chain named the client, the port here is the
/// *connection's* (the proxy's): no port for the client exists anywhere in the
/// request, and `REMOTE_ADDR` is still who the chain says.
fn remote_port(peer: &str) -> Option<u16> {
    blastit_frame::parse_port(peer)
}

/// Split a request target into its path and its query string, both still raw.
///
/// §3.1's `target` is origin form (`/x?y=1`); the query includes everything
/// after the first `?` and may be empty, which is what PEP 3333 wants for
/// `QUERY_STRING`.
fn split_target(target: &[u8]) -> (&[u8], &[u8]) {
    match target.iter().position(|byte| *byte == b'?') {
        Some(at) => (&target[..at], &target[at + 1..]),
        None => (target, &[]),
    }
}

/// Percent-decode a raw path to bytes.
///
/// `%XX` with two hexadecimal digits becomes the byte; `+` is left alone (it is
/// a space only in query strings), and so is a `%` that does not start a
/// complete pair. Nothing is redirected into a decoded `%2F`: the decoded bytes
/// are what the application sees in `PATH_INFO`, exactly as PEP 3333 describes.
fn percent_decode(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%'
            && at + 2 < raw.len()
            && let (Some(high), Some(low)) = (hex_value(raw[at + 1]), hex_value(raw[at + 2]))
        {
            out.push(high * 16 + low);
            at += 3;
            continue;
        }
        out.push(raw[at]);
        at += 1;
    }
    out
}

/// The value of one hexadecimal digit.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decode bytes as latin-1 (PEP 3333's `str` for byte-ish values).
fn latin1_decode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Encode a `str` as latin-1, refusing characters that have no byte.
///
/// Header names and values, and the reason phrase, travel as bytes; PEP 3333
/// says they are latin-1, so a character above `U+00FF` cannot be represented
/// and is an application error.
pub(crate) fn latin1_bytes(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let code = u32::from(ch);
        if code > 0xFF {
            return None;
        }
        out.push(code as u8);
    }
    Some(out)
}

/// Remove `script_name` from the front of a decoded path.
///
/// The strip only happens on a path boundary: with `SCRIPT_NAME` `/app`, the
/// path `/app/list` becomes `/list`, while `/application` is left alone —
/// otherwise a route mounted at `/app` would corrupt the path of a sibling
/// route. A path that does not start with the mount point is returned as it is,
/// because §2.5's sandbox is the allowlist, not the routing.
fn strip_script_name<'a>(path: &'a str, script_name: &str) -> &'a str {
    if script_name.is_empty() {
        return path;
    }
    match path.strip_prefix(script_name) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
        _ => path,
    }
}

/// The headers of a request, sorted into their `environ` roles.
struct EnvironHeaders {
    /// The `CONTENT_TYPE` header, joined if it repeats.
    content_type: Option<Vec<u8>>,
    /// The `CONTENT_LENGTH` header, joined if it repeats.
    content_length: Option<Vec<u8>>,
    /// Every other header, as an `HTTP_*` key and its value.
    http: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The `CONTENT_TYPE`, the `CONTENT_LENGTH` and the rest of the headers.
///
/// `CONTENT_TYPE` and `CONTENT_LENGTH` become their own `environ` keys; every
/// other header becomes `HTTP_<NAME>`, with the name upper-cased and `-`
/// replaced by `_`. Repeated headers are joined with `", "` in arrival order,
/// as PEP 3333 allows.
///
/// A header whose name contains an underscore is **dropped**: otherwise a
/// header called `X_User` and one called `X-User` would both land on
/// `HTTP_X_USER`, and a client could smuggle a value past whatever the
/// application's front end or middleware believes it validated (the classic
/// `X-Forwarded-For` vs `X_Forwarded_For` trick). An empty name is dropped for
/// the same reason: it would become the bare `HTTP_`.
fn partition_headers(headers: &[(Vec<u8>, Vec<u8>)]) -> EnvironHeaders {
    let mut content_type = None;
    let mut content_length = None;
    let mut rest: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(headers.len());

    for (name, value) in headers {
        if name.is_empty() || name.contains(&b'_') {
            continue;
        }
        if name.eq_ignore_ascii_case(b"content-type") {
            merge(&mut content_type, value);
        } else if name.eq_ignore_ascii_case(b"content-length") {
            merge(&mut content_length, value);
        } else if let Some(key) = http_name(name) {
            merge2(&mut rest, key, value);
        }
    }
    EnvironHeaders {
        content_type,
        content_length,
        http: rest,
    }
}

/// The `HTTP_*` key for a header name.
fn http_name(name: &[u8]) -> Option<Vec<u8>> {
    if name.is_empty() || name.contains(&b'_') {
        return None;
    }
    let mut out = Vec::with_capacity(name.len() + 5);
    out.extend_from_slice(b"HTTP_");
    for byte in name {
        out.push(match *byte {
            b'-' => b'_',
            other => other.to_ascii_uppercase(),
        });
    }
    Some(out)
}

/// Append `value` to a single-value slot, joining with `", "` when it repeats.
fn merge(slot: &mut Option<Vec<u8>>, value: &[u8]) {
    match slot {
        Some(joined) => {
            joined.extend_from_slice(b", ");
            joined.extend_from_slice(value);
        }
        None => *slot = Some(value.to_vec()),
    }
}

/// Append `value` to the key in `list`, joining with `", "` when it repeats.
fn merge2(list: &mut Vec<(Vec<u8>, Vec<u8>)>, key: Vec<u8>, value: &[u8]) {
    if let Some((_, joined)) = list.iter_mut().find(|(name, _)| *name == key) {
        joined.extend_from_slice(b", ");
        joined.extend_from_slice(value);
        return;
    }
    list.push((key, value.to_vec()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(pairs: &[&[u8]]) -> Vec<(Vec<u8>, Vec<u8>)> {
        pairs
            .iter()
            .map(|entry| {
                let at = entry
                    .iter()
                    .position(|byte| *byte == 0)
                    .expect("a name and a value");
                (entry[..at].to_vec(), entry[at + 1..].to_vec())
            })
            .collect()
    }

    #[test]
    fn the_target_splits_into_path_and_query() {
        assert_eq!(
            split_target(b"/a/b?x=1&y=%20"),
            (&b"/a/b"[..], &b"x=1&y=%20"[..])
        );
        assert_eq!(split_target(b"/a?"), (&b"/a"[..], &b""[..]));
        assert_eq!(split_target(b"/a"), (&b"/a"[..], &b""[..]));
        assert_eq!(split_target(b""), (&b""[..], &b""[..]));
        // Only the first `?` separates: query strings may contain more.
        assert_eq!(split_target(b"/a?b?c"), (&b"/a"[..], &b"b?c"[..]));
    }

    #[test]
    fn percent_decoding_handles_every_shape() {
        assert_eq!(percent_decode(b"/a%20b"), b"/a b");
        assert_eq!(percent_decode(b"%2F"), b"/");
        assert_eq!(percent_decode(b"%e2%9c%93"), "✓".as_bytes());
        // `+` is not a space in a path, and malformed escapes survive.
        assert_eq!(percent_decode(b"a+b"), b"a+b");
        assert_eq!(percent_decode(b"%zz"), b"%zz");
        assert_eq!(percent_decode(b"%"), b"%");
        assert_eq!(percent_decode(b"a%4"), b"a%4");
        assert_eq!(percent_decode(b"%4f`"), b"O`");
        assert_eq!(percent_decode(b""), b"");
    }

    #[test]
    fn latin1_round_trips_and_rejects_wider_characters() {
        assert_eq!(latin1_decode(&[0xE9]), "é");
        assert_eq!(latin1_bytes("é"), Some(vec![0xE9]));
        assert_eq!(latin1_bytes("✓"), None);
        assert_eq!(latin1_bytes(""), Some(Vec::new()));
        assert_eq!(latin1_decode(b""), "");
        for byte in 0..=0xFFu8 {
            assert_eq!(latin1_bytes(&latin1_decode(&[byte])), Some(vec![byte]));
        }
    }

    #[test]
    fn the_script_name_is_stripped_only_on_a_boundary() {
        assert_eq!(strip_script_name("/app/list", "/app"), "/list");
        assert_eq!(strip_script_name("/app", "/app"), "");
        assert_eq!(strip_script_name("/app/", "/app"), "/");
        assert_eq!(strip_script_name("/application", "/app"), "/application");
        assert_eq!(strip_script_name("/other", "/app"), "/other");
        assert_eq!(strip_script_name("/app", ""), "/app");
        assert_eq!(strip_script_name("", "/app"), "");
    }

    #[test]
    fn header_names_become_http_keys() {
        assert_eq!(
            http_name(b"X-Forwarded-For").as_deref(),
            Some(&b"HTTP_X_FORWARDED_FOR"[..])
        );
        assert_eq!(http_name(b"accept").as_deref(), Some(&b"HTTP_ACCEPT"[..]));
        assert_eq!(
            http_name(b"X_User").as_deref(),
            None,
            "underscores are dropped"
        );
        assert_eq!(http_name(b"").as_deref(), None);
        // Non-ASCII bytes are left as they are: latin-1 decoding turns them
        // back into the character the client sent.
        assert_eq!(
            http_name(&[0xE9]).as_deref(),
            Some(&[b'H', b'T', b'T', b'P', b'_', 0xE9][..])
        );
        assert_eq!(
            http_name(&[0xC3, 0xA9]).as_deref(),
            Some(&b"HTTP_\xC3\xA9"[..])
        );
    }

    #[test]
    fn content_headers_are_split_out_and_repeats_are_joined() {
        let headers = bytes(&[
            b"Host\x00example.test",
            b"X-A\x001",
            b"Content-Type\x00text/plain",
            b"X-A\x002",
            b"content-length\x0012",
            b"X_User\x00evil",
            b"Cookie\x00a=1",
            b"\x00empty-name",
        ]);
        let headers = partition_headers(&headers);
        assert_eq!(headers.content_type.as_deref(), Some(&b"text/plain"[..]));
        assert_eq!(headers.content_length.as_deref(), Some(&b"12"[..]));
        assert_eq!(
            headers.http,
            bytes(&[
                b"HTTP_HOST\x00example.test",
                b"HTTP_X_A\x001, 2",
                b"HTTP_COOKIE\x00a=1"
            ])
        );
    }

    #[test]
    fn a_repeated_content_header_is_still_joined_once() {
        let raw = bytes(&[b"content-type\x00a", b"Content-Type\x00b"]);
        let headers = partition_headers(&raw);
        assert_eq!(headers.content_type.as_deref(), Some(&b"a, b"[..]));
        assert_eq!(headers.content_length, None);
        assert!(headers.http.is_empty());
    }

    #[test]
    fn lowercase_but_dashed_content_names_still_match() {
        // Only `_` is a smuggling vector; case is not.
        let raw = bytes(&[b"CONTENT-LENGTH\x007"]);
        let headers = partition_headers(&raw);
        assert_eq!(headers.content_length.as_deref(), Some(&b"7"[..]));
        assert!(headers.http.is_empty());
    }

    #[test]
    fn scheme_names_are_the_wsgi_ones() {
        assert_eq!(scheme_name(Scheme::Http), "http");
        assert_eq!(scheme_name(Scheme::Https), "https");
    }

    #[test]
    fn the_port_of_a_connection_comes_from_the_peer_address() {
        assert_eq!(remote_port("203.0.113.7:51000"), Some(51000));
        assert_eq!(remote_port("[2001:db8::1]:443"), Some(443));
        assert_eq!(remote_port("[::1]"), None, "no port after the brackets");
        assert_eq!(remote_port("2001:db8::1"), None, "a bare IPv6 address");
        assert_eq!(remote_port("203.0.113.7"), None, "a bare IPv4 address");
        assert_eq!(remote_port("unnamed-socket"), None, "not an address");
        assert_eq!(remote_port(""), None);
        assert_eq!(remote_port("203.0.113.7:70000"), None, "not a port");
        assert_eq!(
            remote_port("203.0.113.7:0"),
            None,
            "port zero is not a port a client connected from"
        );
        assert_eq!(remote_port("[::1]:0"), None);
    }
}
