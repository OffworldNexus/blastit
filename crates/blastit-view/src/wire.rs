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
pub(crate) enum WriteError {
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
pub(crate) struct Writer {
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
    pub(crate) fn new(stream: UnixStream, limits: Limits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                stream,
                limits,
                buf: Vec::new(),
            }),
        }
    }

    /// Send exactly one frame, under `id`.
    pub(crate) fn write_frame(&self, frame: &Frame, id: u32) -> Result<(), WriteError> {
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
    pub(crate) fn write_body(&self, id: u32, body: &[u8], flow: &Flow) -> Result<(), WriteError> {
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
    pub(crate) fn max_payload(&self) -> usize {
        lock(&self.inner).limits.max_payload()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::{Decoder, DoneOutcome, RespHead};
    use std::io::{ErrorKind, Read};
    use std::sync::Arc;

    fn writer_pair(limits: Limits) -> (Writer, Arc<UnixStream>) {
        let (ours, theirs) = UnixStream::pair().expect("socketpair");
        (Writer::new(ours, limits), Arc::new(theirs))
    }

    /// Read whatever the writer has already put on the channel.
    ///
    /// The writer runs on this thread, so everything it wrote is in the peer's
    /// buffer before this is called; a non-blocking read drains it without
    /// risking a test that hangs.
    fn drain(peer: &UnixStream, limits: Limits) -> Vec<(u32, Frame)> {
        let mut reader = peer.try_clone().expect("clone");
        reader.set_nonblocking(true).expect("non-blocking");
        let mut decoder = Decoder::new(limits);
        let mut buf = vec![0u8; 1 << 16];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => decoder.push(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => panic!("read failed: {err}"),
            }
        }
        let mut frames = Vec::new();
        while let Some(frame) = decoder.next_frame().expect("decode") {
            frames.push(frame);
        }
        assert_eq!(decoder.buffered(), 0, "a frame was cut in half");
        frames
    }

    #[test]
    fn frames_arrive_in_order_and_intact() {
        let limits = Limits::default();
        let (writer, peer) = writer_pair(limits);
        writer.write_frame(&Frame::Ready, 0).expect("ready frame");
        writer
            .write_frame(
                &Frame::RespHead(RespHead {
                    status: 204,
                    reason: b"No Content".to_vec(),
                    headers: vec![(b"X-A".to_vec(), b"b".to_vec())],
                }),
                7,
            )
            .expect("head");
        writer.write_frame(&Frame::RespEnd, 7).expect("end");
        writer
            .write_frame(
                &Frame::Done {
                    outcome: DoneOutcome::Completed,
                },
                7,
            )
            .expect("done");

        let frames = drain(&peer, limits);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0], (0, Frame::Ready));
        assert_eq!(frames[3].0, 7);
        assert_eq!(
            frames[3].1,
            Frame::Done {
                outcome: DoneOutcome::Completed
            }
        );
    }

    #[test]
    fn a_body_is_split_into_frames_that_fit_inside_the_window() {
        let limits = Limits::new(64).expect("limits");
        let (writer, peer) = writer_pair(limits);
        let flow = Flow::new();
        let body: Vec<u8> = (0..200u32).map(|i| (i % 251) as u8).collect();
        writer.write_body(3, &body, &flow).expect("body");

        let frames = drain(&peer, limits);
        assert!(frames.len() > 1);
        let mut rebuilt = Vec::new();
        for (id, frame) in frames {
            assert_eq!(id, 3);
            match frame {
                Frame::RespBody(chunk) => rebuilt.extend_from_slice(&chunk),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(rebuilt, body);
    }

    #[test]
    fn an_empty_body_sends_nothing_and_spends_nothing() {
        let limits = Limits::default();
        let (writer, peer) = writer_pair(limits);
        let flow = Flow::new();
        writer.write_body(1, &[], &flow).expect("empty body");
        assert_eq!(drain(&peer, limits), Vec::new());
        assert_eq!(
            flow.response().available(),
            u64::from(blastit_frame::INITIAL_WINDOW),
            "an empty body costs no credit"
        );
    }

    #[test]
    fn a_body_beyond_the_window_stops_at_the_credit_it_has() {
        let limits = Limits::default();
        let (writer, peer) = writer_pair(limits);
        let flow = Flow::new();
        // Spend all but ten bytes of the window, then close it: the write may
        // use the ten it has, and must not wait for the rest.
        let window = u64::from(blastit_frame::INITIAL_WINDOW);
        assert_eq!(
            flow.response().take(window as usize - 10),
            window as usize - 10
        );
        flow.response().close();

        let body = vec![0x5au8; 100];
        let err = writer.write_body(1, &body, &flow).expect_err("no credit");
        assert!(matches!(err, WriteError::WindowClosed));

        let frames = drain(&peer, limits);
        let sent: usize = frames
            .iter()
            .map(|(_, frame)| match frame {
                Frame::RespBody(chunk) => chunk.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(sent, 10, "exactly the credit that was left");
    }

    #[test]
    fn an_oversized_frame_is_a_write_error() {
        let limits = Limits::new(32).expect("limits");
        let (writer, _peer) = writer_pair(limits);
        let err = writer
            .write_frame(
                &Frame::RespHead(RespHead {
                    status: 200,
                    reason: Vec::new(),
                    headers: vec![(b"X-Long".to_vec(), vec![b'v'; 64])],
                }),
                1,
            )
            .expect_err("must not fit");
        assert!(matches!(err, WriteError::Frame(_)));
        // The writer is still usable afterwards.
        assert!(writer.write_frame(&Frame::RespEnd, 1).is_ok());
    }

    #[test]
    fn a_dead_peer_is_an_io_error_and_not_a_panic() {
        let limits = Limits::default();
        let (writer, peer) = writer_pair(limits);
        drop(peer);
        // A first write may or may not fail depending on the socket; loop until
        // it does, so the test does not depend on buffer sizes.
        let mut failed = false;
        for _ in 0..1024 {
            match writer.write_frame(&Frame::Ready, 0) {
                Ok(()) => {}
                Err(err) => {
                    failed = matches!(err, WriteError::Io(_));
                    break;
                }
            }
        }
        assert!(failed, "writing to a closed socket never failed");
    }
}
