//! Protected critical tests for the framework-agnostic `blastit check` engine
//! (`R27`, `R28`, `R32..R38`; `docs/design/ma-lean-dx.md` §1.3, §4.2 E,
//! §7.2).
//!
//! Every test pairs an allowed case with its negative twin: an `add_query` that
//! re-parses, against a redacted `…` and a `SAVEPOINT` that must never become
//! one; the `add_group` a session-setup statement gets, against `SELECT 1` and
//! an ordinary `DELETE` that keep the per-route `add_query`; an unrouted
//! request that proposes a route, against an unrouted SQL statement that must
//! never produce a route-less `missing_grant`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use blastit::audit_v1::{
    self, CheckObsV1, Finding, ObsError, ProxyCounts, Report, SCHEMA_VERSION, Severity,
};
use blastit::infer_routes::infer;
use blastit_manifest::edit::{ManifestEdits, ProposedGrant, ProposedRoute, apply_edits};
use blastit_manifest::{Manifest, ProxyName, UidLayout};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const PROXIES: &str = r#"
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }
"#;

/// A manifest with only routes: no proxy, no groups.
fn routes_manifest(routes: &[(&str, &str)]) -> Manifest {
    let mut source = String::from("version = 1\n");
    for (name, path) in routes {
        source.push_str(&format!("[[route]]\nname = \"{name}\"\npath = {path}\n"));
    }
    Manifest::from_toml_str(&source).expect("the fixture manifest parses")
}

fn http_line(
    method: &str,
    path: &str,
    content_type: Option<&str>,
    body_len: u64,
    sample: Option<Value>,
) -> String {
    json!({
        "method": method,
        "path": path,
        "content_type": content_type,
        "body_len": body_len,
        "sample": sample,
    })
    .to_string()
}

fn routed_sql(route: &str, proxy: &str, sql: &str, verdict: Value, is_tx: bool) -> String {
    json!({
        "target": {"scope": "routed", "route": route},
        "proxy": proxy,
        "kind": "query",
        "sql": sql,
        "is_tx": is_tx,
        "allowed": true,
        "verdict": verdict,
    })
    .to_string()
}

fn unrouted_sql(
    method: &str,
    path: &str,
    proxy: &str,
    sql: &str,
    verdict: Value,
    is_tx: bool,
) -> String {
    json!({
        "target": {"scope": "unrouted", "method": method, "path": path},
        "proxy": proxy,
        "kind": "query",
        "sql": sql,
        "is_tx": is_tx,
        "allowed": true,
        "verdict": verdict,
    })
    .to_string()
}

fn grantable(sql: &str) -> Value {
    json!({"kind": "query", "normalized": sql})
}

fn refused(sql: &str) -> Value {
    json!({
        "kind": "refused",
        "redacted": sql,
        "reason": "the statement inlines a value; parameterise it first",
    })
}

fn observations(http: &[String], sql: &[String]) -> CheckObsV1 {
    CheckObsV1::from_jsonl(&http.join("\n"), &sql.join("\n")).expect("the observations parse")
}

fn run(manifest: &str, http: &[String], sql: &[String]) -> Report {
    let manifest = audit_v1::parse_manifest(manifest).expect("the manifest validates");
    audit_v1::evaluate(&manifest, &observations(http, sql)).expect("the audit runs")
}

/// The number of findings of every `kind`, from the serialised document.
fn kinds(report: &Report) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for finding in &report.findings {
        let value = serde_json::to_value(finding).expect("a finding renders");
        let kind = value["kind"].as_str().expect("every finding has a kind");
        *counts.entry(kind.to_string()).or_insert(0) += 1;
    }
    counts
}

// ---------------------------------------------------------------------------
// R28: all thirteen `infer_routes` fixture rows
// ---------------------------------------------------------------------------

struct Row {
    paths: &'static [&'static str],
    existing: &'static [(&'static str, &'static str)],
    name: &'static str,
    patterns: &'static [&'static str],
    insert_at: usize,
}

#[test]
fn infer_routes_reproduces_all_thirteen_rows() {
    let rows = [
        Row {
            paths: &["/"],
            existing: &[("products", "['/products/']")],
            name: "root",
            patterns: &["/"],
            insert_at: 1,
        },
        Row {
            paths: &["/products/"],
            existing: &[],
            name: "products",
            patterns: &["/products/"],
            insert_at: 0,
        },
        Row {
            paths: &["/products/42/", "/products/99/"],
            existing: &[],
            name: "products",
            patterns: &["/products/[0-9]+/"],
            insert_at: 0,
        },
        Row {
            paths: &["/products/", "/products/42/"],
            existing: &[],
            name: "products",
            patterns: &["/products/", "/products/[0-9]+/"],
            insert_at: 0,
        },
        Row {
            paths: &["/orders/550e8400-e29b-41d4-a716-446655440000"],
            existing: &[],
            name: "orders",
            patterns: &["/orders/[^/]+"],
            insert_at: 0,
        },
        Row {
            paths: &["/123/456"],
            existing: &[],
            name: "route",
            patterns: &["/[0-9]+/[0-9]+"],
            insert_at: 0,
        },
        Row {
            paths: &["/api.v2/items+list/"],
            existing: &[],
            name: "api-v2",
            patterns: &[r"/api\.v2/items\+list/"],
            insert_at: 0,
        },
        Row {
            paths: &["/_internal/ping"],
            existing: &[],
            name: "internal",
            patterns: &["/_internal/ping"],
            insert_at: 0,
        },
        Row {
            paths: &["/café/1"],
            existing: &[],
            name: "caf",
            patterns: &["/café/[0-9]+"],
            insert_at: 0,
        },
        Row {
            paths: &["/products/café/"],
            existing: &[],
            name: "products",
            patterns: &["/products/café/"],
            insert_at: 0,
        },
        Row {
            paths: &["/products/42/"],
            existing: &[("products", "['/products/']")],
            name: "products-2",
            patterns: &["/products/[0-9]+/"],
            insert_at: 1,
        },
        Row {
            paths: &["/robots.txt"],
            existing: &[("products", "['/products/']"), ("fallback", "['.*']")],
            name: "robots-txt",
            patterns: &[r"/robots\.txt"],
            insert_at: 1,
        },
        Row {
            paths: &["/robots.txt"],
            existing: &[("products", "['/products/']"), ("fallback", r#"".*""#)],
            name: "robots-txt",
            patterns: &[r"/robots\.txt"],
            insert_at: 1,
        },
    ];
    for (index, row) in rows.iter().enumerate() {
        let manifest = routes_manifest(row.existing);
        let plan = infer(row.paths.iter().copied(), &manifest);
        assert_eq!(plan.routes.len(), 1, "row {}", index + 1);
        assert_eq!(plan.routes[0].name, row.name, "row {}", index + 1);
        assert_eq!(plan.routes[0].paths, row.patterns, "row {}", index + 1);
        assert_eq!(plan.insert_at, row.insert_at, "row {}", index + 1);
        for path in row.paths {
            assert_eq!(
                plan.by_path.get(*path).map(String::as_str),
                Some(row.name),
                "row {} maps {path}",
                index + 1
            );
        }
    }
}

// ---------------------------------------------------------------------------
// R27 golden: unrouted POST /orders/42/ with two queries
// ---------------------------------------------------------------------------

const GOLDEN_MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "home"
path = ['/']
"#;

const Q1: &str = r#"INSERT INTO "orders" ("id") VALUES ($?)"#;
const Q2: &str = r#"SELECT "orders"."id" FROM "orders" WHERE "orders"."id" = $?"#;

fn golden_http() -> Vec<String> {
    vec![http_line(
        "POST",
        "/orders/42/",
        Some("application/json"),
        2,
        Some(json!({
            "mime": "application/json",
            "truncated": false,
            "valid_json": true,
            "valid_form": false,
        })),
    )]
}

fn golden_sql() -> Vec<String> {
    vec![
        unrouted_sql("POST", "/orders/42/", "db", Q1, grantable(Q1), false),
        unrouted_sql("POST", "/orders/42/", "db", Q2, grantable(Q2), false),
    ]
}

#[test]
fn the_r27_golden_proposes_one_orders_route_with_both_exact_queries() {
    let manifest = audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("the manifest validates");
    let plan = audit_v1::plan_learn(&manifest, &observations(&golden_http(), &golden_sql()))
        .expect("the plan is built");

    assert_eq!(plan.routes(), 1);
    assert_eq!(plan.queries(), 2);
    assert_eq!(
        plan.edits.routes,
        [ProposedRoute {
            name: "orders".to_string(),
            path: vec!["/orders/[0-9]+/".to_string()],
        }]
    );
    assert_eq!(plan.edits.grants.len(), 1);
    let grant = &plan.edits.grants[0];
    assert_eq!(grant.route, "orders");
    assert_eq!(grant.proxy, "db");
    let mut expected = vec![Q1.to_string(), Q2.to_string()];
    expected.sort();
    assert_eq!(grant.queries, expected, "exactly the two observed queries");
    assert!(plan.would_write(false));
    assert!(!plan.would_write(true), "a dry run writes nothing");

    // The proposal is a manifest: `[[route]] name = "orders"` with those
    // exact queries, and every untouched field survives.
    let edited = apply_edits(GOLDEN_MANIFEST, &plan.edits).expect("the edit applies");
    assert!(edited.contains("[[route]]"));
    assert!(edited.contains("name = \"orders\""));
    assert!(edited.contains("path = ['/orders/[0-9]+/']"));
    let (parsed, _warnings) = Manifest::parse_and_validate(&edited, UidLayout::DEFAULT_BASE)
        .expect("the edited manifest boots");
    let route = parsed
        .routes()
        .iter()
        .find(|route| route.name() == "orders")
        .expect("the inferred route is declared");
    assert_eq!(route.path(), ["/orders/[0-9]+/"]);
    let grant = route
        .grant(&ProxyName::new("db".to_string()))
        .and_then(|grant| grant.as_postgresql())
        .expect("the inferred grant is declared");
    for query in [Q1, Q2] {
        assert!(
            grant.queries().iter().any(|candidate| candidate == query),
            "{query} is in the proposed grant"
        );
    }
    // `--learn` never invents table grants.
    assert!(!edited.contains("[route.db.tables]"), "{edited}");

    // The `--learn --json` document: the proposal and the write decision, and
    // intentionally no `schema_version`.
    let http = golden_http().join("\n");
    let sql = golden_sql().join("\n");
    let document: Value = serde_json::from_str(
        &audit_v1::plan_learn_json(GOLDEN_MANIFEST, &http, &sql, false)
            .expect("the document renders"),
    )
    .expect("the document is JSON");
    assert_eq!(document["proposed"]["routes"], 1);
    assert_eq!(document["proposed"]["queries"], 2);
    assert_eq!(document["wrote"], true);
    assert!(document.get("schema_version").is_none());
}

#[test]
fn the_r27_golden_is_a_clean_audit_after_the_edit() {
    // The same two statements, now granted on the proposed route: nothing is
    // missing, so the proposal really did complete the manifest.
    let edited = apply_edits(
        GOLDEN_MANIFEST,
        &ManifestEdits {
            routes: vec![ProposedRoute {
                name: "orders".to_string(),
                path: vec!["/orders/[0-9]+/".to_string()],
            }],
            grants: vec![ProposedGrant {
                route: "orders".to_string(),
                proxy: "db".to_string(),
                queries: vec![Q1.to_string(), Q2.to_string()],
            }],
        },
    )
    .expect("the edit applies");
    let report = run(
        &edited,
        &[
            http_line("GET", "/", None, 0, None),
            http_line("POST", "/orders/42/", None, 0, None),
        ],
        &[
            routed_sql("orders", "db", Q1, grantable(Q1), false),
            routed_sql("orders", "db", Q2, grantable(Q2), false),
        ],
    );
    assert_eq!(report.findings, []);
    assert!(report.ok);
    assert_eq!(report.routes.exercised, 2);
}

// ---------------------------------------------------------------------------
// R33: every `add_query` is bootable
// ---------------------------------------------------------------------------

const RE_PARSE_MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }

[[route]]
name = "catalog"
path = ['/catalog/']
[route.db]
queries = ['SELECT 1']
[route.analytics]
queries = ['SELECT 2']
"#;

#[test]
fn every_add_query_reparses_under_parse_and_validate() {
    let delete = r#"DELETE FROM "p" WHERE "p"."id" = $?"#;
    let insert = r#"INSERT INTO "p" ("id") VALUES ($?)"#;
    let report = run(
        RE_PARSE_MANIFEST,
        &[http_line("GET", "/catalog/", None, 0, None)],
        &[
            routed_sql("catalog", "db", delete, grantable(delete), false),
            routed_sql("catalog", "analytics", insert, grantable(insert), false),
        ],
    );
    let mut add_queries = 0;
    for finding in &report.findings {
        let Finding::MissingGrant {
            proxy, sql, fix, ..
        } = finding
        else {
            continue;
        };
        assert_eq!(fix.action, "add_query", "{sql}");
        assert_eq!(
            fix.to.as_deref(),
            Some(format!("[route.{proxy}].queries").as_str())
        );
        assert!(!sql.contains('\u{2026}'), "a redaction is never grantable");
        let edited = apply_edits(
            RE_PARSE_MANIFEST,
            &ManifestEdits {
                routes: Vec::new(),
                grants: vec![ProposedGrant {
                    route: "catalog".to_string(),
                    proxy: proxy.clone(),
                    queries: vec![sql.clone()],
                }],
            },
        )
        .unwrap_or_else(|error| panic!("{sql} cannot be written: {error}"));
        let (parsed, _warnings) = Manifest::parse_and_validate(&edited, UidLayout::DEFAULT_BASE)
            .expect("the edited manifest boots");
        let route = parsed
            .routes()
            .iter()
            .find(|route| route.name() == "catalog")
            .expect("the route is declared");
        let grant = route
            .grant(&ProxyName::new(proxy.clone()))
            .and_then(|grant| grant.as_postgresql())
            .expect("the proxy is declared");
        assert!(
            grant.queries().iter().any(|candidate| candidate == sql),
            "{sql} is in the manifest after the edit"
        );
        add_queries += 1;
    }
    assert_eq!(add_queries, 2, "both denied statements are grantable");
}

#[test]
fn a_redacted_literal_and_a_savepoint_are_observe_never_add_query() {
    let redacted = r#"SELECT "p"."id" FROM "p" WHERE "p"."name" = …"#;
    let report = run(
        RE_PARSE_MANIFEST,
        &[http_line("POST", "/catalog/", None, 0, None)],
        &[
            routed_sql("catalog", "db", redacted, refused(redacted), false),
            routed_sql(
                "catalog",
                "db",
                "SAVEPOINT s1",
                grantable("SAVEPOINT s1"),
                true,
            ),
            routed_sql(
                "catalog",
                "db",
                r#"DELETE FROM "p" WHERE "p"."id" = $?"#,
                grantable(r#"DELETE FROM "p" WHERE "p"."id" = $?"#),
                false,
            ),
        ],
    );
    let missing: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding, Finding::MissingGrant { .. }))
        .collect();
    assert_eq!(missing.len(), 3);
    for finding in &missing {
        let Finding::MissingGrant { sql, fix, .. } = finding else {
            unreachable!("filtered above");
        };
        if sql.contains('\u{2026}') || sql == "SAVEPOINT" {
            assert_eq!(fix.action, "observe", "{sql}");
            assert_eq!(fix.to, None, "{sql} edits no table");
        } else {
            assert_eq!(fix.action, "add_query", "{sql}");
        }
    }
    assert_eq!(
        missing
            .iter()
            .filter(|finding| matches!(
                finding,
                Finding::MissingGrant { fix, .. } if fix.action == "add_query"
            ))
            .count(),
        1,
        "only the parameterised DELETE is grantable"
    );
}

#[test]
fn a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query() {
    // `AUDIT-MA.2.3` F6: this verdict *claims* `query`, but its normalised
    // text carries `…`. R33 says every `add_query` must be bootable, so the
    // redaction stays `observe` no matter what the verdict says — while its
    // parameterised twin is still granted with a `to` table.
    let redacted = r#"SELECT "p"."id" FROM "p" WHERE "p"."name" = …"#;
    let grantable_sql = DELETE;
    let report = run(
        RE_PARSE_MANIFEST,
        &[http_line("GET", "/catalog/", None, 0, None)],
        &[
            routed_sql("catalog", "db", redacted, grantable(redacted), false),
            routed_sql(
                "catalog",
                "db",
                grantable_sql,
                grantable(grantable_sql),
                false,
            ),
        ],
    );

    let mut fixes: BTreeMap<String, (&'static str, Option<String>)> = BTreeMap::new();
    for finding in &report.findings {
        if let Finding::MissingGrant { sql, fix, .. } = finding {
            fixes.insert(sql.clone(), (fix.action, fix.to.clone()));
        }
    }
    assert_eq!(fixes.len(), 2, "both denials are reported: {fixes:?}");
    assert_eq!(
        fixes.get(redacted),
        Some(&("observe", None)),
        "a redaction is never grantable, even under a `query` verdict"
    );
    assert_eq!(
        fixes.get(grantable_sql),
        Some(&("add_query", Some("[route.db].queries".to_string()))),
        "the parameterised twin is still grantable"
    );
}

// ---------------------------------------------------------------------------
// R33 / `MA.8`: session setup names the `connection` group, never one route
// ---------------------------------------------------------------------------
//
// The protected tier owns both directions of the `add_group` oracle: the
// `fix` a driver's own connection setup gets, and the per-route `add_query`
// every other grantable statement keeps. The unprotected
// `crates/blastit/tests/audit_group_hint.rs` keeps the same cases as a
// standard-tier pointer.

/// The `fix` object of the one `missing_grant` whose `sql` is `sql`, exactly
/// as the `--json` document carries it — so an added or missing field fails
/// the comparison too.
fn fix_of(report: &Report, sql: &str) -> Value {
    let found: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding, Finding::MissingGrant { sql: text, .. } if text == sql))
        .collect();
    assert_eq!(found.len(), 1, "one missing_grant for {sql}: {report:?}");
    let document = serde_json::to_value(found[0]).expect("a finding renders");
    assert_eq!(document["kind"], "missing_grant", "{document}");
    document["fix"].clone()
}

#[test]
fn connection_setup_sql_names_the_connection_group_grant() {
    // Django re-states `TimeZone` when it opens a connection (LESSONS.md), and
    // `SET`/`SHOW` are the driver's session setup. Each runs on whichever route
    // opens the connection first, so a per-route `[route.db].queries` grant
    // leaves every other route denying it: the fix is the `connection` group
    // every route attaches (`MA.8`).
    for sql in [
        "SELECT set_config('TimeZone', $?, false)",
        "SET search_path = public",
        "SHOW standard_conforming_strings",
    ] {
        let report = run(
            GOLDEN_MANIFEST,
            &[http_line("GET", "/", None, 0, None)],
            &[routed_sql("home", "db", sql, grantable(sql), false)],
        );
        assert!(!report.ok, "{sql} is an ungranted statement, not advice");
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "add_group", "to": "[group.connection.db].queries"}),
            "{sql} runs on whichever route opens the connection"
        );
    }
}

#[test]
fn ordinary_statements_keep_the_per_route_add_query() {
    // The twin of the group hint: `SELECT 1` is replayable in a transaction
    // but it belongs to the route that issued it, and `DELETE` is an ordinary
    // statement, so neither may name a group every route attaches.
    for sql in ["SELECT 1", DELETE] {
        let report = run(
            GOLDEN_MANIFEST,
            &[http_line("GET", "/", None, 0, None)],
            &[routed_sql("home", "db", sql, grantable(sql), false)],
        );
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "add_query", "to": "[route.db].queries"}),
            "{sql} is one route's statement, at one route's path"
        );
    }
}

#[test]
fn only_connection_setup_names_a_group_and_never_a_refusal() {
    // One mixed run: exactly the setup statement carries `add_group`, while the
    // per-route statements and the two `observe` cases keep their fixes — so a
    // regression that emits `add_group` everywhere (or `add_query` for setup)
    // fails on a single report.
    let setup = "SELECT set_config('TimeZone', $?, false)";
    let report = run(
        GOLDEN_MANIFEST,
        &[http_line("GET", "/", None, 0, None)],
        &[
            routed_sql("home", "db", setup, grantable(setup), false),
            routed_sql("home", "db", "SELECT 1", grantable("SELECT 1"), false),
            routed_sql("home", "db", DELETE, grantable(DELETE), false),
            routed_sql("home", "db", REDACTED, refused(REDACTED), false),
            routed_sql(
                "home",
                "db",
                "SAVEPOINT s1",
                grantable("SAVEPOINT s1"),
                true,
            ),
        ],
    );
    let grouped: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            Finding::MissingGrant { sql, fix, .. } if fix.action == "add_group" => {
                Some(sql.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        grouped,
        [setup.to_string()],
        "only connection setup names a group: {report:?}"
    );
    assert_eq!(
        fix_of(&report, "SELECT 1"),
        json!({"action": "add_query", "to": "[route.db].queries"})
    );
    assert_eq!(
        fix_of(&report, DELETE),
        json!({"action": "add_query", "to": "[route.db].queries"})
    );
    for sql in [REDACTED, "SAVEPOINT"] {
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "observe"}),
            "{sql} is never grantable, so it is never a group's either"
        );
    }
}

// ---------------------------------------------------------------------------
// §1.3 human contract, all nine kinds and their interaction counts
// ---------------------------------------------------------------------------

const RICH_MANIFEST: &str = r#"
version = 1
groups = ["session"]

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }

[group.session.db]
queries = ['SELECT 1']

[[route]]
name = "catalog"
path = ['/catalog/']
[route.db]
queries = [
  'SELECT "p"."id" FROM "p"',
  'SELECT count(*) FROM "p"',
]

[[route]]
name = "checkout"
path = ['/checkout/']

[[route]]
name = "about"
path = ['/about/']

[[route]]
name = "upload"
path = ['/upload/']

[[route]]
name = "form"
path = ['/form/']
[route.ingress]
body = "json"

[[route]]
name = "raw"
path = ['/raw/']
[route.ingress]
body = "raw"
"#;

const DELETE: &str = r#"DELETE FROM "p" WHERE "p"."id" = $?"#;
const REDACTED: &str = r#"SELECT "p"."id" FROM "p" WHERE "p"."name" = …"#;

fn rich_http() -> Vec<String> {
    vec![
        http_line("GET", "/catalog/", None, 0, None),
        http_line("POST", "/checkout/", None, 0, None),
        http_line(
            "POST",
            "/upload/",
            Some("multipart/form-data; boundary=x"),
            120,
            Some(json!({
                "mime": "multipart/form-data",
                "truncated": false,
                "valid_json": false,
                "valid_form": false,
            })),
        ),
        http_line(
            "POST",
            "/form/",
            Some("application/x-www-form-urlencoded"),
            15,
            Some(json!({
                "mime": "application/x-www-form-urlencoded",
                "truncated": false,
                "valid_json": false,
                "valid_form": true,
            })),
        ),
        http_line("GET", "/raw/", None, 0, None),
        http_line("GET", "/robots.txt", None, 0, None),
    ]
}

fn rich_sql() -> Vec<String> {
    vec![
        routed_sql(
            "catalog",
            "db",
            r#"SELECT "p"."id" FROM "p""#,
            grantable(r#"SELECT "p"."id" FROM "p""#),
            false,
        ),
        routed_sql("catalog", "db", DELETE, grantable(DELETE), false),
        routed_sql("catalog", "db", REDACTED, refused(REDACTED), false),
        routed_sql(
            "catalog",
            "db",
            "SAVEPOINT s1",
            grantable("SAVEPOINT s1"),
            true,
        ),
        unrouted_sql(
            "GET",
            "/robots.txt",
            "db",
            "SELECT 3",
            grantable("SELECT 3"),
            false,
        ),
    ]
}

#[test]
fn every_finding_kind_and_its_interaction_counts() {
    let report = run(RICH_MANIFEST, &rich_http(), &rich_sql());

    let mut expected = BTreeMap::new();
    expected.insert("missing_grant".to_string(), 3);
    expected.insert("dead_grant".to_string(), 2);
    expected.insert("unexercised_route".to_string(), 1);
    expected.insert("missing_ingress".to_string(), 1);
    expected.insert("ingress_mismatch".to_string(), 1);
    expected.insert("raw_ingress".to_string(), 1);
    expected.insert("unrouted_request".to_string(), 1);
    expected.insert("proxy_unused".to_string(), 1);
    assert_eq!(kinds(&report), expected);
    assert!(!report.ok);

    // Counts: the unrouted statement is in `statements` but never in a
    // `missing_grant`; the unexercised route contributes neither a dead grant
    // nor a missing one.
    assert_eq!(report.requests, 6);
    assert_eq!(report.statements, 5);
    assert_eq!(report.routes.total, 6);
    assert_eq!(report.routes.exercised, 5);
    assert_eq!(
        report.proxies["db"],
        ProxyCounts {
            statements: 5,
            denied: 4,
        }
    );
    assert_eq!(
        report.proxies["analytics"],
        ProxyCounts {
            statements: 0,
            denied: 0,
        }
    );
    assert_eq!(
        report.statements,
        report
            .proxies
            .values()
            .map(|counts| counts.statements)
            .sum::<u64>()
    );
    assert_eq!(
        report.ok,
        report
            .findings
            .iter()
            .all(|finding| finding.severity() != Severity::Error)
    );
    // The unexercised route names itself once, never as dead grants.
    let dead_routes: Vec<&str> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            Finding::DeadGrant { route, .. } => Some(route.as_str()),
            _ => None,
        })
        .collect();
    assert!(dead_routes.iter().all(|route| *route != "about"));
    // The group's dead grant is reported once, with its group table.
    assert!(report.findings.iter().any(|finding| matches!(
        finding,
        Finding::DeadGrant { group: Some(group), fix, .. }
            if group == "session"
                && fix.to.as_deref() == Some("[group.session.db].queries")
                && fix.unsafe_ == Some(true)
    )));

    // §1.3, byte for byte.
    let expected = "\
blastit check: 6 requests, 5 statements, 5/6 routes, 2 proxies
  route catalog: 3 statements not granted
    DELETE FROM \"p\" WHERE \"p\".\"id\" = $?
    SAVEPOINT
    SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"name\" = …
  route catalog: 1 grant never used
  group session: 1 grant never used
  route about: never exercised
  route upload: takes a body and declares no ingress validation
  route form: declared body = \"json\" does not match observed application/x-www-form-urlencoded
  route raw: declares body = \"raw\" without allow_raw = true
  request GET /robots.txt: no route
  proxy analytics: declared, 0 statements observed
blastit check: 11 problems · `--json` for the machine-readable list
";
    assert_eq!(audit_v1::human(&report), expected);
}

#[test]
fn an_empty_suite_is_no_observations_and_suppresses_unexercised_routes() {
    let report = run(RICH_MANIFEST, &[], &[]);
    let mut expected = BTreeMap::new();
    expected.insert("no_observations".to_string(), 1);
    expected.insert("proxy_unused".to_string(), 2);
    expected.insert("raw_ingress".to_string(), 1);
    assert_eq!(
        kinds(&report),
        expected,
        "no unexercised_route without HTTP"
    );
    assert_eq!(report.requests, 0);
    assert_eq!(report.statements, 0);
    assert_eq!(report.routes.exercised, 0);
    let expected = "\
blastit check: 0 requests, 0 statements, 0/6 routes, 2 proxies
  route raw: declares body = \"raw\" without allow_raw = true
  suite exercised 0 routes or 0 proxy statements
  proxy analytics: declared, 0 statements observed
  proxy db: declared, 0 statements observed
blastit check: 4 problems · `--json` for the machine-readable list
";
    assert_eq!(audit_v1::human(&report), expected);
}

const ONE_PROXY_MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "home"
path = ['/']
"#;

#[test]
fn requests_without_any_statement_are_no_observations() {
    // `AUDIT-MA.2.3` F7: §4.2 E.3's second clause. The suite reached a route,
    // so the "no route exercised" clause cannot fire; only the "proxy declared
    // but not one statement observed" clause can report this run.
    let report = run(
        ONE_PROXY_MANIFEST,
        &[http_line("GET", "/", None, 0, None)],
        &[],
    );
    assert_eq!(report.requests, 1);
    assert_eq!(report.routes.exercised, 1, "the request is routed");
    assert_eq!(report.statements, 0);
    assert_eq!(
        kinds(&report).get("no_observations"),
        Some(&1),
        "exactly one no_observations for a request-only suite"
    );

    // Twin: one observed statement satisfies the clause, whatever else fires.
    let report = run(
        ONE_PROXY_MANIFEST,
        &[http_line("GET", "/", None, 0, None)],
        &[routed_sql(
            "home",
            "db",
            "SELECT 1",
            grantable("SELECT 1"),
            false,
        )],
    );
    assert_eq!(report.statements, 1);
    assert!(
        !kinds(&report).contains_key("no_observations"),
        "one statement is an observation: {report:?}"
    );
}

#[test]
fn a_clean_run_is_one_line_and_ok() {
    let manifest = format!(
        "version = 1\n{PROXIES}
[[route]]
name = \"notes\"
path = ['/notes/.*']
[route.db]
queries = ['SELECT \"notes\".\"id\" FROM \"notes\"']
"
    );
    let sql = r#"SELECT "notes"."id" FROM "notes""#;
    let report = run(
        &manifest,
        &[http_line("GET", "/notes/", None, 0, None)],
        &[routed_sql("notes", "db", sql, grantable(sql), false)],
    );
    assert_eq!(report.findings, []);
    assert!(report.ok);
    assert_eq!(
        audit_v1::human(&report),
        "blastit check: 1 requests, 1 statements, 1/1 routes, clean\n"
    );
    // One proxy never gets the `, <P> proxies` suffix.
    assert_eq!(report.proxies.len(), 1);
}

#[test]
fn ingress_findings_have_positive_twins() {
    let manifest = r#"
version = 1
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }
[[route]]
name = "raw"
path = ['/raw/']
[route.ingress]
body = "raw"
allow_raw = true
[[route]]
name = "json"
path = ['/json/']
[route.ingress]
body = "json"
[[route]]
name = "none"
path = ['/none/']
"#;
    let http = vec![
        http_line("POST", "/raw/", Some("application/octet-stream"), 4, None),
        http_line(
            "POST",
            "/json/",
            Some("application/json"),
            2,
            Some(json!({
                "mime": "application/json",
                "truncated": false,
                "valid_json": true,
                "valid_form": false,
            })),
        ),
        http_line("GET", "/none/", None, 0, None),
    ];
    let report = run(manifest, &http, &[]);
    for kind in ["missing_ingress", "ingress_mismatch", "raw_ingress"] {
        assert!(
            !kinds(&report).contains_key(kind),
            "{kind} must not fire on its allowed twin"
        );
    }
}

// ---------------------------------------------------------------------------
// R32 and the `--learn` negative twins
// ---------------------------------------------------------------------------

#[test]
fn an_unrouted_statement_never_yields_a_route_less_missing_grant() {
    let sql = unrouted_sql(
        "GET",
        "/oops/",
        "db",
        "SELECT 9",
        grantable("SELECT 9"),
        false,
    );
    let report = run(
        GOLDEN_MANIFEST,
        &[http_line("GET", "/oops/", None, 0, None)],
        &[sql],
    );
    assert!(
        report
            .findings
            .iter()
            .all(|finding| !matches!(finding, Finding::MissingGrant { .. }))
    );
    assert_eq!(kinds(&report)["unrouted_request"], 1);
    assert_eq!(report.proxies["db"].statements, 1);
    assert_eq!(report.proxies["db"].denied, 1);
}

#[test]
fn an_inline_literal_or_zero_requests_never_writes() {
    // An inline literal is refused by the learn policy: the plan is unwritable
    // even though a request was observed.
    let literal = r#"SELECT "p"."id" FROM "p" WHERE "p"."name" = …"#;
    let http = vec![http_line("POST", "/notes/", None, 0, None)];
    let sql = vec![unrouted_sql(
        "POST",
        "/notes/",
        "db",
        "SELECT 3",
        refused(literal),
        false,
    )];
    let plan = audit_v1::plan_learn(
        &audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("valid"),
        &observations(&http, &sql),
    )
    .expect("the plan is built");
    assert_eq!(plan.refused.len(), 1);
    assert!(
        !plan.would_write(false),
        "a refused statement blocks the write"
    );
    assert_eq!(plan.queries(), 0);
    let document: Value = serde_json::from_str(&plan.json(false)).expect("the document is JSON");
    assert_eq!(document["wrote"], false);

    // A verdict that wrongly calls a redaction grantable still never reaches a
    // `queries` list: the plan is unwritable.
    let sneaky = vec![unrouted_sql(
        "POST",
        "/notes/",
        "db",
        "SELECT \u{2026}",
        grantable("SELECT \u{2026}"),
        false,
    )];
    let plan = audit_v1::plan_learn(
        &audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("valid"),
        &observations(&http, &sneaky),
    )
    .expect("the plan is built");
    assert_eq!(plan.queries(), 0);
    assert_eq!(plan.refused.len(), 1);
    assert!(!plan.would_write(false));

    // Zero requests write nothing either, even with a valid manifest.
    let empty = audit_v1::plan_learn(
        &audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("valid"),
        &observations(&[], &[]),
    )
    .expect("the plan is built");
    assert_eq!(empty.requests, 0);
    assert!(!empty.would_write(false));
    let document: Value = serde_json::from_str(&empty.json(false)).expect("the document is JSON");
    assert_eq!(document["wrote"], false);
}

#[test]
fn zero_requests_never_write_even_with_sql_to_learn() {
    // `AUDIT-MA.2.3` F8: the gate can observe SQL without any HTTP request
    // (`plan_learn` takes paths from SQL too), so the old zero-request test
    // could pass on the last conjunct alone. R29: no request, no write.
    let manifest = audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("valid");
    let sql = vec![unrouted_sql(
        "GET",
        "/notes/",
        "db",
        "SELECT 1",
        grantable("SELECT 1"),
        false,
    )];
    let plan =
        audit_v1::plan_learn(&manifest, &observations(&[], &sql)).expect("the plan is built");
    assert_eq!(plan.requests, 0);
    assert_eq!(plan.routes(), 1, "the SQL path alone proposes a route");
    assert_eq!(plan.queries(), 1, "and its grantable statement");
    assert!(
        !plan.would_write(false),
        "zero HTTP requests write nothing, however much SQL was seen"
    );
    assert!(!plan.would_write(true));
    let document: Value = serde_json::from_str(&plan.json(false)).expect("the document is JSON");
    assert_eq!(document["wrote"], false);

    // Twin: the same plan with one routed HTTP line is writable.
    let http = vec![http_line("GET", "/notes/", None, 0, None)];
    let plan =
        audit_v1::plan_learn(&manifest, &observations(&http, &sql)).expect("the plan is built");
    assert_eq!(plan.requests, 1);
    assert_eq!(plan.routes(), 1);
    assert_eq!(plan.queries(), 1);
    assert!(
        plan.would_write(false),
        "one request makes the plan writable"
    );
    let document: Value = serde_json::from_str(&plan.json(false)).expect("the document is JSON");
    assert_eq!(document["wrote"], true);
}

#[test]
fn transaction_control_is_a_hint_never_a_silent_flag() {
    let http = vec![http_line("POST", "/notes/", None, 0, None)];
    let sql = vec![unrouted_sql(
        "POST",
        "/notes/",
        "db",
        "SAVEPOINT s1",
        grantable("SAVEPOINT s1"),
        true,
    )];
    let plan = audit_v1::plan_learn(
        &audit_v1::parse_manifest(GOLDEN_MANIFEST).expect("valid"),
        &observations(&http, &sql),
    )
    .expect("the plan is built");
    assert_eq!(plan.queries(), 0, "no silent transaction control");
    assert_eq!(plan.transaction_control.len(), 1);
    assert!(
        plan.transaction_control[0].contains("transaction_control = true"),
        "{}",
        plan.transaction_control[0]
    );
    let edited = apply_edits(GOLDEN_MANIFEST, &plan.edits).expect("the edit applies");
    assert!(!edited.contains("transaction_control"), "{edited}");
    assert!(!edited.contains("tables"), "{edited}");
}

#[test]
fn schema_version_must_be_one_inside_the_engine_too() {
    let error = CheckObsV1::new(1, Vec::new(), Vec::new()).expect("v1 is accepted");
    assert_eq!(error.schema_version(), SCHEMA_VERSION);
    let error = CheckObsV1::new(7, Vec::new(), Vec::new()).expect_err("v7 is refused");
    assert!(matches!(
        error,
        ObsError::SchemaVersion {
            found: 7,
            expected: 1
        }
    ));
}
