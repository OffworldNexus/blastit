//! Parsing, validating and normalising a request head (ARCHITECTURE.md §2.3).
//!
//! Everything a request must survive before the scheduler sees it lives here:
//! the framing rules (`Content-Length` versus `Transfer-Encoding`), the
//! `Upgrade` and `Expect` refusals, hop-by-hop stripping, and the client
//! address/scheme derivation from `X-Forwarded-*`.

use blastit_events::RejectReason;
use blastit_frame::Scheme;
use httparse::{EMPTY_HEADER, Request as HttparseRequest, Status};

use crate::config::HttpConfig;
use crate::headers;
use crate::headers::LengthError;

/// How many header fields `httparse` may hand back before the head is refused.
///
/// The byte limit would catch a head this large anyway; this only stops
/// `httparse` from overflowing its output array first.
const MAX_HEADERS: usize = 128;

/// Which HTTP version the request line announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// `HTTP/1.0`.
    Http10,
    /// `HTTP/1.1`.
    Http11,
}

impl Version {
    /// Whether a connection speaking this version stays open by default.
    ///
    /// HTTP/1.1 does; HTTP/1.0 only when the client asks for it.
    pub const fn keep_alive_default(self) -> bool {
        matches!(self, Self::Http11)
    }
}

/// How the client framed the request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFraming {
    /// No body at all.
    None,
    /// Exactly this many bytes.
    ContentLength(u64),
    /// `Transfer-Encoding: chunked`.
    Chunked,
}

impl BodyFraming {
    /// Whether any body byte is still to be read.
    pub const fn wants_body(self) -> bool {
        match self {
            Self::None => false,
            Self::ContentLength(0) => false,
            Self::ContentLength(_) | Self::Chunked => true,
        }
    }

    /// How many body bytes the client declared, when it said so.
    pub const fn declared_len(self) -> Option<u64> {
        match self {
            Self::ContentLength(len) => Some(len),
            Self::None | Self::Chunked => None,
        }
    }
}

/// A request head, parsed and normalised, ready to become a `REQ_HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    /// Request method, as received.
    pub method: Vec<u8>,
    /// Request target (origin form), as received.
    pub target: Vec<u8>,
    /// HTTP version of the request line.
    pub version: Version,
    /// Headers to forward: hop-by-hop fields already removed.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// How the body is framed.
    pub body: BodyFraming,
    /// The client asked for `Expect: 100-continue`.
    pub expect_continue: bool,
    /// The client wants the connection kept open after this response.
    pub keep_alive: bool,
    /// The client address: the peer, or the last `X-Forwarded-For` entry.
    pub remote_addr: String,
    /// `http`, or `https` when a trusted `X-Forwarded-Proto` says so.
    pub scheme: Scheme,
    /// `Host` without its port, or `localhost`.
    pub server_name: String,
}

impl RequestHead {
    /// Whether this is a `HEAD` request, which gets headers but no body.
    pub fn is_head(&self) -> bool {
        self.method.eq_ignore_ascii_case(b"HEAD")
    }
}

/// A request refused before the scheduler ever saw it (§8.1 `http.rejected`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    /// Status code to send before closing.
    pub status: u16,
    /// Reason, for the `http.rejected` event.
    pub reason: RejectReason,
    /// The client address, when the headers allowed deriving it.
    pub remote_addr: Option<String>,
}

impl Reject {
    /// A refusal with a fixed status and reason.
    fn new(status: u16, reason: RejectReason) -> Self {
        Self {
            status,
            reason,
            remote_addr: None,
        }
    }

    /// The same refusal, with the derived client address attached.
    fn for_client(mut self, addr: String) -> Self {
        self.remote_addr = Some(addr);
        self
    }
}

/// The outcome of looking at the bytes buffered so far.
#[derive(Debug)]
pub enum Parsed {
    /// The head is not complete yet; read more.
    Partial,
    /// A complete head, and how many bytes of the buffer it used.
    Complete(RequestHead, usize),
    /// The head is unusable, or the request must not be served.
    Reject(Reject),
}

/// Parse and normalise the head of the next request.
///
/// `available` starts at the first byte of the request. Only the caller knows
/// whether that is more than `cfg.max_header_bytes` bytes of *head*, so the
/// size check for a complete head happens here and the one for an incomplete
/// head belongs to the caller.
pub fn parse(available: &[u8], cfg: &HttpConfig, peer: &str) -> Parsed {
    let mut fields = [EMPTY_HEADER; MAX_HEADERS];
    let mut request = HttparseRequest::new(&mut fields);
    let used = match request.parse(available) {
        Ok(Status::Complete(used)) => used,
        Ok(Status::Partial) => return Parsed::Partial,
        Err(httparse::Error::Version) => {
            // A missing or exotic version, including the HTTP/2 preface, which
            // starts with `PRI * HTTP/2.0`.
            return Parsed::Reject(Reject::new(
                505,
                RejectReason::Other("version_unsupported".into()),
            ));
        }
        Err(httparse::Error::TooManyHeaders) => {
            return Parsed::Reject(Reject::new(431, RejectReason::HeaderTooLarge));
        }
        Err(_) => return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest)),
    };

    let version = match request.version {
        Some(0) => Version::Http10,
        Some(1) => Version::Http11,
        // `httparse` only accepts 1.0 and 1.1, so this is unreachable today;
        // staying total costs nothing and keeps a future parser honest.
        _ => {
            return Parsed::Reject(Reject::new(
                505,
                RejectReason::Other("version_unsupported".into()),
            ));
        }
    };

    let Some(method) = request.method else {
        return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest));
    };
    let Some(target) = request.path else {
        return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest));
    };
    if method.is_empty() || target.is_empty() {
        return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest));
    }

    // Raw fields, in the order they arrived, still including hop-by-hop ones.
    let raw: Vec<(Vec<u8>, Vec<u8>)> = request
        .headers
        .iter()
        .map(|header| (header.name.as_bytes().to_vec(), header.value.to_vec()))
        .collect();

    match classify(&raw, version, cfg) {
        Ok((body, expect_continue, keep_alive)) => {
            let head = RequestHead {
                method: method.as_bytes().to_vec(),
                target: target.as_bytes().to_vec(),
                version,
                headers: headers::strip_hop_by_hop(&raw),
                body,
                expect_continue,
                keep_alive,
                remote_addr: client_addr(&raw, cfg, peer),
                scheme: scheme(&raw, cfg),
                server_name: host_name(&raw).unwrap_or_else(|| "localhost".to_string()),
            };
            Parsed::Complete(head, used)
        }
        Err(reject) => {
            // The headers are usable, so the refusal can name the real client
            // rather than the load balancer.
            Parsed::Reject(reject.for_client(client_addr(&raw, cfg, peer)))
        }
    }
}

/// Decide the framing and the connection options, or refuse the request.
fn classify(
    raw: &[(Vec<u8>, Vec<u8>)],
    version: Version,
    cfg: &HttpConfig,
) -> Result<(BodyFraming, bool, bool), Reject> {
    // `Upgrade` (including `h2c` and WebSocket) is a protocol change the front
    // end does not implement: it never speaks anything but HTTP/1.1.
    if headers::values(raw, "upgrade").next().is_some() {
        return Err(Reject::new(501, RejectReason::UpgradeUnsupported));
    }

    let has_transfer_encoding = headers::values(raw, "transfer-encoding").next().is_some();
    let has_content_length = headers::values(raw, "content-length").next().is_some();
    let body = if has_transfer_encoding {
        // Request smuggling starts here: two framings, two possible readings.
        if has_content_length {
            return Err(Reject::new(
                400,
                RejectReason::Other("content_length_conflict".into()),
            ));
        }
        let chunked = headers::tokens(raw, "transfer-encoding")
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>();
        if chunked != [b"chunked".to_vec()] {
            return Err(Reject::new(
                501,
                RejectReason::Other("transfer_encoding_unsupported".into()),
            ));
        }
        BodyFraming::Chunked
    } else {
        match headers::content_length(raw) {
            Ok(None) => BodyFraming::None,
            Ok(Some(len)) if len > cfg.max_body_bytes => {
                return Err(Reject::new(413, RejectReason::BodyTooLarge));
            }
            Ok(Some(len)) => BodyFraming::ContentLength(len),
            Err(LengthError::Duplicate) => {
                return Err(Reject::new(
                    400,
                    RejectReason::Other("duplicate_content_length".into()),
                ));
            }
            Err(LengthError::Invalid) => {
                return Err(Reject::new(
                    400,
                    RejectReason::Other("invalid_content_length".into()),
                ));
            }
        }
    };

    let mut expect_continue = false;
    for value in headers::values(raw, "expect") {
        if headers::trim(value).eq_ignore_ascii_case(b"100-continue") {
            expect_continue = true;
        } else {
            return Err(Reject::new(
                417,
                RejectReason::Other("expect_unsupported".into()),
            ));
        }
    }

    let keep_alive = if headers::has_token(raw, "connection", "close") {
        false
    } else if headers::has_token(raw, "connection", "keep-alive") {
        true
    } else {
        version.keep_alive_default()
    };

    Ok((body, expect_continue, keep_alive))
}

/// The client address to report, from `X-Forwarded-For` when it may be trusted.
///
/// A load balancer *appends* the address it saw, so the last entry is the one
/// it added and the closest thing to the real client. Entries to its left are
/// whatever the client sent and mean nothing here.
fn client_addr(raw: &[(Vec<u8>, Vec<u8>)], cfg: &HttpConfig, peer: &str) -> String {
    if cfg.trust_forwarded
        && let Some(value) = headers::values(raw, "x-forwarded-for").last()
        && let Some(last) = value.rsplit(|byte| *byte == b',').next()
    {
        let last = headers::trim(last);
        if !last.is_empty() {
            return String::from_utf8_lossy(last).into_owned();
        }
    }
    peer.to_string()
}

/// The scheme, from `X-Forwarded-Proto` when it may be trusted.
///
/// Anything that is not exactly `http` or `https` is ignored: a scheme is a
/// two-value enum, and a load balancer that writes something else is not
/// describing a scheme we could report.
fn scheme(raw: &[(Vec<u8>, Vec<u8>)], cfg: &HttpConfig) -> Scheme {
    if cfg.trust_forwarded
        && let Some(value) = headers::values(raw, "x-forwarded-proto").last()
    {
        let value = headers::trim(value);
        if value.eq_ignore_ascii_case(b"https") {
            return Scheme::Https;
        }
    }
    Scheme::Http
}

/// The `Host` name without its port, or `None` when there is no `Host`.
fn host_name(raw: &[(Vec<u8>, Vec<u8>)]) -> Option<String> {
    let value = headers::values(raw, "host").next()?;
    let host = String::from_utf8_lossy(headers::trim(value)).into_owned();
    if host.is_empty() {
        return None;
    }
    // An IPv6 literal is bracketed, so it may contain colons of its own.
    if let Some(rest) = host.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        return Some(rest[..end].to_string());
    }
    if let Some((name, port)) = host.rsplit_once(':')
        && !name.is_empty()
        && !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Some(name.to_string());
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HttpConfig {
        HttpConfig::default()
    }

    /// Parse a head that is expected to be complete.
    fn parse_head(raw: &str) -> RequestHead {
        match parse(raw.as_bytes(), &cfg(), "10.0.0.1:9999") {
            Parsed::Complete(head, used) => {
                assert_eq!(used, raw.len());
                head
            }
            other => panic!("expected a complete head, got {other:?}"),
        }
    }

    /// Parse a head that is expected to be refused.
    fn parse_reject(raw: &str) -> Reject {
        match parse(raw.as_bytes(), &cfg(), "10.0.0.1:9999") {
            Parsed::Reject(reject) => reject,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_get_is_parsed_and_normalised() {
        let head =
            parse_head("GET /x?y=1 HTTP/1.1\r\nHost: example.com:8443\r\nAccept: */*\r\n\r\n");
        assert_eq!(head.method, b"GET");
        assert_eq!(head.target, b"/x?y=1");
        assert_eq!(head.version, Version::Http11);
        assert_eq!(head.body, BodyFraming::None);
        assert!(!head.expect_continue);
        assert!(head.keep_alive);
        assert_eq!(head.remote_addr, "10.0.0.1:9999");
        assert_eq!(head.scheme, Scheme::Http);
        assert_eq!(head.server_name, "example.com");
        assert_eq!(head.headers.len(), 2);
    }

    #[test]
    fn an_incomplete_head_is_partial() {
        for raw in [
            "",
            "GET",
            "GET / HTTP/1.1",
            "GET / HTTP/1.1\r\n",
            "GET / HTTP/1.1\r\nHost: h\r\n",
        ] {
            assert!(
                matches!(parse(raw.as_bytes(), &cfg(), "p"), Parsed::Partial),
                "{raw:?} must wait for more bytes"
            );
        }
    }

    #[test]
    fn a_complete_head_reports_its_own_length() {
        let raw = "GET / HTTP/1.1\r\nHost: h\r\n\r\nGET /next HTTP/1.1\r\nHost: h\r\n\r\n";
        match parse(raw.as_bytes(), &cfg(), "p") {
            Parsed::Complete(head, used) => {
                assert_eq!(head.target, b"/");
                assert_eq!(&raw[used..], "GET /next HTTP/1.1\r\nHost: h\r\n\r\n");
            }
            other => panic!("expected a complete head, got {other:?}"),
        }
    }

    #[test]
    fn malformed_request_lines_are_refused() {
        for raw in [
            "GET / HTTP/1.1\r\nBad Header: x\r\n\r\n",
            "GET / HTTP/1.1\r\nHost h\r\n\r\n",
            "GET\t/ HTTP/1.1\r\nHost: h\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: h\r\nHeader: a\rb\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: h\r\nName\x00: v\r\n\r\n",
        ] {
            let reject = parse_reject(raw);
            assert_eq!(reject.status, 400, "{raw:?}");
            assert_eq!(reject.reason, RejectReason::MalformedRequest, "{raw:?}");
        }
    }

    #[test]
    fn wrong_versions_are_505_and_the_h2_preface_is_not_a_request() {
        for raw in [
            "GET / HTTP/2.0\r\nHost: h\r\n\r\n",
            "GET / HTTP/0.9\r\nHost: h\r\n\r\n",
            "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
            "GET / HTTPS/1.1\r\nHost: h\r\n\r\n",
        ] {
            let reject = parse_reject(raw);
            assert_eq!(reject.status, 505, "{raw:?}");
        }
    }

    #[test]
    fn upgrade_is_refused_with_501() {
        for value in ["websocket", "h2c", "HTTP/2.0, websocket", "foo"] {
            let raw = format!("GET / HTTP/1.1\r\nHost: h\r\nUpgrade: {value}\r\n\r\n");
            let reject = parse_reject(&raw);
            assert_eq!(reject.status, 501, "{value}");
            assert_eq!(reject.reason, RejectReason::UpgradeUnsupported, "{value}");
        }
        // The header names are matched case-insensitively.
        let reject = parse_reject("GET / HTTP/1.1\r\nHost: h\r\nUPGRADE: h2c\r\n\r\n");
        assert_eq!(reject.status, 501);
    }

    #[test]
    fn transfer_encoding_and_content_length_together_are_refused() {
        let reject = parse_reject(
            "POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
        );
        assert_eq!(reject.status, 400);
        assert_eq!(
            reject.reason,
            RejectReason::Other("content_length_conflict".into())
        );
    }

    #[test]
    fn only_chunked_transfer_encoding_is_supported() {
        let good = parse_head("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!(good.body, BodyFraming::Chunked);

        let good = parse_head("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: CHUNKED\r\n\r\n");
        assert_eq!(good.body, BodyFraming::Chunked);

        for value in [
            "identity",
            "gzip",
            "chunked, gzip",
            "gzip, chunked",
            "chunked, chunked",
            "chunked;ext",
        ] {
            let raw = format!("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: {value}\r\n\r\n");
            let reject = parse_reject(&raw);
            assert_eq!(reject.status, 501, "{value}");
            assert_eq!(
                reject.reason,
                RejectReason::Other("transfer_encoding_unsupported".into()),
                "{value}"
            );
        }
    }

    #[test]
    fn content_length_must_be_one_plain_number() {
        let head = parse_head("POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 12\r\n\r\n");
        assert_eq!(head.body, BodyFraming::ContentLength(12));

        let head = parse_head("POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(head.body, BodyFraming::ContentLength(0));
        assert!(!head.body.wants_body());

        for value in ["", " ", "-1", "abc", "1 2", "1, 1", "0x10"] {
            let raw = format!("POST / HTTP/1.1\r\nHost: h\r\nContent-Length: {value}\r\n\r\n");
            let reject = parse_reject(&raw);
            assert_eq!(
                reject.status, 400,
                "Content-Length: {value:?} must be refused"
            );
        }

        let reject = parse_reject(
            "POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\n",
        );
        assert_eq!(reject.status, 400);
    }

    #[test]
    fn a_declared_body_over_the_limit_is_413() {
        let mut cfg = cfg();
        cfg.max_body_bytes = 10;
        let raw = "POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 11\r\n\r\n";
        match parse(raw.as_bytes(), &cfg, "p") {
            Parsed::Reject(reject) => {
                assert_eq!(reject.status, 413);
                assert_eq!(reject.reason, RejectReason::BodyTooLarge);
            }
            other => panic!("expected 413, got {other:?}"),
        }
        // Exactly the limit is fine.
        let raw = "POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\n";
        assert!(matches!(
            parse(raw.as_bytes(), &cfg, "p"),
            Parsed::Complete(..)
        ));
    }

    #[test]
    fn expect_is_only_understood_for_100_continue() {
        let head = parse_head(
            "POST / HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 1\r\n\r\n",
        );
        assert!(head.expect_continue);

        let head = parse_head(
            "POST / HTTP/1.1\r\nHost: h\r\nExpect: 100-CONTINUE\r\nContent-Length: 1\r\n\r\n",
        );
        assert!(head.expect_continue);

        for value in ["", "something-else", "100-continue, foo"] {
            let raw = format!("POST / HTTP/1.1\r\nHost: h\r\nExpect: {value}\r\n\r\n");
            let reject = parse_reject(&raw);
            assert_eq!(reject.status, 417, "Expect: {value:?}");
        }
    }

    #[test]
    fn hop_by_hop_headers_never_reach_the_frames() {
        let head = parse_head(
            "GET / HTTP/1.1\r\nHost: h\r\nConnection: keep-alive, x-drop\r\nx-drop: 1\r\nKeep-Alive: timeout=5\r\nTE: trailers\r\nProxy-Connection: keep-alive\r\nX-Kept: yes\r\n\r\n",
        );
        let names: Vec<String> = head
            .headers
            .iter()
            .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
            .collect();
        assert_eq!(names, ["Host", "X-Kept"]);
        assert!(head.keep_alive);
    }

    #[test]
    fn upgrade_and_transfer_encoding_are_refused_before_normalisation() {
        let reject =
            parse_reject("GET / HTTP/1.1\r\nHost: h\r\nUpgrade: h2c\r\nTE: trailers\r\n\r\n");
        assert_eq!(reject.status, 501);
        assert_eq!(reject.reason, RejectReason::UpgradeUnsupported);

        // `Upgrade` is checked first, so a request with several problems is
        // reported as an upgrade problem.
        let reject = parse_reject(
            "POST / HTTP/1.1\r\nHost: h\r\nUpgrade: h2c\r\nContent-Length: abc\r\n\r\n",
        );
        assert_eq!(reject.status, 501);
        assert_eq!(reject.reason, RejectReason::UpgradeUnsupported);
    }

    #[test]
    fn keep_alive_follows_the_connection_header_and_the_version() {
        assert!(parse_head("GET / HTTP/1.1\r\nHost: h\r\n\r\n").keep_alive);
        assert!(!parse_head("GET / HTTP/1.0\r\nHost: h\r\n\r\n").keep_alive);
        assert!(
            parse_head("GET / HTTP/1.0\r\nHost: h\r\nConnection: keep-alive\r\n\r\n").keep_alive
        );
        assert!(!parse_head("GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n").keep_alive);
        // `close` wins over `keep-alive`.
        assert!(
            !parse_head("GET / HTTP/1.1\r\nHost: h\r\nConnection: close, keep-alive\r\n\r\n")
                .keep_alive
        );
    }

    #[test]
    fn forwarded_headers_are_trusted_only_when_configured() {
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nX-Forwarded-Proto: https\r\n\r\n";
        let head = parse_head(raw);
        assert_eq!(head.remote_addr, "5.6.7.8", "the last entry is the LB's");
        assert_eq!(head.scheme, Scheme::Https);

        let mut cfg = cfg();
        cfg.trust_forwarded = false;
        match parse(raw.as_bytes(), &cfg, "10.0.0.1:9999") {
            Parsed::Complete(head, _) => {
                assert_eq!(head.remote_addr, "10.0.0.1:9999");
                assert_eq!(head.scheme, Scheme::Http);
            }
            other => panic!("expected a complete head, got {other:?}"),
        }

        // A single, unpadded entry, and a scheme we do not know.
        let head = parse_head(
            "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 9.9.9.9\r\nX-Forwarded-Proto: ftp\r\n\r\n",
        );
        assert_eq!(head.remote_addr, "9.9.9.9");
        assert_eq!(head.scheme, Scheme::Http);

        // An empty last entry falls back to the peer.
        let head = parse_head("GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, \r\n\r\n");
        assert_eq!(head.remote_addr, "10.0.0.1:9999");

        let head = parse_head("GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: \r\n\r\n");
        assert_eq!(head.remote_addr, "10.0.0.1:9999");
    }

    #[test]
    fn server_name_comes_from_host_without_its_port() {
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: example.com:8080\r\n\r\n").server_name,
            "example.com"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: example.com\r\n\r\n").server_name,
            "example.com"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: [::1]:8080\r\n\r\n").server_name,
            "::1"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: [::1]\r\n\r\n").server_name,
            "::1"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: \r\n\r\n").server_name,
            "localhost"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\n\r\n").server_name,
            "localhost"
        );
        // Something that only looks like a port is kept whole.
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: example.com:http\r\n\r\n").server_name,
            "example.com:http"
        );
    }

    #[test]
    fn a_refusal_names_the_forwarded_client_when_there_is_one() {
        let reject = parse_reject(
            "POST / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nTransfer-Encoding: gzip\r\n\r\n",
        );
        assert_eq!(reject.status, 501);
        assert_eq!(reject.remote_addr.as_deref(), Some("5.6.7.8"));

        // A malformed request line has no headers to look at.
        let reject = parse_reject("GET / HTTP/1.1\r\nBad Header\r\n\r\n");
        assert_eq!(reject.remote_addr, None);
    }

    #[test]
    fn leading_empty_lines_are_tolerated() {
        let head = parse_head("\r\n\r\nGET / HTTP/1.1\r\nHost: h\r\n\r\n");
        assert_eq!(head.method, b"GET");
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_bytes() {
        let cases: [&[u8]; 8] = [
            b"",
            b"\r\n",
            b"\x00\x01\x02",
            b"GET",
            b"G",
            b"POST / HTTP/1.1\r\n",
            b"GET / HTTP/1.1\r\n\x00: x\r\n\r\n",
            b"GET / HTTP/9.9\r\n\r\n",
        ];
        for case in cases {
            let _ = parse(case, &cfg(), "peer");
        }
    }
}
