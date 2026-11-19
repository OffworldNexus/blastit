//! Egress, and what the plugin itself could not read.

use serde_json::{Value, json};

use super::common::*;
use super::setup::SETUP;
/// One `egress` entry.
fn attempt(host: &str, port: u16) -> String {
    format!(r#"{{"family": "AF_INET", "host": "{host}", "port": {port}}}"#)
}

#[test]
fn a_route_that_opens_connections_without_egress_is_reported() {
    // A route without `egress = true` may not open outbound connections
    // (§7.3): the seccomp filter, or the egress grant, is what allows it.
    let report = run(
        SETUP,
        &document_with_egress(
            "/notes/",
            &format!(
                "{}, {}, {}",
                attempt("redis", 6379),
                attempt("redis", 6379),
                attempt("example.com", 443)
            ),
        ),
    );
    let egress = &route(&report, "notes")["egress"];
    assert_eq!(egress["needed"], true);
    let observed = egress["observed"].as_array().expect("a list");
    assert_eq!(observed.len(), 2, "one line per destination");
    assert_eq!(
        observed[0]["target"], "redis:6379",
        "the most attempted first"
    );
    assert_eq!(observed[0]["host"], "redis");
    assert_eq!(observed[0]["port"], 6379);
    assert_eq!(observed[0]["family"], "AF_INET");
    assert_eq!(observed[0]["count"], 2);
    assert_eq!(observed[0]["tests"], json!(["tests/test_app.py::test_it"]));
    assert_eq!(observed[1]["target"], "example.com:443");
    assert_eq!(observed[1]["count"], 1);
    assert_eq!(report["totals"]["egress"], 1);
    assert_eq!(report["totals"]["clean"], false);
}

#[test]
fn a_route_that_may_reach_the_network_is_not_asking_for_anything() {
    let manifest = format!("{SETUP}\n# the view that runs it may open connections\n");
    let report = run(
        &manifest.replace(
            "view notes \"/notes/*\" {\n",
            "view notes \"/notes/*\" {\n    egress any\n",
        ),
        &document_with_egress("/notes/", &attempt("redis", 6379)),
    );
    let egress = &route(&report, "notes")["egress"];
    assert_eq!(egress["observed"].as_array().unwrap().len(), 1);
    assert_eq!(egress["needed"], false, "the view grants egress already");
    assert_eq!(report["totals"]["egress"], 0);
}

#[test]
fn a_route_that_opens_nothing_reports_no_egress() {
    let report = run(SETUP, &document(&request("GET", "/notes/", "")));
    assert_eq!(
        route(&report, "notes")["egress"],
        json!({"observed": [], "needed": false})
    );
    assert_eq!(route(&report, "assets")["egress"]["observed"], json!([]));
    assert_eq!(report["totals"]["egress"], 0);
}

#[test]
fn statements_the_plugin_could_not_read_are_counted_not_dropped() {
    let observations = r#"{"version": 1, "requests": [], "unrecognised": [
            {"alias": "default", "query": "Template('SELECT ...')", "count": 3},
            {"query": "Template('BEGIN')"}
        ]}"#;
    let report = run(SETUP, observations);
    assert_eq!(report["unrecognised"]["count"], 4);
    let statements = report["unrecognised"]["statements"]
        .as_array()
        .expect("a list");
    assert_eq!(statements.len(), 2);
    assert_eq!(statements[0]["query"], "Template('SELECT ...')");
    assert_eq!(statements[0]["alias"], "default");
    assert_eq!(statements[0]["count"], 3);
    assert_eq!(statements[1]["count"], 1, "a missing count means one");
    assert_eq!(statements[1]["alias"], Value::Null);
    assert_eq!(report["totals"]["unrecognised"], 4);
    assert_eq!(report["totals"]["clean"], false, "the report is incomplete");
}

#[test]
fn a_plugin_warning_is_carried_into_the_report() {
    let observations = r#"{"version": 1, "requests": [], "warnings": [
            "the database alias 'default' does not use server-side binding"
        ]}"#;
    let report = run(SETUP, observations);
    assert_eq!(
        report["plugin_warnings"],
        json!(["the database alias 'default' does not use server-side binding"])
    );
    assert_eq!(report["totals"]["plugin_warnings"], 1);
    assert_eq!(report["totals"]["clean"], false, "a warning is not clean");
}

#[test]
fn an_empty_session_is_clean_with_the_new_sections() {
    let report = run(SETUP, &document(""));
    assert_eq!(
        report["unrecognised"],
        json!({"statements": [], "count": 0})
    );
    assert_eq!(report["plugin_warnings"], json!([]));
    assert_eq!(
        route(&report, "notes")["egress"],
        json!({"observed": [], "needed": false})
    );
    assert_eq!(report["totals"]["clean"], true);
    assert!(
        report.get("script_name").is_none(),
        "no mount prefix to report"
    );
}

//
// A trait is the view's own grant, and global groups do not exist (§7.1).
//

/// Two traits: one every view `use`s, and one only `notes` names.
pub(super) const GLOBAL: &str = r#"
postgres $DATABASE_URL

trait session {
    postgres {
        query 'SELECT 0'
    }
}

trait own {
    postgres {
        query 'SELECT 9'
    }
}

view notes "/notes/*" {
    use session, own
    postgres {
        query 'SELECT 1'
    }
}

view orders "/orders/" {
    use session
    postgres {
        query 'SELECT 2'
    }
}
"#;

#[test]
fn a_trait_every_view_uses_is_reported_per_view_not_once_globally() {
    let report = run(
        GLOBAL,
        &document(&request("GET", "/notes/", &statement("SELECT 1"))),
    );
    // A `Blastfile` has no global groups (§7.1): a trait the views `use` is
    // each view's own grant, so its unmatched statement is that view's
    // `unused` line — and `unused_global` is empty by construction.
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!(["SELECT 0", "SELECT 9"]),
        "the trait every view uses, and the one only this view names"
    );
    assert_eq!(report["unused_global"]["statements"], json!([]));
    assert_eq!(report["unused_global"]["count"], 0);
    assert_eq!(report["totals"]["unused"], 2);
    assert_eq!(report["totals"]["unused_global"], 0);
    assert_eq!(
        report["totals"]["clean"], true,
        "unused grants do not fail strict"
    );
}

#[test]
fn a_trait_statement_one_view_used_is_unused_only_where_unused() {
    let report = run(
        GLOBAL,
        &document(&request("GET", "/notes/", &statement("SELECT 0"))),
    );
    assert_eq!(report["unused_global"]["statements"], json!([]));
    assert_eq!(report["unused_global"]["count"], 0);
    assert_eq!(report["totals"]["unused_global"], 0);
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!(["SELECT 9", "SELECT 1"]),
        "the view ran the trait's statement, so it is not dead here"
    );
}

/// A trait whose only grant is a table grant (D7).
const GLOBAL_TABLES: &str = r#"
postgres $DATABASE_URL

trait catalog {
    postgres {
        select shop_product(id)
    }
}

view notes "/notes/*" {
    use catalog
    postgres {
        query 'SELECT 1'
    }
}
"#;

#[test]
fn a_trait_table_grant_a_statement_needed_is_used() {
    let report = run(
        GLOBAL_TABLES,
        &document(&request(
            "GET",
            "/notes/",
            &statement(r#"SELECT \"shop_product\".\"id\" FROM \"shop_product\""#),
        )),
    );
    assert_eq!(
        proxy(&report, "notes")["missing"],
        json!([]),
        "the trait's table grant allowed it"
    );
    assert_eq!(report["unused_global"]["tables"], json!([]));
    assert_eq!(report["unused_global"]["count"], 0);
}
