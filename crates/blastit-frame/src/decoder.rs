//! The streaming decoder: bytes in, frames out.

use crate::error::FrameError;
use crate::frame::Frame;
use crate::limits::Limits;
use crate::raw::RawFrame;
use crate::wire::HEADER_LEN;

/// Consumed bytes below this mark are not worth moving to the front of the
/// buffer; above it, compaction happens once the read cursor passes halfway.
const COMPACT_THRESHOLD: usize = 4096;

/// The frame header, as soon as four bytes of it are readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    /// Total frame size, `u32 length` field included.
    total: usize,
    ty: u8,
    id: u32,
}

/// A decoder that turns an arbitrarily chunked byte stream into frames.
///
/// A channel reader pushes whatever `read()` returned — one byte at a time is
/// fine, and so are twenty frames in one call — and pulls frames out until
/// [`Decoder::next_frame`] returns `None`.
///
/// The declared frame length is checked as soon as its four bytes are buffered,
/// so an oversized frame is rejected before its payload is read, let alone
/// buffered: the decoder never allocates based on a length it has not
/// validated.
///
/// After any error the decoder is *poisoned*: it drops what it had buffered and
/// every later call to [`Decoder::next_frame`] returns the same error again,
/// because the stream can no longer be resynchronised.
#[derive(Debug, Clone)]
pub struct Decoder {
    limits: Limits,
    buf: Vec<u8>,
    start: usize,
    poisoned: Option<FrameError>,
}

impl Decoder {
    /// A decoder for a channel with these limits.
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            buf: Vec::new(),
            start: 0,
            poisoned: None,
        }
    }

    /// A decoder that pre-allocates `capacity` bytes of buffer.
    ///
    /// One maximum-sized frame plus a header is a sensible starting point when
    /// the channel has a known limit.
    pub fn with_capacity(limits: Limits, capacity: usize) -> Self {
        Self {
            limits,
            buf: Vec::with_capacity(capacity),
            start: 0,
            poisoned: None,
        }
    }

    /// The limits this decoder enforces.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Feed bytes as they arrive from the channel.
    ///
    /// Pushing to a poisoned decoder does nothing: the bytes cannot be trusted
    /// either.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.poisoned.is_some() {
            return;
        }
        self.buf.extend_from_slice(bytes);
        if let Err(err) = self.peek_header() {
            self.poison(err);
        }
    }

    /// The next frame, with its payload still undecoded.
    ///
    /// Returns `Ok(None)` when the buffered bytes do not yet hold a whole
    /// frame. The raw layer does not validate the type code or the payload, so
    /// a frame can be forwarded after [`RawFrame::set_id`].
    ///
    /// # Errors
    ///
    /// The header errors, or the error the decoder was poisoned with.
    pub fn next_raw(&mut self) -> Result<Option<(u32, RawFrame)>, FrameError> {
        if let Some(err) = &self.poisoned {
            return Err(err.clone());
        }
        let header = match self.peek_header() {
            Ok(Some(header)) => header,
            Ok(None) => return Ok(None),
            Err(err) => {
                self.poison(err.clone());
                return Err(err);
            }
        };

        let start = self.start;
        let payload = self.buf[start + HEADER_LEN..start + header.total].to_vec();
        self.start += header.total;
        self.compact();
        Ok(Some((
            header.id,
            RawFrame::new(header.ty, header.id, payload),
        )))
    }

    /// The next frame, decoded into a [`Frame`].
    ///
    /// Body frames take ownership of the bytes taken off the wire, so a body is
    /// copied exactly once, on its way out of the decoder's buffer.
    ///
    /// # Errors
    ///
    /// Any [`FrameError`], including a bad payload and a frame without a stream
    /// (`ctl.chan` and `READY`) that carries a non-zero id.
    pub fn next_frame(&mut self) -> Result<Option<(u32, Frame)>, FrameError> {
        let Some((id, raw)) = self.next_raw()? else {
            return Ok(None);
        };
        let frame = match raw.into_frame() {
            Ok(frame) => frame,
            Err(err) => {
                self.poison(err.clone());
                return Err(err);
            }
        };
        if id != 0 && frame.requires_zero_id() {
            let err = FrameError::IdNotZero { id };
            self.poison(err.clone());
            return Err(err);
        }
        Ok(Some((id, frame)))
    }

    /// Bytes buffered but not yet turned into a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    /// Whether an earlier error has made this decoder unusable.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.is_some()
    }

    /// The error that poisoned this decoder, if any.
    pub fn error(&self) -> Option<&FrameError> {
        self.poisoned.as_ref()
    }

    /// Parse the header when at least four bytes are buffered.
    ///
    /// The length check happens here, which is what lets [`Decoder::push`]
    /// reject an oversized frame before its payload arrives.
    fn peek_header(&self) -> Result<Option<Header>, FrameError> {
        let available = self.buffered();
        if available < 4 {
            return Ok(None);
        }
        let start = self.start;
        let length = u32::from_le_bytes([
            self.buf[start],
            self.buf[start + 1],
            self.buf[start + 2],
            self.buf[start + 3],
        ]);
        let total = self.limits.check_declared_len(length)?;
        if available < total {
            return Ok(None);
        }
        let id = u32::from_le_bytes([
            self.buf[start + 5],
            self.buf[start + 6],
            self.buf[start + 7],
            self.buf[start + 8],
        ]);
        Ok(Some(Header {
            total,
            ty: self.buf[start + 4],
            id,
        }))
    }

    /// Remember an error and drop the buffered bytes.
    fn poison(&mut self, err: FrameError) {
        self.poisoned = Some(err);
        self.buf.clear();
        self.start = 0;
    }

    /// Move live bytes to the front once they are a minority of the buffer.
    fn compact(&mut self) {
        if self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
            return;
        }
        if self.start >= COMPACT_THRESHOLD && self.start * 2 >= self.buf.len() {
            let live = self.buf.len() - self.start;
            self.buf.copy_within(self.start.., 0);
            self.buf.truncate(live);
            self.start = 0;
        }
    }
}

// The one surviving non-§7 suite in this crate: corrupt and truncated `view.chan`
// bytes never come out of the real binary (a peer either speaks the protocol or
// the view is retired), so these decoder refusals are only reachable here.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::frame::{DoneOutcome, ReqHead, RespHead, TypeCode};

    fn limits() -> Limits {
        Limits::default()
    }

    /// Encode one frame with the default limits.
    fn wire(frame: &Frame, id: u32) -> Vec<u8> {
        let mut out = Vec::new();
        crate::encode(frame, id, &mut out).unwrap();
        out
    }

    /// A frame built by hand: the length covers the 5-byte type + id header, so
    /// this produces input the encoder would never emit.
    fn frame_bytes(ty: u8, id: u32, payload: &[u8]) -> Vec<u8> {
        let length = u32::try_from(5 + payload.len()).unwrap();
        let mut out = length.to_le_bytes().to_vec();
        out.push(ty);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Re-declare a frame's length so only `new_payload_len` payload bytes
    /// remain: a frame that looks complete but ends early.
    fn shorten_frame(frame: &[u8], new_payload_len: usize) -> Vec<u8> {
        let mut out = frame[..HEADER_LEN + new_payload_len].to_vec();
        let length = u32::try_from(5 + new_payload_len).unwrap();
        out[..4].copy_from_slice(&length.to_le_bytes());
        out
    }

    #[test]
    fn a_partial_frame_is_not_an_error() {
        let bytes = wire(&Frame::ReqEnd, 7);
        let mut decoder = Decoder::new(limits());
        for byte in &bytes[..bytes.len() - 1] {
            decoder.push(&[*byte]);
            assert_eq!(decoder.next_frame().unwrap(), None);
            assert!(!decoder.is_poisoned());
        }
        decoder.push(&[bytes[bytes.len() - 1]]);
        assert_eq!(decoder.next_frame().unwrap(), Some((7, Frame::ReqEnd)));
        assert_eq!(decoder.next_frame().unwrap(), None);
    }

    #[test]
    fn an_oversized_declared_length_is_rejected_without_buffering() {
        let limits = limits();
        let mut decoder = Decoder::with_capacity(limits, 64);
        // Four length bytes claiming 4 GiB, then the head of the payload.
        let mut input = u32::MAX.to_le_bytes().to_vec();
        input.push(TypeCode::ReqBody.code());
        input.extend_from_slice(&1u32.to_le_bytes());
        decoder.push(&input);

        // The decoder read the length, refused the frame, and kept neither the
        // bytes nor any room for the payload it will never buffer.
        assert!(decoder.is_poisoned());
        assert_eq!(decoder.buffered(), 0);
        assert!(
            decoder.buf.capacity() <= 64,
            "the decoder grew its buffer to {} bytes",
            decoder.buf.capacity()
        );
        decoder.push(&vec![0u8; 1024 * 1024]);
        assert_eq!(decoder.buffered(), 0);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::FrameTooLarge {
                size: usize::try_from(u32::MAX).unwrap() + 4,
                max: limits.max_frame(),
            }
        );
    }

    #[test]
    fn a_short_declared_length_is_rejected() {
        let mut decoder = Decoder::new(limits());
        let mut bytes = 4u32.to_le_bytes().to_vec();
        bytes.push(TypeCode::ReqEnd.code());
        decoder.push(&bytes);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::BadLength { length: 4 }
        );
    }

    #[test]
    fn a_poisoned_decoder_keeps_returning_its_error() {
        let mut decoder = Decoder::new(limits());
        let mut bad = 2u32.to_le_bytes().to_vec();
        bad.push(TypeCode::ReqEnd.code());
        decoder.push(&bad);
        let first = decoder.next_frame().unwrap_err();
        assert_eq!(first, FrameError::BadLength { length: 2 });

        // Pushing a perfectly good frame changes nothing: the stream can no
        // longer be resynchronised.
        decoder.push(&wire(&Frame::ReqEnd, 1));
        assert_eq!(decoder.buffered(), 0);
        assert_eq!(decoder.next_frame().unwrap_err(), first);
        assert_eq!(decoder.next_raw().unwrap_err(), first);
        assert_eq!(decoder.error(), Some(&first));
    }

    #[test]
    fn an_unknown_type_is_refused_typed_and_forwarded_raw() {
        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(0x7f, 3, b"payload"));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::UnknownType { ty: 0x7f }
        );

        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(0x7f, 3, b"payload"));
        assert_eq!(
            decoder.next_raw().unwrap(),
            Some((3, RawFrame::new(0x7f, 3, b"payload".to_vec())))
        );
    }

    #[test]
    fn complete_but_truncated_payloads_are_rejected() {
        let head = wire(&Frame::ReqHead(ReqHead::default()), 1);
        let resp = wire(
            &Frame::RespHead(RespHead {
                status: 200,
                reason: b"OK".to_vec(),
                headers: vec![(b"a".to_vec(), b"b".to_vec())],
            }),
            1,
        );
        let abort = wire(
            &Frame::RespAbort {
                reason: "boom".to_string(),
            },
            1,
        );
        let done = wire(
            &Frame::Done {
                outcome: DoneOutcome::Aborted,
            },
            1,
        );
        for (name, full) in [
            ("REQ_HEAD", head),
            ("RESP_HEAD", resp),
            ("RESP_ABORT", abort),
            ("DONE", done),
        ] {
            let payload_len = full.len() - 9;
            for cut in 0..payload_len {
                let mut decoder = Decoder::new(limits());
                decoder.push(&shorten_frame(&full, cut));
                match decoder.next_frame() {
                    Ok(None) => panic!("{name}: a truncated frame waited for more bytes"),
                    Ok(Some(frame)) => panic!("{name}: a truncated frame decoded to {frame:?}"),
                    Err(_) => {}
                }
            }
        }
    }

    #[test]
    fn a_mutated_stream_never_panics() {
        let limits = Limits::new(48).unwrap();
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..12 {
            let mut stream = Vec::new();
            for _ in 0..24 {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut bits = state;
                bits = (bits ^ (bits >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                bits ^= bits >> 27;
                let code = u8::try_from(bits % 0x46).unwrap();
                let len = usize::try_from(bits % 40).unwrap();
                let payload: Vec<u8> = (0..len).map(|i| (bits >> (i % 8)) as u8).collect();
                stream.extend_from_slice(&frame_bytes(code, bits as u32, &payload));
            }
            let cut = usize::try_from(state % (stream.len() as u64 + 1)).unwrap();
            let mut decoder = Decoder::new(limits);
            decoder.push(&stream[..cut]);
            while let Ok(Some(_)) = decoder.next_frame() {}
        }
    }
}
