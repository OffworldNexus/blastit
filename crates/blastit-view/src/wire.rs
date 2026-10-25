//! The one writer of `view.chan`.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, MutexGuard, PoisonError};

use blastit_frame::{Frame, FrameError, Limits};

/// Anything that can go wrong while sending a frame.
///
/// Neither variant is fatal to the process by itself: a request thread that
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
}

/// Serialises frames onto `view.chan` (ARCHITECTURE.md §2.5, §3.1).
///
/// Every request thread in the process writes its response through this one
/// object, and the mutex is held for a whole frame (and for a whole body chunk,
/// across its several `RESP_BODY` frames), so frames from concurrent requests
/// never interleave. Writers release the GIL around [`Writer::write_frame`], so
/// a slow reader on the scheduler side blocks neither the interpreter nor the
/// channel's I/O thread.
pub(crate) struct Writer {
    inner: Mutex<Inner>,
}

struct Inner {
    stream: UnixStream,
    limits: Limits,
    buf: Vec<u8>,
}

impl Writer {
    /// A writer that owns its end of `view.chan`.
    pub(crate) fn new(stream: UnixStream, limits: Limits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                stream,
                limits,
                buf: Vec::with_capacity(limits.max_frame()),
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

    /// Send `body` as zero or more `RESP_BODY` frames, none larger than the
    /// channel's frame limit.
    ///
    /// An empty body sends nothing: a response ends with `RESP_END`, and §3.1
    /// only splits bodies that are actually there. The scratch buffer holds one
    /// frame at a time, so a large response does not grow the writer.
    pub(crate) fn write_resp_body(&self, id: u32, body: &[u8]) -> Result<(), WriteError> {
        let mut inner = lock(&self.inner);
        let Inner {
            stream,
            limits,
            buf,
        } = &mut *inner;
        let limits = *limits;
        for chunk in limits.body_chunks(body) {
            buf.clear();
            limits.encode(&Frame::RespBody(chunk.to_vec()), id, buf)?;
            stream.write_all(buf)?;
        }
        Ok(())
    }
}

/// Take a lock, treating a poisoned mutex as usable.
///
/// Nothing under these mutexes can panic while it is held — a frame is encoded
/// into a `Vec` and written out — so poisoning would only come from a panic in
/// unrelated code, and `serve` is better off continuing than unwinding into a
/// process that has already lost the GIL.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    fn a_body_is_split_into_frames_that_fit() {
        let limits = Limits::new(64).expect("limits");
        let (writer, peer) = writer_pair(limits);
        let body: Vec<u8> = (0..200u32).map(|i| (i % 251) as u8).collect();
        writer.write_resp_body(3, &body).expect("body");

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
    fn an_empty_body_sends_nothing() {
        let limits = Limits::default();
        let (writer, peer) = writer_pair(limits);
        writer.write_resp_body(1, &[]).expect("empty body");
        assert_eq!(drain(&peer, limits), Vec::new());
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
