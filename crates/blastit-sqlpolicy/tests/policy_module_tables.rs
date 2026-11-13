//! Non-critical tests for the table/column grant surface (`src/tables.rs`) and
//! the transaction-control grammar (`src/statements.rs`), through the public
//! API dbproxy and the pytest plugin use.
//!
//! Every rule is asserted from both sides: the statement a grant allows **and**
//! the twin it has to refuse. A rule that widened would satisfy an
//! allowed-only assertion, and a table grant is the surface that decides which
//! tables, columns and functions a route can reach ([`Allowlist`]), so the
//! refusal is the half that matters.

use std::collections::BTreeSet;

use blastit_sqlpolicy::{Allowlist, Columns, Denied, TableGrant, TableGrants, analyze};

/// A `select = [columns]` grant on one table.
fn selecting(table: &str, columns: &[&str]) -> TableGrants {
    TableGrants::new(
        [(
            table.to_string(),
            TableGrant {
                select: Some(Columns::list(columns.iter().copied())),
                ..TableGrant::default()
            },
        )],
        [],
    )
}

/// The denial reason for `sql` under `grants`, or `None` when it is allowed.
fn denial(grants: &TableGrants, sql: &str) -> Option<String> {
    match analyze(sql) {
        Ok(analysis) => grants
            .check(&analysis)
            .err()
            .map(|denied| denied.reason().to_string()),
        Err(error) => Some(format!("not analysable: {error}")),
    }
}

/// Whether `sql` is allowed by `grants`.
fn allowed(grants: &TableGrants, sql: &str) -> bool {
    denial(grants, sql).is_none()
}

/// The denial for `sql`, which the test expects to happen.
fn denied(grants: &TableGrants, sql: &str) -> Denied {
    match analyze(sql) {
        Ok(analysis) => match grants.check(&analysis) {
            Ok(()) => panic!("{sql} was granted"),
            Err(denied) => denied,
        },
        Err(error) => panic!("{sql} is not analysable: {error}"),
    }
}

/// The smallest grant covering every one of `statements`.
fn covering(statements: &[&str]) -> TableGrants {
    let mut analyses = Vec::new();
    for sql in statements {
        match analyze(sql) {
            Ok(analysis) => analyses.push(analysis),
            Err(error) => panic!("{sql} is not analysable: {error}"),
        }
    }
    TableGrants::covering(analyses.iter())
}

//
// The column model.
//

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_column_list_reaches_exactly_its_columns() {
    let grants = selecting("t", &["id", "name"]);
    assert!(allowed(&grants, "SELECT id FROM t"));
    assert!(allowed(&grants, "SELECT name FROM t WHERE id = $1"));
    assert_eq!(
        denial(&grants, "SELECT password FROM t").as_deref(),
        Some("select on t.password not granted")
    );
    // A whole-row read is not covered by a list: it reads every column.
    assert_eq!(
        denial(&grants, "SELECT * FROM t").as_deref(),
        Some("select on t.* not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_star_grant_reaches_every_column_but_a_list_is_not_a_star() {
    let star = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    assert!(allowed(&star, "SELECT * FROM t"));
    assert!(allowed(&star, "SELECT id FROM t"));
    // A star is per table: the other table is still refused.
    assert_eq!(
        denial(&star, "SELECT * FROM u").as_deref(),
        Some("select on u.* not granted")
    );
}

#[test]
fn columns_are_a_star_or_a_list_and_nothing_else() {
    // `"*"` holds every column, so it is never empty.
    assert!(Columns::All.contains("anything"));
    assert!(!Columns::All.is_empty());
    // `[]` holds no column at all, and is not the default: it still names the
    // table.
    let empty = Columns::List(BTreeSet::new());
    assert!(empty.is_empty());
    assert!(!empty.contains("id"));
    // A list holds exactly its names.
    let listed = Columns::list(["id", "name"]);
    assert!(listed.contains("name"));
    assert!(!listed.contains("password"));
    assert!(!listed.is_empty());
}

#[test]
fn a_written_operation_keeps_its_table_even_when_it_names_no_column() {
    let mut named_only = TableGrants::default();
    named_only.tables.insert(
        "t".to_string(),
        TableGrant {
            select: Some(Columns::List(BTreeSet::new())),
            ..TableGrant::default()
        },
    );
    // A table only counted is still read: `select = []` covers naming it...
    assert!(allowed(&named_only, "SELECT count(*) FROM t"));
    assert_eq!(
        denial(&named_only, "SELECT id FROM t").as_deref(),
        Some("select on t.id not granted")
    );
    // ... so the grant is not empty and the route keeps its listener.
    assert!(!named_only.is_empty());

    // Each operation on its own is a grant, too.
    for grant in [
        TableGrant {
            select: Some(Columns::list(["id"])),
            ..TableGrant::default()
        },
        TableGrant {
            insert: Some(Columns::list(["id"])),
            ..TableGrant::default()
        },
        TableGrant {
            update: Some(Columns::list(["id"])),
            ..TableGrant::default()
        },
        TableGrant {
            delete: true,
            ..TableGrant::default()
        },
    ] {
        let mut grants = TableGrants::default();
        grants.tables.insert("t".to_string(), grant.clone());
        assert!(!grants.is_empty(), "{grant:?} grants the table");
    }
}

//
// The names a grant is looked up by.
//

#[test]
fn a_table_key_is_the_name_the_analysis_resolves() {
    let grants = TableGrants::new(
        [
            (
                "public.t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            ),
            (
                "audit.users".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            ),
        ],
        ["pg_catalog.lower".to_string()],
    );
    // `public.t` is `t`; another schema keeps its spelling.
    assert!(grants.table("t").is_some());
    assert!(grants.table("public.t").is_some(), "looked up canonically");
    assert!(grants.table("audit.users").is_some());
    assert!(grants.table("users").is_none());
    assert!(grants.table("nothing").is_none());
    assert_eq!(grants.tables().len(), 2);
    assert_eq!(
        grants.tables().keys().collect::<Vec<_>>(),
        ["audit.users", "t"]
    );
    // `pg_catalog.lower` is `lower`.
    assert_eq!(grants.functions().collect::<Vec<_>>(), ["lower"]);

    // An empty rest is not a qualified name: `public.` and `pg_catalog.` keep
    // the spelling they were written with.
    let dotted = TableGrants::new(
        [(
            "public.".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        ["pg_catalog.".to_string()],
    );
    assert_eq!(dotted.tables().keys().collect::<Vec<_>>(), ["public."]);
    assert_eq!(dotted.functions().collect::<Vec<_>>(), ["pg_catalog."]);
}

//
// Reads versus writes, both ways.
//

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_read_grant_does_not_allow_the_write_twin() {
    let grants = selecting("t", &["id"]);
    assert!(allowed(&grants, "SELECT id FROM t"));
    assert_eq!(
        denial(&grants, "INSERT INTO t (id) VALUES ($1)").as_deref(),
        Some("insert on t.id not granted")
    );
    assert_eq!(
        denial(&grants, "UPDATE t SET id = $1 WHERE id = $2").as_deref(),
        Some("update on t.id not granted")
    );
    assert_eq!(
        denial(&grants, "DELETE FROM t").as_deref(),
        Some("delete on t not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn an_insert_grant_does_not_allow_the_read_twin() {
    let insert_only = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                insert: Some(Columns::list(["id"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    assert!(allowed(&insert_only, "INSERT INTO t (id) VALUES ($1)"));
    assert_eq!(
        denial(&insert_only, "SELECT id FROM t").as_deref(),
        Some("select on t.id not granted")
    );
    // A wider insert than the grant names is refused, on the extra column.
    assert_eq!(
        denial(&insert_only, "INSERT INTO t (id, total) VALUES ($1, $2)").as_deref(),
        Some("insert on t.total not granted")
    );
    assert_eq!(
        denial(&insert_only, "DELETE FROM t").as_deref(),
        Some("delete on t not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn an_insert_without_a_column_list_needs_the_whole_table() {
    let insert_only = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                insert: Some(Columns::list(["id"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    // No column list means every column, which a list does not grant.
    assert_eq!(
        denial(&insert_only, "INSERT INTO t VALUES ($1)").as_deref(),
        Some("insert on t.* not granted")
    );

    let star = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                insert: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    assert!(allowed(&star, "INSERT INTO t VALUES ($1)"));
}

//
// What the analysis reports through indirection.
//

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_table_reached_through_a_subquery_is_reported() {
    let sql = "SELECT 1 FROM (SELECT 1 FROM u) s";
    let covering = covering(&[sql]);
    assert!(allowed(&covering, sql));
    // The inner table is a real access, not a detail: a grant that names only
    // the outer one is refused.
    let only_s = selecting("s", &["id"]);
    assert_eq!(
        denial(&only_s, sql).as_deref(),
        Some("select on u not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_join_alias_is_reported_under_its_base_table() {
    let sql = "SELECT t.id, u.id FROM t JOIN u ON u.t_id = t.id";
    let covering = covering(&[sql]);
    assert!(allowed(&covering, sql));
    assert_eq!(
        covering.tables().keys().collect::<Vec<_>>(),
        ["t", "u"],
        "a join reports both base tables, by name and not by alias"
    );
    // A grant for one side of the join refuses the join, naming the other.
    let only_t = selecting("t", &["id"]);
    let reason = denial(&only_t, sql).unwrap_or_default();
    assert!(reason.starts_with("select on u."), "{reason}");
    assert!(reason.ends_with("not granted"), "{reason}");
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn the_read_side_of_an_insert_select_is_reported() {
    let sql = "INSERT INTO a (id) SELECT b.id FROM b";
    let covering = covering(&[sql]);
    assert!(allowed(&covering, sql));
    // The grant a manifest would write for the write target alone refuses the
    // statement: `b` is read, and nothing says so in the write.
    let write_only = TableGrants::new(
        [(
            "a".to_string(),
            TableGrant {
                insert: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let reason = denial(&write_only, sql).unwrap_or_default();
    assert!(reason.contains('b'), "{reason}");
    assert!(reason.ends_with("not granted"), "{reason}");
}

//
// The row-lock rule (§6, D3c).
//

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_lock_needs_update_on_one_column() {
    let read_only = selecting("t", &["id"]);
    assert_eq!(
        denial(&read_only, "SELECT id FROM t FOR UPDATE").as_deref(),
        Some("update on t (any column, to lock its rows) not granted")
    );

    let mut with_one = read_only.clone();
    with_one.tables.entry("t".to_string()).or_default().update = Some(Columns::list(["id"]));
    assert!(allowed(&with_one, "SELECT id FROM t FOR UPDATE"));
    // One column is enough to lock the rows, not to write them.
    assert_eq!(
        denial(&with_one, "UPDATE t SET status = $1 WHERE id = $2").as_deref(),
        Some("update on t.status not granted")
    );

    // `update = []` grants no column, so it grants no lock either.
    let mut empty = read_only.clone();
    empty.tables.entry("t".to_string()).or_default().update = Some(Columns::List(BTreeSet::new()));
    assert_eq!(
        denial(&empty, "SELECT id FROM t FOR UPDATE").as_deref(),
        Some("update on t (any column, to lock its rows) not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_covering_grant_writes_the_lock_it_can_name() {
    let sql = "SELECT t.id, t.status FROM t WHERE t.id = $1 FOR UPDATE";
    let covering = covering(&[sql]);
    assert_eq!(
        covering.locks_without_a_column().collect::<Vec<_>>(),
        Vec::<&str>::new(),
        "the lock names a column the route already reads"
    );
    let toml = covering.to_toml("route.db");
    assert!(toml.contains("update = [\"id\"]"), "{toml}");
    assert!(toml.contains("only locks its rows"), "{toml}");
    assert!(
        !toml.contains("update = \"*\""),
        "a lock is never `\"*\"`: {toml}"
    );
    assert!(allowed(&covering, sql));

    // Another column of the table is still not granted.
    assert_eq!(
        denial(&covering, "SELECT password FROM t WHERE id = $1").as_deref(),
        Some("select on t.password not granted")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn a_lock_without_a_column_is_reported_not_widened() {
    let sql = "SELECT 1 FROM (SELECT 1 FROM t) s FOR UPDATE";
    let covering = covering(&[sql]);
    assert_eq!(covering.locks_without_a_column().collect::<Vec<_>>(), ["t"]);
    let toml = covering.to_toml("route.db");
    assert!(
        !toml.contains("update ="),
        "no update entry is invented: {toml}"
    );
    assert!(toml.contains("name one here"), "the author is told: {toml}");
    assert!(
        !allowed(&covering, sql),
        "a grant that cannot name a column cannot allow the lock"
    );
}

#[test]
fn a_written_empty_update_is_left_alone() {
    let mut grants = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::list(["id"])),
                update: Some(Columns::List(BTreeSet::new())),
                ..TableGrant::default()
            },
        )],
        [],
    );
    // Rendered exactly as written, with no claim that the entry only locks.
    assert_eq!(
        grants.to_toml("route.db"),
        "[route.db.tables]\nt = { select = [\"id\"], update = [] }\n"
    );
    // It is not a lock hint, so nothing reports it as one...
    assert_eq!(
        grants.locks_without_a_column().collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
    // ... and nothing fills the list in behind the author's back.
    grants.pick_locking_columns();
    assert_eq!(
        grants.to_toml("route.db"),
        "[route.db.tables]\nt = { select = [\"id\"], update = [] }\n"
    );
    // It still does not grant a lock, because it names no column.
    assert_eq!(
        denial(&grants, "SELECT id FROM t FOR UPDATE").as_deref(),
        Some("update on t (any column, to lock its rows) not granted")
    );
}

//
// Denials, merging, and the transaction grammar.
//

#[test]
fn a_denial_names_the_access_that_is_missing() {
    let grants = selecting("t", &["id"]);
    let refused = denied(&grants, "SELECT password FROM t");
    assert_eq!(refused.reason(), "select on t.password not granted");
    // `reason()`, `Display` and `Into<String>` are the one line a log carries.
    assert_eq!(refused.to_string(), refused.reason());
    let as_string: String = refused.clone().into();
    assert_eq!(as_string, "select on t.password not granted");

    // A function denial names the function, never a value. (The table has to
    // be granted first, or its own denial comes first.)
    let star = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let refused = denied(&star, "SELECT unaccent(name) FROM t");
    assert_eq!(refused.reason(), "the function unaccent is not granted");
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn merging_unions_operations_and_absorbs_a_list_into_star() {
    let mut merged = selecting("t", &["id"]);
    merged.merge_from(&TableGrants::new(
        [
            (
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::list(["name"])),
                    delete: true,
                    ..TableGrant::default()
                },
            ),
            (
                "u".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            ),
        ],
        ["unaccent".to_string()],
    ));
    assert!(allowed(&merged, "SELECT id, name FROM t"));
    assert!(allowed(&merged, "DELETE FROM t"));
    assert!(allowed(&merged, "SELECT * FROM u"));
    assert!(allowed(&merged, "SELECT unaccent(name) FROM t"));
    assert_eq!(
        denial(&merged, "SELECT password FROM t").as_deref(),
        Some("select on t.password not granted")
    );
    assert_eq!(
        denial(&merged, "DELETE FROM u").as_deref(),
        Some("delete on u not granted")
    );

    // `"*"` absorbs a list, whichever side it is on.
    let mut star = selecting("t", &["id"]);
    star.merge_from(&TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    ));
    assert!(allowed(&star, "SELECT * FROM t"));
    assert!(allowed(&star, "SELECT id FROM t"));
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
fn the_transaction_grammar_allows_exactly_transaction_control() {
    let on = Allowlist::new(std::iter::empty::<&str>(), true);
    for sql in [
        "BEGIN",
        "START TRANSACTION READ WRITE",
        "COMMIT",
        "ROLLBACK TO SAVEPOINT s1",
        "SAVEPOINT \"s1_x\"",
    ] {
        assert!(on.check(sql).is_allowed(), "{sql} is transaction control");
    }
    for sql in [
        "BEGIN; DROP TABLE t",
        "SET TIME ZONE 'UTC'",
        "SELECT 1",
        "BEGIN READ",
        "ROLLBACK TO s1 extra",
    ] {
        assert!(
            !on.check(sql).is_allowed(),
            "{sql} is not transaction control"
        );
    }

    // Without the flag, even `BEGIN` is not granted.
    let off = Allowlist::new(std::iter::empty::<&str>(), false);
    assert!(!off.check("BEGIN").is_allowed());
}
