//! Framing for `http.chan`, `view.chan` and `ctl.chan` (ARCHITECTURE.md §3,
//! §3.1).
//!
//! Every blastit channel that carries structured messages uses one wire format:
//!
//! ```text
//! u32 length | u8 type | u32 id | payload        (all little-endian)
//! ```
//!
//! * `length` counts the bytes *after* the length field, so it is always
//!   `5 + payload.len()`.
//! * `type` selects the frame kind, see [`TypeCode`].
//! * `id` is the request or stream id; `ctl.chan` frames always use `0`.
//! * a frame (header included) may not exceed [`Limits::max_frame`], which
//!   defaults to [`MAX_FRAME_DEFAULT`] (1 MiB, §3.1). Bodies larger than that
//!   are split into several `*_BODY` frames, see [`Limits::encode_req_body`].
//!
//! The crate is deliberately I/O-agnostic and allocation-conscious: it never
//! touches a socket, and the caller owns every buffer. A channel reader pushes
//! whatever `read()` returned into a [`Decoder`] and pulls frames out again;
//! the writer encodes into a `Vec<u8>` it reuses.
//!
//! ```
//! use blastit_frame::{Limits, Decoder, Frame, encode};
//!
//! let limits = Limits::default();
//! let mut wire = Vec::new();
//! encode(&Frame::ReqEnd, 7, &mut wire)?;
//!
//! // The bytes may arrive in any chunking, including one byte at a time.
//! let mut decoder = Decoder::new(limits);
//! for byte in &wire {
//!     decoder.push(&[*byte]);
//! }
//! assert_eq!(decoder.next_frame()?, Some((7, Frame::ReqEnd)));
//! assert_eq!(decoder.next_frame()?, None);
//! # Ok::<(), blastit_frame::FrameError>(())
//! ```
//!
//! # Layout of this crate
//!
//! * [`Frame`], [`ReqHead`], [`RespHead`] — the typed vocabulary of §3.1.
//! * [`Ctl`], [`ExitStatus`] — the `ctl.chan` messages of §3 and §4.
//! * [`RawFrame`] — the same frames with the payload left undecoded, for the
//!   scheduler to rewrite ids and forward.
//! * [`Limits`], [`Decoder`], [`encode`] — the codec itself.
//!
//! Errors are always [`FrameError`]; nothing in this crate panics on input.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod ctl;
mod decoder;
mod error;
mod frame;
mod limits;
mod raw;
#[cfg(test)]
mod testutil;
mod wire;

pub use crate::ctl::{Ctl, ExitStatus};
pub use crate::decoder::Decoder;
pub use crate::error::FrameError;
pub use crate::frame::{DoneOutcome, Frame, ReqHead, RespHead, Scheme, TypeCode};
pub use crate::limits::{BodyChunks, Limits, MAX_FRAME_CEILING, MAX_FRAME_DEFAULT, MIN_FRAME};
pub use crate::raw::RawFrame;
pub use crate::wire::{HEADER_LEN, MIN_LENGTH};

/// Encode `frame` with `id`, using the default [`Limits`].
///
/// `out` is appended to and is left untouched when the frame does not fit, so a
/// writer can keep using its buffer after an error. Use [`Limits::encode`] when
/// the channel has its own `max_frame`.
///
/// # Errors
///
/// [`FrameError::FrameTooLarge`] when the frame exceeds the limit, and any
/// payload error (a field that does not fit its wire encoding, or a `ctl.chan`
/// frame with a non-zero id).
pub fn encode(frame: &Frame, id: u32, out: &mut Vec<u8>) -> Result<(), FrameError> {
    Limits::default().encode(frame, id, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Rng;

    /// Every frame kind, including the awkward payloads: empty bodies, binary
    /// header bytes, a body that exactly fills a frame, extreme integers.
    fn every_frame() -> Vec<Frame> {
        let mut header = ReqHead {
            method: b"PUT".to_vec(),
            target: b"/\xe2\x9c\x93".to_vec(),
            headers: Vec::new(),
            remote_addr: String::new(),
            scheme: Scheme::Https,
            server_name: String::new(),
            server_port: 0,
        };
        // Header names and values are byte strings, not text.
        header.headers.push((vec![0xff, 0x00], vec![0xfe]));
        header.headers.push((b"X-Empty".to_vec(), Vec::new()));

        vec![
            Frame::ReqHead(header),
            Frame::ReqHead(ReqHead::default()),
            Frame::ReqBody(Vec::new()),
            Frame::ReqBody(vec![0, 0xff, 0x7f]),
            Frame::ReqEnd,
            Frame::Cancel,
            Frame::RespHead(RespHead::default()),
            Frame::RespHead(RespHead {
                status: 599,
                reason: b"boom".to_vec(),
                headers: vec![(vec![0xc3, 0x28], vec![0]), (b"".to_vec(), b"".to_vec())],
            }),
            Frame::RespBody(Vec::new()),
            Frame::RespBody(vec![9; 1024]),
            Frame::RespEnd,
            Frame::RespAbort {
                reason: String::new(),
            },
            Frame::RespAbort {
                reason: "utf-8 ✓".to_string(),
            },
            Frame::Done {
                outcome: DoneOutcome::Completed,
            },
            Frame::Done {
                outcome: DoneOutcome::Aborted,
            },
            Frame::Done {
                outcome: DoneOutcome::Errored,
            },
            Frame::Ready,
            Frame::Ctl(Ctl::Spawn { route: 0 }),
            Frame::Ctl(Ctl::Spawn { route: u32::MAX }),
            Frame::Ctl(Ctl::Spawned {
                route: 1,
                pid: u32::MAX,
            }),
            Frame::Ctl(Ctl::Retire {
                route: 1,
                pid: 2,
                reason: "lru".to_string(),
            }),
            Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Code(0),
            }),
            Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Code(-1),
            }),
            Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Code(i32::MIN),
            }),
            Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Signal(9),
            }),
            Frame::Ctl(Ctl::Drain),
        ]
    }

    fn frame_id(index: usize) -> u32 {
        // Control frames must go out with id 0, everything else gets an id
        // with plenty of bits set.
        let frame = &every_frame()[index];
        if frame.as_ctl().is_some() {
            0
        } else {
            (index as u32).wrapping_mul(0x9E37_79B9)
        }
    }

    #[test]
    fn every_frame_round_trips() {
        for (index, frame) in every_frame().iter().enumerate() {
            let id = frame_id(index);
            let mut wire = Vec::new();
            encode(frame, id, &mut wire).unwrap();

            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            let decoded = decoder.next_frame().unwrap();
            assert_eq!(decoded, Some((id, frame.clone())), "frame {frame:?}");
            assert_eq!(decoder.next_frame().unwrap(), None);
        }
    }

    #[test]
    fn every_frame_round_trips_through_the_raw_layer() {
        for (index, frame) in every_frame().iter().enumerate() {
            let id = frame_id(index);
            let raw = frame.to_raw(id).unwrap();
            assert_eq!(raw.ty, frame.code());
            assert_eq!(raw.decode().unwrap(), *frame);

            let limits = Limits::default();
            let mut wire = Vec::new();
            raw.encode(&limits, &mut wire).unwrap();
            let mut decoder = Decoder::new(limits);
            decoder.push(&wire);
            assert_eq!(decoder.next_frame().unwrap(), Some((id, frame.clone())));
        }
    }

    #[test]
    fn header_is_little_endian() {
        let mut wire = Vec::new();
        encode(&Frame::ReqEnd, 0x0102_0304, &mut wire).unwrap();
        assert_eq!(
            wire,
            vec![5, 0, 0, 0, 0x03, 0x04, 0x03, 0x02, 0x01],
            "u32 length | u8 type | u32 id"
        );
    }

    #[test]
    fn length_field_covers_everything_after_itself() {
        let frame = Frame::ReqBody(vec![0u8; 37]);
        let mut wire = Vec::new();
        encode(&frame, 1, &mut wire).unwrap();
        assert_eq!(wire.len(), 9 + 37);
        assert_eq!(u32::from_le_bytes([wire[0], wire[1], wire[2], wire[3]]), 42);
        assert_eq!(
            u32::from_le_bytes([wire[0], wire[1], wire[2], wire[3]]) as usize,
            wire.len() - 4
        );
    }

    #[test]
    fn empty_and_maximum_payloads_round_trip() {
        let limits = Limits::default();
        for len in [0usize, 1, 1024, limits.max_payload()] {
            let frame = Frame::ReqBody(vec![0x5a; len]);
            let mut wire = Vec::new();
            limits.encode(&frame, 1, &mut wire).unwrap();
            assert_eq!(wire.len(), 9 + len);
            assert!(wire.len() <= limits.max_frame());

            let mut decoder = Decoder::new(limits);
            decoder.push(&wire);
            assert_eq!(decoder.next_frame().unwrap(), Some((1, frame)));
        }
    }

    #[test]
    fn one_byte_over_the_limit_is_rejected() {
        let limits = Limits::new(32).unwrap();
        let too_big = Frame::ReqBody(vec![0u8; limits.max_payload() + 1]);
        let mut out = Vec::new();
        assert_eq!(
            limits.encode(&too_big, 1, &mut out),
            Err(FrameError::FrameTooLarge { size: 33, max: 32 })
        );
        assert!(out.is_empty(), "a rejected frame must not be written");

        // A frame that exactly fills the limit is fine.
        let exact = Frame::ReqBody(vec![0u8; limits.max_payload()]);
        assert!(limits.encode(&exact, 1, &mut out).is_ok());
        assert_eq!(out.len(), 32);

        // ... and a header-heavy frame is measured by its total size, not by
        // the size of its body.
        let tight = Limits::new(40).unwrap();
        let small = ReqHead {
            method: b"GET".to_vec(),
            target: b"/".to_vec(),
            remote_addr: "::1".to_string(),
            server_name: "h".to_string(),
            server_port: 1,
            ..ReqHead::default()
        };
        assert!(
            tight
                .encode(&Frame::ReqHead(small.clone()), 1, &mut Vec::new())
                .is_ok()
        );
        let mut bigger = small;
        bigger.headers.push((b"X-A".to_vec(), vec![0u8; 6]));
        assert_eq!(
            tight.encode(&Frame::ReqHead(bigger), 1, &mut Vec::new()),
            Err(FrameError::FrameTooLarge { size: 55, max: 40 })
        );
    }

    #[test]
    fn output_buffer_is_untouched_on_error() {
        let limits = Limits::new(16).unwrap();
        let mut out = b"prefix".to_vec();
        let err = limits.encode(&Frame::ReqBody(vec![0u8; 64]), 1, &mut out);
        assert!(err.is_err());
        assert_eq!(out, b"prefix");
        assert_eq!(limits.encode(&Frame::ReqEnd, 1, &mut out), Ok(()));
        assert_eq!(out.len(), 6 + 9);
    }

    #[test]
    fn custom_limits_apply_to_encoding_and_decoding() {
        let limits = Limits::new(64).unwrap();
        let frame = Frame::RespBody(vec![1u8; limits.max_payload()]);
        let mut wire = Vec::new();
        limits.encode(&frame, 5, &mut wire).unwrap();
        assert_eq!(wire.len(), 64);

        let mut decoder = Decoder::new(limits);
        decoder.push(&wire);
        assert_eq!(decoder.next_frame().unwrap(), Some((5, frame)));

        // The same bytes are fine for a bigger codec and too big for a smaller.
        assert!(Limits::new(128).unwrap().check_total(wire.len()).is_ok());
        assert!(Limits::new(40).unwrap().check_total(wire.len()).is_err());
    }

    #[test]
    fn ctl_frames_are_sent_with_id_zero() {
        let frame = Frame::Ctl(Ctl::Spawn { route: 1 });
        let mut out = Vec::new();
        assert_eq!(
            encode(&frame, 7, &mut out),
            Err(FrameError::IdNotZero { id: 7 })
        );
        assert!(out.is_empty());
        assert!(encode(&frame, 0, &mut out).is_ok());
        assert_eq!(
            frame.to_raw(3).unwrap_err(),
            FrameError::IdNotZero { id: 3 }
        );

        // Non-ctl frames carry any id.
        assert!(encode(&Frame::ReqEnd, 7, &mut Vec::new()).is_ok());
        assert!(Frame::Ready.to_raw(7).is_ok());
    }

    #[test]
    fn type_codes_are_distinct_and_documented() {
        let mut seen: Vec<u8> = Vec::new();
        for code in crate::testutil::all_codes() {
            let code = TypeCode::from_code(code).unwrap();
            assert!(!seen.contains(&code.code()), "duplicate code {code:?}");
            seen.push(code.code());
            assert!(!code.name().is_empty());
            assert_eq!(TypeCode::from_code(code.code()), Ok(code));
            assert_eq!(code.is_ctl(), code.code() >= 0x40);
        }
        assert_eq!(
            TypeCode::from_code(0x00),
            Err(FrameError::UnknownType { ty: 0 })
        );
        assert_eq!(
            TypeCode::from_code(0xff),
            Err(FrameError::UnknownType { ty: 0xff })
        );
        for frame in every_frame() {
            let code = frame.code();
            assert_eq!(TypeCode::from_code(code).unwrap().code(), code);
        }
    }

    #[test]
    fn enum_fields_reject_undefined_values() {
        assert_eq!(Scheme::from_code(0), Ok(Scheme::Http));
        assert_eq!(Scheme::from_code(1), Ok(Scheme::Https));
        for bad in [2u8, 3, 255] {
            assert_eq!(
                Scheme::from_code(bad),
                Err(FrameError::BadEnum {
                    field: "scheme",
                    value: u64::from(bad)
                })
            );
        }
        assert_eq!(DoneOutcome::from_code(0), Ok(DoneOutcome::Completed));
        assert_eq!(DoneOutcome::from_code(2), Ok(DoneOutcome::Errored));
        assert_eq!(
            DoneOutcome::from_code(9),
            Err(FrameError::BadEnum {
                field: "outcome",
                value: 9
            })
        );
        assert_eq!(ExitStatus::from_parts(0, 3), Ok(ExitStatus::Code(3)));
        assert_eq!(ExitStatus::from_parts(1, 9), Ok(ExitStatus::Signal(9)));
        assert_eq!(
            ExitStatus::from_parts(5, 0),
            Err(FrameError::BadEnum {
                field: "exit status tag",
                value: 5
            })
        );
    }

    #[test]
    fn non_utf8_string_fields_are_rejected() {
        let invalid = vec![0xf0, 0x28, 0x8c, 0x28];
        let mut method = Vec::new();
        method.extend_from_slice(&1u32.to_le_bytes());
        method.push(b'G');
        let mut target = Vec::new();
        target.extend_from_slice(&0u32.to_le_bytes());
        let mut payload = method.clone();
        payload.extend_from_slice(&target);
        payload.extend_from_slice(&0u16.to_le_bytes()); // no headers
        payload.extend_from_slice(&(invalid.len() as u32).to_le_bytes());
        payload.extend_from_slice(&invalid); // remote_addr
        payload.push(0); // scheme
        payload.extend_from_slice(&0u32.to_le_bytes()); // server_name
        payload.extend_from_slice(&80u16.to_le_bytes()); // server_port

        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::ReqHead.code(),
            1,
            &payload,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::NotUtf8 {
                field: "remote_addr"
            }
        );

        // Same for the server name: swap the two.
        let mut payload2 = method;
        payload2.extend_from_slice(&target);
        payload2.extend_from_slice(&0u16.to_le_bytes());
        payload2.extend_from_slice(&0u32.to_le_bytes()); // remote_addr empty
        payload2.push(0);
        payload2.extend_from_slice(&(invalid.len() as u32).to_le_bytes());
        payload2.extend_from_slice(&invalid); // server_name
        payload2.extend_from_slice(&80u16.to_le_bytes());

        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::ReqHead.code(),
            1,
            &payload2,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::NotUtf8 {
                field: "server_name"
            }
        );

        // And for the two reasons.
        let mut payload3 = (invalid.len() as u32).to_le_bytes().to_vec();
        payload3.extend_from_slice(&invalid);
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::RespAbort.code(),
            1,
            &payload3,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::NotUtf8 { field: "reason" }
        );
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        for code in [
            TypeCode::ReqEnd,
            TypeCode::Cancel,
            TypeCode::RespEnd,
            TypeCode::Ready,
            TypeCode::Drain,
        ] {
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&crate::testutil::frame_bytes(code.code(), 0, b"x"));
            assert_eq!(
                decoder.next_frame().unwrap_err(),
                FrameError::TrailingBytes {
                    ty: code.name(),
                    extra: 1
                }
            );
        }

        // A payload that is complete but has one byte too many.
        let mut extra = Frame::Done {
            outcome: DoneOutcome::Completed,
        }
        .to_raw(1)
        .unwrap()
        .payload;
        extra.push(0);
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::Done.code(),
            1,
            &extra,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::TrailingBytes {
                ty: "DONE",
                extra: 1
            }
        );
    }

    #[test]
    fn too_many_headers_are_rejected() {
        let head = ReqHead {
            headers: vec![(Vec::new(), Vec::new()); u16::MAX as usize + 1],
            ..ReqHead::default()
        };
        let mut out = Vec::new();
        assert_eq!(
            encode(&Frame::ReqHead(head), 1, &mut out),
            Err(FrameError::OutOfRange {
                field: "header_count",
                detail: "larger than u16::MAX"
            })
        );
        assert!(out.is_empty());
    }

    #[test]
    fn lying_header_count_is_rejected_without_huge_allocation() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes()); // method
        payload.extend_from_slice(&0u32.to_le_bytes()); // target
        payload.extend_from_slice(&u16::MAX.to_le_bytes()); // count
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::ReqHead.code(),
            1,
            &payload,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::Truncated {
                field: "header name"
            }
        );
    }

    #[test]
    fn decoding_is_canonical_for_every_mutation_that_succeeds() {
        // Whatever bytes a peer sends, if they decode at all, re-encoding the
        // decoded frame must produce exactly those payload bytes again.
        let mut rng = Rng::new(0xdead_beef);
        let codes = crate::testutil::all_codes();
        for _ in 0..4000 {
            let code = codes[rng.below(codes.len() as u32) as usize];
            let len = rng.below(48) as usize;
            let payload: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
            let wire = crate::testutil::frame_bytes(code, 1, &payload);

            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            match decoder.next_frame() {
                Ok(None) => panic!("a complete frame must not wait for more bytes"),
                Ok(Some((id, frame))) => {
                    assert_eq!(id, 1);
                    assert_eq!(frame.code(), code);
                    let mut again = Vec::new();
                    encode(&frame, 1, &mut again).unwrap();
                    assert_eq!(again, wire, "not canonical for {frame:?}");
                }
                Err(err) => {
                    assert!(decoder.is_poisoned());
                    assert_eq!(Some(&err), decoder.error());
                }
            }
        }
    }
}
