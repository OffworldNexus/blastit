//! One client connection: parsing, limits, forwarding and encoding
//! (ARCHITECTURE.md §2.3, §3.2).
//!
//! The loop is deliberately serial. A connection carries requests one at a
//! time, in the order they arrived, which is what pipelining means; leftover
//! bytes in the read buffer are simply the start of the next request. Only the
//! response is relayed frame by frame, so a large body never has to exist in
//! one piece on either side.

use std::io;

use blastit_events::{Component, Event, EventKind, HttpRejected, RejectReason};
use blastit_frame::{Frame, ReqHead};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout_at};

use crate::Emit;
use crate::chan::{Chan, Stream};
use crate::chunked::{ChunkError, Dechunker, Step};
use crate::config::HttpConfig;
use crate::error::HttpError;
use crate::head::{BodyFraming, Parsed, Reject, RequestHead, Version};
use crate::response::{self, Framing};

/// Most bytes read from the client in one go.
const READ_CHUNK: usize = 16 * 1024;

/// Consumed bytes below this mark are not worth moving to the front of the
/// buffer; above it, the buffer is compacted once the read cursor is past
/// halfway.
const COMPACT_THRESHOLD: usize = 4096;

/// What ended the head-reading phase.
enum HeadOutcome {
    /// Client gone, or an idle connection that timed out: say nothing.
    Close,
    /// Refuse the request, then close.
    Reject(Reject),
    /// A usable request.
    Request(RequestHead),
}

/// What ended the body-reading phase.
enum BodyOutcome {
    /// `REQ_END` was sent.
    Sent,
    /// The client stopped sending; `CANCEL` is due.
    ClientGone,
    /// The body was refused; the request may already have reached the
    /// scheduler, so `CANCEL` is due as well.
    Refused(Reject),
}

/// What to do once a response is finished.
enum Relay {
    /// The response is complete and the connection may carry another request.
    KeepAlive,
    /// The response is complete, but the connection must close.
    Close,
    /// The client can no longer be served: close without finishing.
    Abort,
}

/// The result of waiting for more bytes.
enum Filled {
    /// At least one byte arrived.
    Bytes,
    /// Nothing arrived before the deadline.
    Idle,
    /// The client is gone, or the read failed.
    Closed,
}

/// The response being written for the request in flight.
struct Resp {
    framing: Framing,
    head_written: bool,
    written: u64,
    close: bool,
}

impl Resp {
    fn new() -> Self {
        Self {
            framing: Framing::None,
            head_written: false,
            written: 0,
            close: false,
        }
    }
}

/// One client connection.
pub(crate) struct Conn<'a, IO> {
    io: IO,
    peer: String,
    cfg: &'a HttpConfig,
    emit: &'a Emit,
    chan: &'a Chan,
    buf: Vec<u8>,
    pos: usize,
    scratch: Vec<u8>,
    started: Instant,
    requests: u64,
}

impl<'a, IO> Conn<'a, IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    /// A connection over `io`, accepted at `peer`.
    pub(crate) fn new(
        io: IO,
        peer: String,
        cfg: &'a HttpConfig,
        emit: &'a Emit,
        chan: &'a Chan,
    ) -> Self {
        Self {
            io,
            peer,
            cfg,
            emit,
            chan,
            buf: Vec::new(),
            pos: 0,
            scratch: Vec::new(),
            started: Instant::now(),
            requests: 0,
        }
    }

    /// Serve requests on this connection until it ends.
    ///
    /// Only a broken scheduler channel is an error; every client-side failure
    /// simply ends the connection.
    pub(crate) async fn run(mut self) -> Result<(), HttpError> {
        loop {
            match self.read_head().await {
                HeadOutcome::Close => return Ok(()),
                HeadOutcome::Reject(reject) => {
                    self.reject(reject).await;
                    return Ok(());
                }
                HeadOutcome::Request(head) => {
                    self.requests += 1;
                    if !self.serve(head).await? {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Read and normalise the head of the next request.
    async fn read_head(&mut self) -> HeadOutcome {
        let first = self.requests == 0;

        // Nothing of this request has arrived yet. The first request has to
        // arrive within `header_timeout` of the accept; a later one may sit
        // idle for `keepalive_timeout`, and is then closed silently.
        if self.pos == self.buf.len() {
            let deadline = if first {
                self.started + self.cfg.header_timeout
            } else {
                Instant::now() + self.cfg.keepalive_timeout
            };
            match self.fill_until(deadline).await {
                Filled::Bytes => {}
                Filled::Idle | Filled::Closed => return HeadOutcome::Close,
            }
        }

        // The head itself gets `header_timeout`, measured from the first byte
        // of the request — or from the accept, for the first request.
        let deadline = if first {
            self.started + self.cfg.header_timeout
        } else {
            Instant::now() + self.cfg.header_timeout
        };
        loop {
            match crate::head::parse(&self.buf[self.pos..], self.cfg, &self.peer) {
                Parsed::Complete(head, used) => {
                    if used > self.cfg.max_header_bytes {
                        return HeadOutcome::Reject(too_large());
                    }
                    self.pos += used;
                    return HeadOutcome::Request(head);
                }
                Parsed::Reject(reject) => return HeadOutcome::Reject(reject),
                Parsed::Partial => {
                    // An incomplete head that already fills the limit cannot
                    // become acceptable by growing.
                    if self.buf.len() - self.pos > self.cfg.max_header_bytes {
                        return HeadOutcome::Reject(too_large());
                    }
                }
            }
            match self.fill_until(deadline).await {
                Filled::Bytes => {}
                Filled::Idle => {
                    // Bytes did arrive, so the client is owed an answer.
                    return HeadOutcome::Reject(Reject {
                        status: 408,
                        reason: RejectReason::Other("header_timeout".into()),
                        remote_addr: None,
                    });
                }
                Filled::Closed => return HeadOutcome::Close,
            }
        }
    }

    /// Forward one request and relay its response.
    ///
    /// Returns whether the connection may carry another request.
    async fn serve(&mut self, head: RequestHead) -> Result<bool, HttpError> {
        let mut stream = self.chan.open();
        let request = ReqHead {
            method: head.method.clone(),
            target: head.target.clone(),
            headers: head.headers.clone(),
            remote_addr: head.remote_addr.clone(),
            scheme: head.scheme,
            server_name: head.server_name.clone(),
            server_port: self.cfg.server_port,
        };
        // A head that does not fit one frame cannot be forwarded at all.
        // Checking here, rather than letting the writer task discover it, keeps
        // one oversized request from looking like a dead channel.
        self.scratch.clear();
        let fits = self
            .cfg
            .frame_limits
            .encode(
                &Frame::ReqHead(request.clone()),
                stream.id(),
                &mut self.scratch,
            )
            .is_ok();
        // The probe borrowed the body buffer; hand it back empty.
        self.scratch.clear();
        if !fits {
            self.refuse(&head.remote_addr.clone(), 431, RejectReason::HeaderTooLarge)
                .await;
            return Ok(false);
        }

        match stream.send_head(request).await {
            Ok(()) => {}
            // The scheduler is gone: answer the one request that is in hand,
            // and let the accept loop report the channel as fatal.
            Err(_) => {
                let _ = self.io.write_all(&response::simple(502)).await;
                return Ok(false);
            }
        }

        // `100 Continue` goes out as soon as the head is on its way, and not
        // one step earlier: a client is allowed to wait for it before sending
        // a single body byte.
        if head.expect_continue
            && head.body.wants_body()
            && self.io.write_all(response::CONTINUE).await.is_err()
        {
            let _ = stream.send_cancel().await;
            return Ok(false);
        }

        match self.stream_body(&mut stream, &head).await? {
            BodyOutcome::Sent => {}
            BodyOutcome::ClientGone => {
                let _ = stream.send_cancel().await;
                return Ok(false);
            }
            BodyOutcome::Refused(reject) => {
                let _ = stream.send_cancel().await;
                self.reject(reject).await;
                return Ok(false);
            }
        }

        match self.relay(&mut stream, &head).await? {
            Relay::KeepAlive => Ok(true),
            Relay::Close => Ok(false),
            Relay::Abort => {
                let _ = stream.send_cancel().await;
                Ok(false)
            }
        }
    }

    /// Stream the request body to the scheduler, framed as it arrived.
    async fn stream_body(
        &mut self,
        stream: &mut Stream,
        head: &RequestHead,
    ) -> Result<BodyOutcome, HttpError> {
        match head.body {
            BodyFraming::None | BodyFraming::ContentLength(0) => {
                stream.send_end().await?;
                Ok(BodyOutcome::Sent)
            }
            BodyFraming::ContentLength(len) => self.stream_fixed_body(stream, len).await,
            BodyFraming::Chunked => self.stream_chunked_body(stream, head).await,
        }
    }

    /// Stream a `Content-Length` body, one `REQ_BODY` frame per read.
    async fn stream_fixed_body(
        &mut self,
        stream: &mut Stream,
        len: u64,
    ) -> Result<BodyOutcome, HttpError> {
        let mut remaining = len;
        while remaining > 0 {
            if self.pos == self.buf.len() && !self.fill_idle().await.arrived() {
                return Ok(BodyOutcome::ClientGone);
            }
            let available = (self.buf.len() - self.pos) as u64;
            let take = remaining.min(available) as usize;
            let body = self.buf[self.pos..self.pos + take].to_vec();
            self.pos += take;
            remaining -= take as u64;
            stream.send_body(body).await?;
        }
        stream.send_end().await?;
        Ok(BodyOutcome::Sent)
    }

    /// De-chunk a body as it arrives and stream it on.
    async fn stream_chunked_body(
        &mut self,
        stream: &mut Stream,
        head: &RequestHead,
    ) -> Result<BodyOutcome, HttpError> {
        let mut dechunker = Dechunker::new(self.cfg.max_body_bytes);
        self.scratch.clear();
        loop {
            let pushed = dechunker.push(&self.buf[self.pos..], &mut self.scratch);
            let (used, step) = match pushed {
                Ok((0, _)) => {
                    // Not even half a chunk line: read more and try again.
                    if self.fill_idle().await.arrived() {
                        continue;
                    }
                    return Ok(BodyOutcome::ClientGone);
                }
                Ok(progress) => progress,
                Err(ChunkError::TooLarge) => {
                    return Ok(BodyOutcome::Refused(refuse_body(
                        head,
                        413,
                        RejectReason::BodyTooLarge,
                    )));
                }
                Err(ChunkError::Malformed) => {
                    return Ok(BodyOutcome::Refused(refuse_body(
                        head,
                        400,
                        RejectReason::MalformedRequest,
                    )));
                }
            };
            self.pos += used;
            if !self.scratch.is_empty() {
                let body = std::mem::take(&mut self.scratch);
                stream.send_body(body).await?;
            }
            if step == Step::Done {
                stream.send_end().await?;
                return Ok(BodyOutcome::Sent);
            }
        }
    }

    /// Relay the response frames of a request to the client.
    async fn relay(
        &mut self,
        stream: &mut Stream,
        request: &RequestHead,
    ) -> Result<Relay, HttpError> {
        let mut resp = Resp::new();
        loop {
            let Some(frame) = stream.recv().await? else {
                // No answer can arrive any more.
                return Ok(self.fail(&mut resp, 502).await);
            };
            match frame {
                Frame::RespHead(head) => {
                    if resp.head_written {
                        // A second head would be a second response.
                        return Ok(Relay::Abort);
                    }
                    let no_body = request.is_head() || matches!(head.status, 100..=199 | 204 | 304);
                    // The view may ask for the connection to be closed too,
                    // even though its own `Connection` header never reaches the
                    // client.
                    let may_keep_alive = request.keep_alive
                        && !crate::headers::has_token(&head.headers, "connection", "close");
                    let prepared = match response::prepare(
                        &head,
                        no_body,
                        request.version == Version::Http11,
                        may_keep_alive,
                    ) {
                        Ok(prepared) => prepared,
                        // Injection from the view: nothing is written yet, so
                        // the client gets an honest 502 instead.
                        Err(_) => return Ok(self.fail(&mut resp, 502).await),
                    };
                    let mut out = Vec::new();
                    response::encode_head(&prepared, &mut out);
                    if self.io.write_all(&out).await.is_err() {
                        return Ok(Relay::Abort);
                    }
                    resp.framing = prepared.framing;
                    resp.close = !prepared.keep_alive;
                    resp.head_written = true;
                }
                Frame::RespBody(chunk) => {
                    if self.write_body(&mut resp, &chunk).await.is_err() {
                        return Ok(Relay::Abort);
                    }
                }
                Frame::RespEnd => {
                    if !resp.head_written {
                        return Ok(self.fail(&mut resp, 502).await);
                    }
                    match resp.framing {
                        Framing::Chunked => {
                            if self.io.write_all(response::LAST_CHUNK).await.is_err() {
                                return Ok(Relay::Abort);
                            }
                        }
                        // The view promised more than it sent: the client is
                        // still waiting for bytes that will never come, so the
                        // only honest thing left is to close.
                        Framing::Length(len) if resp.written < len => return Ok(Relay::Abort),
                        _ => {}
                    }
                    return Ok(if resp.close {
                        Relay::Close
                    } else {
                        Relay::KeepAlive
                    });
                }
                // The view could not build the response: 502 if the client has
                // not seen anything yet, otherwise close abruptly — for a
                // chunked body, without ever writing the terminating chunk.
                Frame::RespAbort { .. } => return Ok(self.fail(&mut resp, 502).await),
                _ => {}
            }
        }
    }

    /// Write a response the front end produced itself, or close the connection
    /// when the client has already seen part of one.
    async fn fail(&mut self, resp: &mut Resp, status: u16) -> Relay {
        if resp.head_written {
            return Relay::Abort;
        }
        resp.head_written = true;
        match self.io.write_all(&response::simple(status)).await {
            Ok(()) => Relay::Close,
            Err(_) => Relay::Abort,
        }
    }

    /// Write one body chunk, honouring the framing already announced.
    async fn write_body(&mut self, resp: &mut Resp, chunk: &[u8]) -> io::Result<()> {
        if !resp.head_written {
            // A body before the head is a view bug; ignore it and wait for the
            // head or an abort.
            return Ok(());
        }
        match resp.framing {
            Framing::None => Ok(()),
            Framing::Length(len) => {
                let left = len.saturating_sub(resp.written);
                if left == 0 {
                    // Longer than advertised: truncate and close.
                    resp.close = true;
                    return Ok(());
                }
                let take = left.min(chunk.len() as u64) as usize;
                if take < chunk.len() {
                    resp.close = true;
                }
                self.io.write_all(&chunk[..take]).await?;
                resp.written += take as u64;
                Ok(())
            }
            Framing::Chunked => {
                // An empty chunk would terminate the body.
                if chunk.is_empty() {
                    return Ok(());
                }
                let framed = response::chunk(chunk);
                self.io.write_all(&framed).await?;
                resp.written += chunk.len() as u64;
                Ok(())
            }
            Framing::Close => {
                self.io.write_all(chunk).await?;
                resp.written += chunk.len() as u64;
                Ok(())
            }
        }
    }

    /// Emit `http.rejected` and answer with a status line and no body.
    async fn reject(&mut self, reject: Reject) {
        let remote = reject
            .remote_addr
            .clone()
            .unwrap_or_else(|| self.peer.clone());
        self.refuse(&remote, reject.status, reject.reason).await;
    }

    /// Emit `http.rejected` for `remote`, then write the refusal.
    async fn refuse(&mut self, remote: &str, status: u16, reason: RejectReason) {
        self.emit_rejected(remote, status, reason);
        let _ = self.io.write_all(&response::simple(status)).await;
    }

    /// Publish one `http.rejected` event (§8.1).
    ///
    /// The event carries the address, the status and a fixed label. It never
    /// carries a header value, a query string or a body byte, because none of
    /// them are arguments here.
    fn emit_rejected(&self, remote_addr: &str, status: u16, reason: RejectReason) {
        let kind = EventKind::HttpRejected(HttpRejected::new(remote_addr, status, reason));
        (self.emit)(Event::now(Component::Http, kind));
    }

    /// Read more bytes, giving up at `deadline`.
    async fn fill_until(&mut self, deadline: Instant) -> Filled {
        match timeout_at(deadline, self.fill()).await {
            Err(_) => Filled::Idle,
            Ok(Ok(0)) | Ok(Err(_)) => Filled::Closed,
            Ok(Ok(_)) => Filled::Bytes,
        }
    }

    /// Read more bytes, giving up after `keepalive_timeout` with nothing
    /// arriving.
    ///
    /// §8.2 has no separate body timeout, so a body that stops mid-upload is
    /// treated exactly like an idle connection: the client is gone, and the
    /// request is cancelled.
    async fn fill_idle(&mut self) -> Filled {
        self.fill_until(Instant::now() + self.cfg.keepalive_timeout)
            .await
    }

    /// One read into the buffer, compacting it first when that pays off.
    async fn fill(&mut self) -> io::Result<usize> {
        if self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos >= COMPACT_THRESHOLD && self.pos * 2 >= self.buf.len() {
            self.buf.copy_within(self.pos.., 0);
            self.buf.truncate(self.buf.len() - self.pos);
            self.pos = 0;
        }
        let mut chunk = [0u8; READ_CHUNK];
        let read = self.io.read(&mut chunk).await?;
        self.buf.extend_from_slice(&chunk[..read]);
        Ok(read)
    }
}

impl Filled {
    /// Whether at least one byte arrived.
    const fn arrived(&self) -> bool {
        matches!(self, Self::Bytes)
    }
}

/// A head that is too large to even parse.
fn too_large() -> Reject {
    Reject {
        status: 431,
        reason: RejectReason::HeaderTooLarge,
        remote_addr: None,
    }
}

/// A refusal of a body, naming the client the request came from.
fn refuse_body(head: &RequestHead, status: u16, reason: RejectReason) -> Reject {
    Reject {
        status,
        reason,
        remote_addr: Some(head.remote_addr.clone()),
    }
}
