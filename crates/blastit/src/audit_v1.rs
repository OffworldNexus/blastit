//! The framework-agnostic `blastit check` audit engine (`R27`, `R28`,
//! `R32..R38`; `ADR-0003`, `ADR-0004`).
//!
//! One test run produces two streams: the HTTP requests a WSGI gate observed
//! (`http_obs.jsonl`, one [`HttpObsLine`] per line) and the statements dbproxy
//! saw (`sql_obs.jsonl`, one [`WireSqlObs`] per line). [`CheckObsV1`] is the
//! schema that ties them together — `schema_version == 1`, enforced by
//! [`CheckObsV1::new`] and again by [`evaluate`] — and [`evaluate`] diffs it
//! against a [`Manifest`] using blastit's own routing table, effective grants
//! and allowlist. Nothing about the manifest is re-implemented here.
//!
//! The report is the closed nine-kind enum of `docs/ci.md`: every finding is an
//! `error` in `0.1.1`, `ok` is true exactly when no error-severity finding
//! exists, and `statements` is by construction the sum of
//! `proxies[*].statements` (`R38`). [`human`] renders the same report in the
//! byte-exact `§1.3` form.
//!
//! Two rules protect the machine contract:
//!
//! * `add_query` is emitted **only** for a statement dbproxy's learn policy
//!   proved grantable (`ObsVerdict::Query`, never a redacted `…`). A redacted
//!   literal, and every transaction-control denial, names `observe` instead, so
//!   every `add_query` this engine emits passes
//!   [`Manifest::parse_and_validate`] once written (`R33`);
//! * a grantable statement that is the driver's own session setup — `SELECT
//!   set_config('TimeZone', $?, false)`, `SET …`, `SHOW …` — names `add_group`
//!   and a `[group.connection.<proxy>]` table instead of one route's `queries`:
//!   it runs on whichever route opens the connection first, so a per-route
//!   grant would leave every other route denying it, so the hint is the group
//!   one every route attaches (`MA.8`);
//! * SQL under [`ObsTarget::Unrouted`] counts towards `statements`/`denied`
//!   and never produces a route-less `missing_grant` (`R32`).
//!
//! [`plan_learn`] is the `--learn` side: it maps unrouted SQL through
//! [`crate::infer_routes`] into the proposed route's `[route.<proxy>].queries`,
//! writes only routes and exact queries, and refuses to invent table grants or
//! a `transaction_control` flag (`R27`).
//!
//! # The observations schema (v1)
//!
//! `http_obs.jsonl`:
//!
//! ```json
//! {"method":"POST","path":"/notes/","content_type":"application/json",
//!  "body_len":13,"sample":{"mime":"application/json","truncated":false,
//!  "valid_json":true,"valid_form":false}}
//! ```
//!
//! `sql_obs.jsonl` is one `blastit_dbproxy::WireSqlObs` per line. A malformed
//! line is an [`ObsError::Malformed`] naming the file and the line: the caller
//! (`check::run`) turns that into exit `2`, never a partial report.

use std::collections::{BTreeMap, BTreeSet};

use blastit_dbproxy::{ObsTarget, ObsVerdict, WireSqlObs};
use blastit_ingress::{
    BodyCodec, BodySample, IngressPolicy, parse_mime, probe_sample, suggest_fix,
};
use blastit_manifest::edit::{ManifestEdits, ProposedGrant, ProposedRoute};
use blastit_manifest::{Grant, Manifest, PgGrant, ProxyName, RoutingTable};
use blastit_sqlpolicy::{Allowlist, KNOWN_STATIC_SETUP_SQL, is_introspection_setup_sql, normalize};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::infer_routes::infer;

/// The only observations schema this build reads.
pub const SCHEMA_VERSION: u64 = 1;

/// Why the observations (or the manifest handed with them) could not be read.
///
/// Every variant is a problem in the input, never something blastit could not
/// do: no report is partially produced.
#[derive(Debug, thiserror::Error)]
pub enum ObsError {
    /// One JSONL line is not the document this schema names.
    #[error("{file}: line {line}: {reason}")]
    Malformed {
        /// The file the line came from, as the caller names it.
        file: String,
        /// One-based line number.
        line: usize,
        /// Why the line did not parse.
        reason: String,
    },
    /// The document is a schema this build does not read.
    #[error("schema_version {found} is not {expected}")]
    SchemaVersion {
        /// The version found.
        found: u64,
        /// The version this build reads.
        expected: u64,
    },
    /// The manifest itself is invalid.
    #[error("the manifest is invalid: {0}")]
    Manifest(String),
    /// The report could not be rendered.
    #[error("the report could not be rendered: {0}")]
    Report(String),
}

/// One observed HTTP request (`http_obs.jsonl`).
///
/// `sample` is what the Python gate probed from the body: at most 64 KiB, and
/// never the bytes themselves. It is optional because a request with no body
/// has nothing to sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpObsLine {
    /// The request method, as the app saw it.
    pub method: String,
    /// `PATH_INFO`, the string the scheduler routed on.
    pub path: String,
    /// The raw `Content-Type` header, if any.
    #[serde(default)]
    pub content_type: Option<String>,
    /// How many body bytes the app was handed.
    #[serde(default)]
    pub body_len: u64,
    /// The bounded body probe, if the gate took one.
    #[serde(default)]
    pub sample: Option<BodySample>,
}

/// The complete observations of one `blastit check`/`--learn` run.
///
/// The schema version is part of the value, not of the files: the orchestrator
/// knows which plugin wrote them and states it explicitly. [`CheckObsV1::new`]
/// refuses anything but [`SCHEMA_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckObsV1 {
    schema_version: u64,
    http: Vec<HttpObsLine>,
    sql: Vec<WireSqlObs>,
}

impl CheckObsV1 {
    /// A v1 observations set.
    ///
    /// # Errors
    ///
    /// [`ObsError::SchemaVersion`] when `schema_version != 1`.
    pub fn new(
        schema_version: u64,
        http: Vec<HttpObsLine>,
        sql: Vec<WireSqlObs>,
    ) -> Result<Self, ObsError> {
        if schema_version != SCHEMA_VERSION {
            return Err(ObsError::SchemaVersion {
                found: schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Self {
            schema_version,
            http,
            sql,
        })
    }

    /// Reads both JSONL streams, naming the files in any error.
    ///
    /// # Errors
    ///
    /// [`ObsError::Malformed`] for the first line that is not a v1 record.
    pub fn from_jsonl(http_jsonl: &str, sql_jsonl: &str) -> Result<Self, ObsError> {
        Self::new(
            SCHEMA_VERSION,
            parse_http_obs("http_obs.jsonl", http_jsonl)?,
            parse_sql_obs("sql_obs.jsonl", sql_jsonl)?,
        )
    }

    /// The schema version, always [`SCHEMA_VERSION`] for a value that exists.
    pub fn schema_version(&self) -> u64 {
        self.schema_version
    }

    /// Every observed HTTP request.
    pub fn http(&self) -> &[HttpObsLine] {
        &self.http
    }

    /// Every observed statement.
    pub fn sql(&self) -> &[WireSqlObs] {
        &self.sql
    }
}

/// Reads `http_obs.jsonl`. `file` is what an error names.
///
/// # Errors
///
/// [`ObsError::Malformed`] naming `file`, the line and the reason.
pub fn parse_http_obs(file: &str, text: &str) -> Result<Vec<HttpObsLine>, ObsError> {
    parse_lines(file, text)
}

/// Reads `sql_obs.jsonl`. `file` is what an error names.
///
/// # Errors
///
/// [`ObsError::Malformed`] naming `file`, the line and the reason.
pub fn parse_sql_obs(file: &str, text: &str) -> Result<Vec<WireSqlObs>, ObsError> {
    parse_lines(file, text)
}

/// Reads one JSON document per non-empty line.
fn parse_lines<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<Vec<T>, ObsError> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(line).map_err(|error| ObsError::Malformed {
            file: file.to_string(),
            line: index + 1,
            reason: error.to_string(),
        })?;
        out.push(value);
    }
    Ok(out)
}

/// How a finding fails the run (`R38`: every kind is `error` in `0.1.1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fails the run and sets `ok` to `false`.
    Error,
    /// Printed, but leaves `ok` and the exit code alone.
    Warning,
}

/// The machine-readable `fix` object of a finding.
///
/// `action` is one of the closed set of `R38` — `add_query`, its group-scoped
/// twin `add_group`, `remove_query`, `add_route`, `remove_route`, `add_test`,
/// `declare_ingress`, `fix_ingress` and `observe`; `to` names the manifest
/// table to edit, `codecs`/`allow_raw` an ingress declaration, and `unsafe`
/// marks the destructive `remove_query`/`remove_route` actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// The closed action name.
    pub action: &'static str,
    /// The manifest table the action edits, when it edits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The codecs a `declare_ingress`/`fix_ingress` action proposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codecs: Option<Vec<BodyCodec>>,
    /// Whether `body = "raw"` also needs `allow_raw = true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_raw: Option<bool>,
    /// Whether the action is destructive in production too.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "unsafe")]
    pub unsafe_: Option<bool>,
}

impl Fix {
    fn new(action: &'static str) -> Self {
        Self {
            action,
            to: None,
            codecs: None,
            allow_raw: None,
            unsafe_: None,
        }
    }

    fn observe() -> Self {
        Self::new("observe")
    }

    fn add_route() -> Self {
        Self::new("add_route")
    }

    fn add_test() -> Self {
        Self::new("add_test")
    }

    fn add_query(to: String) -> Self {
        Self {
            to: Some(to),
            ..Self::new("add_query")
        }
    }

    /// The connection-setup hint: a group every route attaches, because the
    /// statement runs on whichever route opens the connection first.
    fn add_group(to: String) -> Self {
        Self {
            to: Some(to),
            ..Self::new("add_group")
        }
    }

    fn remove_query(to: String) -> Self {
        Self {
            to: Some(to),
            unsafe_: Some(true),
            ..Self::new("remove_query")
        }
    }

    fn declare_ingress(codecs: Vec<BodyCodec>, allow_raw: bool) -> Self {
        Self {
            codecs: Some(codecs),
            allow_raw: allow_raw.then_some(true),
            ..Self::new("declare_ingress")
        }
    }

    fn fix_ingress(codecs: Vec<BodyCodec>, allow_raw: bool) -> Self {
        Self {
            codecs: Some(codecs),
            allow_raw: allow_raw.then_some(true),
            ..Self::new("fix_ingress")
        }
    }

    fn fix_raw_ingress(to: String) -> Self {
        Self {
            to: Some(to),
            allow_raw: Some(true),
            ..Self::new("fix_ingress")
        }
    }
}

/// One completeness finding, over the closed nine-kind enum (`§4.2 E.3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// A statement executed on a route but not granted (`R33`).
    MissingGrant {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route that ran it.
        route: String,
        /// The proxy it ran on.
        proxy: String,
        /// The normalised statement, or the transaction-control label.
        sql: String,
        /// `add_query`, `add_group` for connection setup, or `observe`.
        fix: Fix,
    },
    /// A grant nothing used (`R34`).
    DeadGrant {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route whose grant is dead (an attaching route, for a group).
        route: String,
        /// The group the grant belongs to, when it is a group's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        /// The proxy the grant is on.
        proxy: String,
        /// The normalised statement.
        sql: String,
        /// `remove_query`, flagged unsafe.
        fix: Fix,
    },
    /// A declared route no request reached (`R35`).
    UnexercisedRoute {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// `add_test`.
        fix: Fix,
    },
    /// A body was sent to a route with no `[route.ingress]` (`R36`).
    MissingIngress {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// The distinct MIME essences observed.
        observed: Vec<String>,
        /// `declare_ingress`.
        fix: Fix,
    },
    /// The declared codec is not what the suite sent (`R36`).
    IngressMismatch {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// The declared codec.
        declared: BodyCodec,
        /// The distinct MIME essences observed.
        observed: Vec<String>,
        /// `fix_ingress`.
        fix: Fix,
    },
    /// A route declares `body = "raw"` without `allow_raw = true` (`R36`).
    RawIngress {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// `fix_ingress` naming `[route.ingress]`.
        fix: Fix,
    },
    /// A request no route matched (`R32`).
    UnroutedRequest {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The request method.
        method: String,
        /// The request path.
        path: String,
        /// `add_route`.
        fix: Fix,
    },
    /// The suite reached no route or no proxy statement (`R37`).
    NoObservations {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// `observe`.
        fix: Fix,
    },
    /// A declared proxy saw no statement (`R37`).
    ProxyUnused {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The proxy name.
        proxy: String,
        /// `observe`.
        fix: Fix,
    },
}

impl Finding {
    /// The severity of this finding.
    pub fn severity(&self) -> Severity {
        match self {
            Self::MissingGrant { severity, .. }
            | Self::DeadGrant { severity, .. }
            | Self::UnexercisedRoute { severity, .. }
            | Self::MissingIngress { severity, .. }
            | Self::IngressMismatch { severity, .. }
            | Self::RawIngress { severity, .. }
            | Self::UnroutedRequest { severity, .. }
            | Self::NoObservations { severity, .. }
            | Self::ProxyUnused { severity, .. } => *severity,
        }
    }
}

/// How many routes the manifest declares, and how many the suite reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteCounts {
    /// `[[route]]` blocks in the manifest.
    pub total: u64,
    /// Routes with at least one matched request.
    pub exercised: u64,
}

/// What one declared proxy saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyCounts {
    /// Statements observed on that proxy, routed or not.
    pub statements: u64,
    /// How many of them no grant allowed.
    pub denied: u64,
}

/// The `blastit check --json` document (`R38`, `ADR-0004`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Always [`SCHEMA_VERSION`].
    pub schema_version: u64,
    /// True exactly when no finding has [`Severity::Error`].
    pub ok: bool,
    /// Observed HTTP requests.
    pub requests: u64,
    /// Sum of `proxies[*].statements`.
    pub statements: u64,
    /// Route totals.
    pub routes: RouteCounts,
    /// Per declared proxy counters, in name order.
    pub proxies: BTreeMap<String, ProxyCounts>,
    /// The findings, in `§1.3` order.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Renders the report as pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// [`ObsError::Report`] only when the value cannot be rendered, which
    /// cannot happen in practice.
    pub fn to_json(&self) -> Result<String, ObsError> {
        serde_json::to_string_pretty(self).map_err(|error| ObsError::Report(error.to_string()))
    }
}

/// Parses and validates a manifest document.
///
/// # Errors
///
/// [`ObsError::Manifest`] with the manifest's own one-line diagnostics.
pub fn parse_manifest(source: &str) -> Result<Manifest, ObsError> {
    let (manifest, _warnings) =
        Manifest::parse_and_validate(source, blastit_manifest::UidLayout::DEFAULT_BASE)
            .map_err(|errors| ObsError::Manifest(errors.to_string()))?;
    Ok(manifest)
}

/// Audits one run: `check_audit` behind `_blastit.check_audit` and
/// `blastit check --json`.
///
/// # Errors
///
/// [`ObsError`] when the manifest is invalid, the schema version is not
/// [`SCHEMA_VERSION`], or a JSONL line does not parse.
pub fn check_audit(
    manifest_toml: &str,
    http_jsonl: &str,
    sql_jsonl: &str,
) -> Result<String, ObsError> {
    let manifest = parse_manifest(manifest_toml)?;
    let observations = CheckObsV1::from_jsonl(http_jsonl, sql_jsonl)?;
    evaluate(&manifest, &observations)?.to_json()
}

/// The allowlist of one `(route, proxy)` grant.
struct Policy {
    allow: Allowlist,
}

impl Policy {
    fn of(grant: Option<&PgGrant>) -> Self {
        let Some(grant) = grant else {
            return Self {
                allow: Allowlist::default(),
            };
        };
        if grant.is_unrestricted() {
            return Self {
                allow: Allowlist::unrestricted(),
            };
        }
        Self {
            allow: Allowlist::new(grant.queries(), grant.transaction_control())
                .with_tables(grant.tables().clone()),
        }
    }

    fn allows(&self, sql: &str) -> bool {
        self.allow.check(sql).is_allowed()
    }
}

/// The effective `postgresql` grant of one `(route, proxy)` pair, if any.
fn grant_of(manifest: &Manifest, route: usize, proxy: &str) -> Option<PgGrant> {
    manifest
        .effective_grants(route)
        .get(&ProxyName::new(proxy.to_string()))
        .and_then(Grant::as_postgresql)
        .cloned()
}

/// Evaluates the observations against the manifest (`R32..R38`).
///
/// # Errors
///
/// [`ObsError::SchemaVersion`] for a document [`CheckObsV1::new`] would have
/// refused anyway, and [`ObsError::Manifest`] when the routing table cannot be
/// built.
pub fn evaluate(manifest: &Manifest, observations: &CheckObsV1) -> Result<Report, ObsError> {
    if observations.schema_version() != SCHEMA_VERSION {
        return Err(ObsError::SchemaVersion {
            found: observations.schema_version(),
            expected: SCHEMA_VERSION,
        });
    }
    let table =
        RoutingTable::new(manifest).map_err(|errors| ObsError::Manifest(errors.to_string()))?;

    let mut counts: BTreeMap<String, ProxyCounts> = manifest
        .proxies()
        .keys()
        .map(|name| (name.as_str().to_string(), ProxyCounts::default()))
        .collect();
    let mut requests_per_route = vec![0u64; manifest.route_count()];
    let mut bodies_per_route: Vec<Vec<BodySample>> = vec![Vec::new(); manifest.route_count()];
    let mut unrouted: BTreeSet<(String, String)> = BTreeSet::new();
    for line in observations.http() {
        match table.route(&line.method, &line.path) {
            Some(matched) => {
                requests_per_route[matched.route] += 1;
                if line.body_len > 0 {
                    bodies_per_route[matched.route].push(sample_of(line));
                }
            }
            None => {
                unrouted.insert((line.method.clone(), line.path.clone()));
            }
        }
    }

    // Which statements each exercised route actually allowed, for `dead_grant`.
    let mut matched: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    // One finding per `(route, proxy, statement)`.
    let mut missing: BTreeMap<(usize, String, String), Finding> = BTreeMap::new();
    for statement in observations.sql() {
        let proxy = statement.proxy.as_str();
        let index = match &statement.target {
            ObsTarget::Routed { route } => manifest
                .routes()
                .iter()
                .position(|candidate| candidate.name() == route),
            // `R32`: statements under an unrouted target count, but no route
            // exists to blame for them.
            ObsTarget::Unrouted { .. } => None,
        };
        let grant = index.and_then(|index| grant_of(manifest, index, proxy));
        let policy = Policy::of(grant.as_ref());
        let allowed = policy.allows(&statement.sql);

        if let Some(counts) = counts.get_mut(proxy) {
            counts.statements += 1;
            if !allowed {
                counts.denied += 1;
            }
        } else {
            // An observation on a proxy the manifest never declared cannot be
            // granted: it is a denial, but no declared counter owns it.
            continue;
        }

        let Some(index) = index else {
            continue;
        };
        if allowed {
            matched
                .entry(index)
                .or_default()
                .insert(normalize(&statement.sql));
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };

        // R33: only a statement the learn policy proved grantable may become
        // `add_query`; a redacted literal, and every transaction-control
        // denial, is `observe`.
        let (sql, fix) = if statement.is_tx {
            (
                transaction_label(&statement.sql).to_string(),
                Fix::observe(),
            )
        } else if let ObsVerdict::Query { normalized } = &statement.verdict
            && !normalized.contains('\u{2026}')
        {
            // A connection-setup statement is not a route's statement: it runs
            // on whichever route opens the connection first, so a per-route
            // grant would leave the others denying it (`MA.8`). The documented
            // fix is one `connection` group attached to every route.
            let fix = if is_connection_setup(normalized) {
                Fix::add_group(format!("[group.connection.{proxy}].queries"))
            } else {
                Fix::add_query(format!("[route.{proxy}].queries"))
            };
            (normalized.clone(), fix)
        } else {
            (statement.sql.clone(), Fix::observe())
        };
        missing.insert(
            (index, proxy.to_string(), sql.clone()),
            Finding::MissingGrant {
                severity: Severity::Error,
                route: route.name().to_string(),
                proxy: proxy.to_string(),
                sql,
                fix,
            },
        );
    }

    let mut findings: Vec<Finding> = missing.into_values().collect();
    dead_route_grants(manifest, &requests_per_route, &matched, &mut findings);
    dead_group_grants(manifest, &requests_per_route, &matched, &mut findings);
    unexercised_routes(manifest, &requests_per_route, observations, &mut findings);
    ingress_findings(manifest, &bodies_per_route, &mut findings);
    raw_ingress_findings(manifest, &mut findings);
    for (method, path) in unrouted {
        findings.push(Finding::UnroutedRequest {
            severity: Severity::Error,
            method,
            path,
            fix: Fix::add_route(),
        });
    }

    let exercised = requests_per_route
        .iter()
        .filter(|count| **count > 0)
        .count() as u64;
    let statements: u64 = counts.values().map(|count| count.statements).sum();
    if exercised == 0 || (!manifest.proxies().is_empty() && statements == 0) {
        findings.push(Finding::NoObservations {
            severity: Severity::Error,
            fix: Fix::observe(),
        });
    }
    for (name, count) in &counts {
        if count.statements == 0 {
            findings.push(Finding::ProxyUnused {
                severity: Severity::Error,
                proxy: name.clone(),
                fix: Fix::observe(),
            });
        }
    }

    let ok = findings
        .iter()
        .all(|finding| finding.severity() != Severity::Error);
    Ok(Report {
        schema_version: SCHEMA_VERSION,
        ok,
        requests: observations.http().len() as u64,
        statements,
        routes: RouteCounts {
            total: manifest.route_count() as u64,
            exercised,
        },
        proxies: counts,
        findings,
    })
}

/// The body sample of one observed request, deriving one when the gate sent
/// none.
fn sample_of(line: &HttpObsLine) -> BodySample {
    line.sample.clone().unwrap_or_else(|| BodySample {
        mime: parse_mime(line.content_type.as_deref()).0,
        truncated: false,
        valid_json: false,
        valid_form: false,
    })
}

/// The label a transaction-control denial is shown under (`R33`).
///
/// A savepoint's name never appears: `SAVEPOINT s1` and `RELEASE SAVEPOINT s1`
/// are the same denial, and the manifest flag that fixes it is one flag.
fn transaction_label(sql: &str) -> &'static str {
    let first = normalize(sql)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match first.as_str() {
        "START" => "BEGIN",
        "COMMIT" | "END" => "COMMIT",
        "ROLLBACK" => "ROLLBACK",
        "SAVEPOINT" => "SAVEPOINT",
        "RELEASE" => "RELEASE",
        _ => "TRANSACTION",
    }
}

/// Whether a denied statement is the driver's own connection setup.
///
/// `blastit check` learned the statement on one route, but a session-setup
/// statement is not that route's: it runs on whichever route opens the
/// connection first, and every other route would still deny it. The manifest
/// fix is one group attached to all of them, so the `fix` names
/// `[group.connection.<proxy>]` instead of `[route.<proxy>].queries` (`MA.8`).
///
/// The learn policy's replay list is wider than this on purpose: a constant
/// `SELECT 1` and a catalogue read are replayable too, but they are ordinary
/// statements whose per-route grant is correct — only the static setup
/// constants (`SELECT set_config('TimeZone', $?, false)`,
/// `SET TIME ZONE 'UTC'`) and a single `SET`/`SHOW` are session setup.
fn is_connection_setup(normalized: &str) -> bool {
    let text = normalize(normalized);
    if KNOWN_STATIC_SETUP_SQL.contains(&text.as_str()) {
        return true;
    }
    let first = text.split_whitespace().next().unwrap_or_default();
    (first.eq_ignore_ascii_case("SET") || first.eq_ignore_ascii_case("SHOW"))
        && is_introspection_setup_sql(&text)
}

/// `dead_grant` for the routes' own `queries` entries (`R34`).
fn dead_route_grants(
    manifest: &Manifest,
    requests_per_route: &[u64],
    matched: &BTreeMap<usize, BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    let empty = BTreeSet::new();
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    for (index, requests) in requests_per_route.iter().enumerate() {
        if *requests == 0 {
            // An unexercised route says so once; its grants are not dead.
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        let matched_here = matched.get(&index).unwrap_or(&empty);
        for (proxy, grant) in manifest.effective_grants(index) {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            for query in pg.queries() {
                let normalized = normalize(query);
                if normalized.is_empty() || matched_here.contains(&normalized) {
                    continue;
                }
                // A group's query is reported once, by `dead_group_grants`.
                if manifest.is_query_covered_by_tables_or_groups(index, &proxy, &normalized) {
                    continue;
                }
                let key = (
                    route.name().to_string(),
                    proxy.as_str().to_string(),
                    normalized.clone(),
                );
                if !seen.insert(key) {
                    continue;
                }
                findings.push(Finding::DeadGrant {
                    severity: Severity::Error,
                    route: route.name().to_string(),
                    group: None,
                    proxy: proxy.as_str().to_string(),
                    sql: normalized,
                    fix: Fix::remove_query(format!("[route.{}].queries", proxy.as_str())),
                });
            }
        }
    }
}

/// `dead_grant` for attached groups, once per group/proxy/statement (`R34`).
fn dead_group_grants(
    manifest: &Manifest,
    requests_per_route: &[u64],
    matched: &BTreeMap<usize, BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    // Which routes attach each group; a global group attaches to all of them.
    let mut attaching: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for name in manifest.global_groups() {
        attaching
            .entry(name.clone())
            .or_default()
            .extend(0..manifest.route_count());
    }
    for (index, route) in manifest.routes().iter().enumerate() {
        for name in route.groups() {
            attaching.entry(name.clone()).or_default().insert(index);
        }
    }

    for (name, indices) in attaching {
        let Some(group) = manifest.group(&name) else {
            continue;
        };
        let exercised: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|index| {
                requests_per_route
                    .get(*index)
                    .is_some_and(|count| *count > 0)
            })
            .collect();
        let Some(first) = exercised.first().copied() else {
            // The group is attached only to routes nothing reached: those
            // routes are `unexercised`, and their grants are not dead yet.
            continue;
        };
        for (proxy, grant) in group.grants() {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            for query in pg.queries() {
                let normalized = normalize(query);
                if normalized.is_empty() {
                    continue;
                }
                let used = exercised.iter().any(|index| {
                    matched
                        .get(index)
                        .is_some_and(|matched| matched.contains(&normalized))
                });
                if used {
                    continue;
                }
                let route_name = manifest
                    .route(first)
                    .map(|route| route.name().to_string())
                    .unwrap_or_default();
                findings.push(Finding::DeadGrant {
                    severity: Severity::Error,
                    route: route_name,
                    group: Some(name.clone()),
                    proxy: proxy.as_str().to_string(),
                    sql: normalized,
                    fix: Fix::remove_query(format!("[group.{name}.{}].queries", proxy.as_str())),
                });
            }
        }
    }
}

/// `unexercised_route`, suppressed when the suite observed no request at all
/// (`R35`: `no_observations` says that instead).
fn unexercised_routes(
    manifest: &Manifest,
    requests_per_route: &[u64],
    observations: &CheckObsV1,
    findings: &mut Vec<Finding>,
) {
    if observations.http().is_empty() {
        return;
    }
    for (index, requests) in requests_per_route.iter().enumerate() {
        if *requests > 0 {
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        findings.push(Finding::UnexercisedRoute {
            severity: Severity::Error,
            route: route.name().to_string(),
            fix: Fix::add_test(),
        });
    }
}

/// `missing_ingress` and `ingress_mismatch` (`R36`).
fn ingress_findings(
    manifest: &Manifest,
    bodies_per_route: &[Vec<BodySample>],
    findings: &mut Vec<Finding>,
) {
    for (index, samples) in bodies_per_route.iter().enumerate() {
        if samples.is_empty() {
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        let Some(policy) = route.ingress() else {
            let (codecs, allow_raw) = suggest_fix(samples);
            findings.push(Finding::MissingIngress {
                severity: Severity::Error,
                route: route.name().to_string(),
                observed: distinct_mimes(samples),
                fix: Fix::declare_ingress(codecs, allow_raw),
            });
            continue;
        };
        if policy.body == BodyCodec::Raw {
            // `raw` accepts any bytes; whether it was acknowledged is static
            // (`raw_ingress_findings`).
            continue;
        }
        let mismatching: Vec<BodySample> = samples
            .iter()
            .filter(|sample| !codec_accepts(policy, sample))
            .cloned()
            .collect();
        if mismatching.is_empty() {
            continue;
        }
        let (codecs, allow_raw) = suggest_fix(&mismatching);
        findings.push(Finding::IngressMismatch {
            severity: Severity::Error,
            route: route.name().to_string(),
            declared: policy.body,
            observed: distinct_mimes(&mismatching),
            fix: Fix::fix_ingress(codecs, allow_raw),
        });
    }
}

/// The static `raw_ingress` finding (`R36`).
fn raw_ingress_findings(manifest: &Manifest, findings: &mut Vec<Finding>) {
    for (index, _) in manifest.routes().iter().enumerate() {
        let Some(route) = manifest.route(index) else {
            continue;
        };
        if let Some(policy) = route.ingress()
            && policy.body == BodyCodec::Raw
            && !policy.allow_raw
        {
            findings.push(Finding::RawIngress {
                severity: Severity::Error,
                route: route.name().to_string(),
                fix: Fix::fix_raw_ingress("[route.ingress]".to_string()),
            });
        }
    }
}

/// Whether a declared codec accepts one observed sample.
fn codec_accepts(policy: IngressPolicy, sample: &BodySample) -> bool {
    match policy.body {
        BodyCodec::Json => sample.valid_json,
        BodyCodec::Form => sample.valid_form,
        BodyCodec::None => false,
        BodyCodec::Raw => true,
    }
}

/// The distinct MIME essences of a set of samples, sorted.
fn distinct_mimes(samples: &[BodySample]) -> Vec<String> {
    samples
        .iter()
        .map(|sample| sample.mime.clone())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// Renders the report as the byte-exact `§1.3` human block.
///
/// A clean report is the one-line form with `, clean`; a report with findings is
/// the header, one line per finding (with the missing statements indented under
/// their route), and the `--json` footer.
pub fn human(report: &Report) -> String {
    let mut out = header(report);
    if report.findings.is_empty() {
        out.push_str(", clean\n");
        return out;
    }
    out.push('\n');
    let mut at = 0;
    while at < report.findings.len() {
        match &report.findings[at] {
            Finding::MissingGrant { route, .. } => {
                let mut statements = Vec::new();
                while let Some(Finding::MissingGrant {
                    route: same, sql, ..
                }) = report.findings.get(at)
                {
                    if same != route {
                        break;
                    }
                    statements.push(sql.clone());
                    at += 1;
                }
                out.push_str(&format!(
                    "  route {route}: {} {} not granted\n",
                    statements.len(),
                    plural(statements.len(), "statement")
                ));
                for sql in statements {
                    out.push_str(&format!("    {sql}\n"));
                }
            }
            Finding::DeadGrant { route, group, .. } => {
                let mut count = 0;
                while let Some(Finding::DeadGrant {
                    route: same_route,
                    group: same_group,
                    ..
                }) = report.findings.get(at)
                {
                    if same_route != route || same_group != group {
                        break;
                    }
                    count += 1;
                    at += 1;
                }
                let noun = plural(count, "grant");
                match group {
                    Some(group) => {
                        out.push_str(&format!("  group {group}: {count} {noun} never used\n"));
                    }
                    None => {
                        out.push_str(&format!("  route {route}: {count} {noun} never used\n"));
                    }
                }
            }
            Finding::UnexercisedRoute { route, .. } => {
                out.push_str(&format!("  route {route}: never exercised\n"));
                at += 1;
            }
            Finding::MissingIngress { route, .. } => {
                out.push_str(&format!(
                    "  route {route}: takes a body and declares no ingress validation\n"
                ));
                at += 1;
            }
            Finding::IngressMismatch {
                route,
                declared,
                observed,
                ..
            } => {
                let first = observed.first().map(String::as_str).unwrap_or("nothing");
                let declared = match declared {
                    BodyCodec::Json => "json",
                    BodyCodec::Form => "form",
                    BodyCodec::None => "none",
                    BodyCodec::Raw => "raw",
                };
                out.push_str(&format!(
                    "  route {route}: declared body = \"{declared}\" does not match observed {first}\n"
                ));
                at += 1;
            }
            Finding::RawIngress { route, .. } => {
                out.push_str(&format!(
                    "  route {route}: declares body = \"raw\" without allow_raw = true\n"
                ));
                at += 1;
            }
            Finding::UnroutedRequest { method, path, .. } => {
                out.push_str(&format!("  request {method} {path}: no route\n"));
                at += 1;
            }
            Finding::NoObservations { .. } => {
                out.push_str("  suite exercised 0 routes or 0 proxy statements\n");
                at += 1;
            }
            Finding::ProxyUnused { proxy, .. } => {
                out.push_str(&format!(
                    "  proxy {proxy}: declared, 0 statements observed\n"
                ));
                at += 1;
            }
        }
    }
    out.push_str(&format!(
        "blastit check: {} {} · `--json` for the machine-readable list\n",
        report.findings.len(),
        plural(report.findings.len(), "problem")
    ));
    out
}

/// The `blastit check: …` header line, without `, clean`.
fn header(report: &Report) -> String {
    let mut line = format!(
        "blastit check: {} requests, {} statements, {}/{} routes",
        report.requests, report.statements, report.routes.exercised, report.routes.total
    );
    if report.proxies.len() > 1 {
        line.push_str(&format!(", {} proxies", report.proxies.len()));
    }
    line
}

/// `singular` or its plural, for the exact `§1.3` suffixes.
fn plural(count: usize, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
    }
}

/// Renders `_blastit.probe_ingress`: the bounded body sample as JSON.
///
/// # Errors
///
/// [`ObsError::Report`] only when the sample cannot be rendered, which cannot
/// happen in practice.
pub fn probe_ingress_json(
    content_type: Option<&str>,
    prefix: &[u8],
    total_len: u64,
) -> Result<String, ObsError> {
    let sample = probe_sample(content_type, prefix, total_len);
    serde_json::to_string(&sample).map_err(|error| ObsError::Report(error.to_string()))
}

/// What one `--learn` run proposes, and what it refused (`R27`, `R29`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnPlan {
    /// Routes and exact `queries` only.
    pub edits: ManifestEdits,
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
        self.edits.routes.len()
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

/// Plans the routes and exact queries an unrouted observation run adds (`R27`).
///
/// Only `ObsTarget::Unrouted` statements are mapped: they belong to a route
/// that does not exist yet, so they cannot be granted anywhere today. The
/// route's name and path come from [`infer`]; the statement is the learn
/// policy's normalised text. Redacted literals are refused (they make the plan
/// unwritable), transaction control is a hint, and a table grant is never
/// invented.
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
    for line in observations.http() {
        if table.route(&line.method, &line.path).is_none() {
            paths.insert(line.path.clone());
        }
    }
    for statement in observations.sql() {
        if let ObsTarget::Unrouted { path, .. } = &statement.target {
            paths.insert(path.clone());
        }
    }
    let plan = infer(paths.iter().map(String::as_str), manifest);

    // Any refused statement in the run makes the whole plan unwritable: the
    // manifest after the edit would still be incomplete (`R25`, `R29`).
    let mut refused: BTreeSet<String> = BTreeSet::new();
    for statement in observations.sql() {
        if let ObsVerdict::Refused { redacted, reason } = &statement.verdict {
            refused.insert(format!("{redacted}: {reason}"));
        }
    }

    let mut hints: BTreeSet<String> = BTreeSet::new();
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for statement in observations.sql() {
        let ObsTarget::Unrouted { path, .. } = &statement.target else {
            continue;
        };
        let Some(route) = plan.by_path.get(path) else {
            continue;
        };
        if statement.is_tx {
            hints.insert(format!(
                "transaction control is not granted automatically: set transaction_control = true on [route.{}]",
                statement.proxy
            ));
            continue;
        }
        if let ObsVerdict::Query { normalized } = &statement.verdict {
            if normalized.contains('\u{2026}') {
                // A redaction is never grantable, whatever a broken verdict
                // says (`R24`, `R33`): the plan stays unwritable.
                refused.insert(format!("{}: a redacted statement", normalized));
            } else {
                grouped
                    .entry((route.clone(), statement.proxy.clone()))
                    .or_default()
                    .insert(normalized.clone());
            }
        }
    }

    let mut edits = ManifestEdits {
        routes: plan
            .routes
            .iter()
            .map(|route| ProposedRoute {
                name: route.name.clone(),
                path: route.paths.clone(),
            })
            .collect(),
        grants: Vec::new(),
    };
    for ((route, proxy), queries) in grouped {
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
                route,
                proxy,
                queries,
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

/// `check_audit` + `plan_learn`: the `--learn --json` document behind
/// `_blastit.plan_learn` and `blastit check --learn --json`.
///
/// Prints the transaction-control hints to stderr, then returns the document.
///
/// # Errors
///
/// Every [`ObsError`] of [`check_audit`].
pub fn plan_learn_json(
    manifest_toml: &str,
    http_jsonl: &str,
    sql_jsonl: &str,
    dry_run: bool,
) -> Result<String, ObsError> {
    let manifest = parse_manifest(manifest_toml)?;
    let observations = CheckObsV1::from_jsonl(http_jsonl, sql_jsonl)?;
    let plan = plan_learn(&manifest, &observations)?;
    plan.emit_hints();
    Ok(plan.json(dry_run))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
        version = 1
        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }
        [[route]]
        name = "notes"
        path = ['/notes/.*']
        [route.db]
        queries = ['SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1']
    "#;

    fn routed(query: Option<&str>, sql: &str, is_tx: bool) -> String {
        let verdict = match query {
            Some(normalized) => json!({"kind": "query", "normalized": normalized}),
            None => json!({"kind": "refused", "redacted": sql, "reason": "inline literal"}),
        };
        json!({
            "target": {"scope": "routed", "route": "notes"},
            "proxy": "db",
            "kind": "query",
            "sql": sql,
            "is_tx": is_tx,
            "allowed": true,
            "verdict": verdict,
        })
        .to_string()
    }

    fn http(method: &str, path: &str) -> String {
        json!({"method": method, "path": path, "content_type": null, "body_len": 0}).to_string()
    }

    fn run(sql: &str, http: &str) -> Report {
        let manifest = parse_manifest(MANIFEST).expect("the fixture manifest validates");
        let observations = CheckObsV1::from_jsonl(http, sql).expect("the observations parse");
        evaluate(&manifest, &observations).expect("the audit runs")
    }

    #[test]
    fn a_redacted_denial_is_observe_never_add_yet_never_query() {
        let sql = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."name" = …"#;
        let report = run(&routed(None, sql, false), &http("GET", "/notes/"));
        assert_eq!(report.findings[0].severity(), Severity::Error);
        let missing = report
            .findings
            .iter()
            .find(|finding| matches!(finding, Finding::MissingGrant { .. }))
            .expect("the denied statement is a finding");
        match missing {
            Finding::MissingGrant {
                fix, sql: shown, ..
            } => {
                assert_eq!(fix.action, "observe");
                assert_eq!(shown, sql);
            }
            other => panic!("unexpected finding: {other:?}"),
        }
    }

    #[test]
    fn a_savepoint_is_shown_without_its_name_and_never_granted() {
        let report = run(
            &routed(Some("SAVEPOINT s1"), "SAVEPOINT s1", true),
            &http("POST", "/notes/"),
        );
        match &report.findings[0] {
            Finding::MissingGrant { fix, sql, .. } => {
                assert_eq!(fix.action, "observe");
                assert_eq!(sql, "SAVEPOINT");
            }
            other => panic!("unexpected finding: {other:?}"),
        }
    }

    #[test]
    fn an_unrouted_statement_never_produces_a_missing_grant() {
        let sql = json!({
            "target": {"scope": "unrouted", "method": "GET", "path": "/oops/"},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": true,
            "verdict": {"kind": "query", "normalized": "SELECT 1"},
        })
        .to_string();
        let report = run(&sql, &http("GET", "/oops/"));
        assert!(
            report
                .findings
                .iter()
                .all(|finding| !matches!(finding, Finding::MissingGrant { .. }))
        );
        assert_eq!(report.proxies["db"].statements, 1);
        assert_eq!(report.proxies["db"].denied, 1);
        assert_eq!(report.statements, 1);
    }

    #[test]
    fn malformed_observations_name_the_file_and_line() {
        let error = CheckObsV1::from_jsonl("{\"method\":\"GET\",\"path\":\"/\"}\n{not json}\n", "")
            .expect_err("malformed");
        assert!(error.to_string().contains("http_obs.jsonl"), "{error}");
        assert!(error.to_string().contains("line 2"), "{error}");
    }

    #[test]
    fn a_schema_version_other_than_one_is_refused() {
        let error = CheckObsV1::new(2, Vec::new(), Vec::new()).expect_err("bad version");
        assert!(matches!(
            error,
            ObsError::SchemaVersion {
                found: 2,
                expected: 1
            }
        ));
    }
}
