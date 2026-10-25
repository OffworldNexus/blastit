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
    /// # Errors
    ///
    /// Any [`FrameError`], including a bad payload and a `ctl.chan` frame with
    /// a non-zero id.
    pub fn next_frame(&mut self) -> Result<Option<(u32, Frame)>, FrameError> {
        let Some((id, raw)) = self.next_raw()? else {
            return Ok(None);
        };
        let frame = match raw.decode() {
            Ok(frame) => frame,
            Err(err) => {
                self.poison(err.clone());
                return Err(err);
            }
        };
        if id != 0 && frame.as_ctl().is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctl::{Ctl, ExitStatus};
    use crate::frame::{DoneOutcome, ReqHead, RespHead, Scheme, TypeCode};
    use crate::limits::Limits;
    use crate::raw::RawFrame;
    use crate::testutil::{Rng, all_codes, client_exchange, frame_bytes, shorten_frame};

    fn limits() -> Limits {
        Limits::default()
    }

    /// Encode one frame with the default limits.
    fn wire(frame: &Frame, id: u32) -> Vec<u8> {
        let mut out = Vec::new();
        crate::encode(frame, id, &mut out).unwrap();
        out
    }

    #[test]
    fn partial_frame_is_not_an_error() {
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
    fn byte_by_byte_matches_one_shot() {
        let (stream, expected) = client_exchange();
        let mut one_shot = Decoder::new(limits());
        one_shot.push(&stream);
        let mut seen = Vec::new();
        while let Some((id, frame)) = one_shot.next_frame().unwrap() {
            seen.push((id, frame));
        }
        assert_eq!(seen, expected);

        let mut byte_wise = Decoder::new(limits());
        let mut seen = Vec::new();
        for byte in &stream {
            byte_wise.push(&[*byte]);
            while let Some((id, frame)) = byte_wise.next_frame().unwrap() {
                seen.push((id, frame));
            }
        }
        assert_eq!(seen, expected);
        assert_eq!(byte_wise.buffered(), 0);
    }

    #[test]
    fn arbitrary_chunk_sizes_match_one_shot() {
        let (stream, expected) = client_exchange();
        for seed in [1u64, 2, 3, 5, 8, 13, 21, 34, 55, 89] {
            let mut rng = Rng::new(seed);
            let mut decoder = Decoder::new(limits());
            let mut seen = Vec::new();
            let mut offset = 0;
            while offset < stream.len() {
                let chunk = (rng.below(9) + 1) as usize;
                let end = (offset + chunk).min(stream.len());
                decoder.push(&stream[offset..end]);
                offset = end;
                while let Some(frame) = decoder.next_frame().unwrap() {
                    seen.push(frame);
                }
            }
            assert_eq!(seen, expected, "seed {seed}");
        }
    }

    #[test]
    fn several_frames_in_one_push() {
        let mut stream = Vec::new();
        for id in 0..5u32 {
            stream.extend_from_slice(&wire(&Frame::ReqEnd, id));
        }
        let mut decoder = Decoder::new(limits());
        decoder.push(&stream);
        for id in 0..5u32 {
            assert_eq!(decoder.next_frame().unwrap(), Some((id, Frame::ReqEnd)));
        }
        assert_eq!(decoder.next_frame().unwrap(), None);
        assert_eq!(decoder.buffered(), 0);
    }

    #[test]
    fn oversized_declared_length_is_rejected_without_buffering() {
        let limits = limits();
        let mut decoder = Decoder::with_capacity(limits, 64);
        // Four length bytes claiming 4 GiB, then a megabyte of payload.
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

        // Further payload bytes are not buffered either.
        decoder.push(&vec![0u8; 1024 * 1024]);
        assert_eq!(decoder.buffered(), 0);
        assert!(decoder.buf.capacity() <= 64);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::FrameTooLarge {
                size: u32::MAX as usize + 4,
                max: limits.max_frame(),
            }
        );
    }

    #[test]
    fn oversized_declared_length_is_rejected_with_only_four_bytes() {
        let mut decoder = Decoder::new(limits());
        decoder.push(&(u32::MAX - 1).to_le_bytes());
        assert_eq!(decoder.buffered(), 0);
        assert!(decoder.is_poisoned());
        assert_eq!(
            decoder.next_raw().unwrap_err(),
            FrameError::FrameTooLarge {
                size: (u32::MAX - 1) as usize + 4,
                max: limits().max_frame(),
            }
        );
    }

    #[test]
    fn short_declared_length_is_rejected() {
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
    fn unknown_type_code_is_rejected_by_the_typed_layer() {
        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(0x7f, 1, &[]));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::UnknownType { ty: 0x7f }
        );
    }

    #[test]
    fn raw_layer_forwards_unknown_types_and_rewrites_ids() {
        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(0x7f, 3, b"payload"));
        let (id, mut raw) = decoder.next_raw().unwrap().unwrap();
        assert_eq!(id, 3);
        assert_eq!(raw, RawFrame::new(0x7f, 3, b"payload".to_vec()));
        raw.set_id(99);
        let mut out = Vec::new();
        raw.encode(&limits(), &mut out).unwrap();
        let mut next = Decoder::new(limits());
        next.push(&out);
        assert_eq!(next.next_raw().unwrap(), Some((99, raw.clone())));
        assert!(matches!(raw.decode(), Err(FrameError::UnknownType { .. })));
        assert!(!raw.is_ctl());
    }

    #[test]
    fn poisoned_decoder_keeps_returning_the_error() {
        let mut decoder = Decoder::new(limits());
        let mut bad = 2u32.to_le_bytes().to_vec();
        bad.push(TypeCode::ReqEnd.code());
        decoder.push(&bad);
        let first = decoder.next_frame().unwrap_err();
        assert_eq!(first, FrameError::BadLength { length: 2 });

        // Pushing a perfectly good frame changes nothing.
        decoder.push(&wire(&Frame::ReqEnd, 1));
        assert_eq!(decoder.buffered(), 0);
        assert_eq!(decoder.next_frame().unwrap_err(), first);
        assert_eq!(decoder.next_raw().unwrap_err(), first);
        assert_eq!(decoder.error(), Some(&first));
        assert!(decoder.is_poisoned());
    }

    #[test]
    fn decoder_compacts_and_reuses_its_buffer() {
        let body = vec![0xabu8; 64 * 1024];
        let mut stream = Vec::new();
        limits().encode_req_body(1, &body, &mut stream).unwrap();
        let capacity = stream.len() + 16;
        let mut decoder = Decoder::with_capacity(limits(), capacity);
        for _ in 0..8 {
            decoder.push(&stream);
            assert_eq!(
                decoder.next_frame().unwrap(),
                Some((1, Frame::ReqBody(body.clone())))
            );
            assert_eq!(decoder.buffered(), 0);
        }
        assert!(decoder.buf.capacity() >= capacity);
        assert!(decoder.buf.capacity() <= capacity * 2);
    }

    #[test]
    fn next_frame_reports_a_ctl_id_of_zero_only() {
        let mut decoder = Decoder::new(limits());
        decoder.push(&wire(&Frame::Ctl(Ctl::Drain), 0));
        assert_eq!(
            decoder.next_frame().unwrap(),
            Some((0, Frame::Ctl(Ctl::Drain)))
        );

        // Built by hand: the encoder refuses to produce this.
        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(TypeCode::Drain.code(), 4, &[]));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::IdNotZero { id: 4 }
        );
        // The raw layer still forwards it.
        let mut decoder = Decoder::new(limits());
        decoder.push(&frame_bytes(TypeCode::Drain.code(), 4, &[]));
        assert_eq!(
            decoder.next_raw().unwrap(),
            Some((4, RawFrame::new(0x44, 4, Vec::new())))
        );
    }

    #[test]
    fn request_exchange_carries_every_field() {
        let head = ReqHead {
            method: b"POST".to_vec(),
            target: b"/upload?x=1".to_vec(),
            headers: vec![
                (b"Host".to_vec(), b"example.com".to_vec()),
                (b"X-Odd".to_vec(), vec![0xff, 0x00, 0xfe]),
            ],
            remote_addr: "203.0.113.7:41234".to_string(),
            scheme: Scheme::Https,
            server_name: "example.com".to_string(),
            server_port: 8443,
        };
        let resp = RespHead {
            status: 201,
            reason: b"Created".to_vec(),
            headers: vec![(b"Content-Length".to_vec(), b"0".to_vec())],
        };
        let frames = vec![
            (1, Frame::ReqHead(head.clone())),
            (1, Frame::ReqBody(vec![1, 2, 3])),
            (1, Frame::ReqEnd),
            (1, Frame::RespHead(resp.clone())),
            (1, Frame::RespEnd),
            (
                1,
                Frame::Done {
                    outcome: DoneOutcome::Completed,
                },
            ),
        ];
        let mut stream = Vec::new();
        for (id, frame) in &frames {
            limits().encode(frame, *id, &mut stream).unwrap();
        }
        let mut decoder = Decoder::new(limits());
        decoder.push(&stream);
        for expected in &frames {
            assert_eq!(decoder.next_frame().unwrap(), Some(expected.clone()));
        }
        assert_eq!(decoder.next_frame().unwrap(), None);
        assert!(matches!(head.scheme, Scheme::Https));
    }

    #[test]
    fn ctl_messages_round_trip_through_the_decoder() {
        let messages = [
            Ctl::Spawn { route: 3 },
            Ctl::Spawned {
                route: 3,
                pid: 4242,
            },
            Ctl::Retire {
                route: 3,
                pid: 4242,
                reason: "idle for 60s".to_string(),
            },
            Ctl::Exited {
                route: 3,
                pid: 4242,
                status: ExitStatus::Code(0),
            },
            Ctl::Exited {
                route: 3,
                pid: 4242,
                status: ExitStatus::Signal(9),
            },
            Ctl::Drain,
        ];
        let mut stream = Vec::new();
        for message in &messages {
            limits()
                .encode(&Frame::Ctl(message.clone()), 0, &mut stream)
                .unwrap();
        }
        let mut decoder = Decoder::new(limits());
        decoder.push(&stream);
        for message in &messages {
            assert_eq!(
                decoder.next_frame().unwrap(),
                Some((0, Frame::Ctl(message.clone())))
            );
        }
        assert_eq!(decoder.next_frame().unwrap(), None);
    }

    #[test]
    fn mutated_and_garbage_input_never_panics() {
        let limits = Limits::new(48).unwrap();
        for seed in 0..12u64 {
            let mut rng = Rng::new(seed * 0x9E37_79B9 + 1);
            let mut decoder = Decoder::new(limits);
            let mut stream = Vec::new();
            for _ in 0..24 {
                let code = all_codes()[rng.below(all_codes().len() as u32) as usize];
                let len = rng.below(40) as usize;
                let payload: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
                stream.extend_from_slice(&frame_bytes(code, rng.next_u64() as u32, &payload));
            }
            // Chop the stream somewhere random and let the decoder chew on it.
            let cut = rng.below(stream.len() as u32 + 1) as usize;
            decoder.push(&stream[..cut]);
            while let Ok(Some(_)) = decoder.next_frame() {}
        }
    }

    #[test]
    fn complete_but_truncated_payloads_are_rejected() {
        // A stricter version of the fuzz above: the frame header is rewritten
        // so the payload is a *complete* frame that ends early.
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("REQ_HEAD", wire(&Frame::ReqHead(ReqHead::default()), 1)),
            (
                "RESP_HEAD",
                wire(
                    &Frame::RespHead(RespHead {
                        status: 200,
                        reason: b"OK".to_vec(),
                        headers: vec![(b"a".to_vec(), b"b".to_vec())],
                    }),
                    1,
                ),
            ),
            (
                "RESP_ABORT",
                wire(
                    &Frame::RespAbort {
                        reason: "boom".to_string(),
                    },
                    1,
                ),
            ),
            (
                "DONE",
                wire(
                    &Frame::Done {
                        outcome: DoneOutcome::Aborted,
                    },
                    1,
                ),
            ),
            ("SPAWN", wire(&Frame::Ctl(Ctl::Spawn { route: 1 }), 0)),
            (
                "SPAWNED",
                wire(&Frame::Ctl(Ctl::Spawned { route: 1, pid: 2 }), 0),
            ),
            (
                "RETIRE",
                wire(
                    &Frame::Ctl(Ctl::Retire {
                        route: 1,
                        pid: 2,
                        reason: "x".to_string(),
                    }),
                    0,
                ),
            ),
            (
                "EXITED",
                wire(
                    &Frame::Ctl(Ctl::Exited {
                        route: 1,
                        pid: 2,
                        status: ExitStatus::Signal(11),
                    }),
                    0,
                ),
            ),
        ];
        for (name, full) in cases {
            let payload_len = full.len() - HEADER_LEN;
            for cut in 0..payload_len {
                let short = shorten_frame(&full, cut);
                let mut decoder = Decoder::new(limits());
                decoder.push(&short);
                match decoder.next_frame() {
                    Ok(None) => panic!("{name}: truncated frame waited for more bytes"),
                    Ok(Some(frame)) => panic!("{name}: truncated frame decoded to {frame:?}"),
                    Err(_) => {}
                }
            }
        }
    }
}
