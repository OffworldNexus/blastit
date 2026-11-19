//! The `blastit check` document's suites, moved with the code.

use serde_json::json;

use crate::*;

const MANIFEST: &str = r#"
        postgres $DATABASE_URL

        view notes regex '/notes/.*' {
            postgres {
                query 'SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1'
            }
        }
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
fn a_transaction_hint_names_the_directive_and_the_view() {
    // The hint `--learn` prints on stderr has to name the `transactions`
    // keyword and the view that needs it: `--learn` never invents a grant,
    // so the sentence is the whole fix.
    let sql = json!({
        "target": {"scope": "unrouted", "method": "POST", "path": "/cart/"},
        "proxy": "db",
        "kind": "tx",
        "sql": "BEGIN",
        "is_tx": true,
        "allowed": true,
        "verdict": {"kind": "query", "normalized": "BEGIN"},
    })
    .to_string();
    let manifest = parse_manifest(MANIFEST).expect("the fixture manifest validates");
    let observations =
        CheckObsV1::from_jsonl(&http("POST", "/cart/"), &sql).expect("the observations parse");
    let plan = plan_learn(&manifest, &observations).expect("the plan is built");
    assert_eq!(
        plan.transaction_control,
        vec![
            "transaction control is not granted automatically: add `transactions` to \
                 `view cart`'s `postgres` block (or to a trait it `use`s)"
                .to_string()
        ]
    );
    let hint = &plan.transaction_control[0];
    assert!(!hint.contains("[route."), "no retired key: {hint}");
    assert!(!hint.contains("transaction_control"), "{hint}");
}

/// A `Blastfile` that grants a table instead of listing statements.
const TABLE_MANIFEST: &str = r#"
postgres $DATABASE_URL

view items regex '/items/.*' {
    postgres {
        select items(id)
    }
}
"#;

/// One routed observation the gate saw with its normalised text — `$?`
/// placeholders and no wire parameters, which is what `sql_obs.jsonl` holds.
fn normalised(sql: &str) -> String {
    json!({
        "target": {"scope": "routed", "route": "items"},
        "proxy": "db",
        "kind": "query",
        "sql": sql,
        "is_tx": false,
        "allowed": false,
        "verdict": {"kind": "query", "normalized": sql},
    })
    .to_string()
}

#[test]
fn a_table_grant_allows_a_parameterised_statement_the_check_saw_normalised() {
    // Allowed: `check` reads the normalised `$?` text, and the table grant
    // analyses it — `select items(id)` covers `WHERE id = $1`, and the
    // `IN ($?, ...)` shape a Django `__in`/`prefetch_related` sends
    // (`MB.40.2`).
    let manifest = parse_manifest(TABLE_MANIFEST).expect("the fixture manifest validates");
    for allowed in [
        r#"SELECT "items"."id" FROM "items" WHERE "items"."id" = $?"#,
        r#"SELECT "items"."id" FROM "items" WHERE "items"."id" IN ($?, ...)"#,
    ] {
        let observations = CheckObsV1::from_jsonl(&http("GET", "/items/1/"), &normalised(allowed))
            .expect("the observations parse");
        let report = evaluate(&manifest, &observations).expect("the audit runs");
        assert!(report.ok, "{allowed}: {:?}", report.findings);
        assert!(
            report.findings.is_empty(),
            "{allowed}: {:?}",
            report.findings
        );
        assert_eq!(report.proxies["db"].denied, 0, "{allowed}");
    }
}

#[test]
fn a_table_grant_still_denies_an_ungranted_column_on_the_same_statement() {
    // Denied twin: the same shapes, one column the grant does not name.
    let manifest = parse_manifest(TABLE_MANIFEST).expect("the fixture manifest validates");
    for denied in [
        r#"SELECT "items"."secret" FROM "items" WHERE "items"."id" = $?"#,
        r#"SELECT "items"."secret" FROM "items" WHERE "items"."secret" IN ($?, ...)"#,
    ] {
        let observations = CheckObsV1::from_jsonl(&http("GET", "/items/1/"), &normalised(denied))
            .expect("the observations parse");
        let report = evaluate(&manifest, &observations).expect("the audit runs");
        assert!(!report.ok, "{denied}");
        assert!(
            report
                .findings
                .iter()
                .any(|finding| matches!(finding, Finding::MissingGrant { .. })),
            "{denied}: {:?}",
            report.findings
        );
    }
}

#[test]
fn learn_proposes_a_missing_grant_on_a_declared_view() {
    // `check` reports the missing statement, so `check --learn` must propose
    // it: a routed statement on a declared view is not "nothing to learn".
    let sql = routed(
        Some(r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $?"#),
        r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $1"#,
        false,
    );
    let manifest = parse_manifest(MANIFEST).expect("the fixture manifest validates");
    let observations =
        CheckObsV1::from_jsonl(&http("GET", "/notes/"), &sql).expect("the observations parse");
    let plan = plan_learn(&manifest, &observations).expect("the plan is built");
    assert_eq!(plan.routes(), 0, "the view is already declared");
    assert_eq!(plan.queries(), 1);
    assert_eq!(plan.edits.grants[0].view, "notes");
    assert!(plan.would_write(false), "a learnable miss is writable");
}

#[test]
fn learn_does_not_repropose_a_statement_the_view_already_grants() {
    // A statement `check` allows is not a proposal: the two must agree in
    // both directions.
    let granted = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1"#;
    let sql = routed(
        Some(r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $?"#),
        granted,
        false,
    );
    let manifest = parse_manifest(MANIFEST).expect("the fixture manifest validates");
    let observations =
        CheckObsV1::from_jsonl(&http("GET", "/notes/"), &sql).expect("the observations parse");
    let plan = plan_learn(&manifest, &observations).expect("the plan is built");
    assert_eq!(plan.queries(), 0, "{:?}", plan.edits.grants);
}

#[test]
fn learn_refuses_scanner_probes() {
    // Denied twin: a bot's traversal is not a view a developer forgot, and
    // a static mount already answers its own paths (`R14`, `R28`).
    for path in ["/.env", "/../etc/passwd", "/static/.env"] {
        let sql = json!({
            "target": {"scope": "unrouted", "method": "GET", "path": path},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": true,
            "verdict": {"kind": "query", "normalized": "SELECT 1"},
        })
        .to_string();
        let manifest = parse_manifest(MANIFEST).expect("the fixture manifest validates");
        let observations =
            CheckObsV1::from_jsonl(&http("GET", path), &sql).expect("the observations parse");
        let plan = plan_learn(&manifest, &observations).expect("the plan is built");
        assert_eq!(plan.routes(), 0, "learned a view for {path}");
        assert_eq!(plan.queries(), 0, "granted a query for {path}");
    }
}

#[test]
fn every_count_is_written_in_its_own_number() {
    // One and many, for each noun the human form writes — including the
    // `-y` → `-ies` one, so `1 query`/`3 queries` and never `querys`.
    for (count, singular, expected) in [
        (1, "route", "route"),
        (2, "route", "routes"),
        (1, "query", "query"),
        (3, "query", "queries"),
        (1, "statement", "statement"),
        (2, "statement", "statements"),
        (1, "grant", "grant"),
        (2, "grant", "grants"),
        (1, "problem", "problem"),
        (3, "problem", "problems"),
    ] {
        assert_eq!(plural(count, singular), expected, "{count} {singular}");
    }
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
