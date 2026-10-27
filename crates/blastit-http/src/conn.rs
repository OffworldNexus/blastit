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
use blastit_frame::{Frame, INITIAL_WINDOW, ReqHead};
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

/// Response-body credit, in bytes, at or below which the connection grants the
/// view what it has written.
///
/// A `WINDOW` is a whole frame in each direction and a wakeup at each end, so
/// the connection lets a view's credit run down toward this mark before topping
/// it back up, rather than returning every byte the moment it is written: a
/// response whose body stays far below it never costs a `WINDOW` at all. It is
/// the same 64 KiB batch as before — [`INITIAL_WINDOW`] is measured in orders
/// above it — which leaves the view ~960 KiB of batching between grants and,
/// at the mark itself, 64 KiB of credit: far more than the round trip over the
/// local socketpair that separates the two ends, so a grant that is still in
/// flight cannot leave the view waiting for it.
const WINDOW_LOW_WATER: u64 = 64 * 1024;

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
    /// The body took longer than `body_timeout`; `CANCEL` is due.
    TimedOut,
    /// The scheduler channel failed while the body was still arriving.
    ChannelClosed,
}

/// The result of waiting for more of a request body.
enum BodyFill {
    /// At least one byte arrived.
    Bytes,
    /// The client went quiet for `keepalive_timeout`.
    Idle,
    /// `body_timeout` ran out for the request as a whole.
    Expired,
    /// The client is gone, or the read failed.
    Closed,
    /// The scheduler channel failed while the body was still arriving.
    ChannelClosed,
}

/// What ended a wait for more request-body credit.
enum CreditWait {
    /// A `WINDOW` arrived; there may be credit to spend now.
    Granted,
    /// `body_timeout` ran out while waiting.
    Expired,
    /// No credit can arrive any more: the stream failed, or the channel did.
    ChannelClosed,
}

/// The request-body credit of the stream in flight.
///
/// Bodies start with `INITIAL_WINDOW` bytes and may not be sent faster than the
/// scheduler grants more: a slow app slows its own client down, and nothing
/// else.
struct Credit {
    /// Total credit granted so far, as the channel last reported it.
    granted: u64,
    /// Body bytes already sent on this stream.
    sent: u64,
}

impl Credit {
    /// The credit a fresh stream starts with.
    fn new(stream: &Stream) -> Self {
        Self {
            granted: stream.credit(),
            sent: 0,
        }
    }

    /// Body bytes that may still be sent.
    fn available(&self) -> u64 {
        self.granted.saturating_sub(self.sent)
    }

    /// Remember that `bytes` were sent.
    fn spend(&mut self, bytes: u64) {
        self.sent = self.sent.saturating_add(bytes);
    }

    /// Take the channel's latest grant into account.
    fn refresh(&mut self, stream: &Stream) {
        self.granted = stream.credit();
    }
}

/// The result of waiting for more of a request head.
enum Filled {
    /// At least one byte arrived.
    Bytes,
    /// Nothing arrived before the deadline.
    Idle,
    /// The client is gone, or the read failed.
    Closed,
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
    /// Per-connection scratch, reused across requests so that neither a
    /// decoded request body nor an encoded response head has to allocate for
    /// itself. It belongs to the connection and outlives any single request;
    /// each user clears it before writing and is done with it before the next
    /// user touches it. The chunked-body decoder ([`Conn::stream_chunked_body`])
    /// and the response-head writer ([`Conn::relay`]) never run at the same
    /// time — a body is fully streamed before its response is relayed — so one
    /// buffer serves both.
    scratch: Vec<u8>,
    /// De-chunked body bytes that are decoded but not yet sent, because the
    /// stream has no credit left for them.
    pending: Vec<u8>,
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
            pending: Vec::new(),
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
            match self.fill_head(deadline).await {
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
            match self.fill_head(deadline).await {
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
        let Some(mut stream) = self.chan.open() else {
            // Every stream id is in flight: nothing can be forwarded until one
            // finishes, which a front end that is serving at all will do soon.
            self.refuse(
                &head.remote_addr.clone(),
                503,
                RejectReason::Other("stream_ids_exhausted".into()),
            )
            .await;
            return Ok(false);
        };
        let request = ReqHead {
            method: head.method.clone(),
            target: head.target.clone(),
            headers: head.headers.clone(),
            remote_addr: head.remote_addr.clone(),
            scheme: head.scheme,
            server_name: head.server_name.clone(),
            server_port: self.cfg.server_port,
            peer_addr: head.peer_addr.clone(),
        };
        // A head that does not fit one frame cannot be forwarded at all.
        // Measuring it here, rather than letting the writer task discover it,
        // keeps one oversized request from looking like a dead channel — and
        // measuring it with `encoded_len` rather than by encoding it into the
        // scratch buffer means the head is never copied just to be sized.
        let frame = Frame::ReqHead(request);
        if self
            .cfg
            .frame_limits
            .encoded_len(&frame, stream.id())
            .is_err()
        {
            self.refuse(&head.remote_addr.clone(), 431, RejectReason::HeaderTooLarge)
                .await;
            return Ok(false);
        }

        match stream.send_frame(frame).await {
            Ok(()) => {}
            // The scheduler is gone: answer the one request that is in hand,
            // and let the accept loop report the channel as fatal.
            Err(_) => {
                let _ = self.io.write_all(&response::simple(502)).await;
                return Ok(false);
            }
        }

        // The body has to be in by `body_timeout` after the head, whatever
        // else happens, because nothing will answer a request whose upload is
        // still trickling when the client gives up.
        let body_deadline = Instant::now() + self.cfg.body_timeout;

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

        let mut credit = Credit::new(&stream);
        match self
            .stream_body(&mut stream, &head, body_deadline, &mut credit)
            .await
        {
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
            // The body took too long: the request is cancelled, and since
            // nothing of a response has been written, the client gets a 408.
            BodyOutcome::TimedOut => {
                let _ = stream.send_cancel().await;
                self.refuse(
                    &head.remote_addr.clone(),
                    408,
                    RejectReason::Other("body_timeout".into()),
                )
                .await;
                return Ok(false);
            }
            // The scheduler went away while the body was still arriving.
            BodyOutcome::ChannelClosed => {
                let _ = self.io.write_all(&response::simple(502)).await;
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

    /// Stream the request body to the scheduler, framed as it arrived, and no
    /// faster than its credit allows.
    async fn stream_body(
        &mut self,
        stream: &mut Stream,
        head: &RequestHead,
        deadline: Instant,
        credit: &mut Credit,
    ) -> BodyOutcome {
        match head.body {
            BodyFraming::None | BodyFraming::ContentLength(0) => match stream.send_end().await {
                Ok(()) => BodyOutcome::Sent,
                Err(_) => BodyOutcome::ChannelClosed,
            },
            BodyFraming::ContentLength(len) => {
                self.stream_fixed_body(stream, len, deadline, credit).await
            }
            BodyFraming::Chunked => {
                self.stream_chunked_body(stream, head, deadline, credit)
                    .await
            }
        }
    }

    /// Stream a `Content-Length` body, one `REQ_BODY` frame per read.
    async fn stream_fixed_body(
        &mut self,
        stream: &mut Stream,
        len: u64,
        deadline: Instant,
        credit: &mut Credit,
    ) -> BodyOutcome {
        let mut remaining = len;
        while remaining > 0 {
            // Out of credit: stop reading this client's socket until the app
            // has consumed some of what it already has.
            if credit.available() == 0 {
                match self.wait_for_credit(stream, deadline, credit).await {
                    CreditWait::Granted => continue,
                    CreditWait::Expired => return BodyOutcome::TimedOut,
                    CreditWait::ChannelClosed => return BodyOutcome::ChannelClosed,
                }
            }
            if self.pos == self.buf.len() {
                match self.fill_body(stream, deadline).await {
                    BodyFill::Bytes => {}
                    BodyFill::Idle | BodyFill::Closed => return BodyOutcome::ClientGone,
                    BodyFill::Expired => return BodyOutcome::TimedOut,
                    BodyFill::ChannelClosed => return BodyOutcome::ChannelClosed,
                }
            }
            let buffered = (self.buf.len() - self.pos) as u64;
            let take = remaining.min(credit.available()).min(buffered) as usize;
            if take == 0 {
                continue;
            }
            let body = self.buf[self.pos..self.pos + take].to_vec();
            self.pos += take;
            remaining -= take as u64;
            credit.spend(take as u64);
            if stream.send_body(body).await.is_err() {
                return BodyOutcome::ChannelClosed;
            }
        }
        match stream.send_end().await {
            Ok(()) => BodyOutcome::Sent,
            Err(_) => BodyOutcome::ChannelClosed,
        }
    }

    /// De-chunk a body as it arrives and stream it on, no faster than its
    /// credit allows.
    async fn stream_chunked_body(
        &mut self,
        stream: &mut Stream,
        head: &RequestHead,
        deadline: Instant,
        credit: &mut Credit,
    ) -> BodyOutcome {
        let mut dechunker = Dechunker::new(self.cfg.max_body_bytes);
        self.scratch.clear();
        self.pending.clear();
        let mut done = false;
        loop {
            // 1. Hand over what is decoded and already paid for.
            if credit.available() > 0 && !self.pending.is_empty() {
                let take = (self.pending.len() as u64).min(credit.available()) as usize;
                let body: Vec<u8> = self.pending.drain(..take).collect();
                credit.spend(take as u64);
                if stream.send_body(body).await.is_err() {
                    return BodyOutcome::ChannelClosed;
                }
                continue;
            }
            // 2. The body is over and everything decoded has been sent.
            if done && self.pending.is_empty() {
                return match stream.send_end().await {
                    Ok(()) => BodyOutcome::Sent,
                    Err(_) => BodyOutcome::ChannelClosed,
                };
            }
            // 3. Out of credit: stop reading this client's socket until the app
            //    has consumed some of what it already has.
            if credit.available() == 0 {
                match self.wait_for_credit(stream, deadline, credit).await {
                    CreditWait::Granted => continue,
                    CreditWait::Expired => return BodyOutcome::TimedOut,
                    CreditWait::ChannelClosed => return BodyOutcome::ChannelClosed,
                }
            }
            // 4. Decode more, reading from the client only when there is
            //    nothing buffered to work on.
            let pushed = dechunker.push(&self.buf[self.pos..], &mut self.scratch);
            let (used, step) = match pushed {
                Ok((0, _)) => {
                    // Not even half a chunk line: read more and try again.
                    match self.fill_body(stream, deadline).await {
                        BodyFill::Bytes => continue,
                        BodyFill::Idle | BodyFill::Closed => return BodyOutcome::ClientGone,
                        BodyFill::Expired => return BodyOutcome::TimedOut,
                        BodyFill::ChannelClosed => return BodyOutcome::ChannelClosed,
                    }
                }
                Ok(progress) => progress,
                Err(ChunkError::TooLarge) => {
                    return BodyOutcome::Refused(refuse_body(
                        head,
                        413,
                        RejectReason::BodyTooLarge,
                    ));
                }
                Err(ChunkError::Malformed) => {
                    return BodyOutcome::Refused(refuse_body(
                        head,
                        400,
                        RejectReason::MalformedRequest,
                    ));
                }
            };
            self.pos += used;
            self.pending.append(&mut self.scratch);
            done = step == Step::Done;
        }
    }

    /// Wait for request-body credit, bounded by `body_timeout` and by the
    /// channel itself.
    ///
    /// The per-read idle bound deliberately does not apply: while waiting, the
    /// client is not supposed to be sending anything, so going quiet says
    /// nothing about whether it is still there.
    async fn wait_for_credit(
        &mut self,
        stream: &mut Stream,
        deadline: Instant,
        credit: &mut Credit,
    ) -> CreditWait {
        let chan = self.chan;
        tokio::select! {
            granted = stream.more_credit() => {
                credit.refresh(stream);
                if granted {
                    CreditWait::Granted
                } else {
                    CreditWait::ChannelClosed
                }
            }
            () = tokio::time::sleep_until(deadline) => CreditWait::Expired,
            () = chan.closed() => CreditWait::ChannelClosed,
        }
    }

    /// Relay the response frames of a request to the client.
    ///
    /// The response head is written into the connection's scratch buffer,
    /// which the request body path has finished with by now. The buffer is
    /// taken out of `self` for the call so the writes below can borrow `self`
    /// mutably, and put back afterwards so the next response on this connection
    /// reuses the same allocation; it is only ever read between the write that
    /// fills it and the client write that consumes it, so no reused byte can be
    /// mistaken for a new one.
    ///
    /// Response credit is granted lazily. A view starts each body with
    /// [`INITIAL_WINDOW`] bytes, and this is the only granter, so the credit a
    /// view still holds is `INITIAL_WINDOW` minus the body bytes taken off the
    /// stream and not yet granted back. The grant goes out only once that falls
    /// to [`WINDOW_LOW_WATER`], and it returns everything owed, restoring the
    /// view to a full window. Checking the mark *before* waiting for the next
    /// frame is what keeps a view that has spent its credit from deadlocking:
    /// such a view has nothing left to send, so no frame will ever arrive to
    /// trigger the grant it is blocked on.
    async fn relay(
        &mut self,
        stream: &mut Stream,
        request: &RequestHead,
    ) -> Result<Relay, HttpError> {
        let mut head = std::mem::take(&mut self.scratch);
        let outcome = self.relay_frames(stream, request, &mut head).await;
        self.scratch = head;
        outcome
    }

    /// [`Conn::relay`] proper: write the response, frame by frame, into `head`
    /// as its reusable response-head buffer.
    async fn relay_frames(
        &mut self,
        stream: &mut Stream,
        request: &RequestHead,
        head: &mut Vec<u8>,
    ) -> Result<Relay, HttpError> {
        let mut resp = Resp::new();
        // Body bytes taken off the stream, written or knowingly dropped, that
        // have not been granted back yet. The view's remaining credit is
        // `INITIAL_WINDOW` minus this.
        let mut owed: u64 = 0;
        loop {
            // Grant only when the view is running low, and always before
            // waiting: a small response never reaches the mark, so it never
            // costs a `WINDOW`, and a view that has run dry is topped back up
            // rather than left waiting for a frame it cannot send.
            if owed > 0 && u64::from(INITIAL_WINDOW).saturating_sub(owed) <= WINDOW_LOW_WATER {
                let bytes = owed.min(u64::from(u32::MAX)) as u32;
                let _ = stream.send_window(bytes);
                owed -= u64::from(bytes);
            }
            let Some(frame) = stream.recv().await? else {
                // No answer can arrive any more.
                return Ok(self.fail(&mut resp, 502).await);
            };
            match &frame {
                Frame::RespHead(head_frame) => {
                    if resp.head_written {
                        // A second head would be a second response.
                        return Ok(Relay::Abort);
                    }
                    let no_body =
                        request.is_head() || matches!(head_frame.status, 100..=199 | 204 | 304);
                    // The view may ask for the connection to be closed too,
                    // even though its own `Connection` header never reaches the
                    // client.
                    let may_keep_alive = request.keep_alive
                        && !crate::headers::has_token(&head_frame.headers, "connection", "close");
                    let prepared = match response::prepare(
                        head_frame,
                        no_body,
                        request.version == Version::Http11,
                        may_keep_alive,
                        head,
                    ) {
                        Ok(prepared) => prepared,
                        // Injection from the view: nothing is written yet, so
                        // the client gets an honest 502 instead.
                        Err(_) => return Ok(self.fail(&mut resp, 502).await),
                    };
                    if !self.write_client(stream, head).await {
                        return Ok(Relay::Abort);
                    }
                    resp.framing = prepared.framing;
                    resp.close = !prepared.keep_alive;
                    resp.head_written = true;
                }
                Frame::RespBody(chunk) => {
                    if !self.write_body(stream, &mut resp, chunk).await {
                        return Ok(Relay::Abort);
                    }
                    // Credit is owed for every body byte the connection has
                    // dealt with, written or knowingly dropped: the view may
                    // send that much again.
                    owed = owed.saturating_add(chunk.len() as u64);
                }
                Frame::RespEnd => {
                    if !resp.head_written {
                        return Ok(self.fail(&mut resp, 502).await);
                    }
                    match resp.framing {
                        Framing::Chunked => {
                            if !self.write_client(stream, response::LAST_CHUNK).await {
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
            // The frame is off the connection's hands, so a client that is
            // keeping up never counts against its own buffer.
            stream.release(&frame);
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
    ///
    /// Returns whether the client still has the bytes; `false` means the write
    /// failed or the stream was failed underneath it.
    async fn write_body(&mut self, stream: &Stream, resp: &mut Resp, chunk: &[u8]) -> bool {
        if !resp.head_written {
            // A body before the head is a view bug; ignore it and wait for the
            // head or an abort.
            return true;
        }
        match resp.framing {
            Framing::None => true,
            Framing::Length(len) => {
                let left = len.saturating_sub(resp.written);
                if left == 0 {
                    // Longer than advertised: truncate and close.
                    resp.close = true;
                    return true;
                }
                let take = left.min(chunk.len() as u64) as usize;
                if take < chunk.len() {
                    resp.close = true;
                }
                if !self.write_client(stream, &chunk[..take]).await {
                    return false;
                }
                resp.written += take as u64;
                true
            }
            Framing::Chunked => {
                // An empty chunk would terminate the body.
                if chunk.is_empty() {
                    return true;
                }
                let framed = response::chunk(chunk);
                if !self.write_client(stream, &framed).await {
                    return false;
                }
                resp.written += chunk.len() as u64;
                true
            }
            Framing::Close => {
                if !self.write_client(stream, chunk).await {
                    return false;
                }
                resp.written += chunk.len() as u64;
                true
            }
        }
    }

    /// Emit `http.rejected` and answer with a status line and no body.
    async fn reject(&mut self, reject: Reject) {
        // `remote_addr` is the address alone (CGI): a rejection with no
        // resolved client — a head that never parsed — still names the peer,
        // and names it without its port.
        let remote = reject
            .remote_addr
            .clone()
            .unwrap_or_else(|| blastit_frame::peer_address(&self.peer));
        self.refuse(&remote, reject.status, reject.reason).await;
    }

    /// Emit `http.rejected` for `remote`, then write the refusal.
    async fn refuse(&mut self, remote: &str, status: u16, reason: RejectReason) {
        emit_rejected(self.emit, remote, status, reason);
        let _ = self.io.write_all(&response::simple(status)).await;
    }
}

/// Publish one `http.rejected` event (§8.1).
///
/// The event carries the address, the status and a fixed label. It never
/// carries a header value, a query string or a body byte, because none of them
/// are arguments here.
pub(crate) fn emit_rejected(emit: &Emit, remote_addr: &str, status: u16, reason: RejectReason) {
    let kind = EventKind::HttpRejected(HttpRejected::new(remote_addr, status, reason));
    (emit)(Event::now(Component::Http, kind));
}

impl<IO> Conn<'_, IO>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    /// Read more bytes, giving up at `deadline`, and stop early once
    /// `http.chan` is gone: there is no point taking a request nobody can
    /// route.
    async fn fill_head(&mut self, deadline: Instant) -> Filled {
        let chan = self.chan;
        tokio::select! {
            filled = self.fill_until(deadline) => filled,
            () = chan.closed() => Filled::Closed,
        }
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
    /// Two bounds apply to a body: no byte for `keepalive_timeout` means the
    /// client is gone, and `body_timeout` caps the upload as a whole no matter
    /// how steadily it trickles in. Whichever comes first wins — and a channel
    /// that has failed cuts the wait short, because nobody would read the body
    /// anyway.
    async fn fill_body(&mut self, stream: &Stream, deadline: Instant) -> BodyFill {
        let idle_deadline = Instant::now() + self.cfg.keepalive_timeout;
        let (filled, expired) = if idle_deadline < deadline {
            (self.fill_until(idle_deadline), false)
        } else {
            (self.fill_until(deadline), true)
        };
        tokio::select! {
            filled = filled => match (filled, expired) {
                (Filled::Bytes, _) => BodyFill::Bytes,
                (Filled::Closed, _) => BodyFill::Closed,
                (Filled::Idle, false) => BodyFill::Idle,
                (Filled::Idle, true) => BodyFill::Expired,
            },
            () = stream.closed() => BodyFill::ChannelClosed,
        }
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

    /// Write bytes to the client, giving up as soon as the stream fails.
    ///
    /// The channel reader fails a stream whose client has stopped reading, so
    /// this returns `false` rather than waiting for a client that may never
    /// read again. `true` means the bytes are on their way.
    async fn write_client(&mut self, stream: &Stream, bytes: &[u8]) -> bool {
        tokio::select! {
            result = self.io.write_all(bytes) => result.is_ok(),
            () = stream.aborted() => false,
        }
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
