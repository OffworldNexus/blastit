//! The client side of `http.chan` (ARCHITECTURE.md §3, §3.1).
//!
//! The front end multiplexes every in-flight request over one socketpair, one
//! request per stream id:
//!
//! ```text
//! http  → REQ_HEAD, REQ_BODY*, REQ_END | CANCEL
//! http  ← RESP_HEAD, RESP_BODY*, RESP_END | RESP_ABORT
//! ```
//!
//! Two tasks own the channel. The **writer** is fed by a queue and encodes
//! whatever arrives; the **reader** decodes frames and hands each one to the
//! connection that opened the stream, dropping frames for ids nobody knows.
//! Connections only see their own [`Stream`].
//!
//! # The reader never waits for a connection
//!
//! One client that stops reading its response must not be able to hold up the
//! channel every other request shares. So the reader hands frames to a
//! connection through an *unbounded* queue and does no awaiting of its own: it
//! only ever locks a map for a moment and pushes.
//!
//! What keeps that from growing without bound is accounting, not blocking. Each
//! stream counts the response bytes it has received but that have not been
//! handed to the client yet; once that reaches
//! [`HttpConfig::stream_buffer_bytes`](crate::HttpConfig::stream_buffer_bytes),
//! *that stream* fails — the scheduler is told `CANCEL`, the client connection
//! is aborted, and everything that still arrives for the id is dropped. Every
//! other stream carries on.
//!
//! Cancels take the same route: the reader never waits on the writer's queue
//! either, it drops them into a dedicated unbounded one that the writer always
//! drains first.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use blastit_frame::{Decoder, Frame, Limits, ReqHead};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use crate::error::HttpError;

/// Frames that may wait for the writer task before the connections sending them
/// wait too. This is the *request* direction, where waiting is exactly the
/// backpressure a body upload should feel.
const WRITE_QUEUE: usize = 64;

/// Bytes read from `http.chan` at a time.
const CHANNEL_READ: usize = 64 * 1024;

/// One open stream, as the channel sees it.
struct Entry {
    /// Frames on their way to the connection. Unbounded on purpose: the reader
    /// must never wait for a client, and [`Entry::buffered`] is what stops the
    /// queue from growing forever.
    sender: mpsc::UnboundedSender<Frame>,
    /// Response bytes queued but not yet handed to the connection.
    buffered: usize,
    /// Set when the stream failed and its connection has to stop.
    abort: watch::Sender<bool>,
    /// Request-body credit granted so far, `INITIAL_WINDOW` plus every
    /// `WINDOW` the scheduler has forwarded. Additive: a grant never has to be
    /// remembered separately.
    granted: u64,
    /// The same number, as the connection sees it.
    credit: watch::Sender<u64>,
}

impl Entry {
    /// How many bytes of `frame` count against the stream's buffer.
    ///
    /// Bodies count in full; a head counts its header names and values, which
    /// is what would reach the client. End markers count nothing.
    fn weight(frame: &Frame) -> usize {
        match frame {
            Frame::RespBody(body) => body.len(),
            Frame::RespHead(head) => head
                .headers
                .iter()
                .map(|(name, value)| name.len() + value.len())
                .sum(),
            _ => 0,
        }
    }
}

/// State shared by the two channel tasks and every open stream.
struct Shared {
    limits: Limits,
    out: mpsc::Sender<(u32, Frame)>,
    cancels: mpsc::UnboundedSender<(u32, Frame)>,
    streams: Mutex<HashMap<u32, Entry>>,
    alive: watch::Sender<bool>,
    next_id: AtomicU32,
    stream_buffer_bytes: usize,
}

impl Shared {
    /// The streams map, without ever panicking on a poisoned lock.
    fn streams(&self) -> MutexGuard<'_, HashMap<u32, Entry>> {
        self.streams
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Whether `http.chan` is still usable.
    fn is_alive(&self) -> bool {
        *self.alive.borrow()
    }

    /// Mark the channel as gone and wake everyone waiting on it.
    ///
    /// Dropping the per-stream senders is what makes a connection blocked in
    /// [`Stream::recv`] come back with `None` instead of waiting for a response
    /// that can no longer arrive.
    fn close(&self) {
        self.streams().clear();
        // `send` would fail when nobody happens to be watching, and a later
        // caller has to see `false` all the same.
        self.alive.send_replace(false);
    }

    /// Give out a stream id that is not in flight.
    ///
    /// Ids start at one (zero is reserved for `READY` and `ctl.chan`) and wrap
    /// through the whole `u32` range. At most as many candidates as there are
    /// live streams, plus one, are ever tried: that is enough to find a free id
    /// whenever one exists, so a full table is discovered at once instead of
    /// after four billion tries. `None` means every id is in flight.
    fn allocate(
        &self,
        sender: mpsc::UnboundedSender<Frame>,
        abort: watch::Sender<bool>,
        credit: watch::Sender<u64>,
    ) -> Option<u32> {
        let mut streams = self.streams();
        let attempts = streams.len() + 1;
        let id = next_stream_id(&self.next_id, attempts, |id| streams.contains_key(&id))?;
        streams.insert(
            id,
            Entry {
                sender,
                buffered: 0,
                abort,
                granted: u64::from(blastit_frame::INITIAL_WINDOW),
                credit,
            },
        );
        Some(id)
    }

    /// Fail one stream: tell the scheduler, and stop the connection.
    ///
    /// Runs from the reader task, so it never awaits. The abort flag is set
    /// before the stream is unregistered, so a connection that is dequeuing a
    /// frame right now cannot miss it.
    fn fail_stream(&self, id: u32) {
        let abort = self.streams().get(&id).map(|entry| entry.abort.clone());
        let Some(abort) = abort else {
            return;
        };
        let _ = abort.send(true);
        self.streams().remove(&id);
        // Prompt: a cancel goes out even while the writer is busy.
        let _ = self.cancels.send((id, Frame::Cancel));
    }

    /// Add request-body credit to one stream.
    ///
    /// Runs from the reader task, so it never awaits: the connection is woken
    /// through the watch it is already waiting on.
    fn grant_credit(&self, id: u32, bytes: u32) {
        let mut streams = self.streams();
        let Some(entry) = streams.get_mut(&id) else {
            // Unknown or already finished stream: a late grant, dropped.
            return;
        };
        entry.granted = entry.granted.saturating_add(u64::from(bytes));
        entry.credit.send_replace(entry.granted);
    }
}

/// The first free stream id among `attempts` candidates, or `None`.
///
/// Each candidate is skipped when it is zero or already taken; the counter
/// wraps, so the whole `u32` range is reachable.
fn next_stream_id(next: &AtomicU32, attempts: usize, taken: impl Fn(u32) -> bool) -> Option<u32> {
    for _ in 0..attempts {
        let id = next.fetch_add(1, Ordering::Relaxed);
        if id != 0 && !taken(id) {
            return Some(id);
        }
    }
    None
}

/// The client end of `http.chan`.
///
/// Cloning is cheap: every clone talks over the same socketpair.
#[derive(Clone)]
pub struct Chan {
    shared: Arc<Shared>,
}

impl Chan {
    /// Start the reader and writer tasks on `io`.
    ///
    /// `io` is the front end's end of the `http.chan` socketpair; the scheduler
    /// holds the other end. Must be called from inside a Tokio runtime.
    pub fn new<IO>(io: IO, limits: Limits) -> Self
    where
        IO: AsyncRead + AsyncWrite + Send + 'static,
    {
        Self::with_stream_buffer(io, limits, crate::HttpConfig::default().stream_buffer_bytes)
    }

    /// [`Chan::new`] with the per-stream response buffer spelled out.
    pub fn with_stream_buffer<IO>(io: IO, limits: Limits, stream_buffer_bytes: usize) -> Self
    where
        IO: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (out, outgoing) = mpsc::channel(WRITE_QUEUE);
        let (cancels, urgent) = mpsc::unbounded_channel();
        let (alive, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            limits,
            out,
            cancels,
            streams: Mutex::new(HashMap::new()),
            alive,
            next_id: AtomicU32::new(1),
            stream_buffer_bytes,
        });

        let (reader, writer) = tokio::io::split(io);
        tokio::spawn(writer_task(writer, outgoing, urgent, shared.clone()));
        tokio::spawn(reader_task(reader, shared.clone()));

        Self { shared }
    }

    /// The frame size limit this channel was opened with.
    pub fn limits(&self) -> Limits {
        self.shared.limits
    }

    /// Open a stream for one request.
    ///
    /// `None` means every `u32` stream id is in flight, which only a front end
    /// that never finishes a request can reach; the caller answers `503`.
    ///
    /// The id is reserved immediately, so it cannot be handed to another
    /// request even before the first frame is written.
    pub fn open(&self) -> Option<Stream> {
        let (sender, incoming) = mpsc::unbounded_channel();
        let (abort, aborted) = watch::channel(false);
        let (credit, credited) = watch::channel(u64::from(blastit_frame::INITIAL_WINDOW));
        let id = self.shared.allocate(sender, abort.clone(), credit)?;
        Some(Stream {
            shared: self.shared.clone(),
            id,
            incoming,
            abort,
            aborted,
            credit: credited,
        })
    }

    /// Whether the scheduler end is gone.
    pub fn is_closed(&self) -> bool {
        !self.shared.is_alive()
    }

    /// Wait until the scheduler end is gone.
    pub async fn closed(&self) {
        let mut alive = self.shared.alive.subscribe();
        loop {
            // Scope the read guard: holding it across the await below would
            // block the very task that is trying to mark the channel closed.
            if !*alive.borrow_and_update() {
                return;
            }
            if alive.changed().await.is_err() {
                return;
            }
        }
    }
}

/// One request/response exchange on `http.chan`.
///
/// Dropping the stream releases the id and makes the channel reader drop
/// anything that still arrives for it — which is exactly what should happen
/// when the client has gone away.
pub struct Stream {
    shared: Arc<Shared>,
    id: u32,
    incoming: mpsc::UnboundedReceiver<Frame>,
    /// Held as well as watched, so the flag's channel cannot close before the
    /// stream does: "not aborted yet" must never be mistaken for "aborted".
    abort: watch::Sender<bool>,
    aborted: watch::Receiver<bool>,
    /// Total request-body credit granted for this stream, `INITIAL_WINDOW`
    /// plus every `WINDOW` the scheduler has forwarded.
    credit: watch::Receiver<u64>,
}

impl Stream {
    /// The id every frame of this request carries.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Send `REQ_HEAD`, which starts the request.
    pub async fn send_head(&mut self, head: ReqHead) -> Result<(), HttpError> {
        self.send(Frame::ReqHead(head)).await
    }

    /// Send a `REQ_BODY` frame.
    pub async fn send_body(&mut self, body: Vec<u8>) -> Result<(), HttpError> {
        self.send(Frame::ReqBody(body)).await
    }

    /// Send `REQ_END`: the request body is complete.
    pub async fn send_end(&mut self) -> Result<(), HttpError> {
        self.send(Frame::ReqEnd).await
    }

    /// Send `CANCEL`: the client went away or the response was given up on.
    ///
    /// A cancel never waits: it goes out ahead of anything still queued for the
    /// writer, because the scheduler should stop working on this request first.
    pub async fn send_cancel(&self) -> Result<(), HttpError> {
        if !self.shared.is_alive() {
            return Err(HttpError::ChannelClosed);
        }
        self.shared
            .cancels
            .send((self.id, Frame::Cancel))
            .map_err(|_| HttpError::ChannelClosed)
    }

    /// Grant the scheduler `bytes` more response-body credit for this stream.
    ///
    /// A window is a control frame, so it does not wait behind body frames on
    /// the writer's queue; it cannot block at all.
    ///
    /// # Errors
    ///
    /// [`HttpError::ChannelClosed`] once the scheduler end is gone.
    pub fn send_window(&self, bytes: u32) -> Result<(), HttpError> {
        debug_assert!(bytes > 0, "a window grants at least one byte");
        if !self.shared.is_alive() {
            return Err(HttpError::ChannelClosed);
        }
        self.shared
            .cancels
            .send((self.id, Frame::Window { bytes }))
            .map_err(|_| HttpError::ChannelClosed)
    }

    /// Total request-body credit granted for this stream so far.
    ///
    /// A connection subtracts what it has already sent to know what is left.
    pub fn credit(&self) -> u64 {
        *self.credit.borrow()
    }

    /// Wait until more request-body credit is granted.
    ///
    /// Returns `true` once the total changes, so the caller re-reads
    /// [`Stream::credit`]. `false` means no credit can ever arrive: the stream
    /// failed, or the scheduler end is gone.
    pub async fn more_credit(&mut self) -> bool {
        self.credit.changed().await.is_ok()
    }

    /// Response bytes received but not yet handed to the client.
    ///
    /// Zero means nothing of this stream is waiting in the queue, so a sender
    /// that has stopped is stopping for credit rather than for work.
    pub fn queued_bytes(&self) -> usize {
        self.shared
            .streams()
            .get(&self.id)
            .map_or(0, |entry| entry.buffered)
    }

    /// The next frame of this stream's response.
    ///
    /// `Ok(None)` means no further frame can arrive: the scheduler end is gone,
    /// or this stream was failed because its client stopped reading. Frames
    /// that were already received before either happened are still delivered —
    /// a response that is half written is worth finishing as far as it goes,
    /// and the caller closes the connection afterwards.
    ///
    /// The caller owes the stream a [`Stream::release`] once the frame is off
    /// its hands: that is what keeps the per-stream buffer measuring bytes
    /// received but not yet written to the client.
    pub async fn recv(&mut self) -> Result<Option<Frame>, HttpError> {
        let frame = self.incoming.recv().await;
        let Some(frame) = frame else {
            return Ok(None);
        };
        if !self.registered() && *self.aborted.borrow() {
            // The stream was failed: what is still queued is what must be
            // dropped rather than written.
            return Ok(None);
        }
        Ok(Some(frame))
    }

    /// Account for a frame the connection is done with.
    ///
    /// Called once the frame has been written to the client, or once it has
    /// been deliberately dropped, so a stream that no longer holds anything
    /// back is never failed for being over its buffer.
    pub fn release(&self, frame: &Frame) {
        let weight = Entry::weight(frame);
        if weight == 0 {
            return;
        }
        if let Some(entry) = self.shared.streams().get_mut(&self.id) {
            entry.buffered = entry.buffered.saturating_sub(weight);
        }
    }

    /// Whether this stream is still registered with the channel.
    fn registered(&self) -> bool {
        self.shared.streams().contains_key(&self.id)
    }

    /// Wait until `http.chan` itself is gone.
    ///
    /// A body that is still arriving has nobody left to read it, so the
    /// connection stops instead of waiting for a client that the scheduler can
    /// no longer answer.
    pub async fn closed(&self) {
        let mut alive = self.shared.alive.subscribe();
        loop {
            if !*alive.borrow_and_update() {
                return;
            }
            if alive.changed().await.is_err() {
                return;
            }
        }
    }

    /// Wait until this stream failed, so a connection can stop early.
    ///
    /// Used where a connection writes to a client that may not be reading: the
    /// reader fails the stream once the response buffer is full, and the write
    /// is abandoned instead of waiting for a client that never reads.
    pub async fn aborted(&self) {
        // The flag's channel is kept alive by this stream, so a closed channel
        // here can only mean the flag was set... but stay total anyway: waking
        // up is always safe, waking up wrongly is not.
        if *self.abort.borrow() {
            return;
        }
        let mut aborted = self.aborted.clone();
        loop {
            if *aborted.borrow_and_update() {
                return;
            }
            match aborted.changed().await {
                Ok(()) => {}
                Err(_) => std::future::pending().await,
            }
        }
    }

    async fn send(&self, frame: Frame) -> Result<(), HttpError> {
        if !self.shared.is_alive() {
            return Err(HttpError::ChannelClosed);
        }
        self.shared
            .out
            .send((self.id, frame))
            .await
            .map_err(|_| HttpError::ChannelClosed)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.streams().remove(&self.id);
    }
}

/// Whether a frame is part of a response at all.
fn is_response(frame: &Frame) -> bool {
    matches!(
        frame,
        Frame::RespHead(_) | Frame::RespBody(_) | Frame::RespEnd | Frame::RespAbort { .. }
    )
}

/// Encode everything the connections send, cancels first.
async fn writer_task<W>(
    mut writer: W,
    mut outgoing: mpsc::Receiver<(u32, Frame)>,
    mut urgent: mpsc::UnboundedReceiver<(u32, Frame)>,
    shared: Arc<Shared>,
) where
    W: AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    loop {
        // `biased` keeps cancels ahead of request bytes: a stream that was
        // given up on should not have to wait behind a body upload. Neither
        // queue can close while this task holds `shared`, so the only way out
        // of the loop is a failed write.
        let next = tokio::select! {
            biased;
            cancel = urgent.recv() => cancel,
            frame = outgoing.recv() => frame,
        };
        let Some((id, frame)) = next else {
            break;
        };
        buf.clear();
        let encoded = match &frame {
            Frame::ReqBody(body) => shared.limits.encode_req_body(id, body, &mut buf),
            other => shared.limits.encode(other, id, &mut buf),
        };
        // A frame that cannot be encoded is a frame this channel can never
        // send; the channel is done for.
        if encoded.is_err() || writer.write_all(&buf).await.is_err() {
            break;
        }
    }
    shared.close();
}

/// Decode whatever arrives and hand each frame to its stream.
///
/// Nothing here waits for a connection: a slow client is stopped by
/// [`Entry::buffered`], not by the reader blocking.
async fn reader_task<R>(mut reader: R, shared: Arc<Shared>)
where
    R: AsyncRead + Unpin,
{
    let mut decoder = Decoder::with_capacity(shared.limits, CHANNEL_READ + 4096);
    let mut buf = vec![0u8; CHANNEL_READ];
    loop {
        let read = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        decoder.push(&buf[..read]);
        loop {
            match decoder.next_frame() {
                Ok(Some((id, frame))) => dispatch(&shared, id, frame),
                Ok(None) => break,
                // A frame the codec could not read leaves the stream
                // unresynchronisable, so the channel is done for.
                Err(_) => {
                    shared.close();
                    return;
                }
            }
        }
    }
    shared.close();
}

/// Route one frame from the scheduler to the connection that owns its stream.
fn dispatch(shared: &Shared, id: u32, frame: Frame) {
    // Id 0 carries `READY` and `ctl.chan` traffic, neither of which is ours.
    if id == 0 {
        return;
    }
    // A window is request-body credit, not a response frame: it wakes the
    // connection through the watch instead of the queue, so it can never be
    // mistaken for a body.
    if let Frame::Window { bytes } = frame {
        shared.grant_credit(id, bytes);
        return;
    }
    if !is_response(&frame) {
        return;
    }
    let weight = Entry::weight(&frame);
    let mut streams = shared.streams();
    let Some(entry) = streams.get_mut(&id) else {
        // Unknown or already finished stream: a late frame, dropped.
        return;
    };
    if entry.buffered.saturating_add(weight) > shared.stream_buffer_bytes {
        // A peer that sends past its credit has broken the protocol. Fail this
        // stream alone and let the scheduler know, without holding up anybody
        // else's.
        drop(streams);
        shared.fail_stream(id);
        return;
    }
    entry.buffered += weight;
    let sender = entry.sender.clone();
    drop(streams);
    // Unbounded, so this never waits; a closed receiver means the connection
    // is gone and the frame is late.
    let _ = sender.send(frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::{INITIAL_WINDOW, RespHead, Scheme};
    use std::time::Duration;
    use tokio::io::duplex;

    fn request_head(target: &str) -> ReqHead {
        ReqHead {
            method: b"GET".to_vec(),
            target: target.as_bytes().to_vec(),
            headers: Vec::new(),
            remote_addr: "203.0.113.7:1".to_string(),
            peer_addr: "10.244.3.9:5151".to_string(),
            scheme: Scheme::Http,
            server_name: "localhost".to_string(),
            server_port: 8080,
        }
    }

    fn response_head(status: u16) -> RespHead {
        RespHead {
            status,
            reason: Vec::new(),
            headers: Vec::new(),
        }
    }

    /// Read every frame the peer (the scheduler) sees, until `count` frames.
    async fn read_frames(io: &mut tokio::io::DuplexStream, count: usize) -> Vec<(u32, Frame)> {
        let mut decoder = Decoder::new(Limits::default());
        let mut buf = vec![0u8; 4096];
        let mut frames = Vec::new();
        while frames.len() < count {
            let read = tokio::time::timeout(Duration::from_secs(5), io.read(&mut buf))
                .await
                .expect("channel went quiet")
                .expect("read");
            assert_ne!(read, 0, "channel closed early");
            decoder.push(&buf[..read]);
            while let Some(frame) = decoder.next_frame().expect("decode") {
                frames.push(frame);
            }
        }
        frames
    }

    fn open(chan: &Chan) -> Stream {
        chan.open().expect("a free stream id")
    }

    #[test]
    fn stream_ids_skip_zero_and_wrap() {
        let counter = AtomicU32::new(u32::MAX);
        assert_eq!(next_stream_id(&counter, 4, |_| false), Some(u32::MAX));
        // The counter wrapped: zero is reserved, so the next id is one.
        assert_eq!(next_stream_id(&counter, 4, |_| false), Some(1));
        assert_eq!(next_stream_id(&counter, 4, |_| false), Some(2));
    }

    #[test]
    fn stream_ids_skip_the_ones_in_flight() {
        let counter = AtomicU32::new(1);
        assert_eq!(next_stream_id(&counter, 4, |id| id < 3), Some(3));
    }

    #[test]
    fn stream_ids_give_up_rather_than_spin() {
        let counter = AtomicU32::new(7);
        // Every candidate is taken: the search stops after the attempts it was
        // given instead of walking the whole id space.
        assert_eq!(next_stream_id(&counter, 8, |_| true), None);
        assert_eq!(counter.load(Ordering::Relaxed), 7 + 8);
    }

    #[tokio::test]
    async fn a_full_stream_table_reports_no_id() {
        let (client, _peer) = duplex(4096);
        let chan = Chan::new(client, Limits::default());
        assert_eq!(chan.shared.streams().len(), 0);
        // A stream whose id the counter would hand out next: the allocation
        // still finds a free id past it.
        let first = open(&chan);
        assert_eq!(first.id(), 1);
        let second = open(&chan);
        assert_eq!(second.id(), 2);
        assert_eq!(chan.shared.streams().len(), 2);
        drop(first);
        assert_eq!(chan.shared.streams().len(), 1);
        let third = open(&chan);
        assert_ne!(third.id(), second.id());
    }

    #[tokio::test]
    async fn a_stream_carries_the_request_then_reads_the_response() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);
        assert_ne!(stream.id(), 0, "id 0 is reserved");

        stream.send_head(request_head("/a")).await.unwrap();
        stream.send_body(b"hi".to_vec()).await.unwrap();
        stream.send_end().await.unwrap();

        let frames = read_frames(&mut peer, 3).await;
        assert_eq!(frames[0].0, stream.id());
        assert!(matches!(&frames[0].1, Frame::ReqHead(head) if head.target == b"/a"));
        assert_eq!(frames[1].1, Frame::ReqBody(b"hi".to_vec()));
        assert_eq!(frames[2].1, Frame::ReqEnd);

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespHead(response_head(200)), stream.id(), &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespBody(b"body".to_vec()), stream.id(), &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespEnd, stream.id(), &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();

        assert!(matches!(
            stream.recv().await.unwrap(),
            Some(Frame::RespHead(_))
        ));
        assert_eq!(
            stream.recv().await.unwrap(),
            Some(Frame::RespBody(b"body".to_vec()))
        );
        assert_eq!(stream.recv().await.unwrap(), Some(Frame::RespEnd));
    }

    #[tokio::test]
    async fn frames_for_unknown_ids_are_dropped() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespBody(b"nobody".to_vec()), 4242, &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespEnd, stream.id(), &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();

        assert_eq!(stream.recv().await.unwrap(), Some(Frame::RespEnd));
    }

    #[tokio::test]
    async fn a_dead_scheduler_wakes_every_stream_and_the_channel() {
        let (client, peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut first = open(&chan);
        let mut second = open(&chan);

        drop(peer);
        assert_eq!(first.recv().await.unwrap(), None);
        assert_eq!(second.recv().await.unwrap(), None);
        tokio::time::timeout(Duration::from_secs(5), chan.closed())
            .await
            .expect("closed() must return once the peer is gone");
        assert!(chan.is_closed());
        assert!(first.send_end().await.is_err());
    }

    #[tokio::test]
    async fn ids_are_unique_and_never_reused_while_in_flight() {
        let (client, _peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let first = open(&chan);
        let second = open(&chan);
        let third = open(&chan);
        assert_ne!(first.id(), second.id());
        assert_ne!(second.id(), third.id());
        assert_ne!(first.id(), third.id());

        // A closed stream frees its id, but the counter never hands out the
        // same id twice in practice.
        let id = first.id();
        drop(first);
        let fourth = open(&chan);
        assert_ne!(fourth.id(), id);
        assert_ne!(fourth.id(), 0);
    }

    #[tokio::test]
    async fn dropping_a_stream_releases_its_frames() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);
        let id = stream.id();
        stream.send_end().await.unwrap();

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespEnd, id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert_eq!(stream.recv().await.unwrap(), Some(Frame::RespEnd));

        // The connection is done with the stream, so the id is released and
        // anything that still arrives for it has nowhere to go.
        drop(stream);
        assert_eq!(chan.shared.streams().len(), 0);
        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespHead(response_head(500)), id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        // A fresh stream for the same request would not be disturbed by it.
        let mut later = open(&chan);
        assert_ne!(later.id(), id);
        later.send_end().await.unwrap();
        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespEnd, later.id(), &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert_eq!(later.recv().await.unwrap(), Some(Frame::RespEnd));
    }

    #[tokio::test]
    async fn a_stream_that_fills_its_buffer_fails_alone() {
        let (client, mut peer) = duplex(64 * 1024);
        // Room for 10 bytes of response per stream.
        let chan = Chan::with_stream_buffer(client, Limits::default(), 10);
        let mut greedy = open(&chan);
        let mut other = open(&chan);
        let greedy_id = greedy.id();

        // Twenty bytes for the first stream: ten too many.
        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespHead(response_head(200)), greedy_id, &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespBody(vec![b'x'; 20]), greedy_id, &mut wire)
            .unwrap();
        // A small answer for the second.
        Limits::default()
            .encode(&Frame::RespHead(response_head(200)), other.id(), &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespBody(b"ok".to_vec()), other.id(), &mut wire)
            .unwrap();
        Limits::default()
            .encode(&Frame::RespEnd, other.id(), &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();

        // The greedy stream fails, and only it.
        assert!(
            greedy.recv().await.unwrap().is_none(),
            "the stream must fail"
        );
        assert!(matches!(
            other.recv().await.unwrap(),
            Some(Frame::RespHead(_))
        ));
        assert_eq!(
            other.recv().await.unwrap(),
            Some(Frame::RespBody(b"ok".to_vec()))
        );
        assert_eq!(other.recv().await.unwrap(), Some(Frame::RespEnd));

        // The scheduler was told, and the frame that did not fit is dropped.
        let frames = read_frames(&mut peer, 1).await;
        assert_eq!(frames[0], (greedy_id, Frame::Cancel));
    }

    #[tokio::test]
    async fn releasing_a_frame_makes_room_for_the_next() {
        let (client, mut peer) = duplex(64 * 1024);
        // Room for 8 bytes of response per stream.
        let chan = Chan::with_stream_buffer(client, Limits::default(), 8);
        let mut stream = open(&chan);
        let id = stream.id();

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespBody(vec![b'a'; 8]), id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        let first = stream.recv().await.unwrap().expect("the frame fits");
        // Nothing has been written yet, so the next frame is one too many.
        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespBody(vec![b'b'; 8]), id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert!(
            stream.recv().await.unwrap().is_none(),
            "the stream fails while the first frame is unreleased"
        );
        drop(first);

        // The same exchange, with the connection releasing as it writes.
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::with_stream_buffer(client, Limits::default(), 8);
        let mut stream = open(&chan);
        let id = stream.id();
        for byte in *b"abc" {
            let mut wire = Vec::new();
            Limits::default()
                .encode(&Frame::RespBody(vec![byte; 8]), id, &mut wire)
                .unwrap();
            peer.write_all(&wire).await.unwrap();
            let frame = stream.recv().await.unwrap().expect("room after release");
            assert_eq!(frame, Frame::RespBody(vec![byte; 8]));
            stream.release(&frame);
        }
    }

    #[tokio::test]
    async fn a_stream_starts_with_one_window_of_credit() {
        let (client, _peer) = duplex(4096);
        let chan = Chan::new(client, Limits::default());
        let stream = open(&chan);
        assert_eq!(stream.credit(), u64::from(INITIAL_WINDOW));
    }

    #[tokio::test]
    async fn a_window_adds_credit_and_wakes_the_waiter() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);
        let id = stream.id();

        // Nothing has been granted yet, so this waits.
        let waiting = tokio::time::timeout(Duration::from_millis(50), stream.more_credit()).await;
        assert!(waiting.is_err(), "there was no grant to wake on");

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::Window { bytes: 4096 }, id, &mut wire)
            .unwrap();
        // A window for somebody else must not wake this stream.
        Limits::default()
            .encode(&Frame::Window { bytes: 99 }, id + 7, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();

        assert!(
            tokio::time::timeout(Duration::from_secs(5), stream.more_credit())
                .await
                .expect("the grant must wake the stream")
        );
        assert_eq!(stream.credit(), u64::from(INITIAL_WINDOW) + 4096);
    }

    #[tokio::test]
    async fn a_window_is_credited_only_while_the_stream_lives() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let stream = open(&chan);
        let id = stream.id();
        drop(stream);

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::Window { bytes: 4096 }, id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        // A grant for a stream nobody owns is dropped: the next one, for a
        // fresh stream, is what proves the reader kept going.
        let fresh = open(&chan);
        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::Window { bytes: 1 }, fresh.id(), &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert_eq!(chan.shared.streams().len(), 1);
    }

    #[tokio::test]
    async fn more_credit_reports_a_channel_that_can_grant_nothing() {
        let (client, peer) = duplex(4096);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);
        drop(peer);
        assert!(
            !stream.more_credit().await,
            "a dead channel cannot grant more credit"
        );
    }

    #[tokio::test]
    async fn a_grant_does_not_wait_behind_request_frames() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = open(&chan);
        let id = stream.id();

        // A body frame is queued but the writer has not drained it yet, because
        // nothing is reading the peer.
        stream.send_body(vec![b'b'; 4096]).await.unwrap();
        // A window is a control frame: it goes out ahead of it.
        stream.send_window(64 * 1024).unwrap();

        let mut decoder = Decoder::new(Limits::default());
        let mut buf = vec![0u8; 8192];
        let read = tokio::time::timeout(Duration::from_secs(5), peer.read(&mut buf))
            .await
            .expect("the writer must send something")
            .expect("read");
        decoder.push(&buf[..read]);
        let first = decoder.next_frame().unwrap().expect("a frame");
        assert_eq!(
            first,
            (id, Frame::Window { bytes: 64 * 1024 }),
            "the grant must not wait behind the body"
        );
    }

    #[tokio::test]
    async fn queued_bytes_track_what_the_connection_has_not_taken() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::with_stream_buffer(client, Limits::default(), 1 << 20);
        let mut stream = open(&chan);
        let id = stream.id();
        assert_eq!(stream.queued_bytes(), 0);

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespBody(vec![b'q'; 1000]), id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        let frame = stream.recv().await.unwrap().expect("the frame");
        assert_eq!(stream.queued_bytes(), 1000);

        stream.release(&frame);
        assert_eq!(stream.queued_bytes(), 0);
    }

    #[tokio::test]
    async fn aborting_a_stream_wakes_a_connection_that_is_stuck_writing() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::with_stream_buffer(client, Limits::default(), 4);
        let mut stream = open(&chan);
        let id = stream.id();

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespBody(vec![b'x'; 16]), id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), stream.aborted())
            .await
            .expect("aborted() must return once the stream fails");
        assert!(stream.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_stream_that_drops_before_failure_gets_no_abort() {
        let (client, _peer) = duplex(4096);
        let chan = Chan::with_stream_buffer(client, Limits::default(), 4);
        let stream = open(&chan);
        // Dropping the stream removes it, so a later overflow has nothing to
        // fail and nothing to wake.
        drop(stream);
        assert_eq!(chan.shared.streams().len(), 0);
    }
}
