//! The whole point of the merge (`MB.42.2.2.1`): one grant diff, two presenters.
//!
//! `audit` (the pytest plugin's document) and `check_audit` (the CLI's JSONL
//! report) ask the same question of the same run. They group the answer
//! differently by design — the plugin files a named group's dead grant under
//! each route, `check` reports it once — but *what* was missing and *what* was
//! dead is one computation, so this fixture asserts both presentations name the
//! same statements.
//!
//! The fixture has no groups, no connection setup and no bodies, which is
//! exactly the case where the two groupings must coincide: every difference that
//! remains would be a second implementation showing through.
//!
//! # What this suite does and does not prove
//!
//! Both entry points go through the same `Ledger::record`, so the *cross*
//! assertion below catches presenter divergence — a report that groups or
//! filters the shared diff differently — and not a change inside the diff
//! itself, which moves both sides together and would keep them equal. What
//! covers the shared pass is the hard-coded statement literals in the expected
//! values: the missing statement, the dead grant and the two totals are written
//! out as text, so a change to normalisation, the allowlist answer or the
//! matched/missing split fails this suite even though both presenters would
//! still agree with each other. Those literals must stay literals.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_audit::{audit, check_audit};
use serde_json::{Value, json};

/// Two views: one the run reached (a granted statement, a denied one and a grant
/// nothing used) and one it never did.
const MANIFEST: &str = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $?'
        query 'SELECT "notes"."title" FROM "notes" LIMIT 21'
    }
}

view checkout "/checkout/" {
    postgres {
        query 'INSERT INTO "orders" ("id") VALUES ($?)'
    }
}
"#;

/// The two statements the suite ran, as the gate's `sql_obs.jsonl` records them.
///
/// One JSON document per line: `parse_sql_obs` reads line by line, so these are
/// rendered compactly rather than written out as a pretty document.
fn granted() -> String {
    json!({
        "target": {"scope": "routed", "route": "notes"},
        "proxy": "db",
        "kind": "query",
        "sql": "SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1",
        "is_tx": false,
        "allowed": true,
        "verdict": {
            "kind": "query",
            "normalized": "SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $?",
        },
    })
    .to_string()
}

fn denied() -> String {
    json!({
        "target": {"scope": "routed", "route": "notes"},
        "proxy": "db",
        "kind": "query",
        "sql": "DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1",
        "is_tx": false,
        "allowed": false,
        "verdict": {
            "kind": "query",
            "normalized": "DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $?",
        },
    })
    .to_string()
}

/// The plugin's document for the same run: one request, the same two statements.
fn observations() -> String {
    json!({
        "version": 1,
        "requests": [{
            "method": "GET",
            "path_info": "/notes/",
            "nodeid": "tests/test_notes.py::test_delete",
            "transaction_control": false,
            "statements": [
                {"alias": "default", "proxy": "db",
                 "sql": "SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1"},
                {"alias": "default", "proxy": "db",
                 "sql": "DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1"},
            ],
        }],
    })
    .to_string()
}

/// The audit's report, parsed.
fn pytest_report() -> Value {
    let text = audit(MANIFEST, &observations()).expect("the plugin report renders");
    serde_json::from_str(&text).expect("the report is JSON")
}

/// The check document, parsed.
fn check_report() -> Value {
    let http = json!({"method": "GET", "path": "/notes/"}).to_string();
    let sql = [granted(), denied()].join("\n");
    let text = check_audit(MANIFEST, &http, &sql).expect("the check report renders");
    serde_json::from_str(&text).expect("the report is JSON")
}

/// The `missing` statements the plugin's report lists for the view.
fn pytest_missing(report: &Value) -> Vec<String> {
    let mut out: Vec<String> = report["routes"]
        .as_array()
        .map(|routes| {
            routes
                .iter()
                .flat_map(|route| {
                    route["proxies"]
                        .as_array()
                        .map(|proxies| {
                            proxies
                                .iter()
                                .flat_map(|proxy| {
                                    proxy["missing"]
                                        .as_array()
                                        .map(|missing| {
                                            missing
                                                .iter()
                                                .filter_map(|entry| {
                                                    entry["sql"].as_str().map(str::to_string)
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default()
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The `unused` statements the plugin's report lists for the view.
fn pytest_dead(report: &Value) -> Vec<String> {
    let mut out: Vec<String> = report["routes"]
        .as_array()
        .map(|routes| {
            routes
                .iter()
                .flat_map(|route| {
                    route["proxies"]
                        .as_array()
                        .map(|proxies| {
                            proxies
                                .iter()
                                .flat_map(|proxy| {
                                    proxy["unused"]
                                        .as_array()
                                        .map(|unused| {
                                            unused
                                                .iter()
                                                .filter_map(|sql| sql.as_str().map(str::to_string))
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default()
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every `sql` of the check document's findings of one `kind`.
fn check_sql(report: &Value, kind: &str) -> Vec<String> {
    let mut out: Vec<String> = report["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter(|finding| finding["kind"] == kind)
                .filter_map(|finding| finding["sql"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[test]
fn both_presenters_name_the_same_executed_but_not_granted_statements() {
    let plugin = pytest_report();
    let check = check_report();

    let missing = pytest_missing(&plugin);
    assert_eq!(
        missing,
        ["DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $?"],
        "the plugin reports exactly the statement the grant did not allow"
    );
    assert_eq!(
        check_sql(&check, "missing_grant"),
        missing,
        "check_audit reports the same statement through the shared diff"
    );
}

#[test]
fn both_presenters_name_the_same_dead_grants() {
    let plugin = pytest_report();
    let check = check_report();

    let dead = pytest_dead(&plugin);
    assert_eq!(
        dead,
        ["SELECT \"notes\".\"title\" FROM \"notes\" LIMIT 21"],
        "the grant nothing ran is the only one the plugin calls unused"
    );
    assert_eq!(
        check_sql(&check, "dead_grant"),
        dead,
        "check_audit's dead grant is the same grant"
    );
}

#[test]
fn both_presenters_agree_the_run_is_not_clean() {
    // `check` has findings, so `ok` is false; the plugin's totals say the same
    // thing in its own vocabulary.
    let plugin = pytest_report();
    let check = check_report();
    assert_eq!(check["ok"], false);
    assert_eq!(plugin["totals"]["clean"], false);
    assert_eq!(plugin["totals"]["missing"], 1);
    assert_eq!(plugin["totals"]["unused"], 1);
    assert_eq!(check["statements"], 2, "both statements were observed");
}
