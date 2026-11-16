//! Surgical, comment-preserving `Blastfile` edits and atomic writes (`R28`,
//! `R29`).
//!
//! [`crate::blastfile::apply_edits`] is the `Blastfile` editor (§14.5): it
//! splices into the CST of §14.1 and proves the result with
//! [`crate::Manifest::equivalent`]. The crate-private `Editor` seam keeps the
//! splicing in the `blastfile` writer, so every caller (`dev_learn`, `check`,
//! `fold`) reaches it through one function.
//!
//! `--learn` has to add views and exact `queries` to a file a person wrote by
//! hand. Comments, ordering and quoting are the author's, so the editor does
//! not re-serialise the document: it parses the source, locates the insertion
//! points the parse names, splices whole lines, and then re-parses the result
//! to prove that every field it did not mean to touch is unchanged.
//!
//! Three rules shape the output:
//!
//! * a proposed view goes immediately above the first method-unrestricted
//!   catch-all, or at the end of the file;
//! * only exact, normalised `queries` are written. A statement that inlines a
//!   literal or still holds a redacted `…` is refused
//!   ([`EditError::LiteralQuery`], [`EditError::RedactedQuery`]): the message
//!   names the position and the fix, never the statement (`R24`);
//! * `body` is never written. [`crate::blastfile::ProposedView`] and
//!   [`crate::blastfile::ProposedGrant`] have no field for it, so "`--learn`
//!   never writes `body raw unvalidated`" is a property of the types rather
//!   than a policy (§10.1, R11).
//!
//! The edit is idempotent: applying the same
//! [`crate::blastfile::BlastfileEdits`] to its own output returns that output
//! byte for byte, and an edit with nothing to add returns the source untouched.
//!
//! [`write_atomic`] replaces a file the way the documentation promises: a
//! sibling `.<name>.<pid>.tmp`, an `fsync` of the file, a `rename`, an `fsync`
//! of the directory, and no temporary file left behind whether that succeeded
//! or failed. The temp file is created with `O_CREAT | O_EXCL`, so a planted
//! symlink at that name is never written through.
//!
//! ```
//! use blastit_manifest::blastfile::{BlastfileEdits, ProposedView, apply_edits};
//!
//! let source = "view fallback \"/*\" {\n}\n";
//! let edits = BlastfileEdits {
//!     views: vec![ProposedView {
//!         name: "orders".to_string(),
//!         methods: vec!["GET".to_string()],
//!         patterns: vec!["/orders/".to_string()],
//!     }],
//!     grants: vec![],
//! };
//!
//! let edited = apply_edits(source, &edits).expect("the edit applies");
//! assert!(edited.contains("view orders GET \"/orders/\""));
//! // The new view is above the catch-all, and the source is otherwise intact.
//! assert!(edited.find("orders").expect("inserted") < edited.find("fallback").expect("kept"));
//! assert_eq!(apply_edits(&edited, &edits).expect("idempotent"), edited);
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::{ManifestErrors, escape_control, quoted};

/// The seam between the edit types and the `Blastfile` writer (§14.5).
///
/// [`crate::blastfile::apply_edits`] is its one caller and the `Blastfile`
/// editor its one implementation, so the splicing stays in
/// [`crate::blastfile::write`] and every caller (`dev_learn`, `check`, `fold`)
/// reaches it through one function.
pub(crate) trait Editor {
    /// Applies `edits` to `source`, returning the edited document.
    ///
    /// # Errors
    ///
    /// The refusal of the rule the edit broke, or [`EditError::Verification`]
    /// when the splice cannot reproduce the model the edit intended.
    fn apply(
        &self,
        source: &str,
        edits: &crate::blastfile::BlastfileEdits,
    ) -> Result<String, EditError>;
}

/// The `Blastfile` editor: parse, splice, prove (P4).
pub(crate) struct BlastfileEditor;

impl Editor for BlastfileEditor {
    fn apply(
        &self,
        source: &str,
        edits: &crate::blastfile::BlastfileEdits,
    ) -> Result<String, EditError> {
        crate::blastfile::write::apply(source, edits)
    }
}

/// Why a manifest could not be edited.
///
/// Every message names the position and the fix. A statement is never printed:
/// a refused one either inlined a value or is a redaction of one (`R24`), so
/// the message names `queries[N]` and the route instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The document does not parse, so there is nothing to splice into.
    Source(ManifestErrors),
    /// A proposed route name is not `[a-z0-9][a-z0-9_-]*`.
    InvalidRouteName {
        /// The rejected name.
        name: String,
    },
    /// A proposed view has no endpoint pattern at all.
    EmptyPath {
        /// The view's name.
        name: String,
    },
    /// One of a route's path regexes is empty.
    EmptyPattern {
        /// The route's name.
        name: String,
        /// The index in the proposed `path` list.
        rule: usize,
    },
    /// One of a route's path regexes does not compile.
    InvalidPattern {
        /// The route's name.
        name: String,
        /// The index in the proposed `path` list.
        rule: usize,
    },
    /// One of a view's methods is not a method the grammar accepts.
    InvalidMethod {
        /// The view's name.
        name: String,
        /// The rejected method.
        method: String,
    },
    /// Two proposed routes carry the same name.
    DuplicateProposedRoute {
        /// The repeated name.
        name: String,
    },
    /// A proposed name is already declared with a different path.
    RouteConflict {
        /// The colliding name.
        name: String,
    },
    /// A grant names a store no `postgres <name>` declares.
    UnknownProxy {
        /// The view the grant names.
        route: String,
        /// The undeclared store.
        proxy: String,
    },
    /// A grant names a view that is neither declared nor proposed.
    MissingRoute {
        /// The unknown view name.
        route: String,
        /// The store the grant names.
        proxy: String,
    },
    /// A grant is `unrestricted = true`, which cannot sit next to `queries`.
    UnrestrictedGrant {
        /// The route the grant belongs to.
        route: String,
        /// The proxy the grant names.
        proxy: String,
    },
    /// A proposed statement still holds a redacted `…`.
    RedactedQuery {
        /// The route the grant names.
        route: String,
        /// The proxy the grant names.
        proxy: String,
        /// The index in the proposed `queries` list.
        index: usize,
    },
    /// A proposed statement inlines a quoted literal.
    LiteralQuery {
        /// The route the grant names.
        route: String,
        /// The proxy the grant names.
        proxy: String,
        /// The index in the proposed `queries` list.
        index: usize,
    },
    /// A proposed statement is empty after normalisation.
    EmptyQuery {
        /// The route the grant names.
        route: String,
        /// The proxy the grant names.
        proxy: String,
        /// The index in the proposed `queries` list.
        index: usize,
    },
    /// A table, column or function name has no spelling in a grant block.
    ///
    /// A grant block reads `Word` names (§13.1), so a quoted identifier, a
    /// schema-qualified table or a name with a space cannot be written; the
    /// edit is refused rather than written in a form the reader would refuse.
    UnwritableGrantName {
        /// The view the grant belongs to.
        view: String,
        /// The store the grant names.
        store: String,
        /// `table`, `column` or `function`.
        what: &'static str,
        /// The name as the analysis spelled it.
        name: String,
    },
    /// A table grant names no column at all: `select products()` grants nothing.
    EmptyColumnList {
        /// The view the grant belongs to.
        view: String,
        /// The store the grant names.
        store: String,
        /// The table whose column list is empty.
        table: String,
        /// `select`, `insert` or `update`.
        operation: &'static str,
    },
    /// The edit would have changed a part of the document it does not own.
    ///
    /// This is the editor's own postcondition, not the author's mistake: the
    /// edit is discarded and the file is left untouched.
    Verification {
        /// The section that would have changed.
        section: &'static str,
    },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => write!(
                f,
                "the document does not parse, so it cannot be edited: {error}"
            ),
            Self::InvalidRouteName { name } => write!(
                f,
                "name {} is not `[a-z0-9][a-z0-9_-]*`; derive it from the first path segment in \
                 lowercase",
                quoted(name)
            ),
            Self::EmptyPath { name } => write!(
                f,
                "{} has no pattern; a view answers at least one path",
                quoted(name)
            ),
            Self::EmptyPattern { name, rule } => write!(
                f,
                "{}'s patterns[{rule}] is empty; every pattern has to match something",
                quoted(name)
            ),
            Self::InvalidPattern { name, rule } => write!(
                f,
                "{}'s patterns[{rule}] is not a valid path pattern; fix the inferred pattern",
                quoted(name)
            ),
            Self::InvalidMethod { name, method } => write!(
                f,
                "view {}'s method {} is not a method; methods are upper-case (`GET`, `POST`, …)",
                quoted(name),
                quoted(method)
            ),
            Self::DuplicateProposedRoute { name } => write!(
                f,
                "two proposed views are named {}; disambiguate them with `-2`, `-3`, …",
                quoted(name)
            ),
            Self::RouteConflict { name } => write!(
                f,
                "{} is already declared with another method or pattern; --learn has to \
                 disambiguate the inferred name",
                quoted(name)
            ),
            Self::UnknownProxy { route, proxy } => write!(
                f,
                "view {} grants store {} but no `postgres {}` declares it; write the store first",
                quoted(route),
                quoted(proxy),
                escape_control(proxy)
            ),
            Self::MissingRoute { route, proxy } => write!(
                f,
                "a {} grant names view {}, which is neither declared nor proposed; add the view \
                 first",
                quoted(proxy),
                quoted(route)
            ),
            Self::UnrestrictedGrant { route, proxy } => write!(
                f,
                "view {}'s grant on {} is `unrestricted`; an exact query next to it is a boot \
                 error, so remove `unrestricted` first",
                quoted(route),
                quoted(proxy)
            ),
            Self::RedactedQuery {
                route,
                proxy,
                index,
            } => write!(
                f,
                "view {}'s {} grant would write queries[{index}] with a redacted `…`; the value \
                 is gone, so bind it and learn again",
                quoted(route),
                quoted(proxy)
            ),
            Self::LiteralQuery {
                route,
                proxy,
                index,
            } => write!(
                f,
                "view {}'s {} grant would write queries[{index}] with an inline literal; bind the \
                 value and learn again",
                quoted(route),
                quoted(proxy)
            ),
            Self::EmptyQuery {
                route,
                proxy,
                index,
            } => write!(
                f,
                "view {}'s {} grant has an empty queries[{index}]; write a statement or drop it",
                quoted(route),
                quoted(proxy)
            ),
            Self::UnwritableGrantName {
                view,
                store,
                what,
                name,
            } => write!(
                f,
                "view {}'s {store} grant names the {what} {}, which a grant block cannot spell; \
                 a {what} is `[A-Za-z0-9_-]` (a quoted identifier or a schema-qualified table has \
                 no spelling here) — write that grant by hand",
                quoted(view),
                quoted(name)
            ),
            Self::EmptyColumnList {
                view,
                store,
                table,
                operation,
            } => write!(
                f,
                "view {}'s {store} grant would write `{operation} {}(…)` with no column, which \
                 grants nothing; analyze the statement again or grant the table by hand",
                quoted(view),
                escape_control(table)
            ),
            Self::Verification { section } => write!(
                f,
                "the edit would rewrite {section}, which it does not own; the manifest was left \
                 untouched"
            ),
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ManifestErrors> for EditError {
    fn from(error: ManifestErrors) -> Self {
        Self::Source(error)
    }
}

/// Replaces `path` with `content` atomically (`R29`).
///
/// The bytes go to a sibling `.<filename>.<pid>.tmp` created with `O_CREAT |
/// O_EXCL` (so an existing file or a planted symlink at that name is refused),
/// the file is `fsync`ed, the temp file is `rename`d over `path`, and the
/// directory is `fsync`ed. The temp file is removed again on every failure, so
/// neither a successful nor a failed write leaves a `.<filename>.<pid>.tmp`
/// behind. The author's file mode is preserved. An error reported after the
/// rename means the new content is in place but its durability is unproven.
pub fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the manifest path has no file name",
        )
    })?;

    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".{}.tmp", std::process::id()));
    let temporary = directory.join(temporary_name);

    let mut guard = TempFileGuard {
        path: temporary.clone(),
        armed: true,
    };
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
    {
        Ok(file) => file,
        // A temp file from a crashed run of this very pid (pid reuse) is stale
        // and ours to remove; there is no second writer with this pid.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&temporary)?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?
        }
        Err(error) => return Err(error),
    };

    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);

    // A manifest written `0600` stays `0600`: the replacement must not widen
    // what the author restricted.
    if let Ok(metadata) = std::fs::metadata(path) {
        std::fs::set_permissions(&temporary, metadata.permissions())?;
    }

    std::fs::rename(&temporary, path)?;
    guard.armed = false;

    // Durability, not ordering: a rename that is not followed by a directory
    // fsync can be lost by a crash even though the data is on disk.
    File::open(&directory)?.sync_all()?;
    Ok(())
}

/// Removes the temporary file unless the write got as far as the rename.
struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
