//! Protected critical tests for the Python-facing `blastit check` schema
//! (`R32..R38`; `docs/design/ma-lean-dx.md` §4.2 E, §7.2).
//!
//! `crates/blastit-py` is a `cdylib`: a Rust test cannot link the extension as
//! an rlib. The bridge functions are one-line wrappers, so these tests call the
//! exact functions `_blastit.{check_audit, plan_learn, probe_ingress}` wrap —
//! [`blastit::audit_v1`] — with the JSONL a Python gate writes, and check the
//! schema, the conformance and the count invariants at that boundary. The
//! wrappers themselves are unit-tested in `src/check.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit::audit_v1::{self, CheckObsV1, ObsError, Severity};
use blastit_manifest::{Manifest, ProxyName, UidLayout};
use serde_json::{Value, json};

const MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }

[[route]]
name = "notes"
path = ['/notes/.*']
[route.db]
queries = ['SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1']
"#;

/// One `http_obs.jsonl` line exactly as `_check_pytest.py` writes it.
fn python_http(method: &str, path: &str, body: Option<(&str, u64, Value)>) -> String {
    match body {
        None => json!({
            "method": method,
            "path": path,
            "content_type": Value::Null,
            "body_len": 0,
            "sample": Value::Null,
        })
        .to_string(),
        Some((content_type, body_len, sample)) => json!({
            "method": method,
            "path": path,
            "content_type": content_type,
            "body_len": body_len,
            "sample": sample,
        })
        .to_string(),
    }
}

/// One `sql_obs.jsonl` line, as dbproxy's `WireSqlObs` serialises it.
fn python_sql(route: &str, proxy: &str, sql: &str, normalized: &str) -> String {
    json!({
        "target": {"scope": "routed", "route": route},
        "proxy": proxy,
        "kind": "query",
        "sql": sql,
        "is_tx": false,
        "allowed": true,
        "verdict": {"kind": "query", "normalized": normalized},
    })
    .to_string()
}

const GRANTED: &str = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $?"#;
const MISSING: &str = r#"SELECT "notes"."title" FROM "notes" WHERE "notes"."id" = $?"#;

#[test]
fn check_obs_v1_accepts_one_and_rejects_every_other_schema_version() {
    let accepted = CheckObsV1::new(1, Vec::new(), Vec::new()).expect("v1 is the schema");
    assert_eq!(accepted.schema_version(), 1);
    for version in [0u64, 2, 7, u64::MAX] {
        let error =
            CheckObsV1::new(version, Vec::new(), Vec::new()).expect_err("only version 1 is read");
        assert!(matches!(
            error,
            ObsError::SchemaVersion {
                found,
                expected: 1
            } if found == version
        ));
    }
    // The engine refuses a hand-built value too, not only the constructor.
    let manifest = audit_v1::parse_manifest(MANIFEST).expect("the manifest validates");
    let report = audit_v1::evaluate(&manifest, &accepted).expect("v1 evaluates");
    assert_eq!(report.schema_version, 1);
}

#[test]
fn python_http_obs_lines_conform_to_the_engine() {
    let http = [
        python_http("GET", "/notes/", None),
        python_http(
            "POST",
            "/notes/",
            Some((
                "application/json",
                2,
                json!({
                    "mime": "application/json",
                    "truncated": false,
                    "valid_json": true,
                    "valid_form": false,
                }),
            )),
        ),
    ];
    let sql = [python_sql("notes", "db", GRANTED, GRANTED)];
    // This is the function `_blastit.check_audit` calls, verbatim.
    let document = audit_v1::check_audit(MANIFEST, &http.join("\n"), &sql.join("\n"))
        .expect("the Python lines are read");
    let report: Value = serde_json::from_str(&document).expect("the report is JSON");

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["requests"], 2);
    assert_eq!(report["statements"], 1);
    assert_eq!(report["routes"]["total"], 1);
    assert_eq!(report["routes"]["exercised"], 1);
    assert_eq!(report["proxies"]["db"]["statements"], 1);
    assert_eq!(report["proxies"]["analytics"]["statements"], 0);
    assert_eq!(report["ok"], false, "the unused proxy is still a finding");
    assert!(
        report["findings"]
            .as_array()
            .expect("findings is a list")
            .iter()
            .any(|finding| finding["kind"] == "proxy_unused" && finding["proxy"] == "analytics")
    );

    // The typed path reads the same lines, so Python's optional fields are
    // genuinely optional and an unknown key is ignored.
    let tolerant = format!(
        "{}\n{}",
        http[0],
        json!({
            "method": "GET",
            "path": "/notes/",
            "content_type": Value::Null,
            "body_len": 0,
            "sample": Value::Null,
            "pytest_nodeid": "tests/test_notes.py::test_list",
        })
    );
    let observations = CheckObsV1::from_jsonl(&tolerant, "").expect("the lines parse");
    assert_eq!(observations.http().len(), 2);
    assert_eq!(observations.http()[0].method, "GET");
    assert_eq!(observations.http()[0].body_len, 0);
    assert_eq!(observations.http()[0].sample, None);
    assert_eq!(observations.http()[1].sample, None);
}

#[test]
fn statements_is_the_sum_of_the_declared_proxy_counts() {
    let http = [
        python_http("GET", "/notes/", None),
        python_http("GET", "/notes/", None),
    ];
    let sql = [
        python_sql("notes", "db", GRANTED, GRANTED),
        python_sql("notes", "db", MISSING, MISSING),
    ];
    let manifest = audit_v1::parse_manifest(MANIFEST).expect("the manifest validates");
    let observations =
        CheckObsV1::from_jsonl(&http.join("\n"), &sql.join("\n")).expect("the lines parse");
    let report = audit_v1::evaluate(&manifest, &observations).expect("the audit runs");

    let summed: u64 = report
        .proxies
        .values()
        .map(|counts| counts.statements)
        .sum();
    assert_eq!(report.statements, summed);
    assert_eq!(report.statements, 2);
    assert_eq!(report.proxies["db"].statements, 2);
    assert_eq!(report.proxies["db"].denied, 1);
    assert_eq!(report.proxies["analytics"].statements, 0);
    // R38: `ok` is false exactly because an error-severity finding exists.
    assert_eq!(
        report.ok,
        report
            .findings
            .iter()
            .all(|finding| finding.severity() != Severity::Error)
    );
    assert!(!report.ok);
    assert!(report.findings.iter().any(
        |finding| matches!(finding, blastit::audit_v1::Finding::MissingGrant { sql, .. } if sql == MISSING)
    ));
}

#[test]
fn a_corrupt_http_obs_line_names_the_file_and_the_line() {
    let error = audit_v1::check_audit(
        MANIFEST,
        "{\"method\": \"GET\", \"path\": \"/notes/\"}\n{not json}\n",
        "",
    )
    .expect_err("a corrupt line is an error");
    let message = error.to_string();
    assert!(message.contains("http_obs.jsonl"), "{message}");
    assert!(message.contains("line 2"), "{message}");

    // The orchestrator names the real path: the error carries whatever it
    // hands over.
    let error = audit_v1::parse_http_obs("/run/blastit/http_obs.jsonl", "[]\n")
        .expect_err("an array is not an HttpObsLine");
    assert!(
        error.to_string().contains("/run/blastit/http_obs.jsonl"),
        "{error}"
    );
}

#[test]
fn the_bridge_probe_renders_the_body_sample_the_gate_sends() {
    // `_blastit.probe_ingress` calls this function.
    let sample: Value = serde_json::from_str(
        &audit_v1::probe_ingress_json(Some("application/json; charset=utf-8"), b"{}", 2)
            .expect("the sample renders"),
    )
    .expect("the sample is JSON");
    assert_eq!(sample["mime"], "application/json");
    assert_eq!(sample["truncated"], false);
    assert_eq!(sample["valid_json"], true);
    assert_eq!(sample["valid_form"], false);

    let truncated: Value = serde_json::from_str(
        &audit_v1::probe_ingress_json(Some("application/json"), b"{}", 64 * 1024 + 1)
            .expect("the sample renders"),
    )
    .expect("the sample is JSON");
    assert_eq!(truncated["truncated"], true);
    assert_eq!(truncated["valid_json"], false);

    // A missing Content-Type is the empty essence, not an error.
    let none: Value = serde_json::from_str(
        &audit_v1::probe_ingress_json(None, b"", 0).expect("the sample renders"),
    )
    .expect("the sample is JSON");
    assert_eq!(none["mime"], "none");
}

#[test]
fn the_bridge_plan_learn_matches_the_engine_proposal() {
    let http = [python_http(
        "POST",
        "/orders/42/",
        Some((
            "application/json",
            2,
            json!({
                "mime": "application/json",
                "truncated": false,
                "valid_json": true,
                "valid_form": false,
            }),
        )),
    )];
    let sql = json!({
        "target": {"scope": "unrouted", "method": "POST", "path": "/orders/42/"},
        "proxy": "db",
        "kind": "query",
        "sql": r#"SELECT "orders"."id" FROM "orders" WHERE "orders"."id" = $?"#,
        "is_tx": false,
        "allowed": true,
        "verdict": {
            "kind": "query",
            "normalized": r#"SELECT "orders"."id" FROM "orders" WHERE "orders"."id" = $?"#,
        },
    })
    .to_string();
    let document: Value = serde_json::from_str(
        &audit_v1::plan_learn_json(MANIFEST, &http.join("\n"), &sql, false)
            .expect("the plan renders"),
    )
    .expect("the plan is JSON");
    assert_eq!(document["proposed"]["routes"], 1);
    assert_eq!(document["proposed"]["queries"], 1);
    assert_eq!(document["wrote"], true);
    assert!(document.get("schema_version").is_none());

    // The plan applies to a manifest that boots.
    let manifest = audit_v1::parse_manifest(MANIFEST).expect("the manifest validates");
    let observations =
        CheckObsV1::from_jsonl(&http.join("\n"), &sql).expect("the observations parse");
    let plan = audit_v1::plan_learn(&manifest, &observations).expect("the plan is built");
    let edited =
        blastit_manifest::edit::apply_edits(MANIFEST, &plan.edits).expect("the plan applies");
    let (parsed, _warnings) = Manifest::parse_and_validate(&edited, UidLayout::DEFAULT_BASE)
        .expect("the edited manifest boots");
    let route = parsed
        .routes()
        .iter()
        .find(|route| route.name() == "orders")
        .expect("the inferred route is declared");
    assert!(route.grant(&ProxyName::new("db".to_string())).is_some());
}
