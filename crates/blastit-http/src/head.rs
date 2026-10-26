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
    /// The client address, as the one resolver decided it (D5): the peer, or
    /// the address a trusted `BLASTIT_CLIENT_IP_HEADER` chain names.
    ///
    /// This is what the app sees as `REMOTE_ADDR`, so it is an address and
    /// nothing else — a bare IP, normalised. The port the connection arrived
    /// from is not in it (CGI, PEP 3333): it is [`RequestHead::peer_addr`]'s,
    /// and the view reports it as `REMOTE_PORT`.
    pub remote_addr: String,
    /// The address the request actually arrived on, `ip:port` (D5). Never a
    /// header's value, and the only thing the trust decision is made on.
    pub peer_addr: String,
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
        Ok(Status::Partial) => {
            // A bare LF in what has arrived so far is inside the head, because
            // the head is not complete yet: refuse it now rather than wait for
            // a timeout on a request that can never be served.
            if has_bare_lf(available) {
                return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest));
            }
            return Parsed::Partial;
        }
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

    // `httparse` accepts a bare LF as a line ending, and a front end that does
    // the same can be made to disagree with the peer in front of it about where
    // one request ends and the next begins. The head is exactly the first
    // `used` bytes, so only they are checked: a body may contain any LF.
    if has_bare_lf(&available[..used]) {
        return Parsed::Reject(Reject::new(400, RejectReason::MalformedRequest));
    }

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

    if let Err(reject) = check_target(method.as_bytes(), target.as_bytes()) {
        return Parsed::Reject(reject.for_client(client_addr(&raw, cfg, peer)));
    }

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
                peer_addr: peer.to_string(),
                scheme: scheme(&raw, cfg, peer),
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

/// Whether any `LF` is not part of a `CRLF`.
fn has_bare_lf(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r'))
}

/// Whether the request line asks for something this front end can route.
///
/// The load balancer in front of us writes origin-form targets, and a view
/// expects to parse one: anything else is refused rather than guessed at.
fn check_target(method: &[u8], target: &[u8]) -> Result<(), Reject> {
    // `CONNECT` would ask this process to become a tunnel, which it is not.
    if method.eq_ignore_ascii_case(b"CONNECT") {
        return Err(Reject::new(
            501,
            RejectReason::Other("connect_unsupported".into()),
        ));
    }
    // Origin form: the only shape a WSGI `PATH_INFO` can come from.
    if target.starts_with(b"/") {
        return Ok(());
    }
    // Asterisk form is defined for `OPTIONS` and nothing else; it is forwarded
    // as it is, so the app can answer it itself.
    if target == b"*" {
        return if method.eq_ignore_ascii_case(b"OPTIONS") {
            Ok(())
        } else {
            Err(Reject::new(
                400,
                RejectReason::Other("invalid_target".into()),
            ))
        };
    }
    if target.contains(&b':') || target.contains(&b'?') {
        // `scheme://host/path` (absolute form) or `host:port` (authority form).
        return Err(Reject::new(
            400,
            RejectReason::Other("absolute_form_target".into()),
        ));
    }
    Err(Reject::new(
        400,
        RejectReason::Other("invalid_target".into()),
    ))
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

    // `Host` is what the app is told about the request, and WSGI has one place
    // for it. RFC 9112 wants exactly one, and an HTTP/1.1 request without one
    // has no authority at all to report.
    let hosts: Vec<&[u8]> = headers::values(raw, "host").collect();
    if hosts.len() > 1 {
        return Err(Reject::new(
            400,
            RejectReason::Other("duplicate_host".into()),
        ));
    }
    if version == Version::Http11 {
        match hosts.first() {
            Some(host) if valid_host(headers::trim(host)) => {}
            Some(_) => {
                return Err(Reject::new(400, RejectReason::Other("invalid_host".into())));
            }
            None => {
                return Err(Reject::new(400, RejectReason::Other("missing_host".into())));
            }
        }
    }

    let has_transfer_encoding = headers::values(raw, "transfer-encoding").next().is_some();
    let has_content_length = headers::values(raw, "content-length").next().is_some();
    // HTTP/1.0 has no chunked encoding: a `Transfer-Encoding` on one is either
    // a mistake or an attempt to smuggle a second framing past us.
    if has_transfer_encoding && version == Version::Http10 {
        return Err(Reject::new(
            400,
            RejectReason::Other("transfer_encoding_on_http_1_0".into()),
        ));
    }
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

/// Whether a `Host` value is one this front end will report as `server_name`.
///
/// It must not be empty, and it must not contain anything that would change how
/// the field is read: whitespace, a comma (which would make it a list) or a
/// delimiter that belongs to a request target rather than a host.
fn valid_host(host: &[u8]) -> bool {
    !host.is_empty()
        && host.iter().all(|byte| {
            !byte.is_ascii_whitespace()
                && !byte.is_ascii_control()
                && !matches!(byte, b',' | b'/' | b'\\' | b'@' | b'?' | b'#')
        })
}

/// The client address to report, by the one resolver both ends use (D5).
///
/// The **peer** decides whether anything may be believed: a peer outside
/// `BLASTIT_TRUSTED_PROXIES` is the client, whatever its headers say. Only a
/// trusted peer's `BLASTIT_CLIENT_IP_HEADER` chain can name someone else, and a
/// chain with an unparsable entry names nobody.
///
/// What is reported is the address alone, with no port, brackets or zone: it is
/// the `REMOTE_ADDR` the app sees, and the view takes `REMOTE_PORT` from
/// [`RequestHead::peer_addr`] (CGI, PEP 3333).
///
/// A peer that cannot be read as an address has no resolution, and is reported
/// verbatim: this is a front end that logs rather than a scheduler that keys.
fn client_addr(raw: &[(Vec<u8>, Vec<u8>)], cfg: &HttpConfig, peer: &str) -> String {
    cfg.client_ip
        .resolve(peer, raw)
        .map(|client| client.remote_addr())
        .unwrap_or_else(|| blastit_frame::peer_address(peer))
}

/// The scheme, from `X-Forwarded-Proto` when the peer may be trusted.
///
/// The trust check is the client-IP one (D5): a header from a peer we would not
/// believe about addresses is not a header to believe about the transport
/// either. Anything that is not exactly `http` or `https` is ignored: a scheme
/// is a two-value enum, and a load balancer that writes something else is not
/// describing a scheme we could report.
fn scheme(raw: &[(Vec<u8>, Vec<u8>)], cfg: &HttpConfig, peer: &str) -> Scheme {
    if !peer_is_trusted(cfg, peer) {
        return Scheme::Http;
    }
    if let Some(value) = headers::values(raw, "x-forwarded-proto").last() {
        let value = headers::trim(value);
        if value.eq_ignore_ascii_case(b"https") {
            return Scheme::Https;
        }
    }
    Scheme::Http
}

/// Whether the socket peer is one whose headers may be believed.
///
/// A peer that is not an address at all is not trusted: "I could not read where
/// this came from" is not grounds for believing what it says.
fn peer_is_trusted(cfg: &HttpConfig, peer: &str) -> bool {
    blastit_frame::parse_addr(peer).is_some_and(|ip| cfg.client_ip.trusted.contains(&ip))
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
    use blastit_frame::{ClientIpConfig, TrustedProxies};

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
        assert_eq!(
            head.remote_addr, "10.0.0.1",
            "the peer's port is not part of `REMOTE_ADDR`"
        );
        assert_eq!(head.peer_addr, "10.0.0.1:9999", "the peer keeps it");
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
    fn a_peer_we_do_not_trust_cannot_name_itself_in_a_header() {
        // The default trusts only the loopback, so this peer's header is
        // ignored: 1.2.3.4 is whatever the client typed.
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nX-Forwarded-Proto: https\r\n\r\n";
        let head = parse_head(raw);
        assert_eq!(head.remote_addr, "10.0.0.1");
        assert_eq!(head.peer_addr, "10.0.0.1:9999");
        assert_eq!(head.scheme, Scheme::Http, "and neither can its scheme");

        // A peer with no trusted list at all is in the same position.
        let mut cfg = cfg();
        cfg.client_ip.trusted = TrustedProxies::none();
        match parse(raw.as_bytes(), &cfg, "127.0.0.1:9999") {
            Parsed::Complete(head, _) => {
                assert_eq!(head.remote_addr, "127.0.0.1");
                assert_eq!(head.scheme, Scheme::Http);
            }
            other => panic!("expected a complete head, got {other:?}"),
        }
    }

    #[test]
    fn a_trusted_peer_names_the_client_and_the_scheme() {
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nX-Forwarded-Proto: https\r\n\r\n";
        match parse(raw.as_bytes(), &cfg(), "127.0.0.1:9999") {
            Parsed::Complete(head, _) => {
                // Rightmost first: 5.6.7.8 is not us, so it is the client.
                assert_eq!(head.remote_addr, "5.6.7.8");
                assert_eq!(head.peer_addr, "127.0.0.1:9999");
                assert_eq!(head.scheme, Scheme::Https);
            }
            other => panic!("expected a complete head, got {other:?}"),
        }

        // A single entry, and a scheme we do not know.
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 9.9.9.9\r\nX-Forwarded-Proto: ftp\r\n\r\n";
        match parse(raw.as_bytes(), &cfg(), "127.0.0.1:1") {
            Parsed::Complete(head, _) => {
                assert_eq!(head.remote_addr, "9.9.9.9");
                assert_eq!(head.scheme, Scheme::Http);
            }
            other => panic!("expected a complete head, got {other:?}"),
        }

        // An empty header names nobody: the peer stands.
        for value in ["", " ", " , "] {
            let raw = format!("GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: {value}\r\n\r\n");
            match parse(raw.as_bytes(), &cfg(), "127.0.0.1:1") {
                Parsed::Complete(head, _) => assert_eq!(head.remote_addr, "127.0.0.1"),
                other => panic!("expected a complete head, got {other:?}"),
            }
        }

        // An unparsable entry makes the whole header ignored (it never falls
        // through to another header), and IPv4-mapped addresses are normalised.
        for (value, expected) in [
            ("1.2.3.4, garbage", "127.0.0.1"),
            ("::ffff:203.0.113.7", "203.0.113.7"),
            ("[2001:db8::1]", "2001:db8::1"),
        ] {
            let raw = format!("GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: {value}\r\n\r\n");
            match parse(raw.as_bytes(), &cfg(), "127.0.0.1:1") {
                Parsed::Complete(head, _) => assert_eq!(head.remote_addr, expected, "{value}"),
                other => panic!("expected a complete head, got {other:?}"),
            }
        }

        // The resolver reads whichever header it was configured with, so App
        // Platform can use the one the edge overwrites.
        let mut cfg = cfg();
        cfg.client_ip = ClientIpConfig::new("do-connecting-ip", &["10.0.0.0/8"]).unwrap();
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4\r\ndo-connecting-ip: 203.0.113.7\r\n\r\n";
        match parse(raw.as_bytes(), &cfg, "10.244.3.9:1") {
            Parsed::Complete(head, _) => {
                assert_eq!(head.remote_addr, "203.0.113.7");
                assert_eq!(head.peer_addr, "10.244.3.9:1");
            }
            other => panic!("expected a complete head, got {other:?}"),
        }
    }

    #[test]
    fn a_chain_of_trusted_proxies_takes_its_leftmost_entry() {
        let mut cfg = cfg();
        cfg.client_ip = ClientIpConfig::new("x-forwarded-for", &["10.0.0.0/8"]).unwrap();
        let raw = "GET / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 10.1.1.1, 10.2.2.2\r\n\r\n";
        match parse(raw.as_bytes(), &cfg, "10.244.3.9:1") {
            Parsed::Complete(head, _) => assert_eq!(head.remote_addr, "10.1.1.1"),
            other => panic!("expected a complete head, got {other:?}"),
        }
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
        // Something that only looks like a port is kept whole.
        assert_eq!(
            parse_head("GET / HTTP/1.1\r\nHost: example.com:http\r\n\r\n").server_name,
            "example.com:http"
        );
        // HTTP/1.0 has no `Host` to report, and none is required.
        assert_eq!(
            parse_head("GET / HTTP/1.0\r\n\r\n").server_name,
            "localhost"
        );
        assert_eq!(
            parse_head("GET / HTTP/1.0\r\nHost: \r\n\r\n").server_name,
            "localhost"
        );
    }

    #[test]
    fn a_refusal_names_the_resolved_client_when_there_is_one() {
        // `parse_reject` uses an untrusted peer, so the header it carries is
        // ignored: the refusal names the peer.
        let reject = parse_reject(
            "POST / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nTransfer-Encoding: gzip\r\n\r\n",
        );
        assert_eq!(reject.status, 501);
        assert_eq!(reject.remote_addr.as_deref(), Some("10.0.0.1"));

        // From a trusted peer the same refusal names the client.
        let raw = "POST / HTTP/1.1\r\nHost: h\r\nX-Forwarded-For: 1.2.3.4, 5.6.7.8\r\nTransfer-Encoding: gzip\r\n\r\n";
        match parse(raw.as_bytes(), &cfg(), "127.0.0.1:9999") {
            Parsed::Reject(reject) => {
                assert_eq!(reject.status, 501);
                assert_eq!(reject.remote_addr.as_deref(), Some("5.6.7.8"));
            }
            other => panic!("expected a refusal, got {other:?}"),
        }

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
    fn a_bare_lf_line_ending_is_refused() {
        // Each of these is a head `httparse` accepts, so the check that fires
        // is the one for LF without CR.
        for raw in [
            "GET / HTTP/1.1\nHost: h\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: h\n\r\n",
            "GET / HTTP/1.1\r\nHost: h\r\n\n",
            "GET / HTTP/1.1\r\nHost: h\r\nX-A: 1\nX-B: 2\r\n\r\n",
        ] {
            let reject = parse_reject(raw);
            assert_eq!(reject.status, 400, "{raw:?}");
            assert_eq!(reject.reason, RejectReason::MalformedRequest, "{raw:?}");
        }

        // The same head with CRLF everywhere is fine, so it is the line ending
        // that decides.
        let head = parse_head("GET / HTTP/1.1\r\nHost: h\r\nX-A: 1\r\nX-B: 2\r\n\r\n");
        assert_eq!(head.headers.len(), 3);

        // A bare LF in the body is not part of the head and never refused,
        // even when it arrives in the same read as the head.
        let raw = b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 3\r\n\r\na\nb";
        match parse(raw, &cfg(), "p") {
            Parsed::Complete(head, used) => {
                assert_eq!(used, raw.len() - 3, "only the head was consumed");
                assert_eq!(head.body, BodyFraming::ContentLength(3));
            }
            other => panic!("expected a complete head, got {other:?}"),
        }

        // An incomplete head with a bare LF inside it is refused at once.
        assert!(matches!(
            parse(b"GET / HTTP/1.1\nHost: h\r\n", &cfg(), "p"),
            Parsed::Reject(_)
        ));
    }

    #[test]
    fn targets_are_checked_against_the_forms_a_view_can_route() {
        assert_eq!(
            parse_head("GET /x HTTP/1.1\r\nHost: h\r\n\r\n").target,
            b"/x"
        );
        assert_eq!(
            parse_head("OPTIONS * HTTP/1.1\r\nHost: h\r\n\r\n").target,
            b"*"
        );

        for (raw, status) in [
            ("CONNECT example.com:443 HTTP/1.1\r\nHost: h\r\n\r\n", 501),
            ("connect example.com:443 HTTP/1.1\r\nHost: h\r\n\r\n", 501),
            ("GET http://example.com/x HTTP/1.1\r\nHost: h\r\n\r\n", 400),
            ("GET example.com:443 HTTP/1.1\r\nHost: h\r\n\r\n", 400),
            ("GET * HTTP/1.1\r\nHost: h\r\n\r\n", 400),
            ("GET foo HTTP/1.1\r\nHost: h\r\n\r\n", 400),
            ("GET ?a=b HTTP/1.1\r\nHost: h\r\n\r\n", 400),
        ] {
            let reject = parse_reject(raw);
            assert_eq!(reject.status, status, "{raw:?}");
        }
    }

    #[test]
    fn transfer_encoding_needs_http_1_1() {
        let reject =
            parse_reject("POST / HTTP/1.0\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!(reject.status, 400);
        assert_eq!(
            reject.reason,
            RejectReason::Other("transfer_encoding_on_http_1_0".into())
        );

        // HTTP/1.1 with the same header is fine.
        let head = parse_head("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!(head.body, BodyFraming::Chunked);
    }

    #[test]
    fn http_1_1_needs_exactly_one_usable_host() {
        for (raw, reason) in [
            ("GET / HTTP/1.1\r\n\r\n", "missing_host"),
            ("GET / HTTP/1.1\r\nHost: \r\n\r\n", "invalid_host"),
            ("GET / HTTP/1.1\r\nHost:   \r\n\r\n", "invalid_host"),
            ("GET / HTTP/1.1\r\nHost: a b\r\n\r\n", "invalid_host"),
            ("GET / HTTP/1.1\r\nHost: a,b\r\n\r\n", "invalid_host"),
            ("GET / HTTP/1.1\r\nHost: /x\r\n\r\n", "invalid_host"),
            (
                "GET / HTTP/1.1\r\nHost: a\r\nHost: a\r\n\r\n",
                "duplicate_host",
            ),
        ] {
            let reject = parse_reject(raw);
            assert_eq!(reject.status, 400, "{raw:?}");
            assert_eq!(reject.reason, RejectReason::Other(reason.into()), "{raw:?}");
        }

        // HTTP/1.0 may leave it out; it then has no name to report.
        let head = parse_head("GET / HTTP/1.0\r\n\r\n");
        assert_eq!(head.server_name, "localhost");

        // A bracketed IPv6 literal and a name with a dot are both fine.
        for raw in [
            "GET / HTTP/1.1\r\nHost: [2001:db8::1]:8080\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: shop.example.com\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost:8000\r\n\r\n",
        ] {
            assert!(matches!(
                parse(raw.as_bytes(), &cfg(), "p"),
                Parsed::Complete(..)
            ));
        }
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
