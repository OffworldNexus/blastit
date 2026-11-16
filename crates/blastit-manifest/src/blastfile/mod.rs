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
//! `parser::parse` is the one parse: LL(1), one token of lookahead, and
//! `syntax::Cst` is its only product — the lossless arena of §14.1 that `lower`
//! and `print` both walk. Every refusal is one line at the offending token's
//! `line:col`, and the two budgets that bound the tree (`MAX_BLOCK_DEPTH`,
//! `MAX_LIST_ITEMS`) are checked where §4.5 puts them.
//!
//! `pattern` is the one compiler from a path pattern to the regex the
//! scheduler matches (§8.1) and the one method normalisation lowering applies
//! before a `Route` exists (§8.4), so `HEAD` is implied by `GET` once, at
//! lower time, and request matching itself does not change.
//!
//! `lower` is the one lowering: it walks the tree in §7.3's order and builds
//! the `Manifest` the rest of blastit reads, applying §7.4's store slots,
//! §10's ingress policies, §11's traits and `use`, §12's entrypoints and §13's
//! grants. [`Manifest::from_blastfile_str`] is parse + lower and
//! [`Manifest::parse_blastfile`] is parse + lower + `validate`, so the two
//! entry points of §4.4 differ by exactly the whole-file checks.
//!
//! Nothing here reads the environment, and nothing resolves a `$IDENT`: a name
//! is a name, and its value is the builder's business (§17.2, R20/R24).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::indexing_slicing
)]

mod lexer;
mod lower;
mod parser;
mod pattern;
mod print;
mod syntax;
mod tokens;

pub use print::fmt;

use std::io::Read as _;
use std::path::Path;

use crate::error::{ManifestError, ManifestErrors, ManifestWarning, escape_control};
use crate::model::Manifest;

/// Largest `Blastfile` [`read`] accepts, in bytes (§4.5).
pub(crate) const MAX_SOURCE_BYTES: usize = 1_048_576;

/// Most tokens one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TOKENS: usize = 262_144;

/// Most trivia runs one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TRIVIA: usize = 262_144;

/// Deepest block nesting the grammar allows (§4.5): `view { postgres { … } }`
/// and `view { GET "…" { body … } }` are two, and a third is refused.
pub(crate) const MAX_BLOCK_DEPTH: u32 = 2;

/// Most items one list may hold (§4.5): methods, patterns, columns, preload
/// modules, `immutable` patterns and table grants.
pub(crate) const MAX_LIST_ITEMS: usize = 256;

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

impl Manifest {
    /// Parses a `Blastfile` and lowers it, **without** validating (§4.4).
    ///
    /// The two steps are the parser's (`parse`) and `lower`'s; the whole-file
    /// checks of [`Manifest::validate`] are deliberately not run, so a caller
    /// that wants them calls [`Manifest::parse_blastfile`] instead. A parse or
    /// lower refusal never yields a manifest (R21).
    ///
    /// # Errors
    ///
    /// §5.3's lexer refusals, §6.7's parse refusals, §8.3's pattern refusals
    /// and §7.3/§9.1/§10/§11/§12/§13's lowering refusals — each one line, at
    /// the offending token's `line:col` (§18).
    pub fn from_blastfile_str(source: &str) -> Result<Self, ManifestErrors> {
        let cst = parser::parse(source)?;
        lower::lower(&cst, source)
    }

    /// Parses, lowers and validates a `Blastfile`, returning its warnings
    /// (§4.4).
    ///
    /// This is the boot and `check` entry point: [`Manifest::from_blastfile_str`]
    /// plus [`Manifest::validate`], so every whole-file rule the parser cannot
    /// see runs on the lowered model, unchanged (§7.3 step 7).
    ///
    /// # Errors
    ///
    /// Everything [`Manifest::from_blastfile_str`] refuses, and everything
    /// [`Manifest::validate`] refuses.
    pub fn parse_blastfile(
        source: &str,
        base: u32,
    ) -> Result<(Self, Vec<ManifestWarning>), ManifestErrors> {
        let manifest = Self::from_blastfile_str(source)?;
        let warnings = manifest.validate(base)?;
        Ok((manifest, warnings))
    }
}
