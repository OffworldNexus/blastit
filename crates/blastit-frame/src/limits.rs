//! Frame size limits and the body-splitting helpers built on them.

use crate::error::FrameError;
use crate::frame::{Frame, TypeCode};
use crate::wire::{Counter, HEADER_LEN, MIN_LENGTH, len_u32};

/// Default maximum frame size: 1 MiB, the default of `BLASTIT_FRAME_MAX`
/// (ARCHITECTURE.md §3.1).
///
/// The limit covers the *whole* frame, header included, so at most
/// `MAX_FRAME_DEFAULT - 9` payload bytes travel in one frame.
pub const MAX_FRAME_DEFAULT: usize = 1024 * 1024;

/// Largest value [`Limits::new`] accepts, because the `length` field is a `u32`.
pub const MAX_FRAME_CEILING: usize = u32::MAX as usize;

/// Smallest value [`Limits::new`] accepts: an empty header plus one payload
/// byte. A smaller limit could not carry any body, which would make the
/// body-splitting helpers meaningless.
pub const MIN_FRAME: usize = HEADER_LEN + 1;

/// Maximum frame size for one channel.
///
/// `Limits` is `Copy` and cheap to thread through a reader or a writer. The
/// default is [`MAX_FRAME_DEFAULT`]; a channel configured with
/// `BLASTIT_FRAME_MAX` builds its own with [`Limits::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    max_frame: usize,
}

impl Limits {
    /// Build limits with `max_frame` as the total frame size, header included.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::BadLimits`] when `max_frame` is smaller than
    /// [`MIN_FRAME`] or larger than [`MAX_FRAME_CEILING`].
    pub fn new(max_frame: usize) -> Result<Self, FrameError> {
        if !(MIN_FRAME..=MAX_FRAME_CEILING).contains(&max_frame) {
            return Err(FrameError::BadLimits {
                max_frame,
                min: MIN_FRAME,
                max: MAX_FRAME_CEILING,
            });
        }
        Ok(Self { max_frame })
    }

    /// Total frame size allowed on this channel, header included.
    pub const fn max_frame(self) -> usize {
        self.max_frame
    }

    /// Payload bytes that fit in one frame.
    pub const fn max_payload(self) -> usize {
        self.max_frame - HEADER_LEN
    }

    /// Accept a frame whose total size is `total` bytes.
    pub fn check_total(self, total: usize) -> Result<(), FrameError> {
        if total > self.max_frame {
            return Err(FrameError::FrameTooLarge {
                size: total,
                max: self.max_frame,
            });
        }
        Ok(())
    }

    /// Check a `length` field read off the wire, returning the total frame size.
    ///
    /// This is the check that lets a decoder reject an oversized frame from its
    /// four length bytes alone, before the payload is buffered.
    pub fn check_declared_len(self, length: u32) -> Result<usize, FrameError> {
        if length < MIN_LENGTH {
            return Err(FrameError::BadLength { length });
        }
        let total = usize::try_from(length)
            .unwrap_or(usize::MAX)
            .saturating_add(4);
        self.check_total(total)?;
        Ok(total)
    }

    /// The size `frame` would take on the wire, header included, without
    /// encoding it anywhere.
    ///
    /// A frame's header carries its payload length, so [`Limits::encode`] has
    /// to measure the payload before it can write a byte. A caller that only
    /// wants the size — the scheduler's outbound queue does, to account the
    /// bytes against a peer's cap before queueing — would otherwise have to
    /// encode the whole frame into a buffer and throw it away. This runs the
    /// same measuring pass as `encode` and its validation, so it is both
    /// cheaper and an exact predictor of what `encode` would accept.
    ///
    /// # Errors
    ///
    /// Exactly the errors [`Limits::encode`] raises before it writes: any error
    /// [`Frame`] can raise (a field out of range, or a `ctl.chan` frame with a
    /// non-zero id) and [`FrameError::FrameTooLarge`] when the frame does not
    /// fit.
    pub fn encoded_len(self, frame: &Frame, id: u32) -> Result<usize, FrameError> {
        frame.check_id(id)?;
        let mut counter = Counter(0);
        frame.write_payload(&mut counter)?;
        let total = HEADER_LEN + counter.0;
        self.check_total(total)?;
        Ok(total)
    }

    /// Encode one frame with this channel's limit.
    ///
    /// `out` is appended to, and is left untouched when the frame does not fit:
    /// a writer may keep using the buffer after an error.
    ///
    /// # Errors
    ///
    /// [`FrameError::FrameTooLarge`] when the frame does not fit, and any
    /// encoding error [`Frame`] can raise (a field out of range, or a
    /// `ctl.chan` frame with a non-zero id).
    pub fn encode(self, frame: &Frame, id: u32, out: &mut Vec<u8>) -> Result<(), FrameError> {
        // The sizing pass is `encoded_len`, so a frame this rejects is rejected
        // here too, and never in two different places.
        let total = self.encoded_len(frame, id)?;
        let payload_len = total - HEADER_LEN;

        let start = out.len();
        let length = len_u32(MIN_LENGTH as usize + payload_len, "length")?;
        out.reserve(total);
        out.extend_from_slice(&length.to_le_bytes());
        out.push(frame.code());
        out.extend_from_slice(&id.to_le_bytes());
        if let Err(err) = frame.write_payload(out) {
            // Cannot happen: the counter above ran the same checks.
            out.truncate(start);
            return Err(err);
        }
        Ok(())
    }

    /// Split `body` into chunks that each fit one body frame.
    ///
    /// An empty body yields no chunks at all: a request or response with no
    /// body is terminated by `REQ_END` / `RESP_END` without a `*_BODY` frame.
    pub fn body_chunks<'a>(self, body: &'a [u8]) -> BodyChunks<'a> {
        BodyChunks {
            remaining: body,
            chunk: self.max_payload(),
        }
    }

    /// Encode `body` as zero or more `REQ_BODY` frames with `id`.
    ///
    /// Kept separate from [`Limits::encode`] so a large body never has to be
    /// materialised as one oversized `Frame` first.
    pub fn encode_req_body(
        self,
        id: u32,
        body: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        self.write_body_frames(TypeCode::ReqBody, id, body, out)
    }

    /// Encode `body` as zero or more `RESP_BODY` frames with `id`.
    pub fn encode_resp_body(
        self,
        id: u32,
        body: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        self.write_body_frames(TypeCode::RespBody, id, body, out)
    }

    fn write_body_frames(
        self,
        ty: TypeCode,
        id: u32,
        body: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        for chunk in self.body_chunks(body) {
            let length = len_u32(MIN_LENGTH as usize + chunk.len(), "length")?;
            out.reserve(HEADER_LEN + chunk.len());
            out.extend_from_slice(&length.to_le_bytes());
            out.push(ty.code());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(chunk);
        }
        Ok(())
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame: MAX_FRAME_DEFAULT,
        }
    }
}

/// Iterator over the chunks of a body, each small enough for one body frame.
#[derive(Debug, Clone)]
pub struct BodyChunks<'a> {
    remaining: &'a [u8],
    chunk: usize,
}

impl<'a> Iterator for BodyChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let n = self.chunk.min(self.remaining.len());
        let (head, tail) = self.remaining.split_at(n);
        self.remaining = tail;
        Some(head)
    }
}
