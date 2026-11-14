#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Tests assert on `Result`s; production code never unwraps (design §4.5.1).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! The pure FastCGI 1.0 codec (design `docs/design/php-fastcgi.md` §4.5).
//!
//! This crate is bytes in, bytes out: it opens no socket, reads no file, spawns
//! no process, has **no dependencies** and contains no `unsafe`. The scheduler
//! (`blastit-scheduler::upstream::fastcgi`, `MB.3.6.2.1`) owns the socket, the
//! clock and the credit; everything it puts on the wire and everything it reads
//! back is built and checked here.
//!
//! * [`record`] — the 8-byte version-1 header, the 8-byte padding, and the
//!   total decoder: a truncated or over-long record is [`RecordError`], never a
//!   panic or an over-read.
//! * [`param`] — the `PARAMS` name-value codec: a length below 128 in one byte,
//!   otherwise four bytes with the high bit set, and the empty-name terminator.
//! * [`cgi`] — the CGI response head on `STDOUT`: `Status:` in `200..=599`,
//!   control-byte-free values, the four proxy-control names refused, every
//!   other `X-Accel-*` name dropped, and the head caps.
//!
//! Nothing here maps to an HTTP status: the codec-local [`RecordError`] and
//! [`CgiError`] are what the adapter turns into `UpstreamError::Fault` (`502`),
//! and [`Error`] is the single type for a caller that uses both halves.
//!
//! ```
//! use blastit_fastcgi::param::{Decoder, Pair, encode_pair};
//! use blastit_fastcgi::record::{Record, RecordType, encode};
//!
//! let mut content = Vec::new();
//! encode_pair(b"REQUEST_METHOD", b"GET", &mut content)?;
//! let mut wire = Vec::new();
//! encode(RecordType::Params, 1, &content, &mut wire)?;
//!
//! let (record, used) = Record::decode(&wire)?;
//! assert_eq!(used, wire.len());
//! assert_eq!(record.header.kind, RecordType::Params);
//! assert_eq!(
//!     Decoder::new(record.content).next_pair()?,
//!     Some(Pair { name: b"REQUEST_METHOD", value: b"GET" })
//! );
//! # Ok::<(), blastit_fastcgi::Error>(())
//! ```

use core::fmt;

pub mod cgi;
pub mod param;
pub mod record;

pub use cgi::CgiError;
pub use record::RecordError;

/// The only FastCGI protocol version this crate speaks.
pub const FCGI_VERSION_1: u8 = 1;

/// Largest record content, `u16::MAX`: php-fpm refuses a record whose
/// `contentLength + paddingLength` is above this (research §A, §F).
pub const FCGI_MAX_CONTENT: usize = 65_535;

/// Encoded `PARAMS` budget for one request, terminator and padding excluded
/// (design §4.6.2: "encoded ≤ 65 535").
pub const MAX_PARAMS_BYTES: usize = 65_535;

/// Largest single encoded name-value pair, its length prefixes included: the
/// [`MAX_PARAMS_BYTES`] budget minus four bytes (design §4.6.2: "one pair
/// ≤ 65 531 including its length prefixes").
pub const MAX_PARAM_PAIR_BYTES: usize = 65_531;

/// Every failure this crate can report, for a caller that uses more than one
/// module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A record could not be framed or decoded, see [`RecordError`].
    Record(RecordError),
    /// A CGI response head was refused, see [`CgiError`].
    Cgi(CgiError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => error.fmt(f),
            Self::Cgi(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Record(error) => Some(error),
            Self::Cgi(error) => Some(error),
        }
    }
}

impl From<RecordError> for Error {
    fn from(error: RecordError) -> Self {
        Self::Record(error)
    }
}

impl From<CgiError> for Error {
    fn from(error: CgiError) -> Self {
        Self::Cgi(error)
    }
}
