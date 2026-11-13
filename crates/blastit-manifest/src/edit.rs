//! Surgical, comment-preserving manifest edits and atomic writes (`R28`,
//! `R29`).
//!
//! `--learn` has to add routes and exact `queries` to a file a person wrote by
//! hand. Comments, ordering and quoting are the author's, so the editor never
//! re-serialises the document: [`apply_edits`] parses the manifest, locates the
//! insertion points line by line, splices whole `[[route]]` blocks or a single
//! `queries = [...]` assignment, and then re-parses the result to prove that
//! every field it did not mean to touch is unchanged.
//!
//! Two rules from `ADR-0003` shape the output:
//!
//! * a proposed route goes immediately above the first method-unrestricted
//!   catch-all ([`Route::is_unrestricted_catch_all`]), or at the end of the
//!   file. The position comes from the parsed model, so `path = ".*"` and
//!   `path = ['.*']` are the same catch-all;
//! * only exact, normalised `queries` are written. A statement that inlines a
//!   literal or still holds a redacted `…` is refused
//!   ([`EditError::LiteralQuery`], [`EditError::RedactedQuery`]): the message
//!   names the position and the fix, never the statement (`R24`).
//!
//! [`apply_edits`] is idempotent — applying the same [`ManifestEdits`] to its
//! own output returns that output byte for byte, and an edit with nothing to
//! add returns the source untouched. A `queries` list on one line is extended
//! in place (the author's quoting is kept entry by entry); a list spread over
//! several lines is written on one line, because only the list itself is
//! touched. A grant an author wrote as an inline table is refused rather than
//! duplicated ([`EditError::InlineGrant`]).
//!
//! [`write_atomic`] replaces a file the way the documentation promises: a
//! sibling `.<name>.<pid>.tmp`, an `fsync` of the file, a `rename`, an `fsync`
//! of the directory, and no temporary file left behind whether that succeeded
//! or failed. The temp file is created with `O_CREAT | O_EXCL`, so a planted
//! symlink at that name is never written through.
//!
//! ```
//! use blastit_manifest::edit::{ManifestEdits, ProposedRoute, apply_edits};
//!
//! let source = "version = 1\n\
//!               [[route]]\nname = \"fallback\"\npath = \".*\"\n";
//! let edits = ManifestEdits {
//!     routes: vec![ProposedRoute {
//!         name: "orders".to_string(),
//!         path: vec!["/orders/".to_string()],
//!     }],
//!     grants: vec![],
//! };
//!
//! let edited = apply_edits(source, &edits).expect("the edit applies");
//! assert!(edited.contains("name = \"orders\""));
//! // The new route is above the catch-all, and the source is otherwise intact.
//! assert!(edited.find("orders").expect("inserted") < edited.find("fallback").expect("kept"));
//! assert_eq!(apply_edits(&edited, &edits).expect("idempotent"), edited);
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

/// Everything one `--learn` run wants to add to a manifest (`R28`, `R29`).
///
/// Routes are inserted above the method-unrestricted catch-all (or appended),
/// grants are merged into the route's existing `queries` list, and the whole
/// edit is a no-op when there is nothing to add.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEdits {
    /// New `[[route]]` blocks.
    #[serde(default)]
    pub routes: Vec<ProposedRoute>,
    /// Exact statements to add to existing or newly proposed routes.
    #[serde(default)]
    pub grants: Vec<ProposedGrant>,
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
                "the manifest does not parse, so it cannot be edited: {error}"
            ),
            Self::InvalidRouteName { name } => write!(
                f,
                "route name {} is not `[a-z0-9][a-z0-9_-]*`; derive it from the first path \
                 segment in lowercase",
                quoted(name)
            ),
            Self::EmptyPath { name } => write!(
                f,
                "route {} has no `path`; a route needs at least one regex",
                quoted(name)
            ),
            Self::EmptyPattern { name, rule } => write!(
                f,
                "route {}'s path[{rule}] is empty; every pattern has to match something",
                quoted(name)
            ),
            Self::InvalidPattern { name, rule } => write!(
                f,
                "route {}'s path[{rule}] is not a valid Rust regex; fix the inferred pattern",
                quoted(name)
            ),
            Self::DuplicateProposedRoute { name } => write!(
                f,
                "two proposed routes are named {}; disambiguate them with `-2`, `-3`, …",
                quoted(name)
            ),
            Self::RouteConflict { name } => write!(
                f,
                "route {} is already declared with another path; --learn has to disambiguate the \
                 inferred name",
                quoted(name)
            ),
            Self::UnknownProxy { route, proxy } => write!(
                f,
                "route {} grants proxy {} but no `[proxy.{}]` declares it; write the declaration \
                 first",
                quoted(route),
                quoted(proxy),
                escape_control(proxy)
            ),
            Self::MissingRoute { route, proxy } => write!(
                f,
                "a {} grant names route {}, which is neither declared nor proposed; add the route \
                 first",
                quoted(proxy),
                quoted(route)
            ),
            Self::UnrestrictedGrant { route, proxy } => write!(
                f,
                "route {}'s grant on {} is `unrestricted = true`; an exact query next to it is a \
                 boot error, so remove `unrestricted` first",
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
                "route {}'s {} grant would write queries[{index}] with a redacted `…`; the value \
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
                "route {}'s {} grant would write queries[{index}] with an inline literal; bind the \
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
                "route {}'s {} grant has an empty queries[{index}]; write a statement or drop it",
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

/// Applies `--learn`'s proposals to a manifest document (`R28`, `R29`).
///
/// The source is parsed first, then edited line by line: comments, ordering and
/// quoting outside the edited fields are reproduced byte for byte. New routes
/// are inserted immediately above the first method-unrestricted catch-all, or at
/// the end of the file when there is none; a grant already listed for that
/// route is not added twice. The result is re-parsed and compared with the
/// model the edit intended, so an edit that would change anything else fails
/// with [`EditError::Verification`] and no output.
///
/// An edits set with nothing to add returns the source unchanged, which makes
/// repeated learning runs byte-identical.
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
        let Some(mut at) = route_start_line(&lines, plan.insert_at) else {
            return Err(EditError::Verification {
                section: "the route order",
            });
        };
        // A comment written directly above the catch-all still documents it:
        // the new block goes above the comment, not between the two.
        while at > 0
            && lines
                .get(at - 1)
                .is_some_and(|line| line.trim_start().starts_with('#'))
        {
            at -= 1;
        }
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

    let mut stop = assignment + 1;
    while stop < section_end && infos.get(stop - 1).is_some_and(|info| info.depth_end > 0) {
        stop += 1;
    }
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
    let mut block_end = end;
    while block_end > start && lines.get(block_end - 1).is_some_and(|line| is_blank(line)) {
        block_end -= 1;
    }
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
        .with_ingress(route.ingress());
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
    built
}

/// Re-parses the edit and proves nothing outside the edit moved.
fn verify(manifest: &Manifest, expected: &[Route], text: &str) -> Result<(), EditError> {
    let reparsed = Manifest::from_toml_str(text).map_err(|_| EditError::Verification {
        section: "the document syntax",
    })?;

    if reparsed.version() != manifest.version()
        || reparsed.app() != manifest.app()
        || reparsed.preload() != manifest.preload()
        || reparsed.global_groups() != manifest.global_groups()
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
#[derive(Clone, Copy, PartialEq, Eq)]
enum Multi {
    Basic,
    Literal,
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
fn walk(line: &str, mut multi: Option<Multi>, mut depth: usize) -> (Option<Multi>, usize) {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if let Some(kind) = multi {
            match kind {
                Multi::Basic => {
                    if chars[index] == '\\' {
                        index += 2;
                        continue;
                    }
                    if chars[index..].starts_with(&['"', '"', '"']) {
                        multi = None;
                        index += 3;
                        continue;
                    }
                }
                Multi::Literal => {
                    if chars[index..].starts_with(&['\'', '\'', '\'']) {
                        multi = None;
                        index += 3;
                        continue;
                    }
                }
            }
            index += 1;
            continue;
        }

        match chars[index] {
            '#' => break,
            '"' if chars[index..].starts_with(&['"', '"', '"']) => {
                multi = Some(Multi::Basic);
                index += 3;
            }
            '\'' if chars[index..].starts_with(&['\'', '\'', '\'']) => {
                multi = Some(Multi::Literal);
                index += 3;
            }
            '"' => {
                index += 1;
                while index < chars.len() {
                    if chars[index] == '\\' {
                        index += 2;
                        continue;
                    }
                    if chars[index] == '"' {
                        index += 1;
                        break;
                    }
                    index += 1;
                }
            }
            '\'' => {
                index += 1;
                while index < chars.len() && chars[index] != '\'' {
                    index += 1;
                }
                if index < chars.len() {
                    index += 1;
                }
            }
            '[' | '{' => {
                depth = depth.saturating_add(1);
                index += 1;
            }
            ']' | '}' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ => index += 1,
        }
    }

    (multi, depth)
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

    let mut search = 0;
    let mut end = None;
    while let Some(found) = body.get(search..)?.find(close) {
        let at = search + found;
        let rest = body.get(at + close.len()..)?.trim_start();
        if rest.is_empty() || rest.starts_with('#') {
            end = Some(at);
            break;
        }
        search = at + 1;
    }

    let end = end?;
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
fn trailing_comment(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut in_basic = false;
    let mut in_literal = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_basic {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_basic = false;
            }
        } else if in_literal {
            if byte == b'\'' {
                in_literal = false;
            }
        } else {
            match byte {
                b'"' => {
                    if bytes
                        .get(index..)
                        .is_some_and(|rest| rest.starts_with(b"\"\"\""))
                    {
                        return None;
                    }
                    in_basic = true;
                }
                b'\'' => {
                    if bytes
                        .get(index..)
                        .is_some_and(|rest| rest.starts_with(b"'''"))
                    {
                        return None;
                    }
                    in_literal = true;
                }
                b'#' => {
                    let mut start = index;
                    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
                        start -= 1;
                    }
                    return line.get(start..);
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

/// The byte index of the `]` that closes the array value on this line.
fn closing_bracket(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut in_basic = false;
    let mut in_literal = false;
    let mut opened = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_basic {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_basic = false;
            }
        } else if in_literal {
            if byte == b'\'' {
                in_literal = false;
            }
        } else {
            match byte {
                b'#' => break,
                b'"' => {
                    if bytes
                        .get(index..)
                        .is_some_and(|rest| rest.starts_with(b"\"\"\""))
                    {
                        return None;
                    }
                    in_basic = true;
                }
                b'\'' => {
                    if bytes
                        .get(index..)
                        .is_some_and(|rest| rest.starts_with(b"'''"))
                    {
                        return None;
                    }
                    in_literal = true;
                }
                b'[' => {
                    depth = depth.saturating_add(1);
                    opened = true;
                }
                b']' => {
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
        index += 1;
    }
    None
}

/// Extends a one-line `queries = [...]` value with `entries`, leaving every
/// existing byte (and its quoting) where the author put it.
fn append_to_array(line: &str, entries: &[String], was_empty: bool) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let close = closing_bracket(line)?;
    let before = line.get(..close)?;
    let trimmed = before.trim_end();
    let separator = if was_empty {
        ""
    } else if trimmed.ends_with(',') {
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
