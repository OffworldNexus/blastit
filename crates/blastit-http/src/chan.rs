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
//! Two tasks own the channel. The **writer** is fed by an mpsc queue and
//! encodes whatever arrives; the **reader** decodes frames and hands each one
//! to the connection that opened the stream, dropping frames for ids nobody
//! knows. Connections only see their own [`Stream`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use blastit_frame::{Decoder, Frame, Limits, ReqHead};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use crate::error::HttpError;

/// Frames that may wait for one connection before the channel reader stops
/// reading. Small, because a frame can carry a mebibyte of body.
const STREAM_QUEUE: usize = 16;

/// Frames that may wait for the writer task.
const WRITE_QUEUE: usize = 64;

/// Bytes read from `http.chan` at a time.
const CHANNEL_READ: usize = 64 * 1024;

/// State shared by the two channel tasks and every open stream.
struct Shared {
    limits: Limits,
    out: mpsc::Sender<(u32, Frame)>,
    streams: Mutex<HashMap<u32, mpsc::Sender<Frame>>>,
    alive: watch::Sender<bool>,
    next_id: AtomicU32,
}

impl Shared {
    /// The streams map, without ever panicking on a poisoned lock.
    fn streams(&self) -> MutexGuard<'_, HashMap<u32, mpsc::Sender<Frame>>> {
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
    /// Ids start at one (zero is reserved) and wrap through the whole `u32`
    /// range; an id whose previous request has not finished yet is skipped
    /// rather than reused.
    fn allocate(&self, sender: &mpsc::Sender<Frame>) -> u32 {
        let mut streams = self.streams();
        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 && !streams.contains_key(&id) {
                streams.insert(id, sender.clone());
                return id;
            }
        }
    }
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
        let (out, outgoing) = mpsc::channel(WRITE_QUEUE);
        let (alive, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            limits,
            out,
            streams: Mutex::new(HashMap::new()),
            alive,
            next_id: AtomicU32::new(1),
        });

        let (reader, writer) = tokio::io::split(io);
        tokio::spawn(writer_task(writer, outgoing, shared.clone()));
        tokio::spawn(reader_task(reader, shared.clone()));

        Self { shared }
    }

    /// The frame size limit this channel was opened with.
    pub fn limits(&self) -> Limits {
        self.shared.limits
    }

    /// Open a stream for one request.
    ///
    /// The id is reserved immediately, so it cannot be handed to another
    /// request even before the first frame is written.
    pub fn open(&self) -> Stream {
        let (sender, incoming) = mpsc::channel(STREAM_QUEUE);
        let id = self.shared.allocate(&sender);
        Stream {
            shared: self.shared.clone(),
            id,
            incoming,
        }
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
    incoming: mpsc::Receiver<Frame>,
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

    /// Send `CANCEL`: the client went away or the request was refused.
    pub async fn send_cancel(&mut self) -> Result<(), HttpError> {
        self.send(Frame::Cancel).await
    }

    /// The next frame of this stream's response.
    ///
    /// `Ok(None)` means no further frame can arrive: the scheduler end is gone.
    /// The caller answers `502`.
    pub async fn recv(&mut self) -> Result<Option<Frame>, HttpError> {
        Ok(self.incoming.recv().await)
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

    /// Drop the registration of this stream, if it is still there.
    fn release(&self) {
        self.shared.streams().remove(&self.id);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.release();
    }
}

/// Whether a frame ends a response.
fn is_terminal(frame: &Frame) -> bool {
    matches!(frame, Frame::RespEnd | Frame::RespAbort { .. })
}

/// Whether a frame is part of a response at all.
fn is_response(frame: &Frame) -> bool {
    matches!(
        frame,
        Frame::RespHead(_) | Frame::RespBody(_) | Frame::RespEnd | Frame::RespAbort { .. }
    )
}

/// Encode everything the connection tasks send, in order.
async fn writer_task<W>(
    mut writer: W,
    mut outgoing: mpsc::Receiver<(u32, Frame)>,
    shared: Arc<Shared>,
) where
    W: AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    while let Some((id, frame)) = outgoing.recv().await {
        buf.clear();
        let encoded = match &frame {
            Frame::ReqBody(body) => shared.limits.encode_req_body(id, body, &mut buf),
            other => shared.limits.encode(other, id, &mut buf),
        };
        if encoded.is_err() || writer.write_all(&buf).await.is_err() {
            break;
        }
    }
    shared.close();
}

/// Decode whatever arrives and hand each frame to its stream.
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
                Ok(Some((id, frame))) => dispatch(&shared, id, frame).await,
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
async fn dispatch(shared: &Shared, id: u32, frame: Frame) {
    // Id 0 carries `READY` and `ctl.chan` traffic, neither of which is ours.
    if id == 0 || !is_response(&frame) {
        return;
    }
    let terminal = is_terminal(&frame);
    let sender = { shared.streams().get(&id).cloned() };
    let Some(sender) = sender else {
        // Unknown or already finished stream: a late frame, dropped.
        return;
    };
    // A failed send means the connection is gone; the frame is late as well.
    let _ = sender.send(frame).await;
    if terminal {
        shared.streams().remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::{RespHead, Scheme};
    use std::time::Duration;
    use tokio::io::duplex;

    fn request_head(target: &str) -> ReqHead {
        ReqHead {
            method: b"GET".to_vec(),
            target: target.as_bytes().to_vec(),
            headers: Vec::new(),
            remote_addr: "203.0.113.7:1".to_string(),
            scheme: Scheme::Http,
            server_name: "localhost".to_string(),
            server_port: 8080,
        }
    }

    /// Read every frame the peer (the scheduler) sees, until `count` frames.
    async fn read_frames(io: &mut tokio::io::DuplexStream, count: usize) -> Vec<(u32, Frame)> {
        let mut decoder = Decoder::new(Limits::default());
        let mut buf = vec![0u8; 4096];
        let mut frames = Vec::new();
        while frames.len() < count {
            let read = io.read(&mut buf).await.expect("read");
            assert_ne!(read, 0, "channel closed early");
            decoder.push(&buf[..read]);
            while let Some(frame) = decoder.next_frame().expect("decode") {
                frames.push(frame);
            }
        }
        frames
    }

    #[tokio::test]
    async fn a_stream_carries_the_request_then_reads_the_response() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = chan.open();
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
            .encode(
                &Frame::RespHead(RespHead {
                    status: 200,
                    reason: b"OK".to_vec(),
                    headers: Vec::new(),
                }),
                stream.id(),
                &mut wire,
            )
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
        let mut stream = chan.open();

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
        let mut first = chan.open();
        let mut second = chan.open();

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
        let first = chan.open();
        let second = chan.open();
        let third = chan.open();
        assert_ne!(first.id(), second.id());
        assert_ne!(second.id(), third.id());
        assert_ne!(first.id(), third.id());

        // A closed stream frees its id, but the counter never hands out the
        // same id twice in practice.
        let id = first.id();
        drop(first);
        let fourth = chan.open();
        assert_ne!(fourth.id(), id);
        assert_ne!(fourth.id(), 0);
    }

    #[tokio::test]
    async fn a_cancelled_stream_drops_its_late_response() {
        let (client, mut peer) = duplex(64 * 1024);
        let chan = Chan::new(client, Limits::default());
        let mut stream = chan.open();
        let id = stream.id();
        stream.send_end().await.unwrap();

        let mut wire = Vec::new();
        Limits::default()
            .encode(&Frame::RespEnd, id, &mut wire)
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert_eq!(stream.recv().await.unwrap(), Some(Frame::RespEnd));

        // Nothing is registered for the id any more, so a late head is dropped
        // and the stream stays finished.
        let mut wire = Vec::new();
        Limits::default()
            .encode(
                &Frame::RespHead(RespHead {
                    status: 500,
                    reason: Vec::new(),
                    headers: Vec::new(),
                }),
                id,
                &mut wire,
            )
            .unwrap();
        peer.write_all(&wire).await.unwrap();
        assert_eq!(stream.recv().await.unwrap(), None);
    }
}
