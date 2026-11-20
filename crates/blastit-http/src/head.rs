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
