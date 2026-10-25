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

/// A response head that has been checked and turned into wires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The status line, CRLF included.
    pub status_line: Vec<u8>,
    /// The headers to write, in order.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
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

/// Build the status line and the header block to write.
///
/// `no_body` says the request was a `HEAD` or the status forbids a body;
/// `keep_alive` says the *client* asked for the connection to survive this
/// response. The value returned in [`Prepared::keep_alive`] is the decision,
/// which also covers the cases where the framing forces a close.
pub fn prepare(
    head: &RespHead,
    no_body: bool,
    http11: bool,
    keep_alive: bool,
) -> Result<Prepared, ResponseDefect> {
    validate(head)?;

    // Hop-by-hop fields belong to the view's own hop, not to the client's.
    let mut out = headers::strip_hop_by_hop(&head.headers);
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
    // `GET` would have returned, which is what a `HEAD` answer is for.)
    if matches!(framing, Framing::Chunked | Framing::Close) {
        out.retain(|(name, _)| !headers::is(name, "content-length"));
    }
    if framing == Framing::Chunked {
        out.push((b"Transfer-Encoding".to_vec(), b"chunked".to_vec()));
    }
    if !keep_alive {
        out.push((b"Connection".to_vec(), b"close".to_vec()));
    } else if !http11 {
        out.push((b"Connection".to_vec(), b"keep-alive".to_vec()));
    }

    let reason: &[u8] = if head.reason.is_empty() {
        canonical_reason(head.status).unwrap_or("").as_bytes()
    } else {
        &head.reason
    };
    let mut status_line = format!("HTTP/1.1 {} ", head.status).into_bytes();
    status_line.extend_from_slice(reason);
    status_line.extend_from_slice(b"\r\n");

    Ok(Prepared {
        status_line,
        headers: out,
        framing,
        keep_alive,
    })
}

/// Serialise a prepared head into one buffer.
pub fn encode_head(prepared: &Prepared, out: &mut Vec<u8>) {
    out.extend_from_slice(&prepared.status_line);
    for (name, value) in &prepared.headers {
        out.extend_from_slice(name);
        out.extend_from_slice(b": ");
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
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

    fn names(prepared: &Prepared) -> Vec<String> {
        prepared
            .headers
            .iter()
            .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
            .collect()
    }

    #[test]
    fn the_status_line_uses_the_registered_phrase_when_the_view_gives_none() {
        let prepared = prepare(&head(200, &[]), false, true, true).unwrap();
        assert_eq!(prepared.status_line, b"HTTP/1.1 200 OK\r\n");

        let mut resp = head(599, &[]);
        resp.reason = b"Custom".to_vec();
        let prepared = prepare(&resp, false, true, true).unwrap();
        assert_eq!(prepared.status_line, b"HTTP/1.1 599 Custom\r\n");

        // An unknown status with no reason keeps the space, as RFC 9110 spells
        // the status line.
        let prepared = prepare(&head(599, &[]), false, true, true).unwrap();
        assert_eq!(prepared.status_line, b"HTTP/1.1 599 \r\n");
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
            prepare(&resp, false, true, true).err(),
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
    }

    #[test]
    fn framing_follows_content_length_chunked_and_close() {
        // A declared length is kept and used.
        let prepared = prepare(&head(200, &[("Content-Length", "12")]), false, true, true).unwrap();
        assert_eq!(prepared.framing, Framing::Length(12));
        assert_eq!(names(&prepared), ["Content-Length"]);

        // No length, HTTP/1.1: chunked is ours to add.
        let prepared = prepare(&head(200, &[]), false, true, true).unwrap();
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&prepared), ["Transfer-Encoding"]);

        // No length, HTTP/1.0: close-delimited.
        let prepared = prepare(&head(200, &[]), false, false, false).unwrap();
        assert_eq!(prepared.framing, Framing::Close);
        assert_eq!(names(&prepared), ["Connection"]);

        // The view cannot pick its own framing.
        let prepared = prepare(
            &head(
                200,
                &[("Transfer-Encoding", "chunked"), ("Content-Length", "3")],
            ),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(prepared.framing, Framing::Length(3));
        assert_eq!(names(&prepared), ["Content-Length"]);

        // A nonsense length is dropped rather than forwarded.
        let prepared =
            prepare(&head(200, &[("Content-Length", "abc")]), false, true, true).unwrap();
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&prepared), ["Transfer-Encoding"]);

        // Nor can two lengths disagree: both go.
        let prepared = prepare(
            &head(200, &[("Content-Length", "3"), ("Content-Length", "4")]),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(prepared.framing, Framing::Chunked);
        assert_eq!(names(&prepared), ["Transfer-Encoding"]);
    }

    #[test]
    fn keep_alive_is_written_explicitly_when_it_is_not_the_default() {
        let prepared = prepare(&head(200, &[]), false, true, false).unwrap();
        assert_eq!(names(&prepared), ["Transfer-Encoding", "Connection"]);
        assert_eq!(prepared.headers[1].1, b"close");

        // HTTP/1.0 has to be told, and is told once. It can only be kept open
        // when the response says how long it is.
        let prepared = prepare(
            &head(
                200,
                &[("Content-Length", "3"), ("Connection", "keep-alive")],
            ),
            false,
            false,
            true,
        )
        .unwrap();
        assert_eq!(prepared.framing, Framing::Length(3));
        assert_eq!(names(&prepared), ["Content-Length", "Connection"]);
        assert_eq!(prepared.headers[1].1, b"keep-alive");

        // Without a length, HTTP/1.0 gets a close-delimited body and the
        // connection must not be reused.
        let prepared = prepare(&head(200, &[]), false, false, true).unwrap();
        assert_eq!(prepared.framing, Framing::Close);
        assert!(!prepared.keep_alive);
        assert_eq!(names(&prepared), ["Connection"]);
        assert_eq!(prepared.headers[0].1, b"close");
    }

    #[test]
    fn bodyless_responses_keep_the_views_length_but_lose_their_own_framing() {
        let prepared = prepare(&head(204, &[("Content-Length", "0")]), true, true, true).unwrap();
        assert_eq!(prepared.framing, Framing::None);
        assert_eq!(names(&prepared), ["Content-Length"]);
    }

    #[test]
    fn a_head_response_carries_no_framing_of_its_own() {
        let prepared = prepare(&head(200, &[]), true, true, true).unwrap();
        assert_eq!(prepared.framing, Framing::None);
        assert!(prepared.headers.is_empty());
    }

    #[test]
    fn encode_head_writes_the_bytes_a_client_expects() {
        let prepared = prepare(
            &head(
                201,
                &[("Content-Length", "0"), ("X-A", "1"), ("Keep-Alive", "x")],
            ),
            false,
            true,
            true,
        )
        .unwrap();
        let mut out = Vec::new();
        encode_head(&prepared, &mut out);
        assert_eq!(
            out,
            b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nX-A: 1\r\n\r\n"
        );
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
