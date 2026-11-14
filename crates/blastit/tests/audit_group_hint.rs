//! `blastit check` names a group grant for connection-setup SQL (`MA.8`).
//!
//! Django re-states `TimeZone` when it opens a connection
//! (`SELECT set_config('TimeZone', $1, false)`), and the statement runs on
//! whichever route opens the connection first — so a per-route
//! `[route.db].queries` grant leaves every other route denying it. The `fix`
//! for that statement names `[group.connection.db].queries` with
//! `action = "add_group"`, while every other grantable statement keeps its
//! per-route `add_query` twin; a redacted literal and transaction control stay
//! `observe`, exactly as before.
//!
//! Standard-tier twin: the protected `crates/blastit/tests/critical_check_engine.rs`
//! owns this oracle in both directions (`MB.1.13`); these cases stay as a
//! second, unprotected statement of the same contract.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit::audit_v1::{self, CheckObsV1, Finding, Report};
use serde_json::{Value, json};

const MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "home"
path = ['/']
"#;

/// What Django 6.1 sends at connect when the server's `TimeZone` is not `UTC`
/// (`docs/knowledge/LESSONS.md`, `MA.8`), as the learn policy normalises it.
const TIMEZONE_SETUP: &str = "SELECT set_config('TimeZone', $?, false)";

/// A literal the learn policy redacted: grantable only after parameterising.
const REDACTED: &str = r#"SELECT "t"."id" FROM "t" WHERE "t"."name" = …"#;

fn http_line() -> String {
    json!({
        "method": "GET",
        "path": "/",
        "content_type": null,
        "body_len": 0,
        "sample": null,
    })
    .to_string()
}

fn routed_sql(sql: &str, verdict: Value, is_tx: bool) -> String {
    json!({
        "target": {"scope": "routed", "route": "home"},
        "proxy": "db",
        "kind": "query",
        "sql": sql,
        "is_tx": is_tx,
        "allowed": true,
        "verdict": verdict,
    })
    .to_string()
}

/// A verdict the learn policy proved grantable, on `normalized` text.
fn grantable(normalized: &str) -> Value {
    json!({"kind": "query", "normalized": normalized})
}

/// A verdict the learn policy refused because a literal is still inlined.
fn refused(redacted: &str) -> Value {
    json!({
        "kind": "refused",
        "redacted": redacted,
        "reason": "the statement inlines a value; parameterise it first",
    })
}

/// Evaluates one suite run: one routed request and the given statements.
fn report(statements: &[String]) -> Report {
    let manifest = audit_v1::parse_manifest(MANIFEST).expect("the fixture manifest validates");
    let observations = CheckObsV1::from_jsonl(&http_line(), &statements.join("\n"))
        .expect("the observations parse");
    audit_v1::evaluate(&manifest, &observations).expect("the audit runs")
}

/// The `fix` object of the one finding whose `sql` is `sql`, as the document
/// carries it — so an extra field fails the comparison too.
fn fix_of(report: &Report, sql: &str) -> Value {
    let found: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding, Finding::MissingGrant { sql: text, .. } if text == sql))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one missing_grant for {sql}: {:?}",
        report.findings
    );
    let document = serde_json::to_value(found[0]).expect("a finding renders");
    assert_eq!(document["kind"], "missing_grant", "{document}");
    assert_eq!(document["severity"], "error", "{document}");
    document["fix"].clone()
}

#[test]
fn connection_setup_sql_names_a_group_grant() {
    let report = report(&[routed_sql(TIMEZONE_SETUP, grantable(TIMEZONE_SETUP), false)]);
    assert!(
        !report.ok,
        "an ungranted setup statement still fails the run"
    );
    assert_eq!(
        fix_of(&report, TIMEZONE_SETUP),
        json!({"action": "add_group", "to": "[group.connection.db].queries"}),
        "the statement runs on whichever route opens the connection"
    );
}

#[test]
fn a_set_and_a_show_are_session_setup_too() {
    for sql in [
        "SET search_path = public",
        "SHOW standard_conforming_strings",
    ] {
        let report = report(&[routed_sql(sql, grantable(sql), false)]);
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "add_group", "to": "[group.connection.db].queries"}),
            "{sql} is session setup: {sql}"
        );
    }
}

#[test]
fn a_plain_statement_keeps_the_per_route_grant() {
    // The twin: `SELECT 1` is replayable as setup, and `DELETE` is an ordinary
    // statement — neither belongs to a group every route attaches, so both keep
    // the per-route `add_query` this engine has always emitted.
    for sql in ["SELECT 1", "DELETE FROM \"t\" WHERE \"t\".\"id\" = $?"] {
        let report = report(&[routed_sql(sql, grantable(sql), false)]);
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "add_query", "to": "[route.db].queries"}),
            "{sql} is a route's statement"
        );
    }
}

#[test]
fn transaction_control_and_a_redacted_literal_never_name_a_group() {
    let report = report(&[
        routed_sql("SAVEPOINT s1", grantable("SAVEPOINT s1"), true),
        routed_sql(REDACTED, refused(REDACTED), false),
    ]);
    for sql in ["SAVEPOINT", REDACTED] {
        assert_eq!(
            fix_of(&report, sql),
            json!({"action": "observe"}),
            "{sql} is never grantable, so it is never a group's either"
        );
    }
    assert!(
        report.findings.iter().all(|finding| !matches!(
            finding,
            Finding::MissingGrant { fix, .. } if fix.action == "add_group"
        )),
        "no group hint for a refusal: {:?}",
        report.findings
    );
}
