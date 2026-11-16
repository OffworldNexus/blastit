//! Surgical, comment-preserving manifest edits and atomic writes (`R28`,
//! `R29`).
//!
//! Two editors live here while the `Blastfile` cutover is in flight, and they
//! share [`write_atomic`] and the [`EditError`] vocabulary:
//!
//! * [`crate::blastfile::apply_edits`] is the `Blastfile` editor (§14.5): it
//!   splices into the CST of §14.1 and proves the result with
//!   [`Manifest::equivalent`]. It is the editor `--learn` will call once
//!   `MB.21.4.3` moves `dev_learn.rs`, `check.rs` and `audit_v1.rs` off the
//!   retired document;
//! * [`apply_edits`] is the retired TOML patcher, kept alive because those call
//!   sites and their protected suites (`critical_check_engine.rs`,
//!   `critical_check_schema.rs`) still read the retired document. `MB.21.4.3`
//!   flips the call sites and `MB.21.4.5` deletes this function with the
//!   reader.
//!
//! `--learn` has to add views and exact `queries` to a file a person wrote by
//! hand. Comments, ordering and quoting are the author's, so neither editor
//! re-serialises the document: each parses the source, locates the insertion
//! points the parse names, splices whole lines, and then re-parses the result
//! to prove that every field it did not mean to touch is unchanged.
//!
//! Three rules shape the output of both:
//!
//! * a proposed view goes immediately above the first method-unrestricted
//!   catch-all, or at the end of the file. The position comes from the parsed
//!   model, so `view fallback "/*"` and the retired `path = ".*"` are the same
//!   catch-all;
//! * only exact, normalised `queries` are written. A statement that inlines a
//!   literal or still holds a redacted `…` is refused
//!   ([`EditError::LiteralQuery`], [`EditError::RedactedQuery`]): the message
//!   names the position and the fix, never the statement (`R24`);
//! * `body` is never written. [`crate::blastfile::ProposedView`] and
//!   [`crate::blastfile::ProposedGrant`] have no field for it, so "`--learn`
//!   never writes `body raw unvalidated`" is a property of the types rather
//!   than a policy (§10.1, R11).
//!
//! Both are idempotent — applying the same [`ManifestEdits`] to its own output
//! returns that output byte for byte, and an edit with nothing to add returns
//! the source untouched.
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

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::CharIndices;

use blastit_sqlpolicy::{KNOWN_STATIC_SETUP_SQL, has_quoted_literal, normalize};
use serde::{Deserialize, Serialize};

use crate::error::{ManifestErrors, escape_control, quoted};
use crate::model::{Grant, Manifest, PgGrant, ProxyName, Route};

/// One `[[route]]` `--learn` proposes (`R28`).
///
/// The editor writes exactly these two keys: inferring a name from a path and
/// deciding which paths to propose happens before the edit
/// ([`crate::edit::apply_edits`] only writes them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedRoute {
    /// The route's name; `[a-z0-9][a-z0-9_-]*`, unique in the document.
    pub name: String,
    /// The path regexes, matched against the whole of `PATH_INFO`, in order.
    #[serde(default)]
    pub path: Vec<String>,
}

/// Exact `queries` `--learn` proposes for one route on one proxy (`R27`,
/// `R29`).
///
/// The statements arrive as the wire spelled them (`$1`, `$2`, …) and are
/// normalised before they are written; a statement that inlines a literal or
/// holds a redacted `…` is refused instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedGrant {
    /// The `[[route]]` name the grant belongs to.
    pub route: String,
    /// The `[proxy.<name>]` the statements run on; it has to be declared.
    pub proxy: String,
    /// The statements to add, as the wire spelled them.
    #[serde(default)]
    pub queries: Vec<String>,
}

/// Everything the retired TOML patcher wants to add to a manifest (`R28`,
/// `R29`).
///
/// The `Blastfile` editor's own edits type is
/// [`crate::blastfile::BlastfileEdits`]: the two documents need different
/// proposal shapes (a `[[route]]` carries regexes, a `view` carries methods and
/// patterns), and `MB.21.4.3` merges them when it moves the call sites off the
/// retired document. The two share [`write_atomic`] and nothing else: each has
/// its own `ProposedGrant`, because a `[[route]]` grant names a proxy and a
/// `view` grant names a store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEdits {
    /// New `[[route]]` blocks.
    #[serde(default)]
    pub routes: Vec<ProposedRoute>,
    /// Exact statements to add to existing or newly proposed routes.
    #[serde(default)]
    pub grants: Vec<ProposedGrant>,
}

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
    /// A proposed route has no path regex at all.
    EmptyPath {
        /// The route's name.
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
    /// A grant refers to a proxy no `[proxy.<name>]` declares.
    UnknownProxy {
        /// The route the grant names.
        route: String,
        /// The undeclared proxy.
        proxy: String,
    },
    /// A grant names a route that is neither declared nor proposed.
    MissingRoute {
        /// The unknown route name.
        route: String,
        /// The proxy the grant names.
        proxy: String,
    },
    /// A grant is `unrestricted = true`, which cannot sit next to `queries`.
    UnrestrictedGrant {
        /// The route the grant belongs to.
        route: String,
        /// The proxy the grant names.
        proxy: String,
    },
    /// The route's grant is an inline table, so there is no table to patch.
    InlineGrant {
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
            Self::InlineGrant { route, proxy } => write!(
                f,
                "route {}'s grant on {} is an inline table; give it its own `[route.{}]` table so \
                 it can be edited",
                quoted(route),
                quoted(proxy),
                escape_control(proxy)
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

/// Applies `--learn`'s proposals to the **retired TOML** manifest (`R28`,
/// `R29`).
///
/// Retired: [`crate::blastfile::apply_edits`] is the editor `--learn` will
/// call once `MB.21.4.3` moves `dev_learn.rs`, `check.rs` and `audit_v1.rs` off
/// the retired document, and `MB.21.4.5` deletes this function with the reader.
/// Until then it keeps those call sites and their protected suites
/// (`critical_check_engine.rs`, `critical_check_schema.rs`) working, which is
/// why the two editors share [`ManifestEdits`] and [`write_atomic`].
///
/// The source is parsed first, then edited line by line: comments, ordering and
/// quoting outside the edited fields are reproduced byte for byte. New routes
/// are inserted immediately above the first method-unrestricted catch-all, or at
/// the end of the file when there is none; a grant already listed for that
/// route is not added twice. The result is re-parsed and compared with the
/// model the edit intended, so an edit that would change anything else fails
/// with [`EditError::Verification`] and no output. A route's executor blocks
/// (`[route.ingress]`, `[route.static]`, `[route.php]`) travel with it into
/// that postcondition: an edit that cannot reproduce one refuses the write
/// instead of dropping it from the author's file (`R28`, `R29`, `ADR-0010`).
///
/// An edits set with nothing to add returns the source unchanged, which makes
/// repeated learning runs byte-identical.
///
/// # Errors
///
/// [`EditError::Source`] when the document does not parse, every refusal of the
/// retired editor's table, and [`EditError::Verification`] when the
/// postcondition fails.
pub fn apply_edits(source: &str, edits: &ManifestEdits) -> Result<String, EditError> {
    let manifest = Manifest::from_toml_str(source)?;
    let plan = Plan::build(&manifest, edits)?;
    if plan.is_noop() {
        return Ok(source.to_string());
    }

    let edited = render(source, &plan)?;
    let expected = expected_routes(&manifest, &plan);
    verify(&manifest, &expected, &edited)?;
    Ok(edited)
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

/// The statements to write for one route and proxy.
struct GrantTarget {
    proxy: ProxyName,
    /// The complete list to write: existing entries verbatim, then new ones.
    all: Vec<String>,
    /// The entries that are new, so an existing line can be extended in place.
    fresh: Vec<String>,
    /// Whether the existing list is empty, which decides the separating comma.
    was_empty: bool,
}

/// A grant to merge into a route the document already declares.
struct Patch {
    route_index: usize,
    route: String,
    /// Whether the model already has a grant the text does not spell as a table.
    inline: bool,
    grant: GrantTarget,
}

/// A `[[route]]` block to insert.
struct NewRoute {
    name: String,
    path: Vec<String>,
    grants: Vec<GrantTarget>,
}

/// Everything an edits set resolves to against one manifest.
struct Plan {
    insert_at: usize,
    new_routes: Vec<NewRoute>,
    patches: Vec<Patch>,
}

impl Plan {
    fn is_noop(&self) -> bool {
        self.new_routes.is_empty() && self.patches.is_empty()
    }

    fn build(manifest: &Manifest, edits: &ManifestEdits) -> Result<Self, EditError> {
        let mut new_routes: Vec<NewRoute> = Vec::new();
        let mut names: BTreeSet<&str> = BTreeSet::new();

        for proposed in &edits.routes {
            if !valid_route_name(&proposed.name) {
                return Err(EditError::InvalidRouteName {
                    name: proposed.name.clone(),
                });
            }
            if !names.insert(proposed.name.as_str()) {
                return Err(EditError::DuplicateProposedRoute {
                    name: proposed.name.clone(),
                });
            }
            if proposed.path.is_empty() {
                return Err(EditError::EmptyPath {
                    name: proposed.name.clone(),
                });
            }
            for (rule, pattern) in proposed.path.iter().enumerate() {
                if pattern.is_empty() {
                    return Err(EditError::EmptyPattern {
                        name: proposed.name.clone(),
                        rule,
                    });
                }
                if regex::Regex::new(pattern).is_err() {
                    return Err(EditError::InvalidPattern {
                        name: proposed.name.clone(),
                        rule,
                    });
                }
            }

            match route_index(manifest, &proposed.name) {
                Some(index) => {
                    let existing_path = manifest.routes().get(index).map(Route::path);
                    if existing_path != Some(proposed.path.as_slice()) {
                        return Err(EditError::RouteConflict {
                            name: proposed.name.clone(),
                        });
                    }
                }
                None => new_routes.push(NewRoute {
                    name: proposed.name.clone(),
                    path: proposed.path.clone(),
                    grants: Vec::new(),
                }),
            }
        }

        let mut patches: Vec<Patch> = Vec::new();
        for proposed in &edits.grants {
            let proxy = ProxyName::from(proposed.proxy.as_str());
            if manifest.proxy(&proposed.proxy).is_none() {
                return Err(EditError::UnknownProxy {
                    route: proposed.route.clone(),
                    proxy: proposed.proxy.clone(),
                });
            }
            let fresh = normalise_queries(&proposed.route, &proposed.proxy, &proposed.queries)?;
            if fresh.is_empty() {
                continue;
            }

            if let Some(route) = new_routes
                .iter_mut()
                .find(|route| route.name == proposed.route)
            {
                merge_new_grant(&mut route.grants, proxy, fresh);
                continue;
            }

            let Some(index) = route_index(manifest, &proposed.route) else {
                return Err(EditError::MissingRoute {
                    route: proposed.route.clone(),
                    proxy: proposed.proxy.clone(),
                });
            };
            let existing = manifest
                .routes()
                .get(index)
                .and_then(|route| route.grant(&proxy))
                .and_then(Grant::as_postgresql);
            if existing.is_some_and(PgGrant::is_unrestricted) {
                return Err(EditError::UnrestrictedGrant {
                    route: proposed.route.clone(),
                    proxy: proposed.proxy.clone(),
                });
            }

            let written: Vec<String> = existing
                .map(|grant| grant.queries().to_vec())
                .unwrap_or_default();
            let already: BTreeSet<String> = written.iter().map(|query| normalize(query)).collect();
            let fresh: Vec<String> = fresh
                .into_iter()
                .filter(|query| !already.contains(query))
                .collect();
            if fresh.is_empty() {
                continue;
            }

            let was_empty = written.is_empty();
            let mut all = written;
            all.extend(fresh.iter().cloned());
            patches.push(Patch {
                route_index: index,
                route: proposed.route.clone(),
                inline: existing.is_some(),
                grant: GrantTarget {
                    proxy,
                    all,
                    fresh,
                    was_empty,
                },
            });
        }

        let insert_at = manifest
            .routes()
            .iter()
            .position(Route::is_unrestricted_catch_all)
            .unwrap_or_else(|| manifest.route_count());

        Ok(Self {
            insert_at,
            new_routes,
            patches,
        })
    }
}

/// Adds a grant to a route the edit creates, merging a repeated proposal.
fn merge_new_grant(grants: &mut Vec<GrantTarget>, proxy: ProxyName, fresh: Vec<String>) {
    match grants.iter_mut().find(|target| target.proxy == proxy) {
        Some(target) => {
            for query in fresh {
                if !target.all.contains(&query) {
                    target.all.push(query.clone());
                    target.fresh.push(query);
                }
            }
        }
        None => grants.push(GrantTarget {
            proxy,
            all: fresh.clone(),
            fresh,
            was_empty: true,
        }),
    }
}

/// Normalises proposed statements and refuses the two kinds a grant can never
/// hold (`R24`), without putting either in the message.
fn normalise_queries(
    route: &str,
    proxy: &str,
    queries: &[String],
) -> Result<Vec<String>, EditError> {
    let mut out: Vec<String> = Vec::new();
    for (index, query) in queries.iter().enumerate() {
        let normalized = normalize(query);
        if normalized.is_empty() {
            return Err(EditError::EmptyQuery {
                route: route.to_string(),
                proxy: proxy.to_string(),
                index,
            });
        }
        if normalized.contains('…') {
            return Err(EditError::RedactedQuery {
                route: route.to_string(),
                proxy: proxy.to_string(),
                index,
            });
        }
        if has_quoted_literal(&normalized) && !KNOWN_STATIC_SETUP_SQL.contains(&normalized.as_str())
        {
            return Err(EditError::LiteralQuery {
                route: route.to_string(),
                proxy: proxy.to_string(),
                index,
            });
        }
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

/// The index of a declared route, by name.
fn route_index(manifest: &Manifest, name: &str) -> Option<usize> {
    manifest
        .routes()
        .iter()
        .position(|route| route.name() == name)
}

/// A route name is `[a-z0-9][a-z0-9_-]*` (the same rule [`crate::validate`]
/// enforces at boot; refusing it here names the inference that went wrong).
fn valid_route_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        None => false,
        Some(first) => {
            (first.is_ascii_lowercase() || first.is_ascii_digit())
                && bytes.all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        }
    }
}

/// Produces the edited document, splicing only whole lines.
fn render(source: &str, plan: &Plan) -> Result<String, EditError> {
    let mut lines: Vec<String> = source.split_inclusive('\n').map(str::to_string).collect();

    for patch in &plan.patches {
        patch_grant(&mut lines, patch)?;
    }

    if plan.new_routes.is_empty() {
        return Ok(lines.concat());
    }

    let block = render_routes(&plan.new_routes);
    let mut insertion = block;
    let route_count = route_starts(&scan_lines(&lines)).len();
    if plan.insert_at < route_count {
        let Some(at) = route_start_line(&lines, plan.insert_at) else {
            return Err(EditError::Verification {
                section: "the route order",
            });
        };
        // A comment written directly above the catch-all still documents it:
        // the new block goes above the comment, not between the two. One
        // bounded pass over the lines above the header finds where that run of
        // comments starts, so there is no cursor to step backwards.
        let at = lines
            .get(..at)
            .unwrap_or_default()
            .iter()
            .rposition(|line| !line.trim_start().starts_with('#'))
            .map_or(0, |last| last + 1);
        if at > 0 && lines.get(at - 1).is_some_and(|line| !is_blank(line)) {
            insertion.insert(0, String::from("\n"));
        }
        if lines.get(at).is_some_and(|line| !is_blank(line)) {
            insertion.push(String::from("\n"));
        }
        lines.splice(at..at, insertion);
    } else {
        if let Some(last) = lines.last_mut()
            && !last.ends_with('\n')
        {
            last.push('\n');
        }
        if lines.last().is_some_and(|line| !is_blank(line)) {
            lines.push(String::from("\n"));
        }
        lines.extend(insertion);
    }

    Ok(lines.concat())
}

/// The lines of one `[[route]]` block, each ending in a newline.
fn render_routes(routes: &[NewRoute]) -> Vec<String> {
    let mut out = Vec::new();
    for (index, route) in routes.iter().enumerate() {
        if index > 0 {
            out.push(String::from("\n"));
        }
        out.push(String::from("[[route]]\n"));
        out.push(format!("name = {}\n", toml_basic_string(&route.name)));
        out.push(format!("path = {}\n", toml_string_array(&route.path)));
        for target in &route.grants {
            out.push(String::from("\n"));
            out.push(format!("[route.{}]\n", toml_key(target.proxy.as_str())));
            out.push(format!("queries = {}\n", toml_string_array(&target.all)));
        }
    }
    out
}

/// Merges `--learn`'s statements into one route's grant, in place.
fn patch_grant(lines: &mut Vec<String>, patch: &Patch) -> Result<(), EditError> {
    let infos = scan_lines(lines);
    let Some(&start) = route_starts(&infos).get(patch.route_index) else {
        return Ok(());
    };
    let end = route_end(&infos, start);
    let element_end = element_end(&infos, start);
    let expected = format!("route.{}", toml_key(patch.grant.proxy.as_str()));

    let header_line = (start + 1..element_end).find(|&index| {
        infos
            .get(index)
            .and_then(|info| info.header.as_ref())
            .is_some_and(|header| !header.array && header_matches(&header.name, &expected))
    });

    let Some(header_line) = header_line else {
        if patch.inline {
            return Err(EditError::InlineGrant {
                route: patch.route.clone(),
                proxy: patch.grant.proxy.to_string(),
            });
        }
        append_grant_table(lines, start, end, patch);
        return Ok(());
    };

    let section_end = (header_line + 1..element_end)
        .find(|&index| infos.get(index).is_some_and(|info| info.header.is_some()))
        .unwrap_or(element_end);
    let assignment = (header_line + 1..section_end).find(|&index| {
        infos.get(index).is_some_and(|info| {
            info.depth_start == 0
                && !info.in_string_at_start
                && lines
                    .get(index)
                    .is_some_and(|line| is_queries_assignment(line))
        })
    });

    let Some(assignment) = assignment else {
        if let Some(header) = lines.get_mut(header_line)
            && !header.ends_with('\n')
        {
            header.push('\n');
        }
        let indent = lines
            .get(header_line)
            .map_or_else(String::new, |line| indent_of(line).to_string());
        let line = format!(
            "{indent}queries = {}\n",
            toml_string_array(&patch.grant.all)
        );
        lines.insert(header_line + 1, line);
        return Ok(());
    };

    let one_line = infos
        .get(assignment)
        .is_some_and(|info| info.depth_end == 0);
    if one_line
        && let Some(line) = lines.get(assignment).cloned()
        && let Some(extended) = append_to_array(&line, &patch.grant.fresh, patch.grant.was_empty)
    {
        if let Some(slot) = lines.get_mut(assignment) {
            *slot = extended;
        }
        return Ok(());
    }

    // The value continues while the line before the cursor left a bracket
    // open. Scanning the candidate ends in one bounded pass keeps the stop
    // position a function of `section_end`, never of an accumulating step.
    let stop = (assignment + 1..section_end)
        .find(|&index| infos.get(index - 1).is_none_or(|info| info.depth_end == 0))
        .unwrap_or(section_end);
    let had_newline = lines.get(stop - 1).is_some_and(|line| line.ends_with('\n'));
    let indent = lines
        .get(assignment)
        .map_or_else(String::new, |line| indent_of(line).to_string());
    let comment = if one_line {
        lines
            .get(assignment)
            .and_then(|line| trailing_comment(line))
            .unwrap_or_default()
            .to_string()
    } else {
        String::new()
    };
    let mut replacement = format!(
        "{indent}queries = {}{comment}",
        toml_string_array(&patch.grant.all)
    );
    if had_newline && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    if !had_newline && replacement.ends_with('\n') {
        replacement.pop();
    }
    lines.splice(assignment..stop, std::iter::once(replacement));
    Ok(())
}

/// Appends a whole `[route.<proxy>]` table at the end of one route's block.
fn append_grant_table(lines: &mut Vec<String>, start: usize, end: usize, patch: &Patch) {
    // The blank run that closes the route's own block; found in one pass over
    // `start..end` so the cursor cannot step backwards.
    let block_end = lines
        .get(start..end)
        .and_then(|block| block.iter().rposition(|line| !is_blank(line)))
        .map_or(start, |last| start + last + 1);
    if let Some(previous) = block_end
        .checked_sub(1)
        .and_then(|index| lines.get_mut(index))
        && !previous.ends_with('\n')
    {
        previous.push('\n');
    }

    let table = format!("[route.{}]\n", toml_key(patch.grant.proxy.as_str()));
    let values = format!("queries = {}\n", toml_string_array(&patch.grant.all));
    lines.splice(block_end..block_end, [String::from("\n"), table, values]);
}

/// The manifest the edit intends to produce, used as the postcondition.
fn expected_routes(manifest: &Manifest, plan: &Plan) -> Vec<Route> {
    let mut routes = manifest.routes().to_vec();

    for patch in &plan.patches {
        let Some(route) = routes.get(patch.route_index).cloned() else {
            continue;
        };
        let mut grants = route.grants().clone();
        let mut grant = grants
            .get(&patch.grant.proxy)
            .cloned()
            .unwrap_or_else(|| Grant::Postgresql(PgGrant::new(Vec::new(), false)));
        grant.merge_from(&Grant::Postgresql(PgGrant::new(
            patch.grant.all.clone(),
            false,
        )));
        grants.insert(patch.grant.proxy.clone(), grant);
        let rebuilt = Route::new(
            route.name().to_string(),
            route.path().to_vec(),
            route.methods().map(<[String]>::to_vec),
            route.groups().to_vec(),
            route.egress(),
            route.prefork(),
            grants,
        )
        // Every executor key the route carries travels with it. The edit only
        // ever splices a `queries` entry, so an `[route.static]` or
        // `[route.php]` block it cannot reproduce must make verification fail
        // rather than vanish from the postcondition (`R28`, `R29`). The sandbox
        // travels too: a rebuilt route that claimed sandbox `0` would compare
        // unequal to the parsed one and refuse every edit (`MB.3.21`).
        .with_static(route.static_policy().cloned())
        .with_php(route.php().cloned())
        .with_ingress(route.ingress())
        .with_sandbox(route.sandbox());
        if let Some(slot) = routes.get_mut(patch.route_index) {
            *slot = rebuilt;
        }
    }

    let insert_at = plan.insert_at.min(routes.len());
    let mut built: Vec<Route> = routes.drain(..insert_at).collect();
    for new in &plan.new_routes {
        let mut grants: BTreeMap<ProxyName, Grant> = BTreeMap::new();
        for target in &new.grants {
            grants.insert(
                target.proxy.clone(),
                Grant::Postgresql(PgGrant::new(target.all.clone(), false)),
            );
        }
        built.push(Route::new(
            new.name.clone(),
            new.path.clone(),
            None,
            Vec::new(),
            false,
            false,
            grants,
        ));
    }
    built.extend(routes);
    // The retired reader numbers one sandbox per route, so an insertion
    // renumbers every route below it; the postcondition carries the numbers the
    // re-parsed document will have (§7.3).
    built
        .into_iter()
        .enumerate()
        .map(|(index, route)| route.with_sandbox(u32::try_from(index).unwrap_or(u32::MAX)))
        .collect()
}

/// Re-parses the edit and proves nothing outside the edit moved.
fn verify(manifest: &Manifest, expected: &[Route], text: &str) -> Result<(), EditError> {
    let reparsed = Manifest::from_toml_str(text).map_err(|_| EditError::Verification {
        section: "the document syntax",
    })?;

    if reparsed.version() != manifest.version()
        || reparsed.app() != manifest.app()
        || reparsed.name() != manifest.name()
        || reparsed.preload() != manifest.preload()
        || reparsed.global_groups() != manifest.global_groups()
        || reparsed.php() != manifest.php()
    {
        return Err(EditError::Verification {
            section: "the top-level keys",
        });
    }
    if reparsed.proxies() != manifest.proxies() {
        return Err(EditError::Verification {
            section: "the proxy declarations",
        });
    }
    if reparsed.groups() != manifest.groups() {
        return Err(EditError::Verification {
            section: "the groups",
        });
    }
    if reparsed.routes() != expected {
        return Err(EditError::Verification {
            section: "the routes",
        });
    }
    // The mounts and the front controller are what the routes lower to, so a
    // postcondition that kept the routes but lost a mount would still be a
    // rewrite. `MB.21.3.5` replaces this whole function with one
    // `Manifest::equivalent` call.
    if reparsed.mounts() != manifest.mounts() {
        return Err(EditError::Verification {
            section: "the routes",
        });
    }
    Ok(())
}

//
// Structural line scanning. The document is parsed before any of this runs, so
// the scanner only has to answer one question per line: does a table header
// start here, and at what bracket depth does the line end? Strings, comments,
// and multi-line strings are tracked so a `[[route]]` inside a `queries` entry
// is never mistaken for a route.
//

/// Which multi-line string a line is inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Multi {
    Basic,
    Literal,
}

impl Multi {
    /// The quote that opens and closes this kind of string.
    fn quote(self) -> char {
        match self {
            Self::Basic => '"',
            Self::Literal => '\'',
        }
    }
}

/// A table header at the start of a line.
struct Header {
    /// The dotted name, trimmed: `route`, `route.db`, `proxy.app`.
    name: String,
    /// True for `[[name]]`.
    array: bool,
}

/// What the scanner knows about one line.
struct LineInfo {
    header: Option<Header>,
    depth_start: usize,
    depth_end: usize,
    in_string_at_start: bool,
}

/// Scans one document into structural line information.
fn scan_lines(lines: &[String]) -> Vec<LineInfo> {
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    scan(&refs)
}

fn scan(lines: &[&str]) -> Vec<LineInfo> {
    let mut infos = Vec::with_capacity(lines.len());
    let mut multi: Option<Multi> = None;
    let mut depth = 0usize;

    for line in lines {
        let depth_start = depth;
        let in_string_at_start = multi.is_some();
        let mut header = None;

        if multi.is_none()
            && depth == 0
            && let Some(found) = parse_header(line)
        {
            header = Some(found);
        }

        if header.is_none() {
            let (next_multi, next_depth) = walk(line, multi, depth);
            multi = next_multi;
            depth = next_depth;
        }

        infos.push(LineInfo {
            header,
            depth_start,
            depth_end: depth,
            in_string_at_start,
        });
    }

    infos
}

/// Reads one line, returning the multi-line-string state and bracket depth at
/// its end.
///
/// The line is consumed by a character iterator, so every iteration reads the
/// next character and the scan cannot stall on a cursor that fails to move.
fn walk(line: &str, mut multi: Option<Multi>, mut depth: usize) -> (Option<Multi>, usize) {
    let mut chars = line.char_indices().peekable();

    while let Some((_, character)) = chars.next() {
        if let Some(kind) = multi {
            if kind == Multi::Basic && character == '\\' {
                // The escaped character belongs to the string either way.
                chars.next();
                continue;
            }
            if character == kind.quote() && tripled(&chars, character) {
                chars.next();
                chars.next();
                multi = None;
            }
            continue;
        }

        match character {
            '#' => break,
            '"' | '\'' if tripled(&chars, character) => {
                chars.next();
                chars.next();
                multi = Some(if character == '"' {
                    Multi::Basic
                } else {
                    Multi::Literal
                });
            }
            // A single-line string: an unescaped quote of the same kind ends it.
            '"' => skip_string(&mut chars, '"'),
            '\'' => skip_string(&mut chars, '\''),
            '[' | '{' => depth = depth.saturating_add(1),
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    (multi, depth)
}

/// Whether the two characters after an already-read quote are that same quote,
/// which makes three and opens or closes a multi-line string.
///
/// The look-ahead works on a clone of the iterator, so it never moves the
/// scanner: termination depends only on the `next()` calls the caller makes.
fn tripled<I>(chars: &I, quote: char) -> bool
where
    I: Iterator<Item = (usize, char)> + Clone,
{
    let mut look = chars.clone();
    look.next().is_some_and(|(_, second)| second == quote)
        && look.next().is_some_and(|(_, third)| third == quote)
}

/// Consumes the rest of a string value a quote opened, so the scanner never
/// mistakes a `#` or a bracket inside the value for structure.
fn skip_string(chars: &mut Peekable<CharIndices<'_>>, quote: char) {
    while let Some((_, character)) = chars.next() {
        if quote == '"' && character == '\\' {
            chars.next();
            continue;
        }
        if character == quote {
            return;
        }
    }
}

/// Parses a table header that starts the line, if there is one.
fn parse_header(line: &str) -> Option<Header> {
    let trimmed = line.trim_start();
    let after_open = trimmed.strip_prefix('[')?;
    let (array, body) = match after_open.strip_prefix('[') {
        Some(rest) => (true, rest),
        None => (false, after_open),
    };
    let close = if array { "]]" } else { "]" };

    // Every non-overlapping position of the closing token, in order; the one
    // followed only by a comment (or nothing) closes the header. `match_indices`
    // bounds the scan, so no search offset can fail to advance.
    let end = body.match_indices(close).map(|(at, _)| at).find(|&at| {
        body.get(at + close.len()..)
            .map(str::trim_start)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('#'))
    })?;
    let name = body.get(..end)?.trim();
    if name.is_empty() {
        return None;
    }
    Some(Header {
        name: name.to_string(),
        array,
    })
}

/// The line indices where a `[[route]]` element starts.
fn route_starts(infos: &[LineInfo]) -> Vec<usize> {
    infos
        .iter()
        .enumerate()
        .filter_map(|(index, info)| {
            let header = info.header.as_ref()?;
            (header.array && header.name == "route").then_some(index)
        })
        .collect()
}

/// The first line of the `index`th `[[route]]`.
fn route_start_line(lines: &[String], index: usize) -> Option<usize> {
    route_starts(&scan_lines(lines)).get(index).copied()
}

/// The line where one route's own block ends: the next `[[route]]`, the next
/// table outside `route.*`, or the end of the document.
fn route_end(infos: &[LineInfo], start: usize) -> usize {
    for (index, info) in infos.iter().enumerate().skip(start + 1) {
        if let Some(header) = &info.header
            && (header.array || header.name.split('.').next() != Some("route"))
        {
            return index;
        }
    }
    infos.len()
}

/// The line where the `[[route]]` element holding `start` ends. A
/// `[route.<proxy>]` table may legally sit after another table and still belong
/// to this element, so grant lookup uses this bound rather than [`route_end`].
fn element_end(infos: &[LineInfo], start: usize) -> usize {
    for (index, info) in infos.iter().enumerate().skip(start + 1) {
        if info
            .header
            .as_ref()
            .is_some_and(|header| header.array && header.name == "route")
        {
            return index;
        }
    }
    infos.len()
}

/// Whether a header names the table the edit is looking for, tolerating
/// whitespace inside the brackets and quoted proxy names.
fn header_matches(name: &str, expected: &str) -> bool {
    name == expected || simplify_header(name) == simplify_header(expected)
}

fn simplify_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '"' && *character != '\'')
        .collect()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn indent_of(line: &str) -> &str {
    let trimmed = line.trim_start();
    line.get(..line.len() - trimmed.len()).unwrap_or("")
}

/// Whether the line is `queries = …` (and not `queries_extra = …`).
fn is_queries_assignment(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("queries") else {
        return false;
    };
    rest.trim_start().starts_with('=')
}

/// The trailing `# …` comment of a single-line value, with the whitespace that
/// preceded it, so an edited line keeps the author's note.
///
/// The line is consumed by a character iterator: every iteration reads the next
/// character, so a scanner that stops stepping is not representable here.
fn trailing_comment(line: &str) -> Option<&str> {
    let mut chars = line.char_indices().peekable();
    let mut in_basic = false;
    let mut in_literal = false;

    while let Some((index, character)) = chars.next() {
        if in_basic {
            if character == '\\' {
                chars.next();
                continue;
            }
            if character == '"' {
                in_basic = false;
            }
            continue;
        }
        if in_literal {
            if character == '\'' {
                in_literal = false;
            }
            continue;
        }
        match character {
            // A multi-line string is not a single-line value, so there is no
            // trailing comment on this line to keep.
            '"' if tripled(&chars, '"') => return None,
            '\'' if tripled(&chars, '\'') => return None,
            '"' => {
                in_basic = true;
            }
            '\'' => {
                in_literal = true;
            }
            '#' => {
                let head = line.get(..index)?;
                let start = head.trim_end_matches([' ', '\t']).len();
                return line.get(start..);
            }
            _ => {}
        }
    }
    None
}

/// The byte index of the `]` that closes the array value on this line.
///
/// Like [`trailing_comment`], the line is consumed by a character iterator, so
/// the scan is bounded by the line itself rather than by a cursor's arithmetic.
fn closing_bracket(line: &str) -> Option<usize> {
    let mut chars = line.char_indices().peekable();
    let mut depth = 0usize;
    let mut in_basic = false;
    let mut in_literal = false;
    let mut opened = false;

    while let Some((index, character)) = chars.next() {
        if in_basic {
            if character == '\\' {
                chars.next();
                continue;
            }
            if character == '"' {
                in_basic = false;
            }
            continue;
        }
        if in_literal {
            if character == '\'' {
                in_literal = false;
            }
            continue;
        }
        match character {
            '#' => break,
            '"' if tripled(&chars, '"') => return None,
            '\'' if tripled(&chars, '\'') => return None,
            '"' => {
                in_basic = true;
            }
            '\'' => {
                in_literal = true;
            }
            '[' => {
                depth = depth.saturating_add(1);
                opened = true;
            }
            ']' => {
                if !opened {
                    return None;
                }
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extends a one-line `queries = [...]` value with `entries`, leaving every
/// existing byte (and its quoting) where the author put it.
///
/// Only the run of spaces and tabs just before the closing bracket is the
/// editor's: the separator decides it, so a list the author ended with a comma
/// gets one space rather than two.
fn append_to_array(line: &str, entries: &[String], was_empty: bool) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let close = closing_bracket(line)?;
    let before = line.get(..close)?.trim_end_matches([' ', '\t']);
    let separator = if was_empty {
        ""
    } else if before.ends_with(',') {
        " "
    } else {
        ", "
    };
    let payload = entries
        .iter()
        .map(|entry| toml_string(entry))
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::with_capacity(line.len() + separator.len() + payload.len());
    out.push_str(before);
    out.push_str(separator);
    out.push_str(&payload);
    out.push_str(line.get(close..)?);
    Some(out)
}

/// Renders a list of strings as a one-line TOML array.
fn toml_string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&toml_string(value));
    }
    out.push(']');
    out
}

/// Renders one string, preferring a literal string so a SQL statement keeps its
/// `"quoted"` identifiers without backslashes.
fn toml_string(value: &str) -> String {
    let literal = !value.contains('\'')
        && value
            .chars()
            .all(|character| character == '\t' || !character.is_control());
    if literal {
        format!("'{value}'")
    } else {
        toml_basic_string(value)
    }
}

/// Renders a table-key segment: bare when TOML allows it, quoted otherwise.
fn toml_key(name: &str) -> String {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        name.to_string()
    } else {
        toml_basic_string(name)
    }
}

fn toml_basic_string(value: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(character));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    //
    // `verify` is the editor's own postcondition: it re-parses the text the
    // splice produced and refuses to return it when anything it does not own
    // moved. Each comparison has its own section name, so each one is checked
    // here with a document that only that comparison can reject.
    //

    /// A document that has something for every section `verify` compares: an
    /// `app`, a `preload`, a global group, a declared proxy, a group grant and
    /// one route.
    const VERIFIED: &str = "\
version = 1
app = \"shop.wsgi:application\"
preload = [\"shop.boot\"]
groups = [\"shared\"]

[proxy.db]
protocol = \"postgresql\"

[group.shared.db]
queries = []

[[route]]
name = \"products\"
path = ['/products/']
groups = [\"shared\"]
";

    fn verified() -> (Manifest, Vec<Route>) {
        let manifest = Manifest::from_toml_str(VERIFIED).expect("the fixture parses");
        let expected = manifest.routes().to_vec();
        (manifest, expected)
    }

    /// The section a refusal names, or a panic naming what happened instead.
    fn refusal(result: Result<(), EditError>) -> &'static str {
        match result {
            Err(EditError::Verification { section }) => section,
            other => panic!("expected a verification refusal, got {other:?}"),
        }
    }

    #[test]
    fn verify_accepts_the_document_it_was_handed() {
        let (manifest, expected) = verified();
        assert_eq!(verify(&manifest, &expected, VERIFIED), Ok(()));
    }

    #[test]
    fn a_parse_failure_stays_reachable_as_the_error_source() {
        use std::error::Error as _;

        let source_error = Manifest::from_toml_str("version = 1\nthis is not toml\n")
            .expect_err("the text does not parse");
        let edit_error = EditError::from(source_error);

        assert!(matches!(edit_error, EditError::Source(_)), "{edit_error:?}");
        assert!(
            edit_error.source().is_some(),
            "the parse failure is not reachable through `source`"
        );
        // A refusal that is the editor's own has no cause to hand out.
        assert!(
            EditError::Verification {
                section: "the routes"
            }
            .source()
            .is_none()
        );
    }

    #[test]
    fn verify_names_the_document_syntax() {
        let (manifest, expected) = verified();
        assert_eq!(
            refusal(verify(&manifest, &expected, "version = 1\n[[route]\n")),
            "the document syntax"
        );
    }

    #[test]
    fn verify_names_each_top_level_key_it_would_have_changed() {
        let (manifest, expected) = verified();

        // A manifest whose version differs can only be built by hand: a parsed
        // document always reports `1`.
        let version_two = Manifest::assemble(
            2,
            manifest.global_groups().to_vec(),
            manifest.proxies().clone(),
            manifest.groups().clone(),
            expected.clone(),
            Vec::new(),
        );
        assert_eq!(
            refusal(verify(&version_two, &expected, VERIFIED)),
            "the top-level keys",
            "a changed version is not refused"
        );

        let changed_app = VERIFIED.replace("shop.wsgi:application", "shop.wsgi:other");
        assert_ne!(changed_app, VERIFIED);
        assert_eq!(
            refusal(verify(&manifest, &expected, &changed_app)),
            "the top-level keys",
            "a changed app is not refused"
        );

        let changed_preload = VERIFIED.replacen("[\"shop.boot\"]", "[]", 1);
        assert_ne!(changed_preload, VERIFIED);
        assert_eq!(
            refusal(verify(&manifest, &expected, &changed_preload)),
            "the top-level keys",
            "a changed preload is not refused"
        );

        let changed_groups = VERIFIED.replacen("groups = [\"shared\"]\n", "", 1);
        assert_ne!(changed_groups, VERIFIED);
        assert_eq!(
            refusal(verify(&manifest, &expected, &changed_groups)),
            "the top-level keys",
            "a changed global group list is not refused"
        );
    }

    #[test]
    fn verify_names_the_proxy_declarations() {
        let (manifest, expected) = verified();
        let changed = VERIFIED.replace(
            "[proxy.db]\nprotocol = \"postgresql\"",
            "[proxy.db]\nprotocol = \"postgresql\"\noutput = { id = \"ID\" }",
        );
        assert_ne!(changed, VERIFIED);

        assert_eq!(
            refusal(verify(&manifest, &expected, &changed)),
            "the proxy declarations"
        );
    }

    #[test]
    fn verify_names_the_groups() {
        let (manifest, expected) = verified();
        let changed = VERIFIED.replace(
            "[group.shared.db]\nqueries = []",
            "[group.shared.db]\nqueries = ['SELECT \"t\".\"a\" FROM \"t\"']",
        );
        assert_ne!(changed, VERIFIED);

        assert_eq!(
            refusal(verify(&manifest, &expected, &changed)),
            "the groups"
        );
    }

    #[test]
    fn verify_names_the_routes() {
        let (manifest, expected) = verified();
        let changed = VERIFIED.replace("name = \"products\"", "name = \"catalogue\"");
        assert_ne!(changed, VERIFIED);

        assert_eq!(
            refusal(verify(&manifest, &expected, &changed)),
            "the routes"
        );
    }

    //
    // The structural scanner. A line's bracket depth and string state decide
    // which headers the patcher can see, so each state is pinned.
    //

    #[test]
    fn scan_tracks_bracket_depth_across_lines() {
        let infos = scan(&["queries = [\n", "  'SELECT 1',\n", "]\n", "name = 'x'\n"]);

        assert_eq!(infos.len(), 4);
        assert_eq!((infos[0].depth_start, infos[0].depth_end), (0, 1));
        assert!(!infos[0].in_string_at_start);
        assert_eq!((infos[1].depth_start, infos[1].depth_end), (1, 1));
        assert_eq!((infos[2].depth_start, infos[2].depth_end), (1, 0));
        assert_eq!((infos[3].depth_start, infos[3].depth_end), (0, 0));
        assert!(infos.iter().all(|info| info.header.is_none()));

        // A header-shaped line inside an open value is not a header.
        let inside = scan(&["queries = [\n", "[route.db]\n"]);
        assert!(inside[1].depth_start > 0);
        assert!(inside[1].header.is_none());
    }

    #[test]
    fn scan_ignores_brackets_and_headers_inside_strings_and_comments() {
        // A `[` in a comment, a `]` in a trailing comment, and a header-shaped
        // string all leave the depth where it was.
        let infos = scan(&[
            "# a comment with [ an open bracket\n",
            "name = 'x'  # closed with ]\n",
            "queries = ['SELECT \"t\".\"a\" FROM \"t\" WHERE \"t\".\"b\" = \"[[']\n",
        ]);
        assert_eq!(infos[0].depth_end, 0);
        assert_eq!(infos[1].depth_end, 0);
        assert_eq!(infos[2].depth_end, 0);
        assert!(infos.iter().all(|info| info.header.is_none()));
    }

    #[test]
    fn a_multi_line_string_hides_the_headers_and_brackets_it_contains() {
        let infos = scan(&[
            "description = '''\n",
            "[[route]]\n",
            "  SELECT [ x ] FROM y\n",
            "'''\n",
            "[[route]]\n",
        ]);

        assert!(infos[1].in_string_at_start, "the string did not stay open");
        assert!(infos[1].header.is_none(), "a header inside a string");
        assert_eq!(infos[2].depth_end, 0, "a bracket inside a string");
        assert!(!infos[4].in_string_at_start);
        let header = infos[4].header.as_ref().expect("the header after it");
        assert!(header.array);
        assert_eq!(header.name, "route");
    }

    #[test]
    fn walk_reads_escapes_strings_and_brackets() {
        // Nothing is open at the end of any of these lines.
        assert_eq!(walk("x = \"[\"", None, 0), (None, 0));
        assert_eq!(walk("x = '['", None, 0), (None, 0));
        assert_eq!(walk("x = \"a\\\"[\"", None, 0), (None, 0));
        assert_eq!(walk("x = \"a\\\\\"", None, 0), (None, 0));
        // Depth is counted outside strings, and never underflows.
        assert_eq!(walk("x = [1, {2}] ", None, 0), (None, 0));
        assert_eq!(walk("x = [", None, 0), (None, 1));
        assert_eq!(walk("x = ]", None, 0), (None, 0));
        assert_eq!(walk("x = ]", None, 0).1, 0);
        // An `[[route]]` inside a multi-line string does not open a header.
        assert_eq!(
            walk("[[route]]", Some(Multi::Literal), 0),
            (Some(Multi::Literal), 0)
        );
        // A `#` inside a string is not a comment: the bracket after it counts.
        assert_eq!(walk("x = '#' [", None, 0), (None, 1));
        assert_eq!(walk("x = \"#\" [", None, 0), (None, 1));
        // An unterminated one-line string swallows the rest of the line.
        assert_eq!(walk("x = '[' ", None, 0), (None, 0));
        // A string ends at its own quote, so what follows is structure again.
        assert_eq!(walk("x = 'a' [", None, 0), (None, 1));
        assert_eq!(walk("x = \"a[\" [", None, 0), (None, 1));
        // A backslash escapes only inside a basic string: in a literal one it
        // is a character like any other, and the quote after it still closes.
        assert_eq!(walk("x = 'a\\'b", None, 0), (None, 0));
    }

    #[test]
    fn multi_line_delimiters_open_and_close() {
        assert_eq!(walk("x = '''", None, 0), (Some(Multi::Literal), 0));
        assert_eq!(walk("x = \"\"\"", None, 0), (Some(Multi::Basic), 0));
        // Two quotes are an empty one-line string, not a delimiter.
        assert_eq!(walk("x = ''", None, 0), (None, 0));
        assert_eq!(walk("x = \"\"", None, 0), (None, 0));
        // The closing delimiter of the other kind does not close this string.
        assert_eq!(
            walk("''' still here", Some(Multi::Basic), 0),
            (Some(Multi::Basic), 0)
        );
        assert_eq!(walk("'''", Some(Multi::Literal), 0), (None, 0));
        assert_eq!(walk("\"\"\"", Some(Multi::Basic), 0), (None, 0));
        // Two quotes inside a multi-line string are content, not a delimiter.
        assert_eq!(
            walk("x''", Some(Multi::Literal), 0),
            (Some(Multi::Literal), 0)
        );
        assert_eq!(
            walk("abc\"\"\"", Some(Multi::Basic), 0),
            (None, 0),
            "content before the delimiter"
        );
        // An escape inside a basic string hides the delimiter after it.
        assert_eq!(
            walk("\\\"\"\" still here", Some(Multi::Basic), 0),
            (Some(Multi::Basic), 0)
        );
        // A backslash in a literal string escapes nothing.
        assert_eq!(walk("\\'''", Some(Multi::Literal), 0), (None, 0));
    }

    #[test]
    fn header_matching_tolerates_spacing_and_quoted_keys() {
        assert!(header_matches("route.db", "route.db"));
        assert!(header_matches("route . db", "route.db"));
        assert!(header_matches("route.\"db\"", "route.db"));
        assert!(header_matches("route.'db'", "route.db"));
        assert!(!header_matches("route.dx", "route.db"));
        assert!(!header_matches("proxy.db", "route.db"));
        assert_eq!(simplify_header(" route . 'db' "), "route.db");
    }

    #[test]
    fn parse_header_reads_both_shapes_and_refuses_the_rest() {
        let array = parse_header("  [[route]]  # the fallback\n").expect("an array header");
        assert!(array.array);
        assert_eq!(array.name, "route");

        let table = parse_header("[ route . db ]\t# the grant\n").expect("a table header");
        assert!(!table.array);
        assert_eq!(table.name, "route . db");

        // No bracket, no closing token, or a `]` followed by a value.
        assert!(parse_header("name = 'route'\n").is_none());
        assert!(parse_header("[route\n").is_none());
        assert!(parse_header("[route] = 1\n").is_none());
        assert!(parse_header("[]\n").is_none());
        assert!(parse_header("[ ]\n").is_none());
    }

    #[test]
    fn route_bounds_stop_at_the_next_element_and_the_first_foreign_table() {
        let infos = scan(&[
            "[[route]]\n",                 // 0
            "name = \"a\"\n",              // 1
            "[route.db]\n",                // 2
            "queries = []\n",              // 3
            "[proxy.backup]\n",            // 4, a table that does not belong to the route
            "protocol = \"postgresql\"\n", // 5
            "[[other]]\n",                 // 6, an array of tables that is not a route
            "[[route]]\n",                 // 7
            "name = \"b\"\n",              // 8
        ]);

        assert_eq!(route_starts(&infos), vec![0, 7]);
        assert_eq!(route_end(&infos, 0), 4);
        // A `[route.<proxy>]` table can sit after a foreign table and still
        // belong to the element, so the element only ends at the next route.
        assert_eq!(element_end(&infos, 0), 7);
        assert_eq!(route_end(&infos, 7), infos.len());
        assert_eq!(element_end(&infos, 7), infos.len());
    }

    #[test]
    fn route_start_line_finds_the_indexed_element() {
        let lines: Vec<String> = ["version = 1\n", "[[route]]\n", "[[route]]\n"]
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert_eq!(route_start_line(&lines, 0), Some(1));
        assert_eq!(route_start_line(&lines, 1), Some(2));
        assert_eq!(route_start_line(&lines, 2), None);
    }

    //
    // The single-line value scanners: a `#` or a `]` inside a string is part of
    // the value, never structure.
    //

    #[test]
    fn trailing_comment_keeps_the_authors_whitespace() {
        assert_eq!(
            trailing_comment("queries = ['a']  # keep me\n"),
            Some("  # keep me\n")
        );
        assert_eq!(
            trailing_comment("queries = ['a']\t# tab\n"),
            Some("\t# tab\n")
        );
        assert_eq!(
            trailing_comment("# only a comment\n"),
            Some("# only a comment\n")
        );
        // A `#` inside a string is part of the statement…
        assert_eq!(trailing_comment("queries = ['a # not a comment']\n"), None);
        assert_eq!(
            trailing_comment("queries = [\"a \\\" # still in the string\"]\n"),
            None
        );
        assert_eq!(
            trailing_comment("queries = [\"a # not a comment\"]\n"),
            None
        );
        // …and a multi-line string is not a single-line value at all.
        assert_eq!(trailing_comment("queries = ['''a''']\n"), None);
        assert_eq!(trailing_comment("queries = [\"\"\"a\"\"\"] # keep\n"), None);
        // A basic string that closes still leaves the comment to find, and a
        // multi-line one swallows the `#` after it.
        assert_eq!(
            trailing_comment("queries = [\"a\"] # keep\n"),
            Some(" # keep\n")
        );
        assert_eq!(trailing_comment("queries = ['''a''' ] # keep\n"), None);
        // A value without a comment has none.
        assert_eq!(trailing_comment("queries = ['a']"), None);
    }

    #[test]
    fn closing_bracket_skips_strings_and_comments() {
        let line = "queries = ['a', 'b']\n";
        assert_eq!(closing_bracket(line), line.find(']'));
        assert_eq!(
            closing_bracket("queries = []\n"),
            "queries = []\n".find(']')
        );
        assert_eq!(
            closing_bracket("queries = [['a'], ['b']]\n"),
            "queries = [['a'], ['b']]\n".rfind(']')
        );
        // A `]` inside a value is not the end of the array.
        assert_eq!(
            closing_bracket("queries = ['a]b']\n"),
            "queries = ['a]b']\n".rfind(']')
        );
        assert_eq!(
            closing_bracket("queries = [\"a]b\"]\n"),
            "queries = [\"a]b\"]\n".rfind(']')
        );
        assert_eq!(
            closing_bracket("queries = [\"a\\\"]b\"]\n"),
            "queries = [\"a\\\"]b\"]\n".rfind(']')
        );
        // A comment ends the scan, so a `]` in it does not close anything.
        assert_eq!(
            closing_bracket("queries = ['a'] # ]\n"),
            Some(
                "queries = ['a'] # ]\n"
                    .find(']')
                    .expect("the array's own bracket")
            )
        );
        // Unterminated, a multi-line string, or a stray bracket: no array.
        assert_eq!(closing_bracket("queries = ['a'\n"), None);
        assert_eq!(closing_bracket("queries = ['''a''']\n"), None);
        assert_eq!(closing_bracket("queries = [\"\"\"a\"\"\"]\n"), None);
        // A comment ends the scan before the bracket it mentions.
        assert_eq!(closing_bracket("queries = [ # ]\n"), None);
        assert_eq!(closing_bracket("] = 1\n"), None);
        assert_eq!(closing_bracket(""), None);
    }

    #[test]
    fn append_to_array_keeps_the_separator_right() {
        let entry = vec![String::from("SELECT 2")];

        assert_eq!(
            append_to_array("queries = ['SELECT 1']\n", &entry, false).unwrap(),
            "queries = ['SELECT 1', 'SELECT 2']\n"
        );
        // An empty list takes no leading comma…
        assert_eq!(
            append_to_array("queries = []\n", &entry, true).unwrap(),
            "queries = ['SELECT 2']\n"
        );
        // …and a list the author left a trailing comma in needs no second one.
        assert_eq!(
            append_to_array("queries = ['SELECT 1', ]\n", &entry, false).unwrap(),
            "queries = ['SELECT 1', 'SELECT 2']\n"
        );
        // The value after the bracket is the author's, the comment included.
        assert_eq!(
            append_to_array("queries = ['SELECT 1']  # learned\n", &entry, false).unwrap(),
            "queries = ['SELECT 1', 'SELECT 2']  # learned\n"
        );
        // Nothing to add, or nothing to extend.
        assert!(append_to_array("queries = ['a']\n", &[], false).is_none());
        assert!(append_to_array("queries = ['a'\n", &entry, false).is_none());
    }

    //
    // Rendering helpers.
    //

    #[test]
    fn strings_prefer_literal_quoting_and_escape_basic_strings() {
        assert_eq!(
            toml_string("SELECT \"t\".\"a\" FROM \"t\""),
            "'SELECT \"t\".\"a\" FROM \"t\"'"
        );
        assert_eq!(toml_string("don't"), "\"don't\"");
        assert_eq!(toml_string("a\tb"), "'a\tb'");
        assert_eq!(toml_string("a\nb"), "\"a\\nb\"");
        assert_eq!(toml_string("\u{7f}"), "\"\\u007F\"");
        assert_eq!(toml_string("with'quote\n"), "\"with'quote\\n\"");
        assert_eq!(toml_basic_string("a\\b\"c\u{1}"), "\"a\\\\b\\\"c\\u0001\"");
        assert_eq!(toml_basic_string("\r\t"), "\"\\r\\t\"");
        assert_eq!(toml_string_array(&[]), "[]");
        assert_eq!(
            toml_string_array(&[String::from("a"), String::from("b")]),
            "['a', 'b']"
        );
    }

    #[test]
    fn keys_are_bare_only_when_toml_allows_it() {
        assert_eq!(toml_key("db"), "db");
        assert_eq!(toml_key("a-b_1"), "a-b_1");
        assert_eq!(toml_key(""), "\"\"");
        assert_eq!(toml_key("d b"), "\"d b\"");
        assert_eq!(toml_key("d.b"), "\"d.b\"");
    }

    #[test]
    fn line_predicates_recognise_what_they_name() {
        assert!(is_queries_assignment("queries = []"));
        assert!(is_queries_assignment("  queries   = []"));
        assert!(!is_queries_assignment("queries_extra = []"));
        assert!(!is_queries_assignment("name = 'queries = 1'"));

        assert!(is_blank("  \t\n"));
        assert!(!is_blank(" \n# x"));
        assert_eq!(indent_of("  queries = []"), "  ");
        assert_eq!(indent_of("queries = []"), "");
    }

    //
    // Proposal checks.
    //

    #[test]
    fn normalise_queries_refuses_what_a_grant_can_never_hold() {
        assert!(matches!(
            normalise_queries("r", "db", &[String::from("   ")]),
            Err(EditError::EmptyQuery { index: 0, .. })
        ));
        assert!(matches!(
            normalise_queries("r", "db", &[String::from("SELECT \"t\".\"a\" = …")]),
            Err(EditError::RedactedQuery { index: 0, .. })
        ));
        assert!(matches!(
            normalise_queries("r", "db", &[String::from("SELECT \"t\".\"a\" = 'x'")]),
            Err(EditError::LiteralQuery { index: 0, .. })
        ));
        // The two documented setup statements are static SQL, not literals to
        // bind, so they are accepted.
        assert_eq!(
            normalise_queries("r", "db", &[String::from("SET TIME ZONE 'UTC'")]).unwrap(),
            vec![String::from("SET TIME ZONE 'UTC'")]
        );
        assert_eq!(
            normalise_queries(
                "r",
                "db",
                &[String::from("SELECT set_config('TimeZone', $1, false)")]
            )
            .unwrap(),
            vec![String::from("SELECT set_config('TimeZone', $?, false)")]
        );
        // Two bind numbers normalise to the same statement: it is written once.
        assert_eq!(
            normalise_queries(
                "r",
                "db",
                &[String::from("SELECT $1"), String::from("SELECT $2")]
            )
            .unwrap(),
            vec![String::from("SELECT $?")]
        );
        assert!(normalise_queries("r", "db", &[]).unwrap().is_empty());
    }

    #[test]
    fn route_names_follow_the_documented_pattern() {
        assert!(valid_route_name("orders"));
        assert!(valid_route_name("orders-2_x"));
        assert!(valid_route_name("0"));
        assert!(!valid_route_name(""));
        assert!(!valid_route_name("Orders"));
        assert!(!valid_route_name("_orders"));
        assert!(!valid_route_name("orders.x"));
        assert!(!valid_route_name("orders "));
    }

    #[test]
    fn route_index_finds_a_declared_route_by_name() {
        let (manifest, _) = verified();
        assert_eq!(route_index(&manifest, "products"), Some(0));
        assert_eq!(route_index(&manifest, "ghost"), None);
    }
}
