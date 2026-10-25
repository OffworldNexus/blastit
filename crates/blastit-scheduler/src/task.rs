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
/// Body frames are exact without encoding anything; a head or a small frame is
/// measured by encoding it into a scratch buffer, which is cheap because those
/// frames are small and rare.
fn frame_size(frame: &Frame, limits: Limits) -> Result<usize, FrameError> {
    match frame {
        Frame::ReqBody(body) | Frame::RespBody(body) => Ok(HEADER_LEN + body.len()),
        other => {
            let mut scratch = Vec::new();
            limits.encode(other, 0, &mut scratch)?;
            Ok(scratch.len())
        }
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

/// Writes frames to one peer until its queue closes.
///
/// This is the only place a peer can block the scheduler, and it blocks only
/// the writer for that peer.
pub(crate) async fn write_peer<W: AsyncWrite + Unpin>(
    mut write: W,
    limits: Limits,
    mut rx: mpsc::UnboundedReceiver<OutFrame>,
    pending: Arc<AtomicUsize>,
) {
    let mut wire = Vec::new();
    while let Some(OutFrame::Frame { id, frame }) = rx.recv().await {
        wire.clear();
        if limits.encode(&frame, id, &mut wire).is_err() {
            break;
        }
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
