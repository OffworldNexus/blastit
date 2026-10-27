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
//! * `id` is the request or stream id. Frames without a stream — every
//!   `ctl.chan` message and `READY` — always use `0`, see
//!   [`Frame::requires_zero_id`].
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
//!
//! # Log safety
//!
//! `Debug` is the log-safe view of a frame: it never prints a header value or a
//! body, because those routinely carry personal data and secrets and are never
//! logged (ARCHITECTURE.md §8.1). Bodies print their length, heads print their
//! method, path (without the query string), header *names* and connection
//! facts, and [`RawFrame`] prints its type, id and length. Reach for the
//! accessors when the real bytes are needed.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod clientip;
mod ctl;
mod decoder;
mod error;
mod frame;
mod limits;
mod raw;
#[cfg(test)]
mod testutil;
mod wire;

pub use crate::clientip::{
    ClientIp, ClientIpConfig, ClientIpError, ClientIpSource, TrustedProxies, is_header_name,
    normalize_ip, parse_addr, parse_port, peer_address,
};
pub use crate::ctl::{Ctl, ExitStatus, MAX_SIGNAL, MIN_SIGNAL};
pub use crate::decoder::Decoder;
pub use crate::error::FrameError;
pub use crate::frame::{
    DoneOutcome, Frame, INITIAL_WINDOW, MAX_STATUS, MIN_STATUS, ReqHead, RequestCost, RespHead,
    Scheme, TypeCode,
};
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
            peer_addr: String::new(),
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
            Frame::RespEndCost {
                cost: RequestCost::new(12_500_000, Some(3_000_000)),
            },
            Frame::RespEndCost {
                cost: RequestCost::new(u64::MAX, Some(0)),
            },
            Frame::Ready,
            Frame::Window { bytes: 1 },
            Frame::Window { bytes: u32::MAX },
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
        // Frames without a stream must go out with id 0, everything else gets
        // an id with plenty of bits set.
        let frame = &every_frame()[index];
        if frame.requires_zero_id() {
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
        let tight = Limits::new(45).unwrap();
        let small = ReqHead {
            method: b"GET".to_vec(),
            target: b"/".to_vec(),
            remote_addr: "::1".to_string(),
            server_name: "h".to_string(),
            server_port: 1,
            ..ReqHead::default()
        };
        // 9 header + 7 method + 5 target + 2 count + 7 remote_addr + 1 scheme
        // + 5 server_name + 2 port + 4 (empty) peer_addr = 42.
        assert!(
            tight
                .encode(&Frame::ReqHead(small.clone()), 1, &mut Vec::new())
                .is_ok()
        );
        let mut bigger = small;
        bigger.headers.push((b"X-A".to_vec(), vec![0u8; 6]));
        assert_eq!(
            tight.encode(&Frame::ReqHead(bigger), 1, &mut Vec::new()),
            Err(FrameError::FrameTooLarge { size: 59, max: 45 })
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
    fn frames_without_a_stream_are_sent_with_id_zero() {
        for frame in [
            Frame::Ctl(Ctl::Spawn { route: 1 }),
            Frame::Ctl(Ctl::Drain),
            Frame::Ready,
        ] {
            let mut out = Vec::new();
            assert_eq!(
                encode(&frame, 7, &mut out),
                Err(FrameError::IdNotZero { id: 7 }),
                "{frame:?} must not be encodable with a stream id"
            );
            assert!(out.is_empty());
            assert!(encode(&frame, 0, &mut out).is_ok());
            assert_eq!(
                frame.to_raw(3).unwrap_err(),
                FrameError::IdNotZero { id: 3 }
            );
            assert!(frame.requires_zero_id());
        }

        // Frames that do belong to a stream carry any id.
        assert!(encode(&Frame::ReqEnd, 7, &mut Vec::new()).is_ok());
        assert!(encode(&Frame::Cancel, 7, &mut Vec::new()).is_ok());
        assert!(!Frame::RespEnd.requires_zero_id());
        assert!(Frame::RespEnd.to_raw(7).is_ok());
    }

    #[test]
    fn raw_frames_without_a_stream_reject_a_stream_id() {
        for ty in [
            TypeCode::Spawn.code(),
            TypeCode::Spawned.code(),
            TypeCode::Retire.code(),
            TypeCode::Exited.code(),
            TypeCode::Drain.code(),
            TypeCode::Ready.code(),
        ] {
            let raw = RawFrame::new(ty, 9, Vec::new());
            let mut out = Vec::new();
            assert_eq!(
                raw.encode(&Limits::default(), &mut out),
                Err(FrameError::IdNotZero { id: 9 }),
                "type {ty:#04x}"
            );
            assert!(out.is_empty());
            assert!(raw.requires_zero_id());

            let mut raw = raw;
            raw.set_id(0);
            assert!(raw.encode(&Limits::default(), &mut out).is_ok());
            assert!(!out.is_empty());
        }

        // A stream frame keeps its id, and an unknown code is left alone.
        let raw = RawFrame::new(TypeCode::ReqBody.code(), 9, Vec::new());
        assert!(raw.encode(&Limits::default(), &mut Vec::new()).is_ok());
        let raw = RawFrame::new(0x7f, 9, Vec::new());
        assert!(raw.encode(&Limits::default(), &mut Vec::new()).is_ok());
        assert!(!raw.requires_zero_id());
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

        // ... including RETIRE's reason, which also crosses ctl.chan.
        let mut payload4 = Vec::new();
        payload4.extend_from_slice(&3u32.to_le_bytes()); // route
        payload4.extend_from_slice(&4u32.to_le_bytes()); // pid
        payload4.extend_from_slice(&(invalid.len() as u32).to_le_bytes());
        payload4.extend_from_slice(&invalid);
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::Retire.code(),
            0,
            &payload4,
        ));
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::NotUtf8 { field: "reason" }
        );
        // The same payload without the bad bytes is a normal RETIRE.
        let mut payload5 = Vec::new();
        payload5.extend_from_slice(&3u32.to_le_bytes());
        payload5.extend_from_slice(&4u32.to_le_bytes());
        payload5.extend_from_slice(&4u32.to_le_bytes());
        payload5.extend_from_slice(b"idle");
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&crate::testutil::frame_bytes(
            TypeCode::Retire.code(),
            0,
            &payload5,
        ));
        assert_eq!(
            decoder.next_frame().unwrap(),
            Some((
                0,
                Frame::Ctl(Ctl::Retire {
                    route: 3,
                    pid: 4,
                    reason: "idle".to_string(),
                })
            ))
        );
    }

    #[test]
    fn status_outside_the_valid_range_is_rejected() {
        assert_eq!(RespHead::default().status, 200);
        assert_eq!(RespHead::default().validated(), Ok(()));

        for bad in [0u16, 1, 99, 1000, u16::MAX] {
            let head = RespHead {
                status: bad,
                ..RespHead::default()
            };
            assert_eq!(head.validated(), Err(FrameError::BadStatus { status: bad }));
            let mut out = Vec::new();
            assert_eq!(
                encode(&Frame::RespHead(head), 1, &mut out),
                Err(FrameError::BadStatus { status: bad }),
                "status {bad} must not be encodable"
            );
            assert!(out.is_empty());
            assert_eq!(
                Frame::RespHead(RespHead {
                    status: bad,
                    ..RespHead::default()
                })
                .to_raw(1)
                .unwrap_err(),
                FrameError::BadStatus { status: bad }
            );
        }

        for good in [MIN_STATUS, 200, 204, 418, 599, MAX_STATUS] {
            let head = RespHead {
                status: good,
                ..RespHead::default()
            };
            assert_eq!(head.validated(), Ok(()));
            let mut wire = Vec::new();
            encode(&Frame::RespHead(head.clone()), 1, &mut wire).unwrap();
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            assert_eq!(
                decoder.next_frame().unwrap(),
                Some((1, Frame::RespHead(head)))
            );
        }

        // A peer can send any status; the decoder refuses the invalid ones.
        for bad in [0u16, 99, 1000, u16::MAX] {
            let mut payload = bad.to_le_bytes().to_vec();
            payload.extend_from_slice(&0u32.to_le_bytes()); // reason
            payload.extend_from_slice(&0u16.to_le_bytes()); // headers
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&crate::testutil::frame_bytes(
                TypeCode::RespHead.code(),
                1,
                &payload,
            ));
            assert_eq!(
                decoder.next_frame().unwrap_err(),
                FrameError::BadStatus { status: bad }
            );
            assert!(decoder.is_poisoned());
        }
    }

    #[test]
    fn signal_out_of_range_is_rejected() {
        for good in [MIN_SIGNAL, 9, 15, MAX_SIGNAL] {
            assert_eq!(
                ExitStatus::from_parts(1, good),
                Ok(ExitStatus::Signal(good))
            );
            assert_eq!(
                ExitStatus::Signal(good).validated(),
                Ok(ExitStatus::Signal(good))
            );
            let frame = Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Signal(good),
            });
            assert!(encode(&frame, 0, &mut Vec::new()).is_ok());
        }
        for bad in [0i32, -1, -9, MAX_SIGNAL + 1, 1000, i32::MAX, i32::MIN] {
            assert_eq!(
                ExitStatus::from_parts(1, bad),
                Err(FrameError::OutOfRange {
                    field: "signal",
                    detail: "must be in 1..=64"
                }),
                "signal {bad}"
            );
            let frame = Frame::Ctl(Ctl::Exited {
                route: 1,
                pid: 2,
                status: ExitStatus::Signal(bad),
            });
            let mut out = Vec::new();
            assert_eq!(
                encode(&frame, 0, &mut out),
                Err(FrameError::OutOfRange {
                    field: "signal",
                    detail: "must be in 1..=64"
                })
            );
            assert!(out.is_empty());

            // ... and a forged EXITED off the wire is refused too.
            let mut payload = Vec::new();
            payload.extend_from_slice(&1u32.to_le_bytes());
            payload.extend_from_slice(&2u32.to_le_bytes());
            payload.push(1); // signal tag
            payload.extend_from_slice(&bad.to_le_bytes());
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&crate::testutil::frame_bytes(
                TypeCode::Exited.code(),
                0,
                &payload,
            ));
            assert_eq!(
                decoder.next_frame().unwrap_err(),
                FrameError::OutOfRange {
                    field: "signal",
                    detail: "must be in 1..=64"
                }
            );
        }

        // Exit codes are not restricted: a process may return anything.
        for code in [0i32, 1, 255, -1, i32::MIN, i32::MAX] {
            assert_eq!(ExitStatus::from_parts(0, code), Ok(ExitStatus::Code(code)));
            assert_eq!(
                ExitStatus::Code(code).validated(),
                Ok(ExitStatus::Code(code))
            );
        }
    }

    #[test]
    fn debug_output_never_prints_header_values_or_bodies() {
        let secret = "s3cr3t-header-value";
        let head = ReqHead {
            method: b"GET".to_vec(),
            target: b"/search?token=s3cr3t-query&page=2".to_vec(),
            headers: vec![
                (b"Authorization".to_vec(), secret.as_bytes().to_vec()),
                (b"Cookie".to_vec(), b"session=deadbeef".to_vec()),
            ],
            remote_addr: "203.0.113.7".to_string(),
            scheme: Scheme::Https,
            server_name: "secret-host.example".to_string(),
            server_port: 443,
            peer_addr: "10.244.3.9:1234".to_string(),
        };
        let text = format!("{:?}", Frame::ReqHead(head));
        assert!(!text.contains(secret), "header value leaked: {text}");
        assert!(!text.contains("deadbeef"), "header value leaked: {text}");
        assert!(
            !text.contains("s3cr3t-query"),
            "query string leaked: {text}"
        );
        assert!(!text.contains("secret-host.example"), "Host leaked: {text}");
        assert!(
            text.contains("Authorization"),
            "header name missing: {text}"
        );
        assert!(text.contains("Cookie"), "header name missing: {text}");
        assert!(text.contains("GET"), "method missing: {text}");
        assert!(text.contains("/search"), "path missing: {text}");

        let resp = RespHead {
            status: 500,
            reason: b"s3cr3t-reason".to_vec(),
            headers: vec![(b"Set-Cookie".to_vec(), secret.as_bytes().to_vec())],
        };
        let text = format!("{:?}", Frame::RespHead(resp));
        assert!(
            !text.contains(secret),
            "response header value leaked: {text}"
        );
        assert!(!text.contains("s3cr3t-reason"), "reason leaked: {text}");
        assert!(text.contains("Set-Cookie"), "header name missing: {text}");
        assert!(text.contains("500"), "status missing: {text}");

        let body = b"s3cr3t-body-bytes".to_vec();
        for frame in [Frame::ReqBody(body.clone()), Frame::RespBody(body.clone())] {
            let text = format!("{frame:?}");
            assert!(!text.contains("s3cr3t-body-bytes"), "body leaked: {text}");
            assert!(
                text.contains(&body.len().to_string()),
                "length missing: {text}"
            );
        }

        // The raw layer carries exactly the same bytes, so it redacts too.
        let raw = RawFrame::new(TypeCode::RespBody.code(), 1, body);
        let text = format!("{raw:?}");
        assert!(
            !text.contains("s3cr3t-body-bytes"),
            "raw payload leaked: {text}"
        );
        assert!(text.contains("17"), "length missing: {text}");

        // A truncated (non-UTF-8) header name is still printable, and is not
        // printed as a byte array.
        let head = ReqHead {
            headers: vec![(vec![0xff, 0x00, b'A'], vec![0xff])],
            ..ReqHead::default()
        };
        let text = format!("{head:?}");
        assert!(text.contains('A'), "lossy name missing: {text}");
        assert!(
            !text.contains("[255, 0, 65]"),
            "name printed as bytes: {text}"
        );
    }

    #[test]
    fn a_cost_report_round_trips_in_both_wire_forms() {
        // One frame kind, two wire forms: `RESP_END_COST` when the view
        // measured the request, `RESP_END` when it has nothing to report (a
        // build from before D5, a response the I/O thread refused). The
        // scheduler must understand both, so both must round trip.
        let reported = Frame::RespEndCost {
            cost: RequestCost::new(12_345_678, Some(1_234_567)),
        };
        let bare = Frame::RespEnd;
        assert_eq!(reported.code(), TypeCode::RespEndCost.code());
        assert_eq!(bare.code(), TypeCode::RespEnd.code());
        assert_ne!(reported, bare, "the report is not the same frame");

        for frame in [reported.clone(), bare] {
            let mut wire = Vec::new();
            encode(&frame, 9, &mut wire).unwrap();
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            assert_eq!(decoder.next_frame().unwrap(), Some((9, frame.clone())));
        }

        // The layout is fixed: `u64 wall_ns | u8 cpu_present | [u64 cpu_ns]`. The
        // report is the last frame of the response, so the scheduler charges the
        // flow from it; the two wire forms have to be told apart by the type
        // code alone, and a view with no CPU clock to read writes the short
        // form of the report rather than a zero it did not measure.
        let mut wire = Vec::new();
        encode(&reported, 1, &mut wire).unwrap();
        assert_eq!(wire.len(), HEADER_LEN + 8 + 1 + 8);
        assert_eq!(wire[4], TypeCode::RespEndCost.code());
        let mut wire = Vec::new();
        encode(
            &Frame::RespEndCost {
                cost: without_cpu(),
            },
            1,
            &mut wire,
        )
        .unwrap();
        assert_eq!(wire.len(), HEADER_LEN + 8 + 1, "no CPU time, no CPU field");

        // Zero is a legitimate measurement (a request that cost nothing
        // measurable), and so is the whole `u64` range.
        for cost in [
            RequestCost::new(0, Some(0)),
            RequestCost::new(1, Some(0)),
            RequestCost::new(0, Some(u64::MAX)),
            RequestCost::new(u64::MAX, Some(u64::MAX)),
            without_cpu(),
        ] {
            let frame = Frame::RespEndCost { cost };
            let mut wire = Vec::new();
            encode(&frame, 2, &mut wire).unwrap();
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            assert_eq!(decoder.next_frame().unwrap(), Some((2, frame)));
        }
    }

    /// A cost report from a view with no thread clock to read.
    fn without_cpu() -> RequestCost {
        RequestCost::new(12_345_678, None)
    }

    #[test]
    fn a_truncated_cost_report_is_refused() {
        // A `RESP_END_COST` payload is exactly 9 or 17 bytes — a wall time, a
        // presence byte, and the CPU time when there is one — so anything else
        // is a broken frame rather than a request that cost nothing.
        let frame = Frame::RespEndCost {
            cost: RequestCost::new(7, Some(8)),
        };
        let payload = frame.to_raw(1).unwrap().payload;
        assert_eq!(payload.len(), 17);
        for len in [0usize, 1, 8, 9, 16] {
            let wire =
                crate::testutil::frame_bytes(TypeCode::RespEndCost.code(), 1, &payload[..len]);
            let mut decoder = Decoder::new(Limits::default());
            decoder.push(&wire);
            assert!(
                decoder.next_frame().is_err(),
                "a {len}-byte RESP_END_COST payload must not decode"
            );
        }

        // A presence byte that is neither 0 nor 1 is a broken report, not a
        // guess.
        let mut bad = payload.clone();
        bad[8] = 2;
        let wire = crate::testutil::frame_bytes(TypeCode::RespEndCost.code(), 1, &bad);
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::BadEnum {
                field: "cpu_present",
                value: 2
            }
        );

        // One byte too many is a broken frame too, not a cost report.
        let mut extra = payload.clone();
        extra.push(0);
        let wire = crate::testutil::frame_bytes(TypeCode::RespEndCost.code(), 1, &extra);
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::TrailingBytes {
                ty: "RESP_END_COST",
                extra: 1
            }
        );

        // ... and the kind is not a body frame: its payload is a wall time, a
        // presence byte and at most one more integer, so it cannot be smuggled
        // past the expensive path either.
        assert!(
            !Frame::RespEndCost {
                cost: RequestCost::default()
            }
            .requires_zero_id()
        );
    }

    #[test]
    fn the_raw_peer_round_trips_and_is_not_the_resolved_client() {
        // The head carries both: `remote_addr` is what the front end resolved
        // (possibly from a trusted header), `peer_addr` is the socket the
        // request arrived on, which no client can forge.
        let head = ReqHead {
            method: b"GET".to_vec(),
            target: b"/".to_vec(),
            remote_addr: "203.0.113.7".to_string(),
            peer_addr: "10.244.3.9:41337".to_string(),
            ..ReqHead::default()
        };
        let frame = Frame::ReqHead(head.clone());
        let mut wire = Vec::new();
        encode(&frame, 4, &mut wire).unwrap();
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        let Some((4, Frame::ReqHead(decoded))) = decoder.next_frame().unwrap() else {
            panic!("expected a REQ_HEAD");
        };
        assert_eq!(decoded, head);
        assert_eq!(decoded.peer_addr, "10.244.3.9:41337");
        assert_eq!(decoded.remote_addr, "203.0.113.7");

        // A head without a peer (the address could not be read) is valid: the
        // field is a string like any other, and an old-style empty value must
        // still decode.
        let empty = ReqHead {
            method: b"GET".to_vec(),
            peer_addr: String::new(),
            ..ReqHead::default()
        };
        let mut wire = Vec::new();
        encode(&Frame::ReqHead(empty.clone()), 5, &mut wire).unwrap();
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        assert_eq!(
            decoder.next_frame().unwrap(),
            Some((5, Frame::ReqHead(empty)))
        );

        // The log-safe `Debug` prints both addresses and still no header value.
        let text = format!("{head:?}");
        assert!(text.contains("10.244.3.9:41337"), "{text}");
        assert!(text.contains("203.0.113.7"), "{text}");
    }

    #[test]
    fn a_head_with_a_missing_peer_field_is_refused() {
        // The field is the last one in the payload, so a head that stops before
        // it is truncated rather than silently defaulted: the trust decision
        // the scheduler makes on it must not be made on nothing.
        let head = ReqHead {
            method: b"GET".to_vec(),
            target: b"/".to_vec(),
            remote_addr: "127.0.0.1:1".to_string(),
            scheme: Scheme::Https,
            server_name: "h".to_string(),
            server_port: 443,
            peer_addr: "127.0.0.1:1".to_string(),
            ..ReqHead::default()
        };
        let payload = Frame::ReqHead(head).to_raw(1).unwrap().payload;
        let wire = crate::testutil::frame_bytes(
            TypeCode::ReqHead.code(),
            1,
            &payload[..payload.len() - 5],
        );
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        assert_eq!(
            decoder.next_frame().unwrap_err(),
            FrameError::Truncated { field: "peer_addr" }
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

    #[test]
    fn a_window_of_zero_bytes_is_refused_both_ways() {
        let mut out = Vec::new();
        assert!(encode(&Frame::Window { bytes: 0 }, 3, &mut out).is_err());
        assert!(out.is_empty());
        let mut wire = Vec::new();
        encode(&Frame::Window { bytes: 7 }, 3, &mut wire).unwrap();
        let n = wire.len();
        wire[n - 4..].copy_from_slice(&0u32.to_le_bytes());
        let mut decoder = Decoder::new(Limits::default());
        decoder.push(&wire);
        assert!(decoder.next_frame().is_err());
    }
}
