//! The observations document's refusals, and the report's own metadata.

use serde_json::json;

use super::common::*;
use crate::*;
#[test]
fn a_non_postgresql_proxy_cannot_be_used() {
    // Nothing but `postgres` exists in a `Blastfile`, so the parser rejects
    // any other store keyword; the audit reports it as a manifest error.
    let manifest = r#"
amqp $AMQP_URL

view notes "/notes/*" {
}
"#;
    let error = audit(manifest, &document("")).expect_err("unknown protocol");
    assert!(matches!(error, AuditError::Manifest(_)), "{error}");
}

#[test]
fn a_broken_manifest_is_an_error() {
    for manifest in [
        "view a \"/\" {\n",
        "postgres $DB\nview a regex '(' {\n}\n",
        "view a \"/\" {\n    postgres { query 'SELECT 1' }\n}\n",
    ] {
        let error = audit(manifest, &document("")).expect_err("invalid manifest");
        assert!(matches!(error, AuditError::Manifest(_)), "{error}");
    }
}

#[test]
fn broken_observations_are_errors_that_name_where() {
    let cases = [
        ("", "not valid JSON"),
        ("[]", "expected a JSON object"),
        (r#"{"version": 2, "requests": []}"#, "expected 1"),
        (r#"{"version": 1}"#, "missing `requests`"),
        (r#"{"version": 1, "requests": {}}"#, "expected an array"),
        (
            r#"{"version": 1, "requests": [1]}"#,
            "requests[0]: expected an object",
        ),
        (
            r#"{"version": 1, "requests": [{"path_info": "/"}]}"#,
            "requests[0]: missing `method`",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET"}]}"#,
            "requests[0]: missing `path_info`",
        ),
        (
            r#"{"version": 1, "requests": [{"method": 1, "path_info": "/"}]}"#,
            "requests[0]: method: expected a string",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "nodeid": 3}]}"#,
            "nodeid: expected a string or null",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "transaction_control": "yes"}]}"#,
            "transaction_control: expected a boolean",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": {}}]}"#,
            "statements: expected an array",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": [{"sql": 1}]}]}"#,
            "requests[0].statements[0]: sql: expected a string",
        ),
        (
            r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": [{}]}]}"#,
            "statements[0]: missing `sql`",
        ),
        (
            r#"{"version": 1, "requests": [], "connection_setup": {}}"#,
            "connection_setup: expected an array",
        ),
        (
            r#"{"version": 1, "requests": [], "connection_setup": [1]}"#,
            "connection_setup[0]: expected an object",
        ),
        (
            r#"{"version": 1, "requests": [], "connection_setup": [{}]}"#,
            "connection_setup[0]: missing `sql`",
        ),
        (
            r#"{"version": 1, "requests": [], "connection_setup": [{"sql": "SELECT 1", "count": "3"}]}"#,
            "connection_setup[0]: count: expected a non-negative integer",
        ),
        (
            r#"{"version": 1, "requests": [], "connection_setup": [{"sql": 1}]}"#,
            "connection_setup[0]: sql: expected a string",
        ),
    ];
    for (source, expected) in cases {
        let error = audit(SIMPLE, source).expect_err("invalid observations");
        let message = error.to_string();
        assert!(
            message.contains(expected),
            "{source}: expected {expected:?} in {message:?}"
        );
    }
}

#[test]
fn unknown_keys_are_ignored_so_versions_can_differ() {
    let source = r#"{"version": 1, "plugin": "0.2", "requests": [
            {"method": "GET", "path_info": "/notes/", "extra": true, "statements": [
                {"alias": "default", "proxy": "db", "sql": "SELECT 1", "how_long": 3}
            ]}
        ]}"#;
    assert_eq!(run(SIMPLE, source)["totals"]["missing"], 1);
}

#[test]
fn examples_are_deduplicated_and_capped_at_five() {
    let mut requests = Vec::new();
    for index in 0..9 {
        requests.push(format!(
            r#"{{"method": "GET", "path_info": "/notes/{index}/",
                    "nodeid": "tests/test_app.py::test_{index}",
                    "statements": [{}]}}"#,
            statement("SELECT 1")
        ));
    }
    // The same test and path twice, to check they are not listed twice.
    requests.push(requests[0].clone());
    let report = run(SIMPLE, &document(&requests.join(",")));
    let missing = &proxy(&report, "notes")["missing"][0];
    assert_eq!(missing["count"], 10);
    let tests: Vec<&str> = missing["tests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|test| test.as_str().unwrap())
        .collect();
    assert_eq!(tests.len(), 5);
    assert_eq!(tests[0], "tests/test_app.py::test_0");
    assert_eq!(tests[4], "tests/test_app.py::test_4");
    let paths: Vec<&str> = missing["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/notes/0/",
            "/notes/1/",
            "/notes/2/",
            "/notes/3/",
            "/notes/4/"
        ]
    );
}

#[test]
fn a_request_outside_a_test_is_labelled() {
    let report = run(
        SIMPLE,
        &document(
            r#"{"method": "GET", "path_info": "/notes/", "statements": [{"sql": "SELECT 1"}]}"#,
        ),
    );
    assert_eq!(proxy(&report, "notes")["missing"][0]["tests"][0], NO_TEST);
}

#[test]
fn an_empty_session_reports_every_route_as_unexercised() {
    let report = run(MANIFEST, &document(""));
    assert_eq!(report["unexercised_routes"], json!(["notes", "checkout"]));
    assert_eq!(report["totals"]["requests"], 0);
    assert_eq!(report["totals"]["routes_exercised"], 0);
    assert_eq!(report["totals"]["missing"], 0);
    assert_eq!(report["totals"]["unused"], 0);
    assert_eq!(report["totals"]["clean"], true);
    assert_eq!(report["unrouted"], json!([]));
}

#[test]
fn the_report_names_the_manifests_proxies_and_warnings() {
    let manifest = format!(
        "{MANIFEST}\ntrait orphan {{\n    postgres {{\n        query 'SELECT 1'\n    }}\n}}\n"
    );
    let report = run(&manifest, &document(""));
    assert_eq!(report["manifest"]["proxies"], json!({"db": "postgresql"}));
    assert_eq!(report["manifest"]["routes"], 2);
    assert_eq!(report["version"], 1);
    let warnings = report["manifest"]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "one unattached trait: {warnings:?}");
    assert!(
        warnings[0].as_str().unwrap().contains("orphan"),
        "{warnings:?}"
    );
}

#[test]
fn two_aliases_on_one_proxy_are_merged() {
    // Two database aliases mapped to the same proxy are one allowlist: the
    // report is per (route, proxy), not per alias.
    let observations = document(&request(
        "GET",
        "/notes/",
        &format!(
            "{}, {}",
            r#"{"alias": "default", "proxy": "db", "sql": "SELECT 1"}"#,
            r#"{"alias": "replica", "proxy": "db", "sql": "SELECT 1"}"#
        ),
    ));
    let report = run(SIMPLE, &observations);
    assert_eq!(route(&report, "notes")["proxies"][0]["observed"], 2);
    assert_eq!(proxy(&report, "notes")["missing"][0]["count"], 2);
    assert_eq!(route(&report, "notes")["requests"], 1);
}

#[test]
fn the_report_is_stable() {
    let observations = document(&format!(
        "{}, {}",
        request("GET", "/notes/", &statement("SELECT 1")),
        request("GET", "/oops/", "")
    ));
    let first = audit(MANIFEST, &observations).expect("runs");
    let second = audit(MANIFEST, &observations).expect("runs");
    assert_eq!(first, second, "the same observations give the same report");
    assert!(first.ends_with('}'));
}
