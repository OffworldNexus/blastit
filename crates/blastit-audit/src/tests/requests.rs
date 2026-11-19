//! The request/statement diff: missing, unused, unrouted and transaction control.

use serde_json::json;

use super::common::*;
use crate::*;
#[test]
fn grants_that_nothing_matched_are_unused() {
    let manifest = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
        query 'SELECT 2 ;'
        query 'SELECT 2'
    }
}
"#;
    let report = run(
        manifest,
        &document(&request("GET", "/notes/", &statement("SELECT 1;"))),
    );
    // `SELECT 2` and `SELECT 2 ;` are the same grant: it is listed once.
    assert_eq!(proxy(&report, "notes")["unused"], json!(["SELECT 2"]));
    assert_eq!(report["totals"]["unused"], 1);
    assert_eq!(
        report["totals"]["clean"], true,
        "unused grants do not fail strict"
    );
}

#[test]
fn a_route_that_ran_but_never_touched_a_proxy_lists_all_its_grants_as_unused() {
    let report = run(SIMPLE, &document(&request("GET", "/notes/", "")));
    assert_eq!(route(&report, "notes")["requests"], 1);
    assert_eq!(proxy(&report, "notes")["observed"], 0);
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!([r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21"#])
    );
}

#[test]
fn a_route_with_no_grant_denies_everything() {
    let manifest = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
    }
}

view about "/about/*" {
}
"#;
    let report = run(
        manifest,
        &document(&request("GET", "/about/x", &statement("SELECT 1"))),
    );
    assert_eq!(missing_sql(&report, "about"), ["SELECT 1"]);
    assert_eq!(route(&report, "about")["proxies"][0]["name"], "db");
    assert_eq!(
        report["unexercised_routes"],
        json!(["notes"]),
        "the route with a grant was never requested"
    );
    assert_eq!(report["totals"]["unused"], 0);
}

#[test]
fn an_unrouted_request_is_reported_with_its_test() {
    let report = run(
        SIMPLE,
        &document(&format!(
            "{}, {}",
            request("GET", "/oops/", &statement("SELECT 1")),
            request("GET", "/oops/", ""),
        )),
    );
    let unrouted = &report["unrouted"][0];
    assert_eq!(unrouted["method"], "GET");
    assert_eq!(unrouted["path_info"], "/oops/");
    assert_eq!(unrouted["count"], 2);
    assert_eq!(unrouted["statements"], 1);
    assert_eq!(unrouted["tests"][0], "tests/test_app.py::test_it");
    assert_eq!(report["totals"]["unrouted"], 2);
    assert_eq!(report["totals"]["routed"], 0);
    assert_eq!(report["totals"]["clean"], false);
}

#[test]
fn a_savepoint_needs_transaction_control() {
    // A nested `atomic` inside a test is a savepoint, and a nested atomic is
    // what an `ATOMIC_REQUESTS` request looks like from inside pytest-django.
    let denying = run(
        SIMPLE,
        &document(&request("POST", "/notes/", &statement("SAVEPOINT s1"))),
    );
    assert_eq!(missing_sql(&denying, "notes"), ["SAVEPOINT s1"]);
    assert_eq!(route(&denying, "notes")["needs_transaction_control"], true);
    assert_eq!(
        route(&denying, "notes")["proxies"][0]["transaction_control"]["needed"],
        true
    );
    assert_eq!(
        denying["totals"]["needs_transaction_control"],
        json!(["notes"])
    );

    let granted = run(
        MANIFEST,
        &document(&request("POST", "/notes/", &statement("SAVEPOINT s1"))),
    );
    // The session group grants transaction control to every route.
    assert_eq!(missing_sql(&granted, "notes").len(), 0);
    assert_eq!(route(&granted, "notes")["needs_transaction_control"], false);
    assert_eq!(
        route(&granted, "notes")["proxies"][0]["transaction_control"]["granted"],
        true
    );
    assert_eq!(granted["totals"]["clean"], true);
}

#[test]
fn a_request_that_needs_a_transaction_says_so_without_a_statement() {
    // `ATOMIC_REQUESTS` on the alias: the transaction is there even when the
    // test's own atomic swallows the `BEGIN`.
    let observations = format!(
        r#"{{"version": 1, "requests": [{{"method": "POST", "path_info": "/notes/",
                "nodeid": "tests/test_app.py::test_it", "transaction_control": true,
                "statements": [{}]}}]}}"#,
        statement("SELECT 1")
    );
    let report = run(SIMPLE, &observations);
    assert_eq!(route(&report, "notes")["needs_transaction_control"], true);
    assert_eq!(
        report["totals"]["needs_transaction_control"],
        json!(["notes"])
    );
    assert_eq!(report["totals"]["clean"], false);
}

#[test]
fn a_route_no_request_reached_is_named_and_reports_no_proxy() {
    let report = run(MANIFEST, &document(&request("GET", "/notes/", "")));
    assert_eq!(report["unexercised_routes"], json!(["checkout"]));
    assert_eq!(route(&report, "checkout")["requests"], 0);
    assert_eq!(route(&report, "checkout")["proxies"], json!([]));
    assert_eq!(report["totals"]["routes"], 2);
    assert_eq!(report["totals"]["routes_exercised"], 1);
    // `notes` ran without touching the database, so its own grant and the
    // trait it uses are unused; a `Blastfile` has no global groups (D3c),
    // and `checkout` did not run at all, so it claims nothing.
    assert_eq!(report["totals"]["unused"], 2);
    assert_eq!(report["totals"]["unused_global"], 0);
}

#[test]
fn a_method_a_route_does_not_answer_is_unrouted() {
    let report = run(MANIFEST, &document(&request("GET", "/checkout/", "")));
    // `/checkout/` answers no method but POST? It lists none, so GET is
    // routed there; the `notes` route only answers GET and POST.
    assert_eq!(report["unrouted"].as_array().unwrap().len(), 0);
    let report = run(MANIFEST, &document(&request("DELETE", "/notes/", "")));
    assert_eq!(report["unrouted"][0]["method"], "DELETE");
    assert_eq!(report["unrouted"][0]["path_info"], "/notes/");
}

#[test]
fn catches_all_routing_is_the_manifests_own() {
    // First match wins, in Blastfile order.
    let manifest = r#"
postgres $DATABASE_URL

view fallback "/*" {
}

view notes "/notes/*" {
}
"#;
    let report = run(
        manifest,
        &document(&request("GET", "/notes/", &statement("SELECT 1"))),
    );
    assert_eq!(report["unrouted"].as_array().unwrap().len(), 0);
    assert_eq!(report["unexercised_routes"], json!(["notes"]));
    // The catch-all has no grant, so the statement is missing there.
    assert_eq!(
        route(&report, "fallback")["proxies"][0]["missing"][0]["sql"],
        "SELECT 1"
    );
}

#[test]
fn the_only_postgresql_proxy_is_inferred() {
    let observations = document(&request(
        "GET",
        "/notes/",
        r#"{"alias": "replica", "sql": "SELECT 1"}"#,
    ));
    let report = run(SIMPLE, &observations);
    assert_eq!(route(&report, "notes")["proxies"][0]["name"], "db");
    assert_eq!(missing_sql(&report, "notes"), ["SELECT 1"]);
}

#[test]
fn an_undeclared_proxy_is_an_error() {
    let observations = document(&request(
        "GET",
        "/notes/",
        r#"{"proxy": "nope", "sql": "SELECT 1"}"#,
    ));
    let error = audit(SIMPLE, &observations).expect_err("unknown proxy");
    assert!(matches!(error, AuditError::UnknownProxy(ref name) if name == "nope"));
    assert!(error.to_string().contains("nope"), "{error}");
}

#[test]
fn several_proxies_without_a_statement_proxy_is_an_error() {
    let manifest = r#"
postgres $DATABASE_URL
postgres warehouse $WAREHOUSE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
    }
}
"#;
    let observations = document(&request(
        "GET",
        "/notes/",
        r#"{"alias": "default", "sql": "SELECT 1"}"#,
    ));
    let error = audit(manifest, &observations).expect_err("ambiguous");
    assert!(
        matches!(error, AuditError::AmbiguousProxy(ref names) if names == &["db", "warehouse"])
    );
    // Naming the proxy makes it work, on either one.
    let observations = document(&request(
        "GET",
        "/notes/",
        r#"{"alias": "default", "proxy": "warehouse", "sql": "SELECT 1"}"#,
    ));
    let report = run(manifest, &observations);
    let proxies = route(&report, "notes")["proxies"].as_array().unwrap();
    assert_eq!(
        proxies.len(),
        2,
        "the declared grant and the observed proxy"
    );
    assert_eq!(proxies[0]["name"], "db");
    assert_eq!(proxies[0]["observed"], 0);
    assert_eq!(proxies[0]["unused"], json!(["SELECT 1"]));
    assert_eq!(proxies[1]["name"], "warehouse");
    assert_eq!(proxies[1]["missing"][0]["sql"], "SELECT 1");
}

#[test]
fn a_manifest_without_a_postgresql_proxy_is_an_error() {
    let manifest = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
}
"#;
    let observation = document(&request(
        "GET",
        "/notes/",
        r#"{"alias": "default", "sql": "SELECT 1"}"#,
    ));
    // ... a view with no grant still knows the store: the Blastfile has one.
    assert_eq!(run(manifest, &observation)["totals"]["missing"], 1);

    let no_proxy = r#"
view notes "/notes/*" {
}
"#;
    let error = audit(no_proxy, &observation).expect_err("no proxy");
    assert!(matches!(error, AuditError::NoProxy));
}
