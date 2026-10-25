//! Fixtures shared by the integration tests: a fake scheduler on one end of a
//! socketpair, a client on a duplex stream, and a front end in between.
//!
//! Everything here is in-process and deterministic: no network, no ports
//! (except where a test asks for a real listener), no clock beyond the
//! timeouts the tests set themselves.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use blastit_events::{Event, EventKind};
use blastit_frame::{Decoder, Frame, Limits, ReqHead, RespHead};
use blastit_http::{Emit, HttpConfig, HttpError, Server, TcpAcceptor, serve_with};
use tokio::io::{
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf,
};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tokio::task::JoinHandle;

/// The address every connection is accepted from, unless a test says otherwise.
pub const PEER: &str = "203.0.113.7:54321";

/// How long a test waits for bytes before deciding they are not coming.
pub const PATIENCE: Duration = Duration::from_secs(5);

/// How long a test waits to decide a response will not continue.
pub const QUIET: Duration = Duration::from_millis(200);

/// The scheduler end of `http.chan`, driven directly by the tests.
pub struct Scheduler {
    reader: ReadHalf<UnixStream>,
    writer: WriteHalf<UnixStream>,
    decoder: Decoder,
    buf: Vec<u8>,
}

impl Scheduler {
    /// Wrap the scheduler's end of the socketpair.
    pub fn new(io: UnixStream) -> Self {
        let (reader, writer) = tokio::io::split(io);
        Self {
            reader,
            writer,
            decoder: Decoder::new(Limits::default()),
            buf: vec![0u8; 64 * 1024],
        }
    }

    /// The next frame the front end sent.
    pub async fn next(&mut self) -> (u32, Frame) {
        self.next_within(PATIENCE)
            .await
            .expect("http.chan went quiet")
    }

    /// The next frame, or `None` when nothing arrives within `wait`.
    pub async fn next_within(&mut self, wait: Duration) -> Option<(u32, Frame)> {
        loop {
            if let Some(frame) = self.decoder.next_frame().expect("frame on http.chan") {
                return Some(frame);
            }
            let read = tokio::time::timeout(wait, self.reader.read(&mut self.buf))
                .await
                .ok()?
                .expect("read http.chan");
            assert_ne!(read, 0, "http.chan closed");
            self.decoder.push(&self.buf[..read]);
        }
    }

    /// The next `REQ_HEAD`, with its stream id.
    pub async fn head(&mut self) -> (u32, ReqHead) {
        match self.next().await {
            (id, Frame::ReqHead(head)) => (id, head),
            other => panic!("expected REQ_HEAD, got {other:?}"),
        }
    }

    /// The next `REQ_BODY`, with its stream id.
    pub async fn body(&mut self) -> (u32, Vec<u8>) {
        match self.next().await {
            (id, Frame::ReqBody(body)) => (id, body),
            other => panic!("expected REQ_BODY, got {other:?}"),
        }
    }

    /// The next `REQ_END`, with its stream id.
    pub async fn end(&mut self) -> u32 {
        match self.next().await {
            (id, Frame::ReqEnd) => id,
            other => panic!("expected REQ_END, got {other:?}"),
        }
    }

    /// The next `CANCEL`, with its stream id.
    pub async fn cancel(&mut self) -> u32 {
        match self.next().await {
            (id, Frame::Cancel) => id,
            other => panic!("expected CANCEL, got {other:?}"),
        }
    }

    /// Every `REQ_BODY` of `id` up to its `REQ_END`.
    pub async fn whole_body(&mut self, id: u32) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            match self.next().await {
                (_, Frame::ReqBody(chunk)) => body.extend_from_slice(&chunk),
                (ended, Frame::ReqEnd) => {
                    assert_eq!(ended, id, "REQ_END for the wrong stream");
                    return body;
                }
                (_, Frame::Cancel) => panic!("the request was cancelled"),
                other => panic!("expected body frames, got {other:?}"),
            }
        }
    }

    /// Body bytes of `id`, stopping once at least `wanted` have arrived.
    pub async fn whole_body_prefix(&mut self, _id: u32, wanted: usize) -> Vec<u8> {
        let mut body = Vec::new();
        while body.len() < wanted {
            match self.next().await {
                (_, Frame::ReqBody(chunk)) => body.extend_from_slice(&chunk),
                (_, Frame::ReqEnd) => panic!("request ended after {} bytes", body.len()),
                other => panic!("expected body frames, got {other:?}"),
            }
        }

        body
    }

    /// Body bytes of `id` up to its `CANCEL`, which is returned as well.
    pub async fn until_cancelled(&mut self) -> (u32, Vec<u8>) {
        let mut body = Vec::new();
        loop {
            match self.next().await {
                (_, Frame::ReqBody(chunk)) => body.extend_from_slice(&chunk),
                (id, Frame::Cancel) => return (id, body),
                other => panic!("expected body frames or CANCEL, got {other:?}"),
            }
        }
    }

    /// Write one frame to the front end.
    pub async fn send(&mut self, id: u32, frame: &Frame) {
        let mut wire = Vec::new();
        Limits::default()
            .encode(frame, id, &mut wire)
            .expect("encode");
        tokio::time::timeout(PATIENCE, self.writer.write_all(&wire))
            .await
            .expect("http.chan went quiet")
            .expect("write http.chan");
    }

    /// Answer a request: head, one body frame, end.
    pub async fn respond(&mut self, id: u32, status: u16, headers: &[(&str, &str)], body: &[u8]) {
        self.respond_reason(id, status, b"", headers, body).await;
    }

    /// Answer a request with an explicit reason phrase.
    pub async fn respond_reason(
        &mut self,
        id: u32,
        status: u16,
        reason: &[u8],
        headers: &[(&str, &str)],
        body: &[u8],
    ) {
        let head = RespHead {
            status,
            reason: reason.to_vec(),
            headers: pairs(headers),
        };
        self.send(id, &Frame::RespHead(head)).await;
        if !body.is_empty() {
            self.send(id, &Frame::RespBody(body.to_vec())).await;
        }
        self.send(id, &Frame::RespEnd).await;
    }

    /// Answer with a head, several body chunks, then the end.
    pub async fn respond_chunks(
        &mut self,
        id: u32,
        status: u16,
        headers: &[(&str, &str)],
        chunks: &[&[u8]],
    ) {
        self.send(
            id,
            &Frame::RespHead(RespHead {
                status,
                reason: Vec::new(),
                headers: pairs(headers),
            }),
        )
        .await;
        for chunk in chunks {
            self.send(id, &Frame::RespBody(chunk.to_vec())).await;
        }
        self.send(id, &Frame::RespEnd).await;
    }

    /// Close the scheduler end: the front end sees EOF on `http.chan`.
    pub fn close(self) {
        drop(self);
    }
}

/// Borrow a header list built from string pairs.
pub fn pairs(headers: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
        .collect()
}

/// One client connection, as the load balancer would drive it.
///
/// Bytes read from the socket are buffered: one `read` may well return a whole
/// response, or the tail of one response and the head of the next.
pub struct Client<IO = DuplexStream> {
    io: IO,
    pending: Vec<u8>,
}

impl<IO> Client<IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    /// Wrap the client end of a connection.
    pub fn new(io: IO) -> Self {
        Self {
            io,
            pending: Vec::new(),
        }
    }

    /// Send raw bytes.
    pub async fn write(&mut self, bytes: &[u8]) {
        tokio::time::timeout(PATIENCE, self.io.write_all(bytes))
            .await
            .expect("the front end stopped reading")
            .expect("write to the front end");
    }

    /// Read more bytes into the buffer; `0` means the connection closed.
    async fn fill(&mut self) -> usize {
        let mut buf = vec![0u8; 4096];
        let read = tokio::time::timeout(PATIENCE, self.io.read(&mut buf))
            .await
            .expect("the front end stopped answering")
            .expect("read from the front end");
        self.pending.extend_from_slice(&buf[..read]);
        read
    }

    /// Read exactly `n` bytes from the connection.
    pub async fn read_exact_n(&mut self, n: usize) -> Vec<u8> {
        while self.pending.len() < n {
            if self.fill().await == 0 {
                panic!(
                    "the connection closed with {} of {n} bytes",
                    self.pending.len()
                );
            }
        }
        self.pending.drain(..n).collect()
    }

    /// Read until `needle` has been seen, returning everything up to it.
    pub async fn read_until(&mut self, needle: &[u8]) -> Vec<u8> {
        loop {
            if let Some(index) = find(&self.pending, needle) {
                let end = index + needle.len();
                return self.pending.drain(..end).collect();
            }
            if self.fill().await == 0 {
                panic!("the connection closed before {needle:?}");
            }
        }
    }

    /// Read until the front end closes the connection.
    pub async fn read_to_end(&mut self) -> Vec<u8> {
        while self.fill().await != 0 {}
        std::mem::take(&mut self.pending)
    }

    /// Whatever the front end sends within `wait`.
    ///
    /// `None` means nothing arrived; `Some(vec![])` means the connection was
    /// closed with nothing more to read.
    pub async fn read_within(&mut self, wait: Duration) -> Option<Vec<u8>> {
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

    /// The status line and headers of one response.
    pub async fn read_head(&mut self) -> Vec<u8> {
        self.read_until(b"\r\n\r\n").await
    }

    /// One whole response: head plus body, framed according to its headers.
    ///
    /// An interim `100 Continue` only has a head, and is returned as it is. A
    /// `HEAD` answer has a head that claims a length without a body, so use
    /// [`Client::read_head`] for those.
    pub async fn read_response(&mut self) -> Vec<u8> {
        let mut out = self.read_head().await;
        if out.starts_with(b"HTTP/1.1 100 ") {
            return out;
        }
        if let Some(len) = header_usize(&out, "content-length") {
            out.extend_from_slice(&self.read_exact_n(len).await);
        } else if header_has_token(&out, "transfer-encoding", "chunked") {
            loop {
                out.extend_from_slice(&std::mem::take(&mut self.pending));
                if out.ends_with(b"0\r\n\r\n") {
                    break;
                }
                if self.fill().await == 0 {
                    panic!("the connection closed mid-chunked-body");
                }
            }
        } else {
            out.extend_from_slice(&self.read_to_end().await);
        }
        out
    }

    /// Drop the connection, as a client that clicks away would.
    pub fn disconnect(self) {
        drop(self);
    }
}

/// The value of the first `name` header of a raw response, lower-cased.
pub fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let end = find(raw, b"\r\n\r\n").map_or(raw.len(), |index| index + 2);
    let text = String::from_utf8_lossy(&raw[..end]);
    for line in text.split("\r\n").skip(1) {
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case(name)
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Whether the raw response has a `name` header carrying `token`.
pub fn header_has_token(raw: &[u8], name: &str, token: &str) -> bool {
    match header_value(raw, name) {
        Some(value) => value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case(token)),
        None => false,
    }
}

/// Whether the raw response has a `name` header at all.
pub fn header_present(raw: &[u8], name: &str) -> bool {
    header_value(raw, name).is_some()
}

/// The parsed value of the `name` header, when it is a plain number.
pub fn header_usize(raw: &[u8], name: &str) -> Option<usize> {
    header_value(raw, name)?.parse().ok()
}

/// All header names of a raw response, in order.
pub fn header_names(raw: &[u8]) -> Vec<String> {
    let end = find(raw, b"\r\n\r\n").map_or(raw.len(), |index| index + 2);
    let text = String::from_utf8_lossy(&raw[..end]);
    text.split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':').map(|(key, _)| key.to_string()))
        .collect()
}

/// The offset of `needle` in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A log of the events the front end emitted.
#[derive(Clone, Default)]
pub struct EventLog(Arc<Mutex<Vec<Event>>>);

impl EventLog {
    /// A sink that records every event.
    pub fn emit(&self) -> Emit {
        let log = Arc::clone(&self.0);
        Arc::new(move |event| {
            log.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(event);
        })
    }

    /// Every event so far.
    pub fn all(&self) -> Vec<Event> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// The `http.rejected` events, as `(remote_addr, status, reason)`.
    pub fn rejections(&self) -> Vec<(String, u16, String)> {
        self.all()
            .into_iter()
            .filter_map(|event| match event.kind() {
                EventKind::HttpRejected(rejected) => Some((
                    rejected.remote_addr().to_string(),
                    rejected.status(),
                    rejected.reason().as_str().to_string(),
                )),
                _ => None,
            })
            .collect()
    }

    /// How many events were emitted.
    pub fn len(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len()
    }

    /// Whether nothing was emitted at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One front end, one scheduler, as many connections as a test opens.
pub struct Rig {
    /// The front end, shared by every connection.
    pub server: Arc<Server>,
    /// The scheduler end of `http.chan`.
    pub scheduler: Scheduler,
    /// Everything the front end emitted.
    pub events: EventLog,
}

impl Rig {
    /// Start a front end talking to a fake scheduler over a socketpair.
    pub fn new(cfg: HttpConfig) -> Self {
        let (http_chan, scheduler_chan) = UnixStream::pair().expect("socketpair");
        let scheduler = Scheduler::new(scheduler_chan);
        let events = EventLog::default();
        let server = Arc::new(Server::new(http_chan, cfg, events.emit()));
        Self {
            server,
            scheduler,
            events,
        }
    }

    /// Open a client connection, and the task serving it.
    pub fn connect(&self) -> (Client, JoinHandle<Result<(), HttpError>>) {
        self.connect_from(PEER)
    }

    /// Open a client connection accepted from `peer`.
    pub fn connect_from(&self, peer: &str) -> (Client, JoinHandle<Result<(), HttpError>>) {
        let (server_side, client_side) = tokio::io::duplex(1 << 20);
        let server = Arc::clone(&self.server);
        let peer = peer.to_string();
        let task = tokio::spawn(async move { server.serve_connection(server_side, peer).await });
        (Client::new(client_side), task)
    }
}

/// A front end listening on a real loopback socket, served by [`serve_with`].
///
/// This is the only fixture that goes through the accept loop and the real
/// `TcpAcceptor`; everything else drives one connection directly.
pub struct Listening {
    /// The address the listener was bound to.
    pub addr: std::net::SocketAddr,
    /// The scheduler end of `http.chan`.
    pub scheduler: Scheduler,
    /// Everything the front end emitted.
    pub events: EventLog,
    /// The `serve` task itself.
    pub task: JoinHandle<Result<(), HttpError>>,
}

impl Listening {
    /// Bind a loopback listener and start serving on it.
    pub async fn start(cfg: HttpConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback listener");
        let addr = listener.local_addr().expect("listener address");
        let (http_chan, scheduler_chan) = UnixStream::pair().expect("socketpair");
        let events = EventLog::default();
        let scheduler = Scheduler::new(scheduler_chan);
        let task = tokio::spawn(serve_with(
            TcpAcceptor::new(listener),
            http_chan,
            cfg,
            events.emit(),
        ));
        Self {
            addr,
            scheduler,
            events,
            task,
        }
    }

    /// Open a client connection to the listening socket.
    pub async fn connect(&self) -> Client<TcpStream> {
        let io = TcpStream::connect(self.addr).await.expect("connect");
        Client::new(io)
    }
}

/// A rig with a single connection already open.
pub struct Harness {
    /// The client side of one connection.
    pub client: Client,
    /// The scheduler end of `http.chan`.
    pub scheduler: Scheduler,
    /// Everything the front end emitted.
    pub events: EventLog,
    /// The task serving this connection.
    pub task: JoinHandle<Result<(), HttpError>>,
}

impl Harness {
    /// Start a front end and open one connection to it.
    pub fn start(cfg: HttpConfig) -> Self {
        let rig = Rig::new(cfg);
        let (client, task) = rig.connect();
        Self {
            client,
            scheduler: rig.scheduler,
            events: rig.events,
            task,
        }
    }
}
