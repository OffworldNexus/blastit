//! The `Blastfile` language: its reader, its lexer and, from `MB.21.3.2` on,
//! its parser and CST (§4.1).
//!
//! A `Blastfile` is policy, not input: whoever can write it can already grant
//! `unrestricted`, so the reader's job is to be total and cheap rather than to
//! make a hostile file harmless (§17.1). [`read`] is the one reader: it refuses
//! anything that is not a regular file, refuses a file over the 16 MiB cap from
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
mod sites;
mod syntax;
mod tokens;
pub(crate) mod write;

pub use print::fmt;
// §7.4's store slots: the unnamed `postgres { … }` block is the store `db`, and
// a caller that renders a store the way a `Blastfile` writes it (§13.3) needs the
// name to know it is the unnamed one.
pub use lower::DEFAULT_STORE;
// The typed `static` mount policy this grammar lowers to (§9.4). Re-exported
// here because `model` is private to the crate; a caller of `fmt` or of the
// scheduler's fixtures names the type beside the grammar that produces it.
pub use crate::model::{StaticImmutable, StaticPattern};
// The two entry points of the one pattern compiler (§8.1), so `model`'s
// `StaticPattern` compiles a pattern with the scan a `view` uses instead of a
// second copy of the rules. Renamed on the way out so a caller cannot mistake
// them for a regex compiler: both return the compiled fragment.
pub(crate) use pattern::{compile as compile_pattern_text, compile_regex as compile_regex_text};

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::edit::{BlastfileEditor, EditError, Editor as _};
use crate::error::{ManifestError, ManifestErrors, ManifestWarning, Position, escape_control};
use crate::model::Manifest;

/// `TableGrant` is `blastit-sqlpolicy`'s (the crate the analysis and dbproxy
/// share), re-exported through the model so a proposal and a report spell a
/// table grant one way.
pub use blastit_sqlpolicy::TableGrant;

/// One `view` `--learn` proposes (§14.5).
///
/// The editor writes exactly these three facts: inferring a name from a path
/// and deciding which paths to propose happens before the edit ([`apply_edits`]
/// only writes them). There is deliberately no `body` field — a learning run
/// cannot declare an ingress policy (§10.1, R11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedView {
    /// The view's name; `[a-z0-9][a-z0-9_-]*`, unique in the document.
    pub name: String,
    /// The methods the run saw, in the order it saw them.
    ///
    /// `GET` is written and `HEAD` is implied by the model (§8.4), so a run
    /// that saw only `GET` writes `GET`.
    #[serde(default)]
    pub methods: Vec<String>,
    /// The path patterns, matched against the whole of `PATH_INFO`, in order.
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// Exact `queries` `--learn` proposes for one view on one store (§14.5).
///
/// The statements arrive as the wire spelled them (`$1`, `$2`, …) and are
/// normalised before they are written; a statement that inlines a literal or
/// holds a redacted `…` is refused instead. There is deliberately no `body`
/// field (§10.1, R11).
///
/// `tables` and `functions` are the **scope** a `sql.denied` log can prove
/// without an exact statement (`fold --prefer-tables`): a table the suite
/// touched, with the columns each operation used, and the functions beyond the
/// safe set. They are merged into the view's `postgres [store] { }` block the
/// same way `queries` are, so a repeated proposal adds nothing; a name the
/// grammar cannot write as a `Word` (a quoted identifier, a schema-qualified
/// table) and a column list that names no column are refused before anything is
/// spliced (§13.1's spellings). `unrestricted`, `use`, `transactions` and `body`
/// stay unproposable: there is no field for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedGrant {
    /// The view the grant belongs to.
    pub view: String,
    /// The `postgres` store the statements run on, or `None` for the unnamed
    /// `postgres { … }` block, which is the store `db` (§7.4).
    ///
    /// A named store is written as `postgres <store> { … }`; it has to be
    /// declared.
    #[serde(default)]
    pub store: Option<String>,
    /// The statements to add, as the wire spelled them.
    #[serde(default)]
    pub queries: Vec<String>,
    /// `select`/`insert`/`update`/`delete` scope per table, `select
    /// products(id, name)` in the file (§13.1).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<String, TableGrant>,
    /// The functions beyond the safe set the view may call, `functions
    /// st_distance` in the file (§13.1).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub functions: BTreeSet<String>,
}

/// Everything one `--learn` run wants to add to a `Blastfile` (§14.5).
///
/// Views are inserted above the method-unrestricted catch-all (or appended),
/// grants are merged into the view's `postgres` block for that store, and the
/// whole edit is a no-op when there is nothing to add.
///
/// This is the `Blastfile` edits type. The file has one reader
/// ([`read`]) and one writer, and the proposal shape follows the file: a `view`
/// carries methods and patterns, so a proposed view is not a route regex list.
/// The Python side that plans against it moves in `MB.21.4.7`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlastfileEdits {
    /// New `view` blocks.
    #[serde(default)]
    pub views: Vec<ProposedView>,
    /// Exact statements to add to existing or newly proposed views.
    #[serde(default)]
    pub grants: Vec<ProposedGrant>,
}

impl BlastfileEdits {
    /// The manifest these edits intend, so the writer and its test share one
    /// oracle (P4) instead of re-implementing lowering in a test (§14.5).
    ///
    /// The routes are grouped by the view that produced them, the grants are
    /// merged per store, the new views are inserted at the catch-all's index,
    /// and the sandboxes are renumbered by view index — exactly what `lower`
    /// does to the edited text. Every top-level field the edits do not touch
    /// (the `app`, the `preload`, the mounts, the `entrypoint`) is carried
    /// across, so a mismatch means the writer changed something it does not
    /// own.
    pub fn apply_to(&self, manifest: &Manifest) -> Manifest {
        write::expected(self, manifest)
    }
}

/// Applies `--learn`'s proposals to a `Blastfile` document (§14.5, `R28`,
/// `R29`).
///
/// The source is parsed into the one tree of §14.1, then edited at the offsets
/// the tree names: comments, ordering and quoting outside the edited block are
/// reproduced byte for byte. New views are inserted immediately above the
/// first method-unrestricted catch-all view, or at the end of the file when
/// there is none; a statement already granted for that view and store is not
/// added twice. The result is re-parsed, re-lowered and compared with
/// [`BlastfileEdits::apply_to`]'s model, so an edit that would change anything
/// else fails with [`EditError::Verification`] and no output — a view's
/// `static` mount and the `entrypoint` included (`MB.3.21`).
///
/// An edits set with nothing to add returns the source unchanged, which makes
/// repeated learning runs byte-identical.
///
/// This is **the** editor: `blastit check --learn`, `blastit dev --learn` and
/// the Python `fold` binding all apply their proposals through it, so a
/// `Blastfile` has exactly one writer.
///
/// # Errors
///
/// [`EditError::Source`] when the document does not parse or lower, every
/// refusal of §14.5's table, and [`EditError::Verification`] when the
/// postcondition fails.
pub fn apply_edits(source: &str, edits: &BlastfileEdits) -> Result<String, EditError> {
    BlastfileEditor.apply(source, edits)
}

/// Largest `Blastfile` [`read`] accepts, in bytes (§4.5): 16 MiB.
///
/// Public because an editor's own inbound bound is derived from it: a frame
/// that can carry a legal `Blastfile` is `2 × MAX_SOURCE_BYTES + 65_536`
/// (`docs/design/blastfile-lsp-zed.md` §3.1), and writing that constant from the
/// parser's own cap is what keeps the two from drifting apart.
pub const MAX_SOURCE_BYTES: usize = 16_777_216;

/// Most tokens one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TOKENS: usize = 4_194_304;

/// Most trivia runs one `Blastfile` may hold (§4.5).
pub(crate) const MAX_TRIVIA: usize = 4_194_304;

/// Deepest block nesting the grammar allows (§4.5): `view { postgres { … } }`
/// and `view { GET "…" { body … } }` are two, and a third is refused.
pub(crate) const MAX_BLOCK_DEPTH: u32 = 2;

/// Most items one list may hold (§4.5): methods, patterns, columns, preload
/// modules, `immutable` patterns and table grants.
pub(crate) const MAX_LIST_ITEMS: usize = 4_096;

/// Most `char`s an `app "<name>"` may hold (§4.5).
///
/// The cap is on the name, not on the file: `app "shop"` is a label the guide
/// writes first, and a generated name (a tenant, a branch, a deploy id) is
/// long. `lower::app` refuses an empty name, a name over this cap, and a name
/// holding a control byte, at the string's own `line:col`.
pub(crate) const MAX_APP_NAME_CHARS: usize = 256;

pub(crate) use pattern::MAX_PATTERN_BYTES;

/// How many `immutable` patterns one `static` mount may declare (§9.4, `R5`).
///
/// The cap on the list, beside the compiler's cap on each pattern
/// ([`MAX_PATTERN_BYTES`]). It is checked in `lower`, at the pattern that
/// crosses it, so the refusal carries the `line:col` a developer can jump to;
/// [`StaticImmutable`](crate::model::StaticImmutable) is only ever built from a
/// list this has already bounded.
pub(crate) const MAX_PATTERNS: usize = 32;

/// Reads a `Blastfile`: a regular file of at most 16 MiB.
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

/// One `Blastfile` read the way an editor needs it: the model, its warnings, and
/// the source position of each warning, in the same order.
///
/// `sites.len() == warnings.len()` by construction, so a caller zips the two.
#[derive(Debug, Clone)]
pub struct Checked {
    /// The lowered, validated manifest.
    pub manifest: Manifest,
    /// The non-fatal warnings, exactly as [`Manifest::validate`] emits them.
    pub warnings: Vec<ManifestWarning>,
    /// The 1-based site of each warning, or [`Position::NONE`] when the
    /// declaration is not in the tree.
    pub sites: Vec<Position>,
}

/// Parses, lowers, validates and locates each warning (§5.5).
///
/// This is [`Manifest::parse_blastfile`] plus the one CST walk that
/// `parse_blastfile` drops: the tree has to stay in memory for the warning
/// sites, and an editor is the only caller that wants them. Nothing else
/// changes — the manifest, its warnings and their order are the boot's.
///
/// # Errors
///
/// Everything [`Manifest::parse_blastfile`] refuses.
pub fn check(source: &str, base: u32) -> Result<Checked, ManifestErrors> {
    let cst = parser::parse(source)?;
    let manifest = lower::lower(&cst, source)?;
    let warnings = manifest.validate(base)?;
    let sites = sites::sites(&cst, source, &warnings);
    Ok(Checked {
        manifest,
        warnings,
        sites,
    })
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
