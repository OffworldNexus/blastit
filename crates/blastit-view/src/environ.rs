//! The WSGI `environ` of PEP 3333, built from a `REQ_HEAD`.

use std::borrow::Cow;
use std::sync::Mutex;

use blastit_frame::{ReqHead, Scheme};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyString, PyTuple};

use crate::input::Input;
use crate::sync::lock;

/// The interpreter's `sys.stderr`, resolved once per process.
///
/// `wsgi.errors` is the same file for every request either way, but resolving
/// it costs an `import` and an attribute lookup, and neither can change while
/// the process lives; doing it once keeps both off the request path. The cell
/// is only ever filled while the GIL is held — every request thread holds it as
/// it builds an environ — and its initialiser releases the GIL rather than
/// blocking on it, so the first thread to need the value fills the cell the
/// others read and no thread can observe a half-written one.
static STDERR: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

/// `wsgi.version`, the `(1, 0)` tuple, built once per process.
///
/// PEP 3333 fixes both elements, so every request wants the very same object;
/// it is built once, under the GIL, exactly as [`STDERR`] is.
static WSGI_VERSION: PyOnceLock<Py<PyTuple>> = PyOnceLock::new();

/// A bounded cache of the decimal form of a port, keyed by the port.
///
/// A request writes a port at most twice — `SERVER_PORT`, and `REMOTE_PORT` when
/// the peer has one — and both draw from a small set: a process serves one
/// listening port for as long as it lives, and a client's source port is
/// ephemeral. Reusing the `str` saves a `String` and a Python `str` per port.
/// The cache is **deliberately bounded**: `REMOTE_PORT` is chosen by the client,
/// so an unbounded map keyed by it would let a peer grow this process's memory
/// with a stream of fresh source ports. At [`PORT_CACHE`] entries the oldest is
/// evicted, which is what caps what a peer can make this process retain; the
/// keys are `u16` besides, so the whole key space is bounded by construction.
static PORTS: Mutex<Vec<(u16, Py<PyString>)>> = Mutex::new(Vec::new());

/// How many port strings a process keeps — see [`PORTS`].
///
/// Small on purpose: one entry for the process's own port and a handful for the
/// clients a view serves at once is the whole working set.
const PORT_CACHE: usize = 16;

/// Build the `environ` dictionary for one request.
///
/// Every value is a `str` (PEP 3333), decoded as latin-1 wherever the input was
/// bytes: a request path or a header value is a byte string on the wire, and
/// latin-1 is the bijection PEP 3333 prescribes for turning it into text.
///
/// Nothing here that is the same for every request is built from scratch: the
/// keys are interned, the constant values are cached (the `(1, 0)` tuple,
/// `sys.stderr`, the scheme names, `HTTP/1.1`) and the ports come from a bounded
/// cache, so a request reuses objects — and their cached string hashes — instead
/// of minting a fresh `str` for each one. The one deliberate exception is a
/// header's own `HTTP_*` key, which is the client's to choose (see the loop
/// below).
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
    env.set_item(intern!(py, "REQUEST_METHOD"), latin1(&head.method))?;
    env.set_item(intern!(py, "SCRIPT_NAME"), script_name)?;
    env.set_item(
        intern!(py, "PATH_INFO"),
        strip_script_name(&latin1(&percent_decode(raw_path)), script_name),
    )?;
    env.set_item(intern!(py, "QUERY_STRING"), latin1(query))?;
    if let Some(value) = headers.content_type {
        env.set_item(intern!(py, "CONTENT_TYPE"), latin1(&value))?;
    }
    if let Some(value) = headers.content_length {
        env.set_item(intern!(py, "CONTENT_LENGTH"), latin1(&value))?;
    }
    env.set_item(intern!(py, "SERVER_NAME"), &head.server_name)?;
    env.set_item(intern!(py, "SERVER_PORT"), port_str(py, head.server_port)?)?;
    env.set_item(intern!(py, "SERVER_PROTOCOL"), intern!(py, "HTTP/1.1"))?;
    env.set_item(intern!(py, "REMOTE_ADDR"), &head.remote_addr)?;
    // CGI splits the connection's address in two, and PEP 3333 keeps the split:
    // `REMOTE_ADDR` is the address, `REMOTE_PORT` is its port. The only port a
    // request carries is the connection's — a trusted client-IP chain entry is
    // an address and nothing else — so a peer that has no port (a socket with no
    // name) has no `REMOTE_PORT` either, and the key is absent rather than
    // empty.
    if let Some(port) = remote_port(&head.peer_addr) {
        env.set_item(intern!(py, "REMOTE_PORT"), port_str(py, port)?)?;
    }
    // A header's name is the client's, so its `HTTP_*` key is decoded (by
    // `latin1`, which borrows an ASCII name rather than copying it) but **not**
    // cached: a cache keyed by it would grow with whatever a peer sends, and a
    // view process must not be grown by its input. The values are the client's
    // too and are minted fresh for the same reason.
    for (name, value) in &headers.http {
        env.set_item(latin1(name), latin1(value))?;
    }

    env.set_item(intern!(py, "wsgi.version"), wsgi_version(py)?)?;
    env.set_item(intern!(py, "wsgi.url_scheme"), scheme(py, head.scheme))?;
    env.set_item(intern!(py, "wsgi.input"), input)?;
    env.set_item(intern!(py, "wsgi.errors"), stderr(py)?)?;
    // `bool` converts to the interpreter's `True`/`False` singletons, so these
    // two flags allocate no Python object even written per request.
    env.set_item(intern!(py, "wsgi.multithread"), true)?;
    env.set_item(intern!(py, "wsgi.multiprocess"), true)?;
    env.set_item(intern!(py, "wsgi.run_once"), false)?;
    env.set_item(intern!(py, "wsgi.input_terminated"), true)?;
    env.set_item(intern!(py, "blastit.route"), route_name)?;
    Ok(env)
}

/// The interpreter's `sys.stderr` — `wsgi.errors` — from [`STDERR`].
fn stderr<'py>(py: Python<'py>) -> PyResult<&'py Bound<'py, PyAny>> {
    let cached = STDERR.get_or_try_init(py, || -> PyResult<Py<PyAny>> {
        Ok(py.import("sys")?.getattr("stderr")?.unbind())
    })?;
    Ok(cached.bind(py))
}

/// `wsgi.version` as the `(1, 0)` tuple, from [`WSGI_VERSION`].
fn wsgi_version<'py>(py: Python<'py>) -> PyResult<&'py Bound<'py, PyTuple>> {
    let cached = WSGI_VERSION.get_or_try_init(py, || -> PyResult<Py<PyTuple>> {
        Ok(PyTuple::new(py, [1i32, 0i32])?.unbind())
    })?;
    Ok(cached.bind(py))
}

/// `wsgi.url_scheme` for a request's scheme.
///
/// The two values never change, so each is interned once and every request
/// writes the same object — which is also the same cached string hash rather
/// than one recomputed per request.
fn scheme<'py>(py: Python<'py>, scheme: Scheme) -> &'py Bound<'py, PyString> {
    match scheme {
        Scheme::Http => intern!(py, "http"),
        Scheme::Https => intern!(py, "https"),
    }
}

/// The `str` for `port`, from [`PORTS`] or a fresh conversion.
///
/// The cache's lock is held around the lookup and around the insert, and no
/// Python runs while it is held — the `to_string` and `PyString::new` that can
/// allocate are called with it released — which is what keeps a lock that is
/// not the GIL from ever being held across a Python call.
fn port_str<'py>(py: Python<'py>, port: u16) -> PyResult<Bound<'py, PyString>> {
    let cached = {
        let cache = lock(&PORTS);
        cache
            .iter()
            .find(|(cached, _)| *cached == port)
            .map(|(_, text)| text.bind(py).clone())
    };
    if let Some(text) = cached {
        return Ok(text);
    }
    let text = PyString::new(py, &port.to_string());
    let mut cache = lock(&PORTS);
    // Another thread may have filled the entry between the two locks.
    cache.retain(|(cached, _)| *cached != port);
    if cache.len() >= PORT_CACHE {
        cache.remove(0);
    }
    cache.push((port, text.clone().unbind()));
    Ok(text)
}

/// Decode bytes as latin-1 (PEP 3333's `str` for byte-ish values), borrowing
/// when the bytes are pure ASCII.
///
/// A byte below `0x80` is the same character in latin-1 and in UTF-8, so an
/// all-ASCII slice — a method, a header name, most header values — can go to
/// Python as it is, with no intermediate `String`. A byte above `0x80` still
/// goes the long way through [`latin1_decode`], because UTF-8 would read it as
/// the start of a sequence rather than as the one character latin-1 says it is.
///
/// The decode is not done straight into a Python `str`: the only C entry point
/// that would, `PyUnicode_DecodeLatin1`, is reachable only through `ffi`, and
/// this crate forbids `unsafe`.
fn latin1(bytes: &[u8]) -> Cow<'_, str> {
    if bytes.is_ascii()
        && let Ok(text) = std::str::from_utf8(bytes)
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(latin1_decode(bytes))
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
    fn latin1_borrows_ascii_and_owns_the_rest() {
        // The fast path must be invisible: `latin1` returns exactly what the
        // character-by-character decode does, for every byte a request can
        // carry, while borrowing when there is nothing to decode.
        for byte in 0..=0xFFu8 {
            assert_eq!(latin1(&[byte]), latin1_decode(&[byte]));
        }
        assert!(matches!(latin1(b"GET"), Cow::Borrowed("GET")));
        assert!(matches!(latin1(&[0xE9]), Cow::Owned(ref text) if text == "é"));
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
        Python::attach(|py| {
            assert_eq!(scheme(py, Scheme::Http).to_str().expect("ascii"), "http");
            assert_eq!(scheme(py, Scheme::Https).to_str().expect("ascii"), "https");
            // Interned: it is the same object every time, not just the same
            // text, which is what lets a request reuse the cached string hash.
            assert_eq!(
                scheme(py, Scheme::Http).as_ptr(),
                scheme(py, Scheme::Http).as_ptr(),
                "the scheme string is interned"
            );
        });
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
