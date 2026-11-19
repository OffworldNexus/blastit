//! Global groups, table grants and the lowered model.

use serde_json::{Value, json};

use blastit_manifest::ProxyName;

use super::common::*;
use super::egress::GLOBAL;
use crate::*;

#[test]
fn the_covering_grant_of_a_set_of_statements_is_taken_at_once() {
    // A row lock asks for `update` on *one* column, and the covering grant
    // picks one the route already reads when it is given every statement at
    // once — which is why the fold tool calls this function and not the
    // per-statement analysis.
    let statements = vec![
        r#"SELECT "notes_note"."id" FROM "notes_note" WHERE "notes_note"."id" = $1 FOR UPDATE"#
            .to_string(),
        r#"SELECT "notes_note"."id", "notes_note"."owner_id" FROM "notes_note"
                WHERE "notes_note"."owner_id" = $1"#
            .to_string(),
    ];
    let document: Value =
        serde_json::from_str(&covering(&statements, "view notes postgres").expect("renders"))
            .expect("JSON");
    assert_eq!(
        document["tables"]["notes_note"]["select"],
        json!(["id", "owner_id"])
    );
    assert_eq!(document["tables"]["notes_note"]["update"], json!(["id"]));
    assert_eq!(document["functions"], json!([]));
    assert_eq!(document["unanalysable"], json!([]));
    assert_eq!(
        document["blastfile"].as_str().expect("a string"),
        "# view notes postgres\n\
             # notes_note: `update notes_note(id)` only locks its rows (`FOR UPDATE`) — any one \
             column is enough, as PostgreSQL requires.\n\
             select notes_note(id, owner_id)\n\
             update notes_note(id)\n"
    );
}

/// A route whose only evidence is a live server's log (D3c).
#[test]
fn a_live_logs_request_claims_nothing_about_a_routes_grants() {
    let observations = format!(
        r#"{{"version": 1, "requests": [{{"method": "GET", "path_info": "/notes/",
                "source": "live", "statements": [{}]}}]}}"#,
        statement("SELECT 'denied'")
    );
    let report = run(GLOBAL, &observations);
    assert_eq!(
        missing_sql(&report, "notes"),
        ["SELECT 'denied'"],
        "a denial in the log is a missing grant, whether it came from a log or not"
    );
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!([]),
        "the log says what was denied, never what ran: no grant is called dead"
    );
    assert_eq!(report["unused_global"]["statements"], json!([]));
    assert_eq!(report["unused_global"]["count"], 0);
    assert_eq!(report["totals"]["unused"], 0);
    assert_eq!(report["totals"]["unused_global"], 0);
    assert_eq!(report["totals"]["clean"], false, "a grant is missing");
}

#[test]
fn a_route_the_suite_reached_still_reports_its_own_grants_as_unused() {
    let observations = format!(
        r#"{{"version": 1, "requests": [
                {{"method": "GET", "path_info": "/notes/", "statements": []}},
                {{"method": "GET", "path_info": "/orders/", "source": "live",
                  "statements": [{}]}}
            ]}}"#,
        statement("SELECT 'denied'")
    );
    let report = run(GLOBAL, &observations);
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!(["SELECT 0", "SELECT 9", "SELECT 1"]),
        "the suite's own request is evidence about the view it reached"
    );
    assert_eq!(
        proxy(&report, "orders")["unused"],
        json!([]),
        "and a view only a log saw still claims nothing"
    );
    assert_eq!(
        report["unused_global"]["statements"],
        json!([]),
        "a `Blastfile` has no global groups, so the section is always empty"
    );
}

#[test]
fn a_statement_no_table_grant_can_describe_comes_back_separately() {
    let statements = vec!["SET TIME ZONE 'UTC'".to_string()];
    let document: Value =
        serde_json::from_str(&covering(&statements, "view notes postgres").expect("renders"))
            .expect("JSON");
    assert_eq!(document["tables"], json!({}));
    assert_eq!(document["functions"], json!([]));
    let unanalysable = document["unanalysable"].as_array().expect("a list");
    assert_eq!(unanalysable.len(), 1);
    assert_eq!(unanalysable[0]["sql"], "SET TIME ZONE 'UTC'");
    assert!(
        unanalysable[0]["reason"]
            .as_str()
            .expect("a string")
            .contains("only SELECT"),
        "{:?}",
        unanalysable[0]["reason"]
    );
}

#[test]
fn the_model_names_the_stores_traits_and_views() {
    let source = r#"
postgres $DATABASE_URL
postgres warehouse $WAREHOUSE_URL

trait session {
    postgres {
        transactions
        query 'SELECT 40'
    }
}

view notes GET, POST "/notes/*" {
    use session
    postgres {
        select products(id, name)
        query 'SELECT 10'
    }
}
"#;
    let model: Value = serde_json::from_str(&blastfile_model(source).expect("the model renders"))
        .expect("the model is JSON");
    assert_eq!(
        model["stores"],
        json!([
            {"name": "db", "protocol": "postgresql", "url": "DATABASE_URL",
             "inputs": {"url": "DATABASE_URL"}},
            {"name": "warehouse", "protocol": "postgresql",
             "url": "WAREHOUSE_URL", "inputs": {"url": "WAREHOUSE_URL"}},
        ])
    );
    assert_eq!(
        model["traits"]["session"]["db"]["queries"],
        json!(["SELECT 40"])
    );
    assert_eq!(model["traits"]["session"]["db"]["transactions"], true);
    let notes = &model["views"][0];
    assert_eq!(notes["name"], "notes");
    assert_eq!(notes["methods"], json!(["GET", "HEAD", "POST"]));
    assert_eq!(notes["patterns"], json!(["/notes/.*"]));
    assert_eq!(notes["use"], json!(["session"]));
    assert_eq!(notes["egress"], false);
    assert_eq!(notes["grants"]["db"]["queries"], json!(["SELECT 10"]));
    assert_eq!(
        notes["grants"]["db"]["tables"]["products"]["select"],
        json!(["id", "name"])
    );
    assert_eq!(notes["grants"]["db"]["unrestricted"], false);
}

#[test]
fn the_model_refuses_a_document_that_is_not_a_blastfile() {
    let error = blastfile_model("version = 1\n[proxy.db]\n").expect_err("not a Blastfile");
    assert!(matches!(error, AuditError::Manifest(_)), "{error}");
}

#[test]
fn the_audit_reads_the_blastfile_reader_and_not_the_retired_one() {
    // A document in the retired shape parses to a `Blastfile` with a syntax
    // error, which is the proof that no default path reaches the old reader.
    let retired =
        "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = { url = \"DATABASE_URL\" }\n";
    let error = audit(retired, &document("")).expect_err("the retired document is refused");
    assert!(matches!(error, AuditError::Manifest(_)), "{error}");
}

/// A trait whose table grant covers its own query, used by one view that also
/// grants a statement the table grant already covers.
///
/// `check` reports the group's dead grant once, on the trait; the plugin report
/// keeps it on every route that names the group. Both name the same statement,
/// and neither calls the route's own covered query dead — the divergence is
/// grouping, and this fixture pins it in both directions.
const COVERED_GROUP: &str = r#"
postgres $DATABASE_URL

trait session {
    postgres {
        query 'SELECT "products"."id" FROM "products" LIMIT 21'
        select products(id)
    }
}

view notes "/notes/*" {
    use session
    postgres {
        query 'SELECT "products"."id" FROM "products"'
    }
}
"#;

#[test]
fn a_groups_dead_grant_and_the_plugins_unused_line_name_the_same_statement() {
    let observations = document(&request("GET", "/notes/", ""));
    let plugin = run(COVERED_GROUP, &observations);

    assert_eq!(
        proxy(&plugin, "notes")["unused"],
        json!(["SELECT \"products\".\"id\" FROM \"products\" LIMIT 21"]),
        "the plugin keeps a named group's unmatched statement on the route that \
         names it, even though the group's own table grant covers it"
    );

    // The same run through `check`: one dead grant, owned by the trait.
    let http = json!({"method": "GET", "path": "/notes/"}).to_string();
    let check: Value = serde_json::from_str(
        &check_audit(COVERED_GROUP, &http, "").expect("the check document renders"),
    )
    .expect("JSON");
    let dead: Vec<&Value> = check["findings"]
        .as_array()
        .expect("findings is a list")
        .iter()
        .filter(|finding| finding["kind"] == "dead_grant")
        .collect();
    assert_eq!(
        dead.len(),
        1,
        "the trait reports it once, not once per naming route: {dead:?}"
    );
    assert_eq!(dead[0]["group"], "session");
    assert_eq!(
        dead[0]["sql"],
        "SELECT \"products\".\"id\" FROM \"products\" LIMIT 21"
    );
    assert_eq!(
        dead[0]["fix"]["to"], "trait session",
        "and its fix edits the trait, the node the grant lives on"
    );

    // The route's own query is covered by the group's table grant, so neither
    // report calls it dead (`R34`): that part must not drift while the two
    // groupings differ.
    assert!(
        !proxy(&plugin, "notes")["unused"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|sql| sql == "SELECT \"products\".\"id\" FROM \"products\"")
    );
    assert!(
        dead.iter()
            .all(|finding| finding["sql"] != "SELECT \"products\".\"id\" FROM \"products\"")
    );
}

#[test]
fn an_unrestricted_grant_counts_an_allowed_statement_as_matched() {
    // `Ledger::record` files a statement under `matched` when the pair's grant
    // **allowed** it, not only when the grant lists it. An `unrestricted` grant
    // lists nothing (`GrantCheck::granted()` is empty by design), so the older
    // gate left every statement it allowed out of `matched`; `check`'s dead-grant
    // rule needs the allow answer, and one meaning of `matched` is the point.
    let manifest = parse_manifest(
        "postgres $DATABASE_URL\n\nview shop \"/shop/*\" {\n    postgres unrestricted\n}\n",
    )
    .expect("the fixture manifest validates");
    let check = GrantCheck::of(grant_of(&manifest, 0, "db").as_ref());
    assert!(
        check.granted().is_empty(),
        "an unrestricted grant holds no statements"
    );
    assert!(check.allows("SELECT 1"));

    let mut ledger = Ledger::new(&manifest);
    ledger.request(0, false);
    let decision = ledger.record(0, &ProxyName::new("db"), "SELECT 1", None, None);
    assert!(decision.granted);
    assert!(
        ledger.matched_contains(0, &normalize("SELECT 1")),
        "the statement the grant allowed is the statement the route matched"
    );
}
