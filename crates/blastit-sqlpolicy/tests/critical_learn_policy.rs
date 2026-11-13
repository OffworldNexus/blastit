//! Protected critical tests for the SQL learn-mode and the table/group policy
//! (R24, R25, R31; §6, §7.2; ADR-0003).
//!
//! These are the twins the policy guards were missing (`F8`, `SP1..SP11` in
//! `docs/testing/AUDIT-MA.3.md`): every rule that refuses a statement or an
//! access has an allow case and a deny case here, so a change that widens what
//! is grantable and a change that narrows it both fail.
//!
//! `tools/critical_guard.py` protects this file: it may never be weakened, and
//! no test here may skip. The guards pinned below live in `src/learn.rs` and
//! `src/tables.rs`; deleting one must not be able to delete its only oracle.

use blastit_sqlpolicy::{
    Allowlist, Columns, Decision, RefuseReason, TableGrant, TableGrants, analyze, check_learnable,
    is_introspection_setup_sql,
};

//
// Helpers.
//

/// One statement against one grant, with the analysis failure folded in: a
/// statement the analysis refuses is never allowed by a table grant.
fn decide(grants: &TableGrants, sql: &str) -> Result<(), String> {
    match analyze(sql) {
        Ok(analysis) => grants
            .check(&analysis)
            .map_err(|denied| denied.reason().to_string()),
        Err(error) => Err(format!("not analysable: {error}")),
    }
}

/// The denial reason, or `None` when the grant allowed the statement.
fn denial(grants: &TableGrants, sql: &str) -> Option<String> {
    decide(grants, sql).err()
}

/// A `select = "*"` grant.
fn select_all() -> TableGrant {
    TableGrant {
        select: Some(Columns::All),
        ..TableGrant::default()
    }
}

/// A `select = [columns]` grant.
fn select_columns(columns: &[&str]) -> TableGrant {
    TableGrant {
        select: Some(Columns::list(columns.iter().copied())),
        ..TableGrant::default()
    }
}

/// `SELECT (SELECT … pg_catalog.version())`, nested `depth` times.
fn nested_catalogue_select(depth: usize) -> String {
    let mut sql = String::from("SELECT pg_catalog.version()");
    for _ in 0..depth {
        sql = format!("SELECT ({sql})");
    }
    sql
}

//
// R25: what `--learn` may propose.
//

#[test]
fn copy_program_is_refused_and_a_plain_copy_is_grantable() {
    // R25: `COPY … PROGRAM` runs a command on the server, so `--learn` refuses
    // it — in both directions — and never prints the command it carried.
    for sql in [
        "COPY t FROM PROGRAM 'cmd-from-the-wire'",
        "COPY (SELECT 1) TO PROGRAM 'cmd-from-the-wire'",
        "copy t from program 'cmd-from-the-wire'",
    ] {
        let verdict = check_learnable(sql);
        assert_eq!(
            verdict.reason(),
            Some(&RefuseReason::CopyProgram),
            "{sql:?} -> {verdict:?}"
        );
        let Some(redacted) = verdict.redacted() else {
            panic!("a refused statement carries its redacted shape: {verdict:?}");
        };
        assert!(
            !redacted.contains("cmd-from-the-wire"),
            "the command must not survive {sql:?}: {redacted:?}"
        );
    }
    // A `COPY … TO STDOUT` runs no command, so it stays a grantable statement.
    for sql in ["COPY t TO STDOUT", "COPY (SELECT 1) TO STDOUT"] {
        let verdict = check_learnable(sql);
        assert!(verdict.is_query(), "{sql:?} -> {verdict:?}");
    }
}

#[test]
fn multi_statement_text_is_refused_with_its_count() {
    // R25: one grant covers exactly one statement, so text with a second
    // statement behind the first is refused and the count is carried.
    for (sql, count) in [
        ("SELECT 1; SELECT 2", 2usize),
        ("SELECT id FROM t; DROP TABLE t", 2),
        ("BEGIN; COMMIT", 2),
        ("SELECT 1; SELECT 2; SELECT 3", 3),
    ] {
        assert_eq!(
            check_learnable(sql).reason(),
            Some(&RefuseReason::MultipleStatements(count)),
            "{sql:?}"
        );
    }
    // The single-statement twin stays grantable.
    let verdict = check_learnable("SELECT id FROM t WHERE id = $1");
    assert!(verdict.is_query(), "{verdict:?}");
}

#[test]
fn an_inline_literal_is_refused_and_only_the_redacted_shape_is_carried() {
    // R24, R25: an inline literal is refused at learn time; the refusal carries
    // the redacted shape and nothing a grant could hold.
    for sql in [
        "SELECT 'x'",
        "SELECT E'\\n'",
        "SELECT B'1'",
        "SELECT X'ff'",
        "SELECT N'x'",
        r"SELECT U&'\0041'",
        "SELECT $$x$$",
        "SELECT $t$x$t$",
    ] {
        let verdict = check_learnable(sql);
        assert_eq!(
            verdict.reason(),
            Some(&RefuseReason::InlineLiteral),
            "{sql:?} -> {verdict:?}"
        );
        assert_eq!(verdict.redacted(), Some("SELECT …"), "for {sql:?}");
        assert_eq!(
            verdict.normalized(),
            None,
            "a refused statement proposes no grant text: {sql:?}"
        );
    }
    // The parameterised twin is what a grant holds.
    let verdict = check_learnable("SELECT id FROM t WHERE id = $1");
    assert!(verdict.is_query(), "{verdict:?}");
    assert_eq!(
        verdict.normalized(),
        Some("SELECT id FROM t WHERE id = $?"),
        "a grant holds the normalised text"
    );
}

#[test]
fn a_call_and_a_three_part_name_cannot_hide_a_dangerous_function() {
    // R25: `CALL p(…)` keeps its `FuncCall` outside the one-key node wrapper,
    // and PostgreSQL accepts one more qualifier than the two-part resolution,
    // so both shapes have to be refused — while the same name in another
    // schema is a different function.
    for (sql, name) in [
        ("CALL pg_read_file($1)", "pg_read_file"),
        ("CALL pg_catalog.pg_read_file($1)", "pg_read_file"),
        (
            "SELECT pg_catalog.pg_catalog.pg_read_file($1)",
            "pg_read_file",
        ),
        ("CALL pg_catalog.dblink_exec($1)", "dblink_exec"),
    ] {
        assert_eq!(
            check_learnable(sql).reason(),
            Some(&RefuseReason::DangerousFunction(name.to_string())),
            "{sql:?}"
        );
    }
    for sql in [
        "SELECT public.pg_read_file($1)",
        "SELECT shop.pg_read_file($1)",
        "CALL public.pg_read_file($1)",
    ] {
        let verdict = check_learnable(sql);
        assert!(
            verdict.is_query(),
            "{sql:?} names another schema's function: {verdict:?}"
        );
    }
}

//
// R31: session-init replay.
//

#[test]
fn only_single_statement_catalogue_setup_is_replayed() {
    // R31: `SET`, `SHOW` and a single read-only catalogue `SELECT` are setup; a
    // second statement behind one, or a base table, is not.
    for sql in [
        "SET TIME ZONE 'UTC'",
        "SET search_path = public",
        "SHOW standard_conforming_strings",
        "SELECT pg_catalog.version()",
        "SELECT to_regtype($?)",
        "SELECT to_regtype($1)",
        "SELECT * FROM pg_catalog.pg_type WHERE oid = $?",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?} is setup");
    }
    for sql in [
        "SET x; DROP TABLE t",
        "SELECT 1; SHOW x",
        "SELECT * FROM auth_user",
        "SELECT * FROM pg_catalog.pg_authid",
        "DROP TABLE t",
        "",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?} is not setup");
    }
}

#[test]
fn a_deeply_nested_catalogue_select_is_not_replayed() {
    // R31, §7.2: the introspection walk is recursive and bounded, so a
    // catalogue `SELECT` nested past its budget is refused instead of replayed
    // (`SP8`). Refusing is the safe direction: the statement is then simply not
    // session setup, and the route's grant has to cover it.
    assert!(
        is_introspection_setup_sql(&nested_catalogue_select(4)),
        "a shallow catalogue select is still setup"
    );
    // PostgreSQL's own parser, not the walk, is what stops this shape at 16
    // nestings (`tree depth ~160`); every depth it does accept past the walk's
    // budget must stay out of the replay list. The audit's 18-deep probe is
    // covered by the last assertion, and 14/15 are the depths the parser
    // accepts, so the denial below is not the parser's.
    let mut deepest_parseable = 0;
    for depth in 0..=32 {
        let sql = nested_catalogue_select(depth);
        if pg_query::parse(&sql).is_err() {
            break;
        }
        deepest_parseable = depth;
        if depth >= 14 {
            assert!(
                !is_introspection_setup_sql(&sql),
                "depth {depth} must not be replayed"
            );
        }
    }
    assert!(
        deepest_parseable >= 14,
        "the shape must nest at least 14 deep for this oracle to bite: {deepest_parseable}"
    );
    assert!(
        !is_introspection_setup_sql(&nested_catalogue_select(18)),
        "the 18-deep catalogue select must not be replayed"
    );
}

//
// §6: table and column grants, and the group merge.
//

#[test]
fn public_t_and_t_are_one_table() {
    // §6: `public.t` and `t` are one table, so a grant and a statement may each
    // spell either spelling (`SP11`).
    for key in ["t", "public.t"] {
        let grants = TableGrants::new([(key.to_string(), select_all())], []);
        for lookup in ["t", "public.t"] {
            assert!(
                grants.table(lookup).is_some(),
                "a grant written {key:?} is not reachable as {lookup:?}"
            );
        }
        for sql in ["SELECT id FROM t", "SELECT id FROM public.t"] {
            let outcome = decide(&grants, sql);
            assert!(
                outcome.is_ok(),
                "a grant written {key:?} does not allow {sql:?}: {outcome:?}"
            );
        }
    }
    // Any other schema is part of the name: `users` is not `audit.users`.
    let audit = TableGrants::new([("audit.users".to_string(), select_all())], []);
    assert!(audit.table("audit.users").is_some());
    assert!(audit.table("users").is_none());
    assert!(decide(&audit, "SELECT id FROM audit.users").is_ok());
    assert!(
        decide(&audit, "SELECT id FROM users").is_err(),
        "an unqualified name is not the audit schema's table"
    );
}

#[test]
fn a_whole_row_read_needs_the_star_grant() {
    // §6, D7: a `*` read is the whole-row access, so a column list does not
    // allow it — and `"*"` does allow a named column (`SP10`).
    let listed = TableGrants::new([("t".to_string(), select_columns(&["id", "name"]))], []);
    assert!(decide(&listed, "SELECT id, name FROM t WHERE id = $1").is_ok());
    assert_eq!(
        denial(&listed, "SELECT * FROM t").as_deref(),
        Some("select on t.* not granted")
    );

    let star = TableGrants::new([("t".to_string(), select_all())], []);
    assert!(decide(&star, "SELECT * FROM t").is_ok());
    assert!(decide(&star, "SELECT id FROM t").is_ok());

    // The same for a write with no column list.
    let insert_listed = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                insert: Some(Columns::list(["id"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    assert_eq!(
        denial(&insert_listed, "INSERT INTO t VALUES ($1)").as_deref(),
        Some("insert on t.* not granted")
    );
    assert!(decide(&insert_listed, "INSERT INTO t (id) VALUES ($1)").is_ok());
}

#[test]
fn a_group_grant_allows_and_an_ungranted_statement_is_denied() {
    // §6 "Effective grants": a route's tables are the union of its own, its
    // groups' and the global groups', so a group grant allows what the route's
    // own grant does not — and a statement nothing grants stays denied.
    let mut route = TableGrants::new(
        [(
            "orders_order".to_string(),
            select_columns(&["id", "status"]),
        )],
        [],
    );
    assert!(decide(&route, "SELECT id FROM orders_order WHERE status = $1").is_ok());
    assert_eq!(
        denial(&route, "SELECT total FROM orders_order").as_deref(),
        Some("select on orders_order.total not granted")
    );
    assert_eq!(
        denial(&route, "SELECT id FROM restaurants_restaurant").as_deref(),
        Some("select on restaurants_restaurant.id not granted")
    );

    let group = TableGrants::new(
        [("restaurants_restaurant".to_string(), select_all())],
        ["st_distance".to_string()],
    );
    route.merge_from(&group);
    assert!(decide(&route, "SELECT * FROM restaurants_restaurant").is_ok());
    assert!(
        decide(
            &route,
            "SELECT st_distance(location, $1) FROM restaurants_restaurant WHERE id = $2"
        )
        .is_ok(),
        "the group's functions entry allows st_distance"
    );
    // A function neither the route nor the group names stays refused.
    assert_eq!(
        denial(&route, "SELECT uuid_generate_v4() FROM orders_order").as_deref(),
        Some("the function uuid_generate_v4 is not granted")
    );
}

#[test]
fn the_allowlist_allows_table_grants_and_denies_the_rest() {
    // §6: the runtime decision is the same one — an exact query, a
    // table-granted statement and an ungranted statement each get their own
    // verdict, and the denial names the missing access.
    let allowlist = Allowlist::new(["SELECT 1"], false).with_tables(TableGrants::new(
        [("t".to_string(), select_columns(&["id"]))],
        [],
    ));
    assert!(
        allowlist
            .check("SELECT id FROM t WHERE id = $1")
            .is_allowed()
    );
    assert!(allowlist.check("SELECT 1").is_allowed());
    match allowlist.check("SELECT secret FROM t WHERE id = $1") {
        Decision::Denied { normalized, reason } => {
            assert_eq!(reason, "select on t.secret not granted");
            assert!(
                normalized.contains("$?"),
                "the denial carries the normalised text: {normalized:?}"
            );
        }
        other => panic!("the statement must be denied: {other:?}"),
    }
}
