//! `connection_setup`: the statements a connection runs on its own.

use serde_json::json;

use super::common::*;
use crate::*;
#[test]
fn normalisation_is_the_policys_own() {
    assert_eq!(normalize("  SELECT  1 ;  "), "SELECT 1");
    assert_eq!(normalize("SELECT 'a  b'"), "SELECT 'a  b'");
    assert_eq!(normalize("SELECT 1 -- c\nFROM t"), "SELECT 1 FROM t");
    for sql in ["SELECT 1 ;", "SELECT 'a  b'", "", ";", "SELECT $1"] {
        let once = normalize(sql);
        assert_eq!(normalize(&once), once, "idempotent for {sql:?}");
    }
}

#[test]
fn odd_inputs_never_panic() {
    let observations = document(&format!(
        "{}, {}",
        request("GET", "/notes/", &statement("")),
        r#"{"method": "", "path_info": "", "nodeid": "", "statements": [{"sql": "🦀"}]}"#
    ));
    let report = run(SIMPLE, &observations);
    assert_eq!(report["totals"]["routed"], 1);
    let sqls = missing_sql(&report, "notes");
    assert!(
        sqls.contains(&String::new()),
        "an empty statement is denied, never matched: {sqls:?}"
    );
}

#[test]
fn transaction_control_is_recognised_by_the_policy() {
    let probe = TcProbe::new();
    for sql in [
        "BEGIN",
        "commit ;",
        "ROLLBACK;",
        "SAVEPOINT s1",
        "SAVEPOINT \"s1_x\"",
        "RELEASE SAVEPOINT s1",
        "ROLLBACK TO SAVEPOINT s1",
        "BEGIN; -- a comment",
    ] {
        assert!(
            probe.is_transaction_control(&normalize(sql)),
            "{sql:?} is transaction control"
        );
    }
    for sql in [
        "SELECT 1",
        "BEGIN; DROP TABLE t",
        "SAVEPOINT",
        "SAVEPOINT s1; SELECT 1",
        "",
        "SET TIME ZONE 'UTC'",
    ] {
        assert!(
            !probe.is_transaction_control(&normalize(sql)),
            "{sql:?} is not transaction control"
        );
    }
}

#[test]
fn a_query_followed_by_a_second_statement_never_matches() {
    // The whole normalised text is compared, so a listed query with a second
    // statement appended is a different text (§6).
    let report = run(
        SIMPLE,
        &document(&request(
            "GET",
            "/notes/",
            &statement(
                r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21; DROP TABLE \"notes\""#,
            ),
        )),
    );
    assert_eq!(
        missing_sql(&report, "notes"),
        [
            r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21; DROP TABLE "notes""#
        ]
    );
}

/// A one-route manifest whose grant holds exactly `queries`, as TOML basic
/// strings (the escaping is JSON's).
fn manifest_with_queries(queries: &[String]) -> String {
    let mut out = String::from(
        "postgres $DATABASE_URL\n\nview notes GET, POST \"/notes/*\" {\n    postgres {\n",
    );
    for query in queries {
        out.push_str("        query ");
        out.push_str(&blastfile_string(query));
        out.push('\n');
    }
    out.push_str("    }\n}\n");
    out
}

/// A `Blastfile` string for `sql`: a raw `'…'` when it holds no `'`, else a
/// `"…"` with JSON's escapes (which are the grammar's for these texts).
fn blastfile_string(sql: &str) -> String {
    if sql.contains('\'') {
        serde_json::to_string(sql).expect("a string")
    } else {
        format!("'{sql}'")
    }
}

/// One observed statement whose SQL is escaped for the JSON document, for a
/// test that would rather not escape it by hand.
fn json_statement(sql: &str) -> String {
    format!(
        r#"{{"alias": "default", "proxy": "db", "sql": {}}}"#,
        serde_json::to_string(sql).expect("a string")
    )
}

#[test]
fn a_list_of_placeholders_after_in_is_one_grant_of_any_length() {
    // D3a: `IN ($1)`, `IN ($1, $2)`, `IN ($1, $2, $3)`… are one shape, so a
    // prefix or a `prefetch_related` of ten objects needs no grant of its
    // own — which is exactly the statement an `__in` lookup produces.
    let one = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" IN ($1)"#;
    let three = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" IN ($1, $2, $3)"#;
    let report = run(
        &manifest_with_queries(&[one.to_string()]),
        &document(&format!(
            "{}, {}",
            request("GET", "/notes/", &json_statement(one)),
            request("GET", "/notes/", &json_statement(three)),
        )),
    );
    assert_eq!(
        report["totals"]["missing"], 0,
        "one grant covers both list lengths"
    );
    assert_eq!(report["totals"]["unused"], 0, "and the grant was used");
    assert_eq!(report["totals"]["statements"], 2);
}

#[test]
fn a_run_of_values_rows_is_one_grant_and_a_single_row_is_another() {
    // D3a again, for `bulk_create`: the run of identical rows collapses, so
    // one grant fits three rows, thirty rows, or none of them twice. A
    // single row is left alone on purpose, and is therefore a shape of its
    // own — one grant never covers both.
    let three = r#"INSERT INTO "notes" ("owner_id", "body") VALUES ($1, $2), ($3, $4), ($5, $6) RETURNING "notes"."id""#;
    let one = r#"INSERT INTO "notes" ("owner_id", "body") VALUES ($1, $2) RETURNING "notes"."id""#;

    let bare = run(
        &manifest_with_queries(&[]),
        &document(&request("POST", "/notes/", &json_statement(three))),
    );
    let shape = missing_sql(&bare, "notes")[0].clone();
    assert!(
        shape.contains(", ..."),
        "the run of rows collapses: {shape}"
    );

    let manifest = manifest_with_queries(std::slice::from_ref(&shape));
    let many = run(
        &manifest,
        &document(&request("POST", "/notes/", &json_statement(three))),
    );
    assert_eq!(
        many["totals"]["missing"], 0,
        "one grant, any number of rows"
    );

    let single = run(
        &manifest,
        &document(&request("POST", "/notes/", &json_statement(one))),
    );
    assert_eq!(missing_sql(&single, "notes"), [normalize(one)]);
    assert_ne!(normalize(one), shape, "the two shapes stay distinct");

    // ... and granting both shapes makes both requests clean: the report is
    // what tells a human there are two of them.
    let both = run(
        &manifest_with_queries(&[shape, normalize(one)]),
        &document(&format!(
            "{}, {}",
            request("POST", "/notes/", &json_statement(one)),
            request("POST", "/notes/", &json_statement(three)),
        )),
    );
    assert_eq!(both["totals"]["missing"], 0);
    assert_eq!(both["totals"]["unused"], 0, "each grant matched its shape");
}

/// A `Blastfile` with a connection trait used by one view, a view that
/// grants everything, one that grants something else, and one with no
/// listener.
pub(super) const SETUP: &str = r#"
postgres $DATABASE_URL

trait connection {
    postgres {
        query "SET TIME ZONE 'UTC'"
    }
}

view notes "/notes/*" {
    use connection
}

view checkout "/checkout/" {
    postgres unrestricted
}

view about "/about/" {
    postgres {
        query 'SELECT "cms_page"."body" FROM "cms_page" LIMIT 21'
    }
}

view assets "/static/*" {
}
"#;

/// One `connection_setup` entry.
fn setup(sql: &str, count: u64) -> String {
    format!(
        r#"{{"alias": "default", "proxy": "db", "sql": {}, "count": {count}}}"#,
        serde_json::to_string(sql).expect("a string")
    )
}

/// A document with only connection-setup statements.
fn setup_document(statements: &str) -> String {
    format!(r#"{{"version": 1, "requests": [], "connection_setup": [{statements}]}}"#)
}

#[test]
fn connection_setup_names_the_routes_that_do_not_grant_it() {
    // No request at all: these statements are not a route's, they run when a
    // connection is established — which, under blastit, every request does
    // afresh (§6, "What the app must do").
    let report = run(SETUP, &setup_document(&setup("SET TIME ZONE 'UTC'", 2)));
    let entry = &report["connection_setup"]["statements"][0];
    assert_eq!(entry["sql"], "SET TIME ZONE 'UTC'");
    assert_eq!(entry["wire_sql"], "SET TIME ZONE 'UTC'");
    assert_eq!(entry["proxy"], "db");
    assert_eq!(entry["count"], 2);
    assert_eq!(
        entry["ungranted_in"],
        json!(["about"]),
        "the global group covers notes, `unrestricted` covers checkout, \
             and the route with no grant has no listener to run it on"
    );
    assert_eq!(entry["granted_in"], 2);
    assert_eq!(report["connection_setup"]["ungranted"], 1);
    assert_eq!(report["totals"]["connection_setup"], 1);
    assert_eq!(report["totals"]["connection_setup_ungranted"], 1);
    assert_eq!(report["totals"]["clean"], false);
}

#[test]
fn connection_setup_granted_everywhere_is_clean() {
    let report = run(
        &manifest_with_queries(&["SET TIME ZONE 'UTC'".to_string()]),
        &setup_document(&setup("SET TIME ZONE 'UTC'", 1)),
    );
    assert_eq!(
        report["connection_setup"],
        json!({"statements": [{
                "sql": "SET TIME ZONE 'UTC'",
                "proxy": "db",
                "count": 1,
                "wire_sql": "SET TIME ZONE 'UTC'",
                "ungranted_in": [],
                "granted_in": 1,
            }], "ungranted": 0})
    );
    assert_eq!(report["totals"]["clean"], true);
    assert_eq!(report["totals"]["connection_setup_ungranted"], 0);
}

#[test]
fn a_route_with_no_listener_is_not_counted_in_connection_setup() {
    // `assets` has no `postgres` grant, so it gets no listener at all: it
    // can never run one of these, and naming it would be noise.
    let report = run(SETUP, &setup_document(&setup("SELECT 1", 1)));
    let entry = &report["connection_setup"]["statements"][0];
    assert_eq!(entry["ungranted_in"], json!(["notes", "about"]));
    assert_eq!(
        entry["granted_in"], 1,
        "only the unrestricted route allows it"
    );
}

#[test]
fn connection_setup_is_aggregated_normalised_and_proxied() {
    // Two aliases, the same statement twice, written differently: one entry,
    // because that is what a grant holds. No proxy named, one declared.
    let report = run(
        SETUP,
        &setup_document(&format!(
            "{}, {}, {}",
            setup("SET TIME ZONE 'UTC'", 2),
            r#"{"alias": "replica", "sql": "SET   TIME   ZONE  'UTC' ;", "count": 3}"#,
            setup("SELECT 1", 1),
        )),
    );
    let statements = report["connection_setup"]["statements"]
        .as_array()
        .expect("a list");
    assert_eq!(statements.len(), 2);
    let app = statements
        .iter()
        .find(|entry| entry["sql"] == "SET TIME ZONE 'UTC'")
        .expect("the SET is there");
    assert_eq!(app["count"], 5, "2 plus a reformatted 3");
    assert_eq!(app["proxy"], "db", "inferred from the only proxy");
    assert_eq!(app["wire_sql"], "SET TIME ZONE 'UTC'");
}

/// A `Blastfile` whose connection trait grants Django's connection
/// housekeeping to the view that uses it (§6).
///
/// `SELECT set_config('TimeZone', $?, false)` only runs while a connection
/// opens, so no request ever runs it; the second statement is the control —
/// a grant nothing used at all.
const CONNECTION: &str = r#"
postgres $DATABASE_URL

trait connection {
    postgres {
        query "SELECT set_config('TimeZone', $?, false)"
        query 'SELECT "never_runs"."id" FROM "never_runs" WHERE "never_runs"."id" = $?'
    }
}

view notes "/notes/*" {
    use connection
    postgres {
        query 'SELECT 1'
    }
}
"#;

#[test]
fn a_statement_connection_setup_accounts_for_is_used_rather_than_unused() {
    // The report used to say both things at once: `connection_setup` showed
    // the trait's `set_config` granted in the view, and the view's `unused`
    // called the same grant one that "nothing matched" — because it only
    // runs while a connection opens, which is not a request. A statement
    // the connection-setup section accounts for is *used*, whether a trait
    // or a global group grants it.
    let observations = format!(
        r#"{{"version": 1, "requests": [{}], "connection_setup": [{}]}}"#,
        request("GET", "/notes/", &statement("SELECT 1")),
        setup("SELECT set_config('TimeZone', $1, false)", 2),
    );
    let report = run(CONNECTION, &observations);

    let entry = &report["connection_setup"]["statements"][0];
    assert_eq!(entry["sql"], "SELECT set_config('TimeZone', $?, false)");
    assert_eq!(entry["granted_in"], 1, "the trait grants it to the view");
    assert_eq!(entry["ungranted_in"], json!([]));
    assert_eq!(report["totals"]["clean"], true);

    // The grant it accounts for is not in the view's `unused` line, and the
    // summary cannot say "nothing matched" about it. The control statement,
    // which nothing ran at all, still is — the fix is about the statements
    // that run while a connection opens, not about every statement. A
    // `Blastfile` has no global groups (§7.1), so `unused_global` is empty
    // by construction.
    assert_eq!(report["unused_global"]["count"], 0, "{report}");
    assert_eq!(report["unused_global"]["statements"], json!([]));
    assert_eq!(report["totals"]["unused_global"], 0);
    assert_eq!(
        proxy(&report, "notes")["unused"],
        json!([r#"SELECT "never_runs"."id" FROM "never_runs" WHERE "never_runs"."id" = $?"#]),
        "the control statement, and only it"
    );
    assert_eq!(
        report["totals"]["unused"], 1,
        "the view's own grant matched, the trait's setup statement is used"
    );
}

#[test]
fn connection_setup_can_be_absent_and_is_never_present_twice() {
    let report = run(SETUP, &document(""));
    assert_eq!(
        report["connection_setup"],
        json!({"statements": [], "ungranted": 0})
    );
    assert_eq!(report["totals"]["connection_setup"], 0);
    assert_eq!(report["totals"]["clean"], true);
}

#[test]
fn an_unrestricted_route_has_nothing_to_fill_in() {
    // `unrestricted = true` (§6, D3a) allows every statement, transaction
    // control included, so nothing can be missing there — and the report
    // must not ask for a `transaction_control` flag the route already has.
    let report = run(
        SETUP,
        &document(&request(
            "POST",
            "/checkout/",
            &format!(
                "{}, {}",
                json_statement(r#"DELETE FROM "orders" WHERE "orders"."id" = $1"#),
                json_statement("SAVEPOINT s1"),
            ),
        )),
    );
    assert_eq!(missing_sql(&report, "checkout"), Vec::<String>::new());
    assert_eq!(proxy(&report, "checkout")["unused"], json!([]));
    assert_eq!(
        proxy(&report, "checkout")["transaction_control"],
        json!({"observed": true, "granted": true, "needed": false})
    );
    assert_eq!(
        route(&report, "checkout")["needs_transaction_control"],
        false
    );
    assert_eq!(report["totals"]["missing"], 0);
}
