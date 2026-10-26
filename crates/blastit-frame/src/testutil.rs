//! Deterministic helpers shared by the unit tests.
//!
//! No `rand`, no clock, no filesystem: every test in this crate runs the same
//! way on every machine.

use crate::ctl::{Ctl, ExitStatus};
use crate::frame::{DoneOutcome, Frame, ReqHead, RequestCost, RespHead, Scheme};

/// A splitmix64 generator, used for chunk sizes and for mutated payloads.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// Seed the generator.
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 pseudo-random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`. `bound` must not be zero.
    pub fn below(&mut self, bound: u32) -> u32 {
        (self.next_u64() % u64::from(bound.max(1))) as u32
    }
}

/// Every type code this crate defines, for exhaustive tests.
pub fn all_codes() -> [u8; 17] {
    [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x40, 0x41, 0x42,
        0x43, 0x44,
    ]
}

/// Build a frame header plus payload by hand, bypassing the encoder.
///
/// Tests use this to produce input the encoder would never emit: unknown type
/// codes, payloads that do not match their frame kind, lying counts.
pub fn frame_bytes(ty: u8, id: u32, payload: &[u8]) -> Vec<u8> {
    let length = (5 + payload.len()) as u32;
    let mut out = Vec::with_capacity(9 + payload.len());
    out.extend_from_slice(&length.to_le_bytes());
    out.push(ty);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Re-declare a frame's length so that only `new_payload_len` payload bytes
/// remain: a frame that looks complete but is cut short mid-field.
pub fn shorten_frame(frame: &[u8], new_payload_len: usize) -> Vec<u8> {
    let mut out = frame[..9 + new_payload_len].to_vec();
    out[..4].copy_from_slice(&((5 + new_payload_len) as u32).to_le_bytes());
    out
}

/// Total frame size a `length` field asks for, saturating at `usize::MAX`.
///
/// Spelled out here so the expectations stay 32-bit-safe: `4 + u32::MAX` does
/// not fit a 32-bit `usize`, and the decoder saturates rather than wrapping.
pub fn total_frame_size(length: u32) -> usize {
    (u64::from(length) + 4).min(usize::MAX as u64) as usize
}

/// A whole request/response exchange with every frame kind on `http.chan` or
/// `view.chan`, as `(encoded stream, expected frames)`.
pub fn client_exchange() -> (Vec<u8>, Vec<(u32, Frame)>) {
    let head = ReqHead {
        method: b"GET".to_vec(),
        target: b"/x".to_vec(),
        headers: vec![(b"Host".to_vec(), b"example.com".to_vec())],
        remote_addr: "127.0.0.1".to_string(),
        scheme: Scheme::Http,
        server_name: "example.com".to_string(),
        server_port: 80,
        peer_addr: "127.0.0.1:5000".to_string(),
    };
    let frames = vec![
        (1, Frame::ReqHead(head)),
        (1, Frame::ReqBody(vec![0, 1, 2, 3])),
        (1, Frame::ReqEnd),
        (2, Frame::ReqHead(ReqHead::default())),
        (2, Frame::Cancel),
        (
            1,
            Frame::RespHead(RespHead {
                status: 200,
                reason: b"OK".to_vec(),
                headers: Vec::new(),
            }),
        ),
        (1, Frame::RespBody(b"hello".to_vec())),
        (1, Frame::RespEnd),
        (
            3,
            Frame::RespAbort {
                reason: "client gone".to_string(),
            },
        ),
        (
            1,
            Frame::Done {
                outcome: DoneOutcome::Completed,
            },
        ),
        (
            1,
            Frame::RespEndCost {
                cost: RequestCost::new(12_500_000, 3_000_000),
            },
        ),
        (
            2,
            Frame::RespEndCost {
                cost: RequestCost::new(0, 0),
            },
        ),
        (
            2,
            Frame::Done {
                outcome: DoneOutcome::Errored,
            },
        ),
        (
            0,
            Frame::Ctl(Ctl::Exited {
                route: 0,
                pid: 7,
                status: ExitStatus::Code(1),
            }),
        ),
        (0, Frame::Ready),
    ];
    let limits = crate::Limits::default();
    let mut stream = Vec::new();
    for (id, frame) in &frames {
        limits
            .encode(frame, *id, &mut stream)
            .expect("test frames are within the default limits");
    }
    (stream, frames)
}
