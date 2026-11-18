//! The `view.chan` transport: frames, credit windows and the loaded-file
//! datagram every view kind speaks (ARCHITECTURE.md §2.5, §3.1, §9.3).
//!
//! A view process is one route of the manifest, running under that route's uid
//! and talking to the scheduler over one socket. This crate is that socket's
//! vocabulary and nothing above it: [`wire`] serialises frames, [`flow`] holds
//! the two credit windows of §3.1, [`loaded`] is the dev-only loaded-file
//! report (`ADR-0014`), [`path`] is the one request-target decoder, and
//! [`ViewError`] is what a broken channel is reported as.
//!
//! # What lives here, and what does not
//!
//! The interpreter (or C SAPI) that actually answers a request is a **sibling
//! crate per view kind** — `blastit-view-python` for the embedded CPython,
//! `blastit-view-php` for `libphp` — each owning its innards end to end and
//! building on these types. What stays here is everything two view kinds must
//! agree on, so a second worker can never invent a second framing, a second
//! window or a second meaning for `%2F` (design `view-crates-architecture.md`
//! §4.2.1, `ADR-0016`).
//!
//! The worker-side runtime seam ([`WorkerRuntime`], [`RequestIo`],
//! [`RequestEnd`]) is here because `php.rs` still implements it; it is deleted
//! once no sibling uses it (`MB.36.5`).
//!
//! # Code
//!
//! The crate is `#![forbid(unsafe_code)]`: framing, credit and the datagram
//! codec are all plain `std`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![forbid(unsafe_code)]

mod config;
mod error;
pub mod flow;
// The dev-only loaded-file report (`MB.23.4`, `ADR-0014`): one bounded,
// versioned datagram codec, shared by every worker. It is plain `std`, so it is
// compiled whether or not a worker that reports is.
pub mod loaded;
// The one request-target decoder (`MB.36.2`, D3): the scheduler routes on it
// and the static responder resolves on it, so there is never a second
// implementation of what `%2F` means.
pub mod path;
// The composition root's seam to every view crate (`MB.36.4`, design §4.4
// D1/D2): the boot data a forked child is made from, and the fork itself as an
// injected parameter, so a view crate never calls `fork(2)` and never names
// `blastit`.
pub mod spawn;
// Embedded PHP (`MB.3.13`): `PhpEmbedRuntime` and the single-request channel
// loop. The crate stays `#![forbid(unsafe_code)]`; the `dlopen` and the C sink
// trampolines are in `blastit-php-sapi`.
#[cfg(feature = "php")]
pub mod php;
mod runtime;
pub mod stream;
pub mod sync;
pub mod wire;

pub use crate::config::ViewConfig;
pub use crate::error::ViewError;
// The dev-only loaded-file report: the codec and its bounds, so a consumer can
// decode a datagram without guessing a number, and the socket pair a view is
// handed once.
pub use crate::loaded::{
    Batch, DecodeError, EncodeError, FLAG_BOOT, FLAG_RESYNC, HEADER_BYTES, LoadedReport,
    MAX_BATCH_BYTES, MAX_PATH_BYTES, MAX_PATHS, PRELOAD_SOURCE, Reporter, Sent, Sequence, Step,
    VERSION, chunks, decode,
};
// The worker-side runtime seam (`MB.3.23`, design §4.1): what a route worker
// implements, so the scheduler never learns which language it is serving.
pub use crate::runtime::{RequestEnd, RequestIo, WorkerRuntime};
pub use crate::stream::{OUTPUT_LINE_CAP, OutputStream};

use blastit_frame::{Frame, TypeCode};

/// The name of a frame's kind, for a protocol error (never its payload bytes).
///
/// `pub` because a view kind reports a protocol error from its own crate; the
/// one rule that matters is that the payload never appears in the message,
/// since a body frame's bytes are an application's data.
pub fn frame_kind(frame: &Frame) -> &'static str {
    TypeCode::from_code(frame.code())
        .map(TypeCode::name)
        .unwrap_or("unknown")
}
