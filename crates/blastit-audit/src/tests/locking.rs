//! Row locks and table grants: what a statement's grant has to say.

use serde_json::json;

use super::common::*;

/// A row lock a table grant can name a column for, and one it cannot (D3c).
#[test]
fn a_locking_statement_gets_a_column_named_never_a_star() {
    let manifest = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
    }
}
"#;
    let locked = run(
        manifest,
        &document(&request(
            "GET",
            "/notes/",
            &statement(
                r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"id\" = $1 FOR UPDATE"#,
            ),
        )),
    );
    let grants = &proxy(&locked, "notes")["table_grants"];
    assert_eq!(grants["tables"]["notes"]["update"], json!(["id"]));
    assert!(
        !grants["blastfile"]
            .as_str()
            .expect("a string")
            .contains("update notes\n"),
        "a lock never widens to every column: {}",
        grants["blastfile"]
    );
    assert_eq!(
        grants["blastfile"],
        json!(
            "# view notes postgres\n\
                 # notes: `update notes(id)` only locks its rows (`FOR UPDATE`) — any one \
                 column is enough, as PostgreSQL requires.\n\
                 select notes(id)\n\
                 update notes(id)\n"
        ),
        "the report publishes the directives a pasteable fragment holds"
    );
    assert_eq!(grants["unanalysable"], json!([]));

    // `SELECT *` names no column to give the lock: the covering grant cannot
    // allow the statement, and the report says so instead of writing `"*"`.
    let star = run(
        manifest,
        &document(&request(
            "GET",
            "/notes/",
            &statement(r#"SELECT * FROM \"notes\" FOR UPDATE"#),
        )),
    );
    let grants = &proxy(&star, "notes")["table_grants"];
    assert_eq!(
        grants["tables"]["notes"]["update"],
        json!([]),
        "the lock is named by nothing: the entry grants no column"
    );
    assert!(
        !grants["blastfile"]
            .as_str()
            .expect("a string")
            .contains("update"),
        "and the rendered grant leaves it out: {}",
        grants["blastfile"]
    );
    assert_eq!(grants["unanalysable"].as_array().expect("a list").len(), 1);
    assert_eq!(
        grants["unanalysable"][0]["sql"],
        r#"SELECT * FROM "notes" FOR UPDATE"#
    );
    assert!(
        grants["unanalysable"][0]["reason"]
            .as_str()
            .expect("a string")
            .contains("primary key"),
        "{}",
        grants["unanalysable"][0]["reason"]
    );
    assert!(
        !grants["blastfile"]
            .as_str()
            .expect("a string")
            .contains("update notes\n"),
        "{}",
        grants["blastfile"]
    );
}

#[test]
fn a_lock_another_statement_reads_a_column_for_is_named_from_it() {
    // The pair's grant is merged statement by statement; the lock of one is
    // named from what the other reads, because PostgreSQL asks for the UPDATE
    // privilege on any single column of a locked table (D3c).
    let manifest = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
    }
}
"#;
    let report = run(
        manifest,
        &document(&request(
            "GET",
            "/notes/",
            &format!(
                "{}, {}",
                statement(r#"SELECT 1 FROM \"notes\" FOR UPDATE"#),
                statement(r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"id\" = $1"#,),
            ),
        )),
    );
    let grants = &proxy(&report, "notes")["table_grants"];
    assert_eq!(grants["tables"]["notes"]["update"], json!(["id"]));
    assert_eq!(
        grants["unanalysable"],
        json!([]),
        "the lock is named after all"
    );
}

#[test]
fn a_granted_statement_is_neither_missing_nor_unused() {
    let report = run(
        SIMPLE,
        &document(&request(
            "GET",
            "/notes/",
            &statement(
                r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21"#,
            ),
        )),
    );
    assert_eq!(report["totals"]["missing"], 0);
    assert_eq!(report["totals"]["unused"], 0);
    assert_eq!(report["totals"]["clean"], true);
    assert_eq!(proxy(&report, "notes")["observed"], 1);
    assert_eq!(route(&report, "notes")["requests"], 1);
    assert_eq!(missing_sql(&report, "notes").len(), 0);
}

#[test]
fn reformatting_a_granted_statement_does_not_make_it_missing() {
    // Same statement, laid out differently and with a trailing separator:
    // normalisation is what the comparison reads (§6).
    let report = run(
        SIMPLE,
        &document(&request(
            "GET",
            "/notes/",
            &statement(
                "SELECT   \\\"notes\\\".\\\"id\\\"\\n  FROM \\\"notes\\\" WHERE \\\"notes\\\".\\\"owner_id\\\" = $1 LIMIT 21;",
            ),
        )),
    );
    assert_eq!(report["totals"]["missing"], 0);
    assert_eq!(report["totals"]["unused"], 0);
}

#[test]
fn a_statement_executed_but_not_granted_is_reported_with_its_context() {
    let report = run(
        SIMPLE,
        &document(&request(
            "POST",
            "/notes/3/delete/",
            &format!(
                "{}, {}",
                statement(
                    r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21"#,
                ),
                statement(r#"DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1"#),
            ),
        )),
    );
    assert_eq!(report["totals"]["missing"], 1);
    assert_eq!(report["totals"]["missing_distinct"], 1);
    assert_eq!(report["totals"]["clean"], false);
    let missing = &proxy(&report, "notes")["missing"][0];
    assert_eq!(
        missing["sql"],
        r#"DELETE FROM "notes" WHERE "notes"."id" = $?"#
    );
    assert_eq!(missing["count"], 1);
    assert_eq!(
        missing["wire_sql"],
        r#"DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1"#.replace("\\\"", "\"")
    );
    assert_eq!(
        missing["tests"][0], "tests/test_app.py::test_it",
        "the test that ran it is named"
    );
    assert_eq!(missing["paths"][0], "/notes/3/delete/");
    // The granted statement was used, so nothing is unused.
    assert_eq!(
        proxy(&report, "notes")["unused"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn the_normalised_sql_is_what_a_grant_wants() {
    // The report hands over the normalised text, which is what the manifest
    // comparison reads: pasting it into a grant must make it match.
    let report = run(
        SIMPLE,
        &document(&request(
            "GET",
            "/notes/",
            &statement("DELETE   FROM \\\"notes\\\"  WHERE  id = $1 ;"),
        )),
    );
    assert_eq!(
        missing_sql(&report, "notes"),
        ["DELETE FROM \"notes\" WHERE id = $?"]
    );
    let normalised = missing_sql(&report, "notes")[0].clone();
    let repaired = SIMPLE.replace(
        "'SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $? LIMIT 21'",
        &format!("'{normalised}'"),
    );
    let after = run(
        &repaired,
        &document(&request(
            "GET",
            "/notes/",
            &statement("DELETE   FROM \\\"notes\\\"  WHERE  id = $1 ;"),
        )),
    );
    assert_eq!(
        after["totals"]["missing"], 0,
        "the normalised text pasted into a grant matches the statement"
    );
}

#[test]
fn every_execution_of_a_missing_statement_counts() {
    let statements = [
        statement(r#"DELETE FROM \"notes\" WHERE id = $1"#),
        statement("SELECT 1"),
    ];
    let report = run(
        SIMPLE,
        &document(&format!(
            "{}, {}, {}",
            request("GET", "/notes/", &statements.join(", ")),
            request("GET", "/notes/", &statements[0]),
            request("GET", "/notes/", &statements[0]),
        )),
    );
    assert_eq!(report["totals"]["requests"], 3);
    assert_eq!(
        report["totals"]["missing"], 4,
        "two here, then one, then one"
    );
    assert_eq!(report["totals"]["missing_distinct"], 2);
    // Sorted by how often they ran, so the biggest hole comes first.
    assert_eq!(
        missing_sql(&report, "notes"),
        [r#"DELETE FROM "notes" WHERE id = $?"#, "SELECT 1"]
    );
    let counts: Vec<u64> = proxy(&report, "notes")["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["count"].as_u64().unwrap())
        .collect();
    assert_eq!(counts, [3, 1]);
}
