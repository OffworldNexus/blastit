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
//! human `§1.3` form, naming for every finding the facts `--json` carries for
//! it: the statement a grant finding is about, the proxy it ran on, the MIME
//! types a body finding observed, and the `Blastfile` node its `fix` edits.
//!
//! Two rules protect the machine contract:
//!
//! * `add_query` is emitted **only** for a statement dbproxy's learn policy
//!   proved grantable (`LearnVerdict::Query`, never a redacted `…`). A redacted
//!   literal, and every transaction-control denial, names `observe` instead, so
//!   every `add_query` this engine emits passes
//!   [`Manifest::parse_blastfile`] once written (`R33`);
//! * a grantable statement that is the driver's own session setup — `SELECT
//!   set_config('TimeZone', $?, false)`, `SET …`, `SHOW …` — names `add_group`
//!   and the `trait connection` node instead of one view's grants: it runs on
//!   whichever view opens the connection first, so a per-view grant would leave
//!   every other one denying it, so the hint is the trait every view `use`s
//!   (`MA.8`, §13.3);
//! * SQL under [`ObsTarget::Unrouted`] counts towards `statements`/`denied`
//!   and never produces a route-less `missing_grant` (`R32`).
//!
//! [`plan_learn`] is the `--learn` side: it maps unrouted SQL through
//! [`crate::infer_routes`] into the proposed view's `postgres { query … }`
//! block, writes only views and exact queries, and refuses to invent table
//! grants or a `transactions` flag (`R27`).
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

use blastit_dbproxy::ObsTarget;
use blastit_ingress::{BodySample, parse_mime};
use blastit_manifest::{Manifest, ProxyName, RoutingTable};
use blastit_sqlpolicy::{
    KNOWN_STATIC_SETUP_SQL, LearnVerdict, is_introspection_setup_sql, normalize,
};

use crate::dead::{dead_group_grants, dead_route_grants, unexercised_routes};
use crate::diff::{GrantCheck, grant_locator, trait_locator};
use crate::error::ObsError;
use crate::findings::{Finding, Fix, ProxyCounts, Report, RouteCounts, Severity};
use crate::ingress::{ingress_findings, raw_ingress_findings};
use crate::ledger::{Decision, Ledger};
use crate::obs::{CheckObsV1, HttpObsLine, SCHEMA_VERSION};

/// The trait every view attaches so a connection-setup statement is granted once
/// (`MA.8`, §13.3): `trait connection { … }` plus `use connection`.
const CONNECTION_TRAIT: &str = "connection";
/// Parses and validates a manifest document.
///
/// # Errors
///
/// [`ObsError::Manifest`] with the manifest's own one-line diagnostics.
pub fn parse_manifest(source: &str) -> Result<Manifest, ObsError> {
    let (manifest, _warnings) =
        Manifest::parse_blastfile(source, blastit_manifest::UidLayout::DEFAULT_BASE)
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
    manifest_text: &str,
    http_jsonl: &str,
    sql_jsonl: &str,
) -> Result<String, ObsError> {
    let manifest = parse_manifest(manifest_text)?;
    let observations = CheckObsV1::from_jsonl(http_jsonl, sql_jsonl)?;
    evaluate(&manifest, &observations)?.to_json()
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
    let mut bodies_per_route: Vec<Vec<BodySample>> = vec![Vec::new(); manifest.route_count()];
    let mut unrouted: BTreeSet<(String, String)> = BTreeSet::new();
    // The one grant diff: request counts, the statements each pair was allowed
    // and the ones it was denied, all in the same record the pytest report
    // reads — so `check`'s `dead_grant`/`missing_grant` and `audit`'s
    // `unused`/`missing` cannot disagree about what happened.
    let mut ledger = Ledger::new(manifest);
    for line in observations.http() {
        match table.route(&line.method, &line.path) {
            Some(matched) => {
                ledger.request(matched.route, false);
                if line.body_len > 0 {
                    bodies_per_route[matched.route].push(sample_of(line));
                }
            }
            None => {
                unrouted.insert((line.method.clone(), line.path.clone()));
            }
        }
    }

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
        let decision = match index {
            Some(index) => ledger.record(
                index,
                &ProxyName::new(proxy.to_string()),
                &statement.sql,
                None,
                None,
            ),
            // No route owns the statement, so no grant can allow it — and it
            // produces no finding, only a denial in the proxy's counter.
            None => Decision {
                normalized: normalize(&statement.sql),
                granted: GrantCheck::of(None).allows(&statement.sql),
                transaction_control: false,
            },
        };
        let allowed = decision.granted;

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
        } else if let LearnVerdict::Query { normalized } = &statement.verdict
            && !normalized.contains('\u{2026}')
        {
            // A connection-setup statement is not a route's statement: it runs
            // on whichever route opens the connection first, so a per-route
            // grant would leave the others denying it (`MA.8`). The documented
            // fix is one `connection` trait attached to every view.
            let fix = if is_connection_setup(normalized) {
                Fix::add_group(trait_locator(CONNECTION_TRAIT))
            } else {
                Fix::add_query(grant_locator(route.name(), proxy))
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
    dead_route_grants(manifest, &ledger, &mut findings);
    dead_group_grants(manifest, &ledger, &mut findings);
    unexercised_routes(manifest, &ledger, observations, &mut findings);
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

    let exercised = ledger
        .routes()
        .filter(|(_, route)| route.requests > 0)
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
