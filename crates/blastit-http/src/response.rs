//! Encoding the view's response for the client (ARCHITECTURE.md §2.3).
//!
//! A view produces a status, headers and body chunks; it never writes bytes to
//! the client. That is what lets this module be the only place with an opinion
//! about framing, and the only place that has to refuse what a view sends:
//! a header name that is not a token, or a value carrying CR, LF or NUL, would
//! otherwise let a view write a second response of its own.

use blastit_frame::RespHead;

use crate::headers;
use crate::reason::canonical_reason;

/// Why a response from a view cannot be forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseDefect {
    /// A header name is not an RFC 7230 token.
    HeaderName,
    /// A header value contains a control byte (CR, LF, NUL, …).
    HeaderValue,
    /// The reason phrase contains a control byte, so the status line would
    /// wrap.
    Reason,
}

/// How the response body is delimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// There is no body at all: a `HEAD` request, or a 1xx, 204 or 304 status.
    None,
    /// Exactly this many bytes, from the view's own `Content-Length`.
    Length(u64),
    /// `Transfer-Encoding: chunked`, for an HTTP/1.1 client.
    Chunked,
    /// No framing headers: the end of the body is the end of the connection,
    /// which is all an HTTP/1.0 client can understand.
    Close,
}

/// What the front end decided while writing a response's head: how the body
/// that follows is framed, and whether the connection survives it.
///
/// The bytes themselves are not here: [`prepare`] writes them straight into the
/// caller's buffer, so a head costs no allocation of its own and this is only
/// the decision the connection needs to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prepared {
    /// How the body that follows is framed.
    pub framing: Framing,
    /// Whether the connection may carry another request after this response.
    pub keep_alive: bool,
}

/// Check a response head before any of it is written.
///
/// # Errors
///
/// The first [`ResponseDefect`] found. The caller writes `502` when nothing has
/// been written yet, and closes the connection otherwise.
pub fn validate(head: &RespHead) -> Result<(), ResponseDefect> {
    if !head.reason.is_empty() && !headers::is_field_value(&head.reason) {
        return Err(ResponseDefect::Reason);
    }
    for (name, value) in &head.headers {
        if !headers::is_token(name) {
            return Err(ResponseDefect::HeaderName);
        }
        if !headers::is_field_value(value) {
            return Err(ResponseDefect::HeaderValue);
        }
    }
    Ok(())
}

/// Validate a response head and write it straight into `out`.
///
/// `out` is cleared first, so it holds exactly this head when the call returns
/// and is the caller's to reuse for the next response — the connection keeps one
/// such buffer for the life of the connection. Writing in place is why nothing
/// here clones a header name or value, and why the status line is rendered into
/// the same buffer rather than through `format!`: a head is the busiest
/// allocation site on the response path, and this leaves it with none.
///
/// `no_body` says the request was a `HEAD` or the status forbids a body;
/// `keep_alive` says the *client* asked for the connection to survive this
/// response. The value returned in [`Prepared::keep_alive`] is the decision,
/// which also covers the cases where the framing forces a close.
///
/// # Errors
///
/// The first [`ResponseDefect`] found, in which case `out` is left untouched.
/// The caller writes `502` when nothing has been written yet, and closes the
/// connection otherwise.
pub fn prepare(
    head: &RespHead,
    no_body: bool,
    http11: bool,
    keep_alive: bool,
    out: &mut Vec<u8>,
) -> Result<Prepared, ResponseDefect> {
    validate(head)?;

    let declared = headers::content_length(&head.headers).ok().flatten();

    let framing = if no_body {
        Framing::None
    } else if let Some(len) = declared {
        Framing::Length(len)
    } else if http11 {
        Framing::Chunked
    } else {
        Framing::Close
    };
    let keep_alive = keep_alive && framing != Framing::Close;

    // The framing is ours to decide, so a `Content-Length` only survives when
    // it is the framing we chose. (For a bodyless response it describes what a
    // `GET` would have returned, which is what a `HEAD` answer is for.) 1xx and
    // 204 can never carry a body, so a length on them is a lie; a 304 keeps it,
    // reporting what the cached `GET` would have returned.
    let drop_length = matches!(framing, Framing::Chunked | Framing::Close)
        || matches!(head.status, 100..=199 | 204);

    out.clear();
    out.extend_from_slice(b"HTTP/1.1 ");
    push_decimal(out, head.status);
    out.push(b' ');
    let reason: &[u8] = if head.reason.is_empty() {
        canonical_reason(head.status).unwrap_or("").as_bytes()
    } else {
        &head.reason
    };
    out.extend_from_slice(reason);
    out.extend_from_slice(b"\r\n");

    // Hop-by-hop fields belong to the view's own hop, not to the client's. The
    // filter is load-bearing: it is what keeps a view's `Connection` or
    // `Transfer-Encoding` from reaching the client, so it is applied here, per
    // header, rather than by building a stripped copy first.
    for (name, value) in &head.headers {
        if headers::is_hop_by_hop(&head.headers, name) {
            continue;
        }
        if drop_length && headers::is(name, "content-length") {
            continue;
        }
        out.extend_from_slice(name);
        out.extend_from_slice(b": ");
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    }
    if framing == Framing::Chunked {
        out.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
    }
    if !keep_alive {
        out.extend_from_slice(b"Connection: close\r\n");
    } else if !http11 {
        out.extend_from_slice(b"Connection: keep-alive\r\n");
    }
    out.extend_from_slice(b"\r\n");

    Ok(Prepared {
        framing,
        keep_alive,
    })
}

/// Write a `u16` as plain decimal into `out`, without allocating.
///
/// A status code is at most five digits, so it fits a small stack array;
/// `format!` would allocate a `String` for the busiest line of every response.
fn push_decimal(out: &mut Vec<u8>, value: u16) {
    let mut digits = [0u8; 5];
    let mut index = digits.len();
    let mut rest = value;
    loop {
        index -= 1;
        digits[index] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[index..]);
}

/// The status line and headers of a response the front end writes itself: a
/// refusal, or a `502` for a response that never arrived.
pub fn simple(status: u16) -> Vec<u8> {
    let phrase = canonical_reason(status).unwrap_or("");
    let mut out = format!("HTTP/1.1 {status} {phrase}\r\n").into_bytes();
    out.extend_from_slice(b"Content-Length: 0\r\nConnection: close\r\n\r\n");
    out
}

/// The interim `100 Continue` answer to `Expect: 100-continue`.
pub const CONTINUE: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

/// The terminating chunk of a chunked body.
pub const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

/// One chunk of a chunked body, size line included.
pub fn chunk(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    out
}
