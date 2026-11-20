//! §7 "HTTP/1.1 framing & body enforcement": request framing, the `body`
//! policies **as the framing layer sees them**, payload size limits, client
//! disconnects and timeouts, the accept-loop connection cap, and the refusals
//! that stop `Content-Length`/`Transfer-Encoding` request smuggling, protocol
//! upgrades and non-routable request targets.
//!
//! The codec refusals (malformed JSON, duplicate keys, BOM, lone surrogate, a
//! bad form escape, `body none` **with** a body) belong to
//! `crates/blastit-ingress/tests/critical_ingress.rs`; this crate does not
//! depend on `blastit-ingress` and nothing here duplicates them. What is
//! asserted below is the framing promise: a body crosses this layer byte for
//! byte, only its *framing* is inspected, and an event never carries a header
//! value, a target or a body. `RESULT.md` holds the case map.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use blastit_events::{Event, EventKind, encode_json};
use blastit_frame::{Decoder, Frame, Limits, RespHead};
use blastit_http::{Emit, HttpConfig, HttpError, Server, TcpAcceptor};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tokio::task::JoinHandle;

/// The address every connection is accepted from.
const PEER: &str = "203.0.113.7:54321";
const PEER_IP: &str = "203.0.113.7";
/// How long a test waits for bytes before deciding they are not coming.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long a test waits to decide a response will not continue.
const QUIET: Duration = Duration::from_millis(200);

//
// Harness: a scripted scheduler on one end of a socketpair, a client on the
// other, and one front end in between.
//

/// The scheduler end of `http.chan`, driven directly here.
struct Scheduler {
    io: UnixStream,
    decoder: Decoder,
    buf: Vec<u8>,
}

impl Scheduler {
    fn new(io: UnixStream) -> Self {
        Self {
            io,
            decoder: Decoder::new(Limits::default()),
            buf: vec![0u8; 64 * 1024],
        }
    }

    /// The next frame, `WINDOW` (flow control) skipped.
    async fn next(&mut self) -> (u32, Frame) {
        loop {
            if let Some(frame) = self.raw_next(PATIENCE).await {
                if !matches!(frame.1, Frame::Window { .. }) {
                    return frame;
                }
            } else {
                panic!("http.chan went quiet");
            }
        }
    }

    async fn raw_next(&mut self, wait: Duration) -> Option<(u32, Frame)> {
        loop {
            if let Some(frame) = self.decoder.next_frame().expect("frame on http.chan") {
                return Some(frame);
            }
            let read = tokio::time::timeout(wait, self.io.read(&mut self.buf))
                .await
                .ok()?
                .expect("read http.chan");
            assert_ne!(read, 0, "http.chan closed");
            self.decoder.push(&self.buf[..read]);
        }
    }

    /// The next frame, or `None` when nothing arrives within `wait`.
    async fn next_within(&mut self, wait: Duration) -> Option<(u32, Frame)> {
        loop {
            match self.raw_next(wait).await? {
                (_, Frame::Window { .. }) => continue,
                frame => return Some(frame),
            }
        }
    }

    async fn head(&mut self) -> (u32, blastit_frame::ReqHead) {
        match self.next().await {
            (id, Frame::ReqHead(head)) => (id, head),
            other => panic!("expected REQ_HEAD, got {other:?}"),
        }
    }

    async fn body(&mut self) -> Vec<u8> {
        match self.next().await {
            (_, Frame::ReqBody(body)) => body,
            other => panic!("expected REQ_BODY, got {other:?}"),
        }
    }

    async fn end(&mut self) -> u32 {
        match self.next().await {
            (id, Frame::ReqEnd) => id,
            other => panic!("expected REQ_END, got {other:?}"),
        }
    }

    async fn cancel(&mut self) -> u32 {
        match self.next().await {
            (id, Frame::Cancel) => id,
            other => panic!("expected CANCEL, got {other:?}"),
        }
    }

    /// Every `REQ_BODY` of the request up to its `REQ_END`.
    async fn whole_body(&mut self, id: u32) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            match self.next().await {
                (_, Frame::ReqBody(chunk)) => body.extend_from_slice(&chunk),
                (ended, Frame::ReqEnd) => {
                    assert_eq!(ended, id, "REQ_END for the wrong stream");
                    return body;
                }
                other => panic!("expected body frames, got {other:?}"),
            }
        }
    }

    /// The body bytes already forwarded, and then the stream's `CANCEL`.
    async fn until_cancelled(&mut self) -> u32 {
        loop {
            match self.next().await {
                (_, Frame::ReqBody(_)) => continue,
                (id, Frame::Cancel) => return id,
                other => panic!("expected body frames or CANCEL, got {other:?}"),
            }
        }
    }

    async fn send(&mut self, id: u32, frame: &Frame) {
        let mut wire = Vec::new();
        Limits::default()
            .encode(frame, id, &mut wire)
            .expect("encode");
        tokio::time::timeout(PATIENCE, self.io.write_all(&wire))
            .await
            .expect("http.chan went quiet")
            .expect("write http.chan");
    }

    /// Answer a request: head, one body frame, end.
    async fn respond(&mut self, id: u32, status: u16, headers: &[(&str, &str)], body: &[u8]) {
        let head = RespHead {
            status,
            reason: Vec::new(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
        };
        self.send(id, &Frame::RespHead(head)).await;
        if !body.is_empty() {
            self.send(id, &Frame::RespBody(body.to_vec())).await;
        }
        self.send(id, &Frame::RespEnd).await;
    }
}

/// One client connection, as the load balancer would drive it.
struct Client<IO = tokio::io::DuplexStream> {
    io: IO,
    pending: Vec<u8>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Client<IO> {
    fn new(io: IO) -> Self {
        Self {
            io,
            pending: Vec::new(),
        }
    }

    async fn write(&mut self, bytes: &[u8]) {
        tokio::time::timeout(PATIENCE, self.io.write_all(bytes))
            .await
            .expect("the front end stopped reading")
            .expect("write to the front end");
    }

    /// Read more bytes; `0` means the connection closed.
    async fn fill(&mut self) -> usize {
        let mut buf = vec![0u8; 4096];
        let read = tokio::time::timeout(PATIENCE, self.io.read(&mut buf))
            .await
            .expect("the front end stopped answering")
            .expect("read from the front end");
        self.pending.extend_from_slice(&buf[..read]);
        read
    }

    async fn read_head(&mut self) -> Vec<u8> {
        let mut scanned = 0usize;
        loop {
            let from = scanned.saturating_sub(3);
            if let Some(index) = find(&self.pending[from..], b"\r\n\r\n") {
                return self.pending.drain(..from + index + 4).collect();
            }
            scanned = self.pending.len();
            assert_ne!(self.fill().await, 0, "closed before the head ended");
        }
    }

    async fn read_to_end(&mut self) -> Vec<u8> {
        while self.fill().await != 0 {}
        std::mem::take(&mut self.pending)
    }

    /// One whole response: head plus the body its own headers frame.
    async fn read_response(&mut self) -> Vec<u8> {
        let mut out = self.read_head().await;
        if let Some(len) = content_length(&out) {
            while self.pending.len() < len {
                assert_ne!(self.fill().await, 0, "closed mid-body");
            }
            out.extend(self.pending.drain(..len));
        } else {
            loop {
                out.extend_from_slice(&std::mem::take(&mut self.pending));
                if out.ends_with(b"0\r\n\r\n") {
                    break;
                }
                assert_ne!(self.fill().await, 0, "closed mid-chunked-body");
            }
        }
        out
    }

    /// Whatever arrives within `wait`; `Some(vec![])` means it was closed.
    async fn read_within(&mut self, wait: Duration) -> Option<Vec<u8>> {
        if !self.pending.is_empty() {
            return Some(std::mem::take(&mut self.pending));
        }
        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(wait, self.io.read(&mut buf)).await {
            Err(_) => None,
            Ok(Err(_)) | Ok(Ok(0)) => Some(Vec::new()),
            Ok(Ok(read)) => {
                buf.truncate(read);
                Some(buf)
            }
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The response's `Content-Length`, when it declared one.
fn content_length(raw: &[u8]) -> Option<usize> {
    String::from_utf8_lossy(raw).split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

/// Everything the front end emitted.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<Event>>>);

impl Events {
    fn emit(&self) -> Emit {
        let log = Arc::clone(&self.0);
        Arc::new(move |event| {
            log.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(event);
        })
    }

    fn all(&self) -> Vec<Event> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// The `http.rejected` events, as `(status, reason)`.
    fn rejections(&self) -> Vec<(u16, String)> {
        self.all()
            .into_iter()
            .filter_map(|event| match event.kind() {
                EventKind::HttpRejected(rejected) => {
                    Some((rejected.status(), rejected.reason().as_str().to_string()))
                }
                _ => None,
            })
            .collect()
    }
}

/// One front end and one connection to it.
struct Harness {
    client: Client,
    scheduler: Scheduler,
    events: Events,
    /// Held so the connection task lives as long as the harness does.
    #[allow(dead_code)]
    task: JoinHandle<Result<(), HttpError>>,
}

impl Harness {
    fn start(cfg: HttpConfig) -> Self {
        let (http_chan, scheduler_chan) = UnixStream::pair().expect("socketpair");
        let events = Events::default();
        let server = Arc::new(Server::new(http_chan, cfg, events.emit()));
        let (server_side, client_side) = tokio::io::duplex(1 << 20);
        let task = tokio::spawn(async move { server.serve_connection(server_side, PEER).await });
        Self {
            client: Client::new(client_side),
            scheduler: Scheduler::new(scheduler_chan),
            events,
            task,
        }
    }
}

/// The status line of a refusal the front end wrote itself.
fn refusal(status: u16, phrase: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} {phrase}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

/// Write `request`, read the refusal, and check the event it produced.
async fn refusal_for(cfg: HttpConfig, request: &[u8], status: u16, phrase: &str, reason: &str) {
    let mut h = Harness::start(cfg);
    h.client.write(request).await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(status, phrase),
        "request {request:?}"
    );
    assert_eq!(
        h.events.rejections(),
        [(status, reason.to_string())],
        "request {request:?}"
    );
    assert!(
        h.scheduler.next_within(QUIET).await.is_none(),
        "a refused request never reaches the scheduler"
    );
}

//
// Request framing (§2.3).
//

#[tokio::test]
async fn a_chunked_body_is_dechunked_and_its_hop_headers_dropped() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(
            b"POST /chunked HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n\
              5;name=value\r\nhello\r\n\
              0001\r\n \r\n\
              5\r\nworld\r\n\
              0;final\r\nX-Trailer: 1\r\nAnother: 2\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(
        head.remote_addr, PEER_IP,
        "the peer's address, without its port (CGI)"
    );
    assert!(
        !head
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"transfer-encoding")),
        "the chunked framing is hop-by-hop: extensions and trailers are dropped"
    );
    assert_eq!(h.scheduler.whole_body(id).await, b"hello world");
    h.scheduler.respond(id, 200, &[], b"done").await;
    let _ = h.client.read_response().await;
    assert!(h.events.rejections().is_empty());
}

#[tokio::test]
async fn a_body_crosses_before_all_of_it_has_arrived() {
    // The memory promise: the front end does not buffer a whole upload, so the
    // first three of eight bytes are on `http.chan` already.
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST /up HTTP/1.1\r\nHost: h\r\nContent-Length: 8\r\n\r\nabc")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.body().await, b"abc");
    h.client.write(b"defgh").await;
    assert_eq!(h.scheduler.whole_body(id).await, b"defgh");
    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;
}

#[tokio::test]
async fn pipelined_requests_are_answered_in_order_with_their_own_streams() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"GET /first HTTP/1.1\r\nHost: h\r\n\r\nGET /second HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (first, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/first");
    assert_eq!(h.scheduler.end().await, first);
    assert!(
        h.scheduler.next_within(QUIET).await.is_none(),
        "the second request waits for the first response"
    );
    h.scheduler
        .respond(first, 200, &[("Content-Length", "5")], b"first")
        .await;
    let (second, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/second");
    assert_ne!(first, second);
    assert_eq!(h.scheduler.end().await, second);
    h.scheduler.respond(second, 204, &[], b"").await;

    let mut all = h.client.read_response().await;
    all.extend_from_slice(&h.client.read_head().await);
    assert_eq!(
        all,
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst\
          HTTP/1.1 204 No Content\r\n\r\n"
    );
}

#[tokio::test]
async fn a_content_length_of_zero_reads_nothing_and_sends_no_interim() {
    // `body none` at the framing layer: no body frame, and no interim answer
    // even when the client asked for one.
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(
            b"POST /empty HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"POST");
    assert_eq!(h.scheduler.end().await, id, "REQ_END without a body frame");
    assert!(h.scheduler.next_within(QUIET).await.is_none());
    h.scheduler
        .respond(id, 204, &[("Content-Length", "0")], b"")
        .await;
    assert_eq!(
        h.client.read_head().await,
        b"HTTP/1.1 204 No Content\r\n\r\n",
        "no `100 Continue` first, and a 204 carries no length"
    );
}

#[tokio::test]
async fn expect_100_continue_is_answered_before_the_body_is_read() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST /up HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n")
        .await;
    // No body byte has been sent, and the answer is already there.
    assert_eq!(h.client.read_head().await, b"HTTP/1.1 100 Continue\r\n\r\n");
    let (id, _) = h.scheduler.head().await;
    h.client.write(b"abc").await;
    assert_eq!(h.scheduler.whole_body(id).await, b"abc");
    h.scheduler.respond(id, 200, &[], b"thanks").await;
    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nthanks\r\n0\r\n\r\n"
    );
}

//
// `body` policies at the framing layer: the front end relays, never decodes.
//

#[tokio::test]
async fn every_body_policy_crosses_the_framing_layer_verbatim() {
    // A duplicate key, a UTF-8 BOM and a lone-surrogate escape (ingress refuses
    // each one); a bad percent escape and a `+` in a form body; and every byte
    // value, which is what `raw unvalidated` is.
    let json: &[u8] = b"\xEF\xBB\xBF{\"a\":1,\"a\":2,\"b\":\"\\ud800\"}";
    let form: &[u8] = b"a=1&b=%zz&c=+d&d";
    let raw: Vec<u8> = (0..=255u8).collect();
    for (content_type, body) in [
        ("application/json", json.to_vec()),
        ("application/x-www-form-urlencoded", form.to_vec()),
        ("application/octet-stream", raw),
    ] {
        let mut request = format!(
            "POST /body HTTP/1.1\r\nHost: h\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(&body);
        let mut h = Harness::start(HttpConfig::default());
        h.client.write(&request).await;
        let (id, _) = h.scheduler.head().await;
        assert_eq!(h.scheduler.whole_body(id).await, body, "{content_type}");
        h.scheduler.respond(id, 200, &[], b"").await;
        assert!(h.events.all().is_empty(), "{content_type}");
    }
}

//
// Payload size limits (§8.2).
//

#[tokio::test]
async fn a_declared_or_chunked_body_over_the_limit_is_413() {
    refusal_for(
        HttpConfig {
            max_body_bytes: 10,
            ..HttpConfig::default()
        },
        b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 11\r\n\r\n",
        413,
        "Content Too Large",
        "body_too_large",
    )
    .await;

    // A chunked body is limited chunk by chunk, and the stream is cancelled
    // after the bytes already forwarded.
    let mut h = Harness::start(HttpConfig {
        max_body_bytes: 8,
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    h.client.write(b"10\r\n0123456789abcdef\r\n0\r\n\r\n").await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(413, "Content Too Large")
    );
    assert_eq!(h.scheduler.until_cancelled().await, id);
    assert_eq!(h.events.rejections(), [(413, "body_too_large".to_string())]);
}

//
// Client disconnects, and the two timeouts that end a request (§3.1, §8.2).
//

#[tokio::test]
async fn a_client_that_goes_away_or_quiet_mid_body_cancels_the_stream() {
    // A client that vanishes.
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.body().await, b"abc");
    drop(h.client);
    assert_eq!(h.scheduler.cancel().await, id);
    assert!(h.scheduler.next_within(QUIET).await.is_none());

    // ... and one that stops sending, which is treated as gone: no answer.
    let mut h = Harness::start(HttpConfig {
        keepalive_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.body().await, b"abc");
    assert_eq!(h.scheduler.cancel().await, id);
    assert_eq!(
        h.client.read_within(Duration::from_secs(2)).await,
        Some(Vec::new())
    );
    assert!(h.events.rejections().is_empty());
}

#[tokio::test]
async fn a_body_that_cannot_finish_in_body_timeout_is_408_and_cancelled() {
    // The total deadline fires even though no single wait looked idle.
    let mut h = Harness::start(HttpConfig {
        body_timeout: Duration::from_millis(120),
        keepalive_timeout: Duration::from_secs(30),
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST /slow HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.body().await, b"abc");
    assert_eq!(
        h.client.read_to_end().await,
        refusal(408, "Request Timeout"),
        "an upload that cannot finish in time is refused"
    );
    assert_eq!(h.scheduler.until_cancelled().await, id);
    assert_eq!(h.events.rejections(), [(408, "body_timeout".to_string())]);
}

#[tokio::test]
async fn a_head_over_the_limit_is_431_and_a_stalled_head_is_408() {
    let mut request = b"GET / HTTP/1.1\r\nHost: h\r\n".to_vec();
    for index in 0..10 {
        request.extend_from_slice(format!("X-Pad-{index}: {}\r\n", "p".repeat(20)).as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    refusal_for(
        HttpConfig {
            max_header_bytes: 128,
            ..HttpConfig::default()
        },
        &request,
        431,
        "Request Header Fields Too Large",
        "header_too_large",
    )
    .await;

    // A head that never terminates is answered rather than waited on forever.
    let mut h = Harness::start(HttpConfig {
        header_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    });
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n").await;
    assert_eq!(
        h.client.read_to_end().await,
        refusal(408, "Request Timeout")
    );
    assert_eq!(h.events.rejections(), [(408, "header_timeout".to_string())]);
    assert!(h.scheduler.next_within(QUIET).await.is_none());
}

#[tokio::test]
async fn an_expectation_other_than_100_continue_is_417() {
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nExpect: 200-ok\r\nContent-Length: 1\r\n\r\n",
        417,
        "Expectation Failed",
        "expect_unsupported",
    )
    .await;
}

//
// Request smuggling, protocol upgrades and targets the front end cannot route.
//

#[tokio::test]
async fn request_framing_headers_are_refused() {
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
        400,
        "Bad Request",
        "content_length_conflict",
    )
    .await;
    for value in ["", "-1", "abc", "1 2"] {
        let request = format!("POST / HTTP/1.1\r\nHost: h\r\nContent-Length: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            400,
            "Bad Request",
            "invalid_content_length",
        )
        .await;
    }
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\n",
        400,
        "Bad Request",
        "duplicate_content_length",
    )
    .await;
    for value in ["gzip", "identity", "chunked, gzip"] {
        let request = format!("POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            501,
            "Not Implemented",
            "transfer_encoding_unsupported",
        )
        .await;
    }
    refusal_for(
        HttpConfig::default(),
        b"POST / HTTP/1.0\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n",
        400,
        "Bad Request",
        "transfer_encoding_on_http_1_0",
    )
    .await;
}

#[tokio::test]
async fn an_upgrade_a_connect_or_a_non_origin_target_is_refused() {
    for value in ["websocket", "h2c"] {
        let request =
            format!("GET / HTTP/1.1\r\nHost: h\r\nConnection: Upgrade\r\nUpgrade: {value}\r\n\r\n");
        refusal_for(
            HttpConfig::default(),
            request.as_bytes(),
            501,
            "Not Implemented",
            "upgrade_unsupported",
        )
        .await;
    }
    refusal_for(
        HttpConfig::default(),
        b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
        501,
        "Not Implemented",
        "connect_unsupported",
    )
    .await;
    // Absolute form, authority form and a bare word: only origin form routes.
    for (request, reason) in [
        (
            &b"GET http://example.com/x HTTP/1.1\r\nHost: h\r\n\r\n"[..],
            "absolute_form_target",
        ),
        (
            b"GET example.com:443 HTTP/1.1\r\nHost: h\r\n\r\n",
            "absolute_form_target",
        ),
        (b"GET foo HTTP/1.1\r\nHost: h\r\n\r\n", "invalid_target"),
        (
            b"POST * HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n",
            "invalid_target",
        ),
    ] {
        refusal_for(HttpConfig::default(), request, 400, "Bad Request", reason).await;
    }
    // ... while `OPTIONS *` is defined, and is forwarded as it is.
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"OPTIONS * HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"OPTIONS");
    assert_eq!(head.target, b"*");
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 204, &[], b"").await;
    let _ = h.client.read_head().await;
}

#[tokio::test]
async fn http_2_and_a_bare_lf_in_the_head_are_refused() {
    for request in [
        &b"GET / HTTP/2.0\r\nHost: h\r\n\r\n"[..],
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            505,
            "HTTP Version Not Supported",
            "version_unsupported",
        )
        .await;
    }
    for request in [
        &b"GET / HTTP/1.1\nHost: h\r\n\r\n"[..],
        b"GET / HTTP/1.1\r\nHost: h\nAccept: */*\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: h\r\n\n",
    ] {
        refusal_for(
            HttpConfig::default(),
            request,
            400,
            "Bad Request",
            "malformed_request",
        )
        .await;
    }
}

#[tokio::test]
async fn http_1_1_needs_exactly_one_usable_host() {
    for (request, reason) in [
        (&b"GET / HTTP/1.1\r\n\r\n"[..], "missing_host"),
        (b"GET / HTTP/1.1\r\nHost: a b\r\n\r\n", "invalid_host"),
        (
            b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n",
            "duplicate_host",
        ),
    ] {
        refusal_for(HttpConfig::default(), request, 400, "Bad Request", reason).await;
    }
}

//
// The accept loop's connection cap, and what an event may never carry.
//

#[tokio::test]
async fn connections_over_the_cap_are_answered_with_503() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let (http_chan, scheduler_chan) = UnixStream::pair().expect("socketpair");
    let events = Events::default();
    let server = Arc::new(Server::new(
        http_chan,
        HttpConfig {
            max_connections: 1,
            ..HttpConfig::default()
        },
        events.emit(),
    ));
    let serving = Arc::clone(&server);
    let task = tokio::spawn(async move { serving.run(TcpAcceptor::new(listener)).await });
    let mut scheduler = Scheduler::new(scheduler_chan);

    // The first connection stays open after one request.
    let mut first = Client::new(TcpStream::connect(addr).await.expect("connect"));
    first.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);
    scheduler
        .respond(id, 200, &[("Content-Length", "3")], b"one")
        .await;
    let _ = first.read_response().await;

    // The second is answered without ever touching `http.chan`.
    let mut second = Client::new(TcpStream::connect(addr).await.expect("connect"));
    second.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    assert_eq!(
        second.read_to_end().await,
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(scheduler.next_within(QUIET).await.is_none());
    let rejections = events.rejections();
    assert_eq!(rejections, [(503, "too_many_connections".to_string())]);

    // The first connection is unaffected, which is the point of answering.
    first.write(b"GET /again HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);
    scheduler.respond(id, 204, &[], b"").await;
    let _ = first.read_head().await;
    task.abort();
}

#[tokio::test]
async fn a_rejection_event_carries_no_header_a_target_or_a_body() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(
            b"POST /secret?token=abc HTTP/1.1\r\nHost: h\r\nAuthorization: Bearer hunter2\r\n\
              Content-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await;
    assert_eq!(h.client.read_to_end().await, refusal(400, "Bad Request"));
    let events = h.events.all();
    assert_eq!(events.len(), 1);
    let encoded = encode_json(&events[0]);
    for needle in [
        "secret",
        "token",
        "abc",
        "hunter2",
        "Authorization",
        "content-length",
        "chunked",
    ] {
        assert!(!encoded.contains(needle), "{needle:?} leaked: {encoded}");
    }
    assert!(encoded.contains("\"http.rejected\""), "{encoded}");
    assert!(encoded.contains("400"), "{encoded}");
}
