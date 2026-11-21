//! The one writer of `view.chan`.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use blastit_frame::{Frame, FrameError, Limits};

use crate::flow::Flow;
use crate::sync::lock;

/// Anything that can go wrong while sending a frame.
///
/// None of these is fatal to the process by itself: a request thread that
/// cannot write can only give up on its request, and `DONE` is sent on a
/// best-effort basis. The I/O thread notices a dead channel on its own next
/// read.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The frame does not fit the channel's frame limit, or a field could not
    /// be encoded. A `*_BODY` frame never fails this way, because bodies are
    /// split first; a `RESP_HEAD` with a very large header block can.
    #[error(transparent)]
    Frame(#[from] FrameError),

    /// The socket write failed: the scheduler is gone or its buffer is full and
    /// the connection was reset.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The stream ran out of flow-control credit and the window was closed
    /// (a `CANCEL`, or the end of the channel) before more arrived, so the
    /// write is abandoned rather than waited for.
    #[error("the stream's flow-control window was closed")]
    WindowClosed,
}

/// Serialises frames onto `view.chan` (ARCHITECTURE.md §2.5, §3.1).
///
/// Every request thread in the process writes its response through this one
/// object, and the mutex is held for **one frame at a time** — never for a
/// whole body chunk — so no writer, and in particular not the I/O thread
/// sending a `WINDOW` or a refusal, waits behind a long write. Each hold is one
/// `write(2)` of at most one frame.
///
/// Callers release the GIL around these calls, so a slow reader on the
/// scheduler side blocks neither the interpreter nor the channel's I/O thread.
pub struct Writer {
    inner: Mutex<Inner>,
}

struct Inner {
    stream: UnixStream,
    limits: Limits,
    /// Scratch space for the frame being written.
    ///
    /// Allocated on first use and cleared before every frame, so it never holds
    /// more than one frame's bytes however large a response is.
    buf: Vec<u8>,
}

impl Writer {
    /// A writer that owns its end of `view.chan`.
    pub fn new(stream: UnixStream, limits: Limits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                stream,
                limits,
                buf: Vec::new(),
            }),
        }
    }

    /// Send exactly one frame, under `id`.
    pub fn write_frame(&self, frame: &Frame, id: u32) -> Result<(), WriteError> {
        let mut inner = lock(&self.inner);
        let Inner {
            stream,
            limits,
            buf,
        } = &mut *inner;
        let limits = *limits;
        buf.clear();
        limits.encode(frame, id, buf)?;
        stream.write_all(buf)?;
        Ok(())
    }

    /// Send `body` as `RESP_BODY` frames, spending `flow`'s response credit.
    ///
    /// Nothing is sent beyond the credit the scheduler granted: the loop takes
    /// what is available, sends exactly that much in one frame, and waits for
    /// more. A frame is never larger than the channel's payload limit, so a
    /// full frame always fits inside a fresh window.
    ///
    /// # Errors
    ///
    /// [`WriteError::WindowClosed`] when the window is closed and empty (the
    /// request was cancelled, or the channel's far end is gone): the caller
    /// gives up on the response instead of blocking for a grant that cannot
    /// come.
    pub fn write_body(&self, id: u32, body: &[u8], flow: &Flow) -> Result<(), WriteError> {
        let credit = flow.response();
        let mut sent = 0;
        while sent < body.len() {
            let want = (body.len() - sent).min(self.max_payload());
            let granted = credit.take(want);
            if granted == 0 {
                return Err(WriteError::WindowClosed);
            }
            let chunk = &body[sent..sent + granted];
            self.write_frame(&Frame::RespBody(chunk.to_vec()), id)?;
            sent += granted;
        }
        Ok(())
    }

    /// The payload size a body frame may carry, so callers can size their
    /// reads.
    pub fn max_payload(&self) -> usize {
        lock(&self.inner).limits.max_payload()
    }
}
