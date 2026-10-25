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
        frame.check_id(id)?;
        let mut counter = Counter(0);
        frame.write_payload(&mut counter)?;
        let payload_len = counter.0;
        let total = HEADER_LEN + payload_len;
        self.check_total(total)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::Decoder;

    #[test]
    fn default_limits_are_one_mib() {
        let limits = Limits::default();
        assert_eq!(limits.max_frame(), 1024 * 1024);
        assert_eq!(limits.max_frame(), MAX_FRAME_DEFAULT);
        assert_eq!(limits.max_payload(), MAX_FRAME_DEFAULT - HEADER_LEN);
    }

    #[test]
    fn limits_reject_out_of_range_values() {
        for bad in [0, 1, MIN_FRAME - 1] {
            assert!(
                matches!(Limits::new(bad), Err(FrameError::BadLimits { .. })),
                "max_frame {bad} should be rejected"
            );
        }
        // One above the ceiling, where the platform's `usize` has room for it
        // (on a 32-bit target there is no larger value, so nothing to test).
        if let Some(too_big) = MAX_FRAME_CEILING.checked_add(1) {
            assert!(matches!(
                Limits::new(too_big),
                Err(FrameError::BadLimits { .. })
            ));
        }
        assert!(Limits::new(MIN_FRAME).is_ok());
        assert!(Limits::new(MAX_FRAME_CEILING).is_ok());
        assert_eq!(Limits::new(64).map(Limits::max_frame), Ok(64));
    }

    #[test]
    fn check_total_boundaries() {
        let limits = Limits::new(64).unwrap();
        assert!(limits.check_total(64).is_ok());
        assert_eq!(
            limits.check_total(65),
            Err(FrameError::FrameTooLarge { size: 65, max: 64 })
        );
        assert!(limits.check_total(9).is_ok());
    }

    #[test]
    fn check_declared_len_boundaries() {
        let limits = Limits::new(64).unwrap();
        assert_eq!(
            limits.check_declared_len(4),
            Err(FrameError::BadLength { length: 4 })
        );
        assert_eq!(limits.check_declared_len(5), Ok(9));
        assert_eq!(limits.check_declared_len(60), Ok(64));
        assert_eq!(
            limits.check_declared_len(61),
            Err(FrameError::FrameTooLarge { size: 65, max: 64 })
        );
        assert_eq!(
            limits.check_declared_len(u32::MAX),
            Err(FrameError::FrameTooLarge {
                size: crate::testutil::total_frame_size(u32::MAX),
                max: 64
            })
        );
    }

    #[test]
    fn body_chunks_respect_the_limit() {
        let limits = Limits::new(19).unwrap(); // 10 payload bytes per frame
        assert_eq!(limits.max_payload(), 10);

        let body: Vec<u8> = (0..35).map(|i| i as u8).collect();
        let chunks: Vec<&[u8]> = limits.body_chunks(&body).collect();
        assert_eq!(chunks.len(), 4);
        for chunk in &chunks {
            assert!(chunk.len() <= limits.max_payload());
        }
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[3].len(), 5);
        assert_eq!(chunks.concat(), body);

        assert_eq!(limits.body_chunks(&[]).count(), 0);
        let exact = vec![7u8; 20];
        let chunks: Vec<&[u8]> = limits.body_chunks(&exact).collect();
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|c| c.len() == 10));
    }

    #[test]
    fn encoded_body_frames_stay_under_the_limit() {
        let limits = Limits::new(19).unwrap();
        let body: Vec<u8> = (0..25).map(|i| i as u8).collect();
        let mut wire = Vec::new();
        limits.encode_req_body(0xabc, &body, &mut wire).unwrap();

        let mut decoder = Decoder::new(limits);
        decoder.push(&wire);
        let mut rebuilt = Vec::new();
        let mut frames = 0;
        while let Some((id, frame)) = decoder.next_frame().unwrap() {
            assert_eq!(id, 0xabc);
            match frame {
                Frame::ReqBody(chunk) => {
                    assert!(chunk.len() <= limits.max_payload());
                    rebuilt.extend_from_slice(&chunk);
                    frames += 1;
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(frames, 3);
        assert_eq!(rebuilt, body);
    }

    #[test]
    fn empty_body_produces_no_frames() {
        let limits = Limits::new(64).unwrap();
        let mut wire = Vec::new();
        limits.encode_req_body(1, &[], &mut wire).unwrap();
        limits.encode_resp_body(1, &[], &mut wire).unwrap();
        assert!(wire.is_empty());
    }

    #[test]
    fn big_body_is_split_rather_than_rejected() {
        let limits = Limits::default();
        // Twice the payload of one frame, plus a byte.
        let body = vec![0x5au8; limits.max_payload() * 2 + 1];
        let mut wire = Vec::new();
        limits.encode_resp_body(3, &body, &mut wire).unwrap();
        let total = wire.len();
        assert_eq!(total, body.len() + 3 * HEADER_LEN);
        assert!(total > limits.max_frame());
    }
}
