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
//!   scheduler to rewrite ids and forward, and [`ReqHeadView`] — a borrowed
//!   view of a request head it reads where it lies.
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

mod clientip;
mod ctl;
mod decoder;
mod error;
mod frame;
mod limits;
mod raw;
mod wire;

pub use crate::clientip::{
    ClientIp, ClientIpConfig, ClientIpError, ClientIpSource, TrustedProxies, is_header_name,
    normalize_ip, parse_addr, parse_port, peer_address, values_of,
};
pub use crate::ctl::{Ctl, ExitStatus, MAX_SIGNAL, MIN_SIGNAL};
pub use crate::decoder::Decoder;
pub use crate::error::FrameError;
pub use crate::frame::{
    DoneOutcome, Frame, INITIAL_WINDOW, MAX_STATUS, MIN_STATUS, ReqHead, RequestCost, RespHead,
    Scheme, TypeCode,
};
pub use crate::limits::{BodyChunks, Limits, MAX_FRAME_CEILING, MAX_FRAME_DEFAULT, MIN_FRAME};
pub use crate::raw::{RawFrame, ReqHeadView, ReqHeaders};
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
