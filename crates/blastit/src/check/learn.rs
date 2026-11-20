//! The `--learn` half: the routes and exact queries a run would add (`R27`,
//! `R29`).
//!
//! Every observed statement is mapped to the view it belongs to: the route a
//! routed request resolved to, or the view the run proposes for an unrouted one.
//! A statement on a **declared** view is proposed only when the same allowlist
//! `check` audits with reports it missing, so `check --learn` answers `check`'s
//! `missing_grant`. Only exact queries are written — never a table grant and
//! never a `transactions` flag — and a redacted literal makes the plan
//! unwritable rather than half-granted.

use std::collections::{BTreeMap, BTreeSet};

use blastit_dbproxy::ObsTarget;
use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView};
use blastit_manifest::{Grant, Manifest, MountTable, ProxyName, RoutingTable};
use blastit_sqlpolicy::{LearnVerdict, normalize};
use serde_json::json;

use super::diff::{GrantCheck, grant_of};
use super::error::ObsError;
use super::evaluate::parse_manifest;
use super::infer_routes::{RoutePlan, infer, is_learnable_path};
use super::obs::{CheckObsV1, SCHEMA_VERSION};
use super::patterns::writable_pattern;
/// What one `--learn` run proposes, and what it refused (`R27`, `R29`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnPlan {
    /// Routes and exact `queries` only.
    pub edits: BlastfileEdits,
    /// HTTP requests the run observed.
    pub requests: u64,
    /// Statements the learn policy refused, rendered for stderr.
    pub refused: Vec<String>,
    /// Ungranted transaction-control hints, for stderr.
    pub transaction_control: Vec<String>,
}

impl LearnPlan {
    /// How many routes the plan adds.
    pub fn routes(&self) -> usize {
        self.edits.views.len()
    }

    /// How many exact queries the plan adds.
    pub fn queries(&self) -> usize {
        self.edits
            .grants
            .iter()
            .map(|grant| grant.queries.len())
            .sum()
    }

    /// Whether `check --learn` may write this plan (`R29`).
    ///
    /// The caller still has to know that pytest exited `0`; everything else is
    /// here: at least one request, nothing refused, something to write, and not
    /// a dry run.
    pub fn would_write(&self, dry_run: bool) -> bool {
        !dry_run
            && self.requests > 0
            && self.refused.is_empty()
            && (self.routes() > 0 || self.queries() > 0)
    }

    /// The `--learn --json` document (`§1.3`): the proposal and whether it was
    /// written, intentionally without a `schema_version`.
    pub fn json(&self, dry_run: bool) -> String {
        json!({
            "proposed": {"routes": self.routes(), "queries": self.queries()},
            "wrote": self.would_write(dry_run),
        })
        .to_string()
    }

    /// Writes every transaction-control hint to stderr (`R27`).
    ///
    /// A hint, never a silent flag: `--learn` does not invent a
    /// `transaction_control = true` an author did not ask for.
    pub fn emit_hints(&self) {
        for hint in &self.transaction_control {
            eprintln!("learn: {hint}");
        }
    }
}

impl LearnPlan {
    /// The document this plan would write, for `--dry-run`.
    ///
    /// These are the writer's own bytes: [`apply_edits`] splices the edits into
    /// `source` at the tree's own offsets, and the write path (`check::learn`)
    /// hands that same string to [`write_atomic`] — so a dry run prints exactly
    /// what would land on disk, and the two cannot drift. No second renderer,
    /// no second lexer, and deliberately no `fmt`: `--learn` is a surgical
    /// edit, so canonicalising the whole document here would print a file the
    /// write path (and `dev --learn`) never produces.
    ///
    /// # Errors
    ///
    /// The editor's one-line refusal, so a dry run reports the same sentence a
    /// real run reports instead of silently printing nothing.
    ///
    /// [`apply_edits`]: blastit_manifest::blastfile::apply_edits
    /// [`write_atomic`]: blastit_manifest::edit::write_atomic
    pub fn proposal(&self, source: &str) -> Result<String, String> {
        blastit_manifest::blastfile::apply_edits(source, &self.edits)
            .map_err(|error| error.to_string())
    }
}

fn learn_route_plan(plan: &RoutePlan) -> BlastfileEdits {
    let mut patterns: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, name) in &plan.by_path {
        let Some(pattern) = writable_pattern(path) else {
            continue;
        };
        let proposed = patterns.entry(name.clone()).or_default();
        if !proposed.contains(&pattern) {
            proposed.push(pattern);
        }
    }
    BlastfileEdits {
        views: patterns
            .into_iter()
            .map(|(name, patterns)| ProposedView {
                name,
                methods: Vec::new(),
                patterns,
            })
            .collect(),
        grants: Vec::new(),
    }
}
/// Plans the routes and exact queries an observation run adds (`R27`).
///
/// Every observed statement is mapped to the view it belongs to: the route a
/// routed request resolved to, or the view this run proposes for an unrouted
/// one. A statement on a **declared** view is proposed only when the same
/// allowlist `check` audits with reports it missing, so `check --learn` answers
/// `check`'s `missing_grant` instead of reporting `0 proposed`. The view's name
/// comes from [`infer`]; its patterns come from the paths the run saw, with
/// numeric and UUID-shaped segments becoming parameters; the statement is the
/// learn policy's normalised text. Redacted literals are refused (they make the
/// plan unwritable), transaction control is a hint, and a table grant is never
/// invented. A path `--learn` refuses (a traversal, a dotfile, a `static`
/// mount) proposes no view and no grant.
///
/// # Errors
///
/// [`ObsError::SchemaVersion`] for a document this build does not read, and
/// [`ObsError::Manifest`] when the routing table cannot be built.
pub fn plan_learn(manifest: &Manifest, observations: &CheckObsV1) -> Result<LearnPlan, ObsError> {
    if observations.schema_version() != SCHEMA_VERSION {
        return Err(ObsError::SchemaVersion {
            found: observations.schema_version(),
            expected: SCHEMA_VERSION,
        });
    }
    let table =
        RoutingTable::new(manifest).map_err(|errors| ObsError::Manifest(errors.to_string()))?;

    let mut paths: BTreeSet<String> = BTreeSet::new();
    // A `static` mount answers its files before any view sees them (`R14`), so a
    // view for one of those paths would never run; and a scanner probe is never
    // a view a developer forgot (`infer_routes::is_learnable_path`).
    let mounts = MountTable::new(manifest);
    let learnable = |path: &str| is_learnable_path(path) && mounts.lookup(path).is_none();
    for line in observations.http() {
        if table.route(&line.method, &line.path).is_none() && learnable(&line.path) {
            paths.insert(line.path.clone());
        }
    }
    for statement in observations.sql() {
        if let ObsTarget::Unrouted { path, .. } = &statement.target
            && learnable(path)
        {
            paths.insert(path.clone());
        }
    }
    let plan = infer(paths.iter().map(String::as_str), manifest);

    // Any refused statement in the run makes the whole plan unwritable: the
    // manifest after the edit would still be incomplete (`R25`, `R29`).
    let mut refused: BTreeSet<String> = BTreeSet::new();
    for statement in observations.sql() {
        if let LearnVerdict::Refused { redacted, reason } = &statement.verdict {
            refused.insert(format!("{redacted}: {reason}"));
        }
    }

    let mut hints: BTreeSet<String> = BTreeSet::new();
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for statement in observations.sql() {
        // The view this statement belongs to: a declared view the request was
        // routed to, or the view this run proposes for an unrouted request. The
        // first is what makes `check --learn` answer `check`'s `missing_grant`
        // for an existing view instead of proposing nothing at all.
        let route = match &statement.target {
            ObsTarget::Routed { route } => Some(route.clone()),
            ObsTarget::Unrouted { path, .. } => plan.by_path.get(path).cloned(),
        };
        let Some(route) = route else {
            continue;
        };
        if statement.is_tx {
            hints.insert(format!(
                "transaction control is not granted automatically: add `transactions` to \
                 `view {route}`'s `postgres` block (or to a trait it `use`s)"
            ));
            continue;
        }
        if let LearnVerdict::Query { normalized } = &statement.verdict {
            if normalized.contains('\u{2026}') {
                // A redaction is never grantable, whatever a broken verdict
                // says (`R24`, `R33`): the plan stays unwritable.
                refused.insert(format!("{}: a redacted statement", normalized));
                continue;
            }
            // A declared view is only proposed a statement `check` really
            // reports as missing: the same allowlist, asked about the same
            // observation, so the two commands cannot disagree.
            if let Some(index) = manifest
                .routes()
                .iter()
                .position(|candidate| candidate.name() == route)
                && GrantCheck::of(grant_of(manifest, index, &statement.proxy).as_ref())
                    .allows(&statement.sql)
            {
                continue;
            }
            grouped
                .entry((route.clone(), statement.proxy.clone()))
                .or_default()
                .insert(normalized.clone());
        }
    }

    let mut edits = learn_route_plan(&plan);
    let proposed: BTreeSet<String> = edits.views.iter().map(|view| view.name.clone()).collect();
    let declared: BTreeSet<String> = manifest
        .routes()
        .iter()
        .map(|route| route.name().to_string())
        .collect();
    for ((route, proxy), queries) in grouped {
        if !proposed.contains(&route) && !declared.contains(&route) {
            // No view could be written for this route: every path it saw is not
            // expressible as a quoted pattern, so a grant would name a view the
            // edit does not create.
            continue;
        }
        let already = manifest
            .routes()
            .iter()
            .position(|candidate| candidate.name() == route);
        let queries: Vec<String> = queries
            .into_iter()
            .filter(|query| match already {
                Some(index) => !granted_already(manifest, index, &proxy, query),
                None => true,
            })
            .collect();
        if !queries.is_empty() {
            edits.grants.push(ProposedGrant {
                view: route,
                // The unnamed `postgres { … }` block is the store `db` (§7.4);
                // a named store is written `postgres <name> { … }`.
                store: (proxy != blastit_manifest::blastfile::DEFAULT_STORE).then_some(proxy),
                queries,
                tables: std::collections::BTreeMap::new(),
                functions: std::collections::BTreeSet::new(),
            });
        }
    }

    Ok(LearnPlan {
        edits,
        requests: observations.http().len() as u64,
        refused: refused.into_iter().collect(),
        transaction_control: hints.into_iter().collect(),
    })
}

/// Whether one normalised statement is already allowed on a route/proxy.
fn granted_already(manifest: &Manifest, route: usize, proxy: &str, sql: &str) -> bool {
    let proxy = ProxyName::new(proxy.to_string());
    let granted = manifest
        .effective_grants(route)
        .get(&proxy)
        .and_then(Grant::as_postgresql)
        .is_some_and(|grant| grant.queries().iter().any(|query| normalize(query) == sql));
    granted || manifest.is_query_covered_by_tables_or_groups(route, &proxy, sql)
}

/// `plan_learn` + [`LearnPlan::json`]: the `--learn --json` document over the
/// two JSONL streams.
///
/// Prints the transaction-control hints to stderr, then returns the document.
///
/// # Errors
///
/// Every [`ObsError`] of [`parse_manifest`] and [`plan_learn`].
pub fn plan_learn_json(
    manifest_text: &str,
    http_jsonl: &str,
    sql_jsonl: &str,
    dry_run: bool,
) -> Result<String, ObsError> {
    let manifest = parse_manifest(manifest_text)?;
    let observations = CheckObsV1::from_jsonl(http_jsonl, sql_jsonl)?;
    let plan = plan_learn(&manifest, &observations)?;
    plan.emit_hints();
    Ok(plan.json(dry_run))
}
