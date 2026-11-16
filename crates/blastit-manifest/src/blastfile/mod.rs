//! The `Blastfile` language: its reader, its lexer and, from `MB.21.3.2` on,
//! its parser and CST (§4.1).
//!
//! A `Blastfile` is policy, not input: whoever can write it can already grant
//! `unrestricted`, so the reader's job is to be total and cheap rather than to
//! make a hostile file harmless (§17.1). [`read`] is the one reader: it refuses
//! anything that is not a regular file, refuses a file over the 1 MiB cap from
//! its metadata before reading a byte, and never allocates more than the cap.
//!
//! The lexer is one byte-indexed pass (§5.4). It emits every token and every
//! piece of trivia with a byte span and a `line:col`, so the parser and the
//! printer consume the same stream: `Token::leading` names the trivia before a
//! token, which is what makes an untouched file printable byte for byte.
//!
//! Nothing here reads the environment, and nothing resolves a `$IDENT`: a name
//! is a name, and its value is the builder's business (§17.2, R20/R24).

// The lexer, its types and the parser that follows are consumed by the public
// entry points `MB.21.3.8` adds; until then only this module's tests call them,
// so staged code is allowed to be unreachable in a non-test build. Delete this
// line when `Manifest::from_blastfile_str` lands.
#![cfg_attr(not(test), allow(dead_code))]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::indexing_slicing
)]

mod lexer;
mod tokens;

use std::io::Read as _;
use std::path::Path;

use crate::error::{ManifestError, ManifestErrors, escape_control};

/// Largest `Blastfile` [`read`] accepts, in bytes (§4.5).
pub(crate) const MAX_SOURCE_BYTES: usize = 1_048_576;

/// Most tokens one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TOKENS: usize = 262_144;

/// Most trivia runs one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TRIVIA: usize = 262_144;

/// Reads a `Blastfile`: a regular file of at most 1 MiB.
///
/// The size is known before anything is read, so a file one byte over the cap
/// is refused from its metadata — a mode-`000` file of that size is
/// [`ManifestError::TooLarge`], not a permission error. A FIFO, a device or a
/// directory is refused from the same metadata, so `/dev/zero` and a FIFO with
/// no writer can never block the boot. The read itself is bounded by
/// `take(MAX_SOURCE_BYTES + 1)`, so a file that grows while it is read is
/// refused too.
///
/// The refusals carry no path and no position: the caller prefixes the path
/// exactly once, giving `<path>: <message>` (§18).
///
/// # Errors
///
/// [`ManifestError::TooLarge`] over the cap, [`ManifestError::NotRegularFile`]
/// for anything but a regular file, and [`ManifestError::Unreadable`] when the
/// file cannot be opened or is not UTF-8.
pub fn read(path: &Path) -> Result<String, ManifestErrors> {
    let metadata = std::fs::metadata(path).map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(ManifestError::NotRegularFile.into());
    }
    if metadata.len() > MAX_SOURCE_BYTES as u64 {
        return Err(ManifestError::TooLarge {
            bytes: metadata.len(),
        }
        .into());
    }
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut text = String::new();
    file.take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    if text.len() > MAX_SOURCE_BYTES {
        return Err(ManifestError::TooLarge {
            bytes: text.len() as u64,
        }
        .into());
    }
    Ok(text)
}

/// One position-less refusal from an operating-system error, escaped so it
/// stays on one line (§18).
fn unreadable(error: std::io::Error) -> ManifestErrors {
    ManifestError::Unreadable {
        message: escape_control(&error.to_string()),
    }
    .into()
}
