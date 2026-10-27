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

impl ResponseDefect {
    /// A short label for error messages and tests.
    #[cfg(test)]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::HeaderName => "invalid header name",
            Self::HeaderValue => "invalid header value",
            Self::Reason => "invalid reason phrase",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn head(status: u16, headers: &[(&str, &str)]) -> RespHead {
        RespHead {
            status,
            reason: Vec::new(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
        }
    }

    /// Prepare `head` into a fresh buffer, returning the bytes and the decision.
    fn encode(
        head: &RespHead,
        no_body: bool,
        http11: bool,
        keep_alive: bool,
    ) -> (Vec<u8>, Prepared) {
        let mut out = Vec::new();
        let prepared = prepare(head, no_body, http11, keep_alive, &mut out).unwrap();
        (out, prepared)
    }

    /// The header names of an encoded head, in order.
    fn names(encoded: &[u8]) -> Vec<String> {
        let end = encoded
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map_or(encoded.len(), |index| index + 2);
        let text = String::from_utf8_lossy(&encoded[..end]);
        text.split("\r\n")
            .skip(1)
            .filter_map(|line| line.split_once(':').map(|(name, _)| name.to_string()))
            .collect()
    }

    #[test]
    fn the_status_line_uses_the_registered_phrase_when_the_view_gives_none() {
        let (out, _) = encode(&head(200, &[]), false, true, true);
        assert!(out.starts_with(b"HTTP/1.1 200 OK\r\n"), "{out:?}");

        let mut resp = head(599, &[]);
        resp.reason = b"Custom".to_vec();
        let (out, _) = encode(&resp, false, true, true);
        assert!(out.starts_with(b"HTTP/1.1 599 Custom\r\n"), "{out:?}");

        // An unknown status with no reason keeps the space, as RFC 9110 spells
        // the status line.
        let (out, _) = encode(&head(599, &[]), false, true, true);
        assert!(out.starts_with(b"HTTP/1.1 599 \r\n"), "{out:?}");
    }

    #[test]
    fn injection_attempts_are_refused() {
        // Each defect has a label of its own, for messages and for tests.
        assert_eq!(ResponseDefect::HeaderName.detail(), "invalid header name");
        assert_eq!(ResponseDefect::HeaderValue.detail(), "invalid header value");
        assert_eq!(ResponseDefect::Reason.detail(), "invalid reason phrase");

        let mut resp = head(200, &[("X-Ok", "fine")]);
        resp.headers
            .push((b"X-Bad".to_vec(), b"a\r\nX-Smuggled: 1".to_vec()));
        assert_eq!(validate(&resp), Err(ResponseDefect::HeaderValue));
        assert_eq!(
            prepare(&resp, false, true, true, &mut Vec::new()).err(),
            Some(ResponseDefect::HeaderValue)
        );

        let resp = head(200, &[("X Bad", "v")]);
        assert_eq!(validate(&resp), Err(ResponseDefect::HeaderName));

        let resp = head(200, &[("", "v")]);
        assert_eq!(validate(&resp), Err(ResponseDefect::HeaderName));

        let resp = head(200, &[("X-Ok", "a\0b")]);
        assert_eq!(validate(&resp), Err(ResponseDefect::HeaderValue));

        let mut resp = head(200, &[]);
        resp.reason = b"OK\r\nX-Smuggled: 1".to_vec();
        assert_eq!(validate(&resp), Err(ResponseDefect::Reason));

        // A UTF-8 value is not injection: obs-text is allowed.
        let resp = head(200, &[("X-Name", "café")]);
        assert_eq!(validate(&resp), Ok(()));

        // A refused head writes nothing into the caller's buffer.
        let mut out = b"stale".to_vec();
        assert_eq!(
            prepare(&resp_with_injection(), false, true, true, &mut out).err(),
            Some(ResponseDefect::HeaderValue)
        );
        assert_eq!(out, b"stale");
    }

    /// A head whose one header carries a smuggled CRLF.
    fn resp_with_injection() -> RespHead {
        head(200, &[("X-Smuggled", "a\r\nX-Evil: 1")])
    }

    #[test]
    fn framing_follows_content_length_chunked_and_close() {
        // A declared length is kept and used.
        let (out, prepared) = encode(&head(200, &[("Content-Length", "12")]), false, true, true);
        assert_eq!(prepared.framing, Framing::Length(12));
        assert_eq!(names(&out), ["Content-Length"]);

        // No length, HTTP/1.1: chunked is ours to add.
        let (out, prepared) = encode(&head(200, &[]), false, true, true);
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&out), ["Transfer-Encoding"]);

        // No length, HTTP/1.0: close-delimited.
        let (out, prepared) = encode(&head(200, &[]), false, false, false);
        assert_eq!(prepared.framing, Framing::Close);
        assert_eq!(names(&out), ["Connection"]);

        // The view cannot pick its own framing.
        let (out, prepared) = encode(
            &head(
                200,
                &[("Transfer-Encoding", "chunked"), ("Content-Length", "3")],
            ),
            false,
            true,
            true,
        );
        assert_eq!(prepared.framing, Framing::Length(3));
        assert_eq!(names(&out), ["Content-Length"]);

        // A nonsense length is dropped rather than forwarded.
        let (out, prepared) = encode(&head(200, &[("Content-Length", "abc")]), false, true, true);
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&out), ["Transfer-Encoding"]);

        // Nor can two lengths disagree: both go.
        let (out, prepared) = encode(
            &head(200, &[("Content-Length", "3"), ("Content-Length", "4")]),
            false,
            true,
            true,
        );
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&out), ["Transfer-Encoding"]);
    }

    #[test]
    fn keep_alive_is_written_explicitly_when_it_is_not_the_default() {
        let (out, _) = encode(&head(200, &[]), false, true, false);
        assert_eq!(
            out,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        );

        // HTTP/1.0 has to be told, and is told once. It can only be kept open
        // when the response says how long it is.
        let (out, prepared) = encode(
            &head(
                200,
                &[("Content-Length", "3"), ("Connection", "keep-alive")],
            ),
            false,
            false,
            true,
        );
        assert_eq!(prepared.framing, Framing::Length(3));
        assert_eq!(
            out,
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: keep-alive\r\n\r\n"
        );

        // Without a length, HTTP/1.0 gets a close-delimited body and the
        // connection must not be reused.
        let (out, prepared) = encode(&head(200, &[]), false, false, true);
        assert_eq!(prepared.framing, Framing::Close);
        assert!(!prepared.keep_alive);
        assert_eq!(out, b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
    }

    #[test]
    fn bodyless_responses_drop_a_length_that_could_never_be_true() {
        // A `HEAD` answer reports what the `GET` would have returned, so its
        // length is kept, as is a 304's.
        let (out, prepared) = encode(&head(200, &[("Content-Length", "5")]), true, true, true);
        assert_eq!(prepared.framing, Framing::None);
        assert_eq!(names(&out), ["Content-Length"]);

        let (out, prepared) = encode(&head(304, &[("Content-Length", "5")]), true, true, true);
        assert_eq!(prepared.framing, Framing::None);
        assert_eq!(names(&out), ["Content-Length"]);

        // 1xx and 204 can never carry a body at all, so a length on them is a
        // lie the client would act on.
        for status in [100u16, 102, 199, 204] {
            let (out, prepared) =
                encode(&head(status, &[("Content-Length", "5")]), true, true, true);
            assert_eq!(prepared.framing, Framing::None, "status {status}");
            assert!(names(&out).is_empty(), "status {status}: {out:?}");
        }
    }

    #[test]
    fn a_head_response_carries_no_framing_of_its_own() {
        let (out, prepared) = encode(&head(200, &[]), true, true, true);
        assert_eq!(prepared.framing, Framing::None);
        assert_eq!(out, b"HTTP/1.1 200 OK\r\n\r\n");
    }

    #[test]
    fn prepare_writes_the_bytes_a_client_expects() {
        let (out, _) = encode(
            &head(
                201,
                &[("Content-Length", "0"), ("X-A", "1"), ("Keep-Alive", "x")],
            ),
            false,
            true,
            true,
        );
        assert_eq!(
            out,
            b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nX-A: 1\r\n\r\n"
        );
    }

    #[test]
    fn a_warm_buffer_encodes_head_after_head_in_place() {
        // The connection passes the same buffer to every response, so a head
        // must fit the space the last one left without reallocating: this is
        // what makes the response head allocation-free after the first request.
        let mut out = Vec::new();
        prepare(
            &head(200, &[("Content-Length", "5")]),
            false,
            true,
            true,
            &mut out,
        )
        .unwrap();
        let capacity = out.capacity();
        let pointer = out.as_ptr();
        for _ in 0..64 {
            prepare(
                &head(200, &[("Content-Length", "5")]),
                false,
                true,
                true,
                &mut out,
            )
            .unwrap();
        }
        assert_eq!(out.capacity(), capacity, "the buffer was reallocated");
        assert_eq!(out.as_ptr(), pointer, "the buffer was replaced");
        assert_eq!(out, b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n");
    }

    #[test]
    fn simple_responses_are_fixed_and_close_the_connection() {
        assert_eq!(
            simple(400),
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(
            simple(502),
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(
            simple(599),
            b"HTTP/1.1 599 \r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(CONTINUE, b"HTTP/1.1 100 Continue\r\n\r\n");
        assert_eq!(LAST_CHUNK, b"0\r\n\r\n");
    }

    #[test]
    fn chunks_are_written_with_their_size() {
        assert_eq!(chunk(b"hello"), b"5\r\nhello\r\n");
        assert_eq!(chunk(&[0u8; 255]), {
            let mut expected = b"ff\r\n".to_vec();
            expected.extend_from_slice(&[0u8; 255]);
            expected.extend_from_slice(b"\r\n");
            expected
        });
    }
}
