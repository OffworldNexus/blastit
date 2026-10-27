//! Frame plumbing: peer writers with byte accounting, and channel readers.
//!
//! Every peer gets one writer task fed by an **unbounded queue with byte
//! accounting**. Nothing that runs on the scheduler's own task ever awaits a
//! peer: [`Outbound::push`] is synchronous, and the writer is the only thing
//! that can block on a socket — and only on *that* peer's socket
//! (ARCHITECTURE.md §3.1, "no shared channel ever waits for one slow
//! stream").
//!
//! The queue is unbounded so a push can always succeed immediately, but it is
//! bounded in *bytes*: each [`Outbound`] has a cap, and a push that would take
//! the pending bytes past it is refused. The scheduler decides what a refusal
//! means per peer: a view that has fallen that far behind is recycled, and
//! http overflowing its much larger cap is fatal.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use blastit_frame::{Ctl, Decoder, Frame, FrameError, HEADER_LEN, Limits};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// One framed message for a peer.
pub(crate) enum OutFrame {
    /// A frame with the id it goes out under.
    Frame {
        /// Stream or request id.
        id: u32,
        /// The frame itself.
        frame: Frame,
    },
}

/// Why a push was refused.
pub(crate) enum PushError {
    /// The peer's writer is gone: its channel closed.
    Closed,
    /// The queue already holds this many bytes, and the cap is `cap`.
    Overflow {
        /// Bytes that would have been pending.
        bytes: usize,
        /// The peer's cap.
        cap: usize,
    },
    /// The frame cannot be encoded at all, which is a bug, not backpressure.
    Frame(FrameError),
}

/// The sending half of one peer's channel.
#[derive(Clone)]
pub(crate) struct Outbound {
    tx: mpsc::UnboundedSender<OutFrame>,
    pending: Arc<AtomicUsize>,
    cap: usize,
    limits: Limits,
}

impl Outbound {
    /// A queue for one peer, with `cap` as the number of bytes it may hold.
    pub(crate) fn new(
        limits: Limits,
        cap: usize,
    ) -> (Self, mpsc::UnboundedReceiver<OutFrame>, Arc<AtomicUsize>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let pending = Arc::new(AtomicUsize::new(0));
        (
            Self {
                tx,
                pending: pending.clone(),
                cap,
                limits,
            },
            rx,
            pending,
        )
    }

    /// Queues one frame. Never awaits.
    pub(crate) fn push(&self, id: u32, frame: Frame) -> Result<(), PushError> {
        let size = match frame_size(&frame, self.limits) {
            Ok(size) => size,
            Err(error) => return Err(PushError::Frame(error)),
        };
        let before = self.pending.fetch_add(size, Ordering::AcqRel);
        if before.saturating_add(size) > self.cap {
            self.pending.fetch_sub(size, Ordering::AcqRel);
            return Err(PushError::Overflow {
                bytes: before.saturating_add(size),
                cap: self.cap,
            });
        }
        if self.tx.send(OutFrame::Frame { id, frame }).is_err() {
            self.pending.fetch_sub(size, Ordering::AcqRel);
            return Err(PushError::Closed);
        }
        Ok(())
    }

    /// Whether this queue's writer is still running.
    pub(crate) fn is_open(&self) -> bool {
        !self.tx.is_closed()
    }
}

/// The size a frame will take on the wire, header included.
///
/// Body frames are exact without measuring anything; a head or a small frame
/// goes through [`Limits::encoded_len`], which runs the encoder's own
/// measuring pass rather than encoding the frame into a throwaway buffer just
/// to read its length back.
fn frame_size(frame: &Frame, limits: Limits) -> Result<usize, FrameError> {
    match frame {
        Frame::ReqBody(body) | Frame::RespBody(body) => Ok(HEADER_LEN + body.len()),
        other => limits.encoded_len(other, 0),
    }
}

/// Subtracts `size` from the pending byte count without wrapping.
pub(crate) fn release(pending: &AtomicUsize, size: usize) {
    let mut current = pending.load(Ordering::Acquire);
    loop {
        let next = current.saturating_sub(size);
        match pending.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

/// What a `ctl.chan` message costs on the wire, the same way
/// [`Outbound::push`] computes it — `ctl_task` needs it to keep the queue's
/// accounting honest while it writes with `CtlChannel::send` instead of
/// [`write_peer`].
pub(crate) fn ctl_frame_size(message: &Ctl, limits: Limits) -> usize {
    frame_size(&Frame::Ctl(message.clone()), limits).unwrap_or_default()
}

/// Most frames one batch may carry.
///
/// Batching is what turns a burst of already-queued frames into one syscall
/// instead of one per frame, but a batch that could grow without bound would
/// let a flood build a buffer without limit and would make the frames behind it
/// wait for all of it. This and [`BATCH_BYTES`] stop the drain, whichever is
/// reached first, so the worst case is one frame larger than that cap.
const BATCH_FRAMES: usize = 64;

/// Most bytes one batch may carry; see [`BATCH_FRAMES`].
const BATCH_BYTES: usize = 256 * 1024;

/// The next frame for the batch being built, or `None` once it is full.
///
/// Everything already queued when the writer wakes rides the same buffer, which
/// is what collapses a burst into one `write_all`; the caps keep that buffer,
/// and the wait for the frames queued behind it, bounded.
fn next_batch_frame(
    rx: &mut mpsc::UnboundedReceiver<OutFrame>,
    frames: usize,
    bytes: usize,
) -> Option<(u32, Frame)> {
    if frames >= BATCH_FRAMES || bytes >= BATCH_BYTES {
        return None;
    }
    match rx.try_recv() {
        Ok(OutFrame::Frame { id, frame }) => Some((id, frame)),
        // Empty, or the queue closed: either way this batch ends here, and a
        // closed queue makes the next `recv` end the loop.
        Err(_) => None,
    }
}

/// Writes frames to one peer until its queue closes.
///
/// This is the only place a peer can block the scheduler, and it blocks only
/// the writer for that peer. One frame waits for the queue, and everything
/// already queued behind it is drained into the same buffer, so a burst costs
/// one syscall rather than one per frame.
pub(crate) async fn write_peer<W: AsyncWrite + Unpin>(
    mut write: W,
    limits: Limits,
    mut rx: mpsc::UnboundedReceiver<OutFrame>,
    pending: Arc<AtomicUsize>,
) {
    let mut wire = Vec::new();
    'batching: loop {
        let Some(OutFrame::Frame { id, frame }) = rx.recv().await else {
            break;
        };
        wire.clear();
        let mut frames = 0;
        let mut next = Some((id, frame));
        while let Some((id, frame)) = next {
            if limits.encode(&frame, id, &mut wire).is_err() {
                // A frame that cannot be encoded is a bug in blastit's own
                // code, not backpressure: the channel is done for.
                break 'batching;
            }
            frames += 1;
            next = next_batch_frame(&mut rx, frames, wire.len());
        }
        // The whole batch is out or the peer is gone; either way the bytes that
        // left are the ones to release.
        let size = wire.len();
        if write.write_all(&wire).await.is_err() {
            break;
        }
        release(&pending, size);
    }
    // Whatever is still queued is never coming back; make the accounting
    // honest so a later push does not see phantom bytes.
    pending.store(0, Ordering::Release);
    let _ = write.shutdown().await;
}

/// Reads frames until the channel closes, a frame does not decode, or the
/// scheduler stops listening.
///
/// `end` turns the reason into the channel's own message: http turns a decode
/// error into a fatal one (§3.1 frames come from blastit's own code), and a
/// view's is a process that died.
pub(crate) async fn read_frames<R, M, Map, End>(
    mut read: R,
    limits: Limits,
    tx: mpsc::Sender<M>,
    map: Map,
    end: End,
) where
    R: AsyncRead + Unpin,
    Map: Fn(u32, Frame) -> M,
    End: Fn(Option<FrameError>) -> M,
{
    let mut decoder = Decoder::with_capacity(limits, 8 * 1024);
    let mut buf = vec![0u8; 64 * 1024];
    let mut failure = None;
    loop {
        let read_bytes = match read.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        decoder.push(&buf[..read_bytes]);
        loop {
            match decoder.next_frame() {
                Ok(Some((id, frame))) => {
                    if tx.send(map(id, frame)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if failure.is_some() {
            break;
        }
    }
    let _ = tx.send(end(failure)).await;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll};

    /// A sink that counts its `poll_write` calls, so a test can prove a burst
    /// of queued frames leaves as one write rather than one each.
    #[derive(Clone, Default)]
    struct Recorder {
        written: Arc<Mutex<Vec<u8>>>,
        writes: Arc<AtomicUsize>,
    }

    impl Recorder {
        /// How many times the writer handed bytes to the sink.
        fn writes(&self) -> usize {
            self.writes.load(Ordering::SeqCst)
        }

        /// Everything the sink was handed, in order.
        fn written(&self) -> Vec<u8> {
            self.written.lock().unwrap().clone()
        }
    }

    impl AsyncWrite for Recorder {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.written.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_burst_of_queued_frames_leaves_in_one_write() {
        let limits = Limits::default();
        let (out, rx, pending) = Outbound::new(limits, 1 << 20);
        let id = 7;
        let frames = 5usize;
        for byte in 0..frames {
            let pushed = out.push(id, Frame::RespBody(vec![byte as u8; 8]));
            assert!(pushed.is_ok(), "the queue has room");
        }
        // Closing the queue lets `write_peer` finish once it has drained it.
        drop(out);

        let recorder = Recorder::default();
        write_peer(recorder.clone(), limits, rx, pending.clone()).await;

        assert_eq!(recorder.writes(), 1, "a drained burst is one syscall");
        assert_eq!(
            pending.load(Ordering::Acquire),
            0,
            "every byte was released"
        );

        // That one write carried every frame, in the order they were queued.
        let mut decoder = Decoder::new(limits);
        decoder.push(&recorder.written());
        let mut seen = 0usize;
        while let Some((frame_id, frame)) = decoder.next_frame().expect("decode") {
            assert_eq!(frame_id, id);
            assert_eq!(frame, Frame::RespBody(vec![seen as u8; 8]));
            seen += 1;
        }
        assert_eq!(seen, frames);
    }

    #[tokio::test]
    async fn a_batch_stops_at_its_frame_cap() {
        let limits = Limits::default();
        let (out, rx, pending) = Outbound::new(limits, 1 << 20);
        // One more than a batch may carry: the first write takes a full batch,
        // the rest goes out in following ones.
        let queued = BATCH_FRAMES + 1;
        for _ in 0..queued {
            let pushed = out.push(1, Frame::RespEnd);
            assert!(pushed.is_ok(), "the queue has room");
        }
        drop(out);

        let recorder = Recorder::default();
        write_peer(recorder.clone(), limits, rx, pending.clone()).await;

        assert_eq!(recorder.writes(), 2, "the tail is a batch of its own");
        assert_eq!(pending.load(Ordering::Acquire), 0);
    }
}
