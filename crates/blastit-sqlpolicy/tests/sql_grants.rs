//! §7 "SQL grant matching & parameterization", the grant extents: the
//! table/column/function/lock surface a `postgres` grant decides on, the
//! indirect access every statement reports (CTEs, `UNION`, `USING`, `FOR UPDATE
//! OF`), the covering grant a report publishes, and the denial text.
//!
//! Each rule is asserted from both sides, and every "the grant covers it" case
//! first pins the exact access list: a grant derived from the analysis it is
//! then checked against would prove nothing.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;

use blastit_sqlpolicy::{Allowlist, Columns, Decision, Denied, TableGrant, TableGrants, analyze};

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

/// Each access as `op:table:column`, `*` for a table-only, whole-row or
/// any-column extent, in the sorted order the analysis hands back.
fn accesses(sql: &str) -> Vec<String> {
    let analysis = analyze(sql).unwrap_or_else(|error| panic!("{sql} is not analysable: {error}"));
    analysis
        .accesses()
        .map(|access| {
            format!(
                "{:?}:{}:{}",
                access.op(),
                access.table(),
                access.column().unwrap_or("*")
            )
        })
        .collect()
}

#[test]
fn a_column_list_reaches_exactly_its_columns_and_a_star_is_per_table() {
    let grants = selecting("t", &["id", "name"]);
    assert!(allowed(&grants, "SELECT id FROM t"));
    assert!(allowed(&grants, "SELECT name FROM t WHERE id = $1"));
    assert_eq!(
        denial(&grants, "SELECT password FROM t").as_deref(),
        Some("select on t.password not granted")
    );
    // A whole-row read reads every column, which a list does not grant.
    assert_eq!(
        denial(&grants, "SELECT * FROM t").as_deref(),
        Some("select on t.* not granted")
    );

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
    assert_eq!(
        denial(&star, "SELECT * FROM u").as_deref(),
        Some("select on u.* not granted"),
        "a star is per table"
    );
}

#[test]
fn read_and_write_grants_are_separate_in_both_directions() {
    let read_only = selecting("t", &["id"]);
    assert!(allowed(&read_only, "SELECT id FROM t"));
    for (sql, reason) in [
        (
            "INSERT INTO t (id) VALUES ($1)",
            "insert on t.id not granted",
        ),
        (
            "UPDATE t SET id = $1 WHERE id = $2",
            "update on t.id not granted",
        ),
        ("DELETE FROM t", "delete on t not granted"),
    ] {
        assert_eq!(denial(&read_only, sql).as_deref(), Some(reason), "{sql}");
    }

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
    assert_eq!(
        denial(&insert_only, "INSERT INTO t (id, total) VALUES ($1, $2)").as_deref(),
        Some("insert on t.total not granted")
    );
    // No column list means every column, which a list does not grant.
    assert_eq!(
        denial(&insert_only, "INSERT INTO t VALUES ($1)").as_deref(),
        Some("insert on t.* not granted")
    );
}

#[test]
fn indirect_access_is_reported_and_a_covering_grant_covers_it() {
    // A subquery reads its inner table; the outer alias is not a table.
    let sql = "SELECT 1 FROM (SELECT 1 FROM u) s";
    assert_eq!(accesses(sql), ["Select:u:*"]);
    let grants = covering(&[sql]);
    assert!(allowed(&grants, sql));
    assert_eq!(
        denial(&selecting("s", &["id"]), sql).as_deref(),
        Some("select on u not granted")
    );

    // A join reports both base tables by name, never by alias.
    let sql = "SELECT t.id, u.id FROM t JOIN u ON u.t_id = t.id";
    assert_eq!(
        accesses(sql),
        ["Select:t:id", "Select:u:id", "Select:u:t_id"]
    );
    let grants = covering(&[sql]);
    assert!(allowed(&grants, sql));
    let reason = denial(&selecting("t", &["id"]), sql).unwrap_or_default();
    assert!(reason.starts_with("select on u."), "{reason}");
    assert!(reason.ends_with("not granted"), "{reason}");

    // `INSERT … SELECT` reads its source table too.
    let sql = "INSERT INTO a (id) SELECT b.id FROM b";
    assert_eq!(accesses(sql), ["Select:b:id", "Insert:a:id"]);
    let grants = covering(&[sql]);
    assert!(allowed(&grants, sql));
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
    assert!(
        denial(&write_only, sql).is_some_and(|reason| reason.ends_with("not granted")),
        "the read side of a write must be granted"
    );
}

#[test]
fn cte_writes_and_unions_report_every_extent() {
    // A data-modifying CTE: the `DELETE` on `t` and the column its `RETURNING`
    // hands the outer query.
    let sql = "WITH d AS (DELETE FROM t RETURNING id) SELECT 1";
    assert_eq!(accesses(sql), ["Select:t:id", "Delete:t:*"]);
    assert!(allowed(&covering(&[sql]), sql));
    assert_eq!(
        denial(&selecting("t", &["id"]), sql).as_deref(),
        Some("delete on t not granted")
    );

    // An `UPDATE` CTE reads the column it assigns from, and its `RETURNING *`
    // is a whole-row read of the same table.
    let sql = "WITH u AS (UPDATE t SET a = b RETURNING *) SELECT 1";
    assert_eq!(accesses(sql), ["Select:t:*", "Select:t:b", "Update:t:a"]);
    assert!(allowed(&covering(&[sql]), sql));

    // The outer `SELECT *` of the insert CTE reads `t` whole; the CTE itself
    // only writes its one column.
    let sql = "WITH i AS (INSERT INTO t (a) VALUES ($1) RETURNING *) SELECT * FROM i";
    assert_eq!(accesses(sql), ["Select:t:*", "Insert:t:a"]);
    assert!(allowed(&covering(&[sql]), sql));

    // Both arms of a `UNION` are read, and neither hides the other.
    let sql = "SELECT * FROM t1 UNION SELECT password FROM auth_user";
    assert_eq!(accesses(sql), ["Select:auth_user:password", "Select:t1:*"]);
    assert_eq!(
        denial(
            &TableGrants::new(
                [(
                    "t1".to_string(),
                    TableGrant {
                        select: Some(Columns::All),
                        ..TableGrant::default()
                    },
                )],
                [],
            ),
            sql
        )
        .as_deref(),
        Some("select on auth_user.password not granted")
    );
}

#[test]
fn delete_using_and_for_update_of_report_their_extents() {
    // `DELETE … USING` reads the joined table as well as writing the target.
    let sql = "DELETE FROM t USING u";
    assert_eq!(accesses(sql), ["Select:u:*", "Delete:t:*"]);
    assert!(allowed(&covering(&[sql]), sql));
    assert_eq!(
        denial(&selecting("t", &["id"]), sql).as_deref(),
        Some("select on u not granted")
    );

    // `FOR UPDATE OF t` locks `t` and nothing else — the join's other side is a
    // read, not a lock.
    let sql = "SELECT 1 FROM t JOIN u ON true FOR UPDATE OF t";
    assert_eq!(accesses(sql), ["Select:t:*", "Select:u:*", "Update:t:*"]);
    let grants = covering(&[sql]);
    assert_eq!(grants.locks_without_a_column().collect::<Vec<_>>(), ["t"]);
    assert!(
        !allowed(&grants, sql),
        "a covering grant refuses to invent the update entry a lock needs"
    );

    // A derived alias is not a lockable relation: the lock lands on the table
    // the alias reads.
    let sql = "SELECT * FROM (SELECT 1 FROM t) s FOR UPDATE OF s";
    assert_eq!(accesses(sql), ["Select:t:*", "Update:t:*"]);
    assert_eq!(
        covering(&[sql])
            .locks_without_a_column()
            .collect::<Vec<_>>(),
        ["t"]
    );
}

#[test]
fn a_row_lock_needs_update_on_one_column() {
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
    let mut empty = read_only;
    empty.tables.entry("t".to_string()).or_default().update = Some(Columns::List(BTreeSet::new()));
    assert_eq!(
        denial(&empty, "SELECT id FROM t FOR UPDATE").as_deref(),
        Some("update on t (any column, to lock its rows) not granted")
    );
}

#[test]
fn table_keys_are_canonical_and_a_merge_widens_only_what_it_names() {
    // `public.t` is `t`; another schema is part of the name.
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
    assert!(grants.table("t").is_some());
    assert!(grants.table("users").is_none());
    assert_eq!(
        grants.tables().keys().collect::<Vec<_>>(),
        ["audit.users", "t"]
    );
    assert_eq!(grants.functions().collect::<Vec<_>>(), ["lower"]);
    assert!(allowed(&grants, "SELECT id FROM public.t"));
    assert!(!allowed(&grants, "SELECT id FROM users"));

    // A route's own grant plus a group's: every operation and function the
    // group names is unioned in, and nothing else is.
    let mut route = TableGrants::new(
        [(
            "orders_order".to_string(),
            TableGrant {
                select: Some(Columns::list(["id", "status"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    route.merge_from(&TableGrants::new(
        [
            (
                "orders_order".to_string(),
                TableGrant {
                    delete: true,
                    ..TableGrant::default()
                },
            ),
            (
                "restaurants_restaurant".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            ),
        ],
        ["st_distance".to_string()],
    ));
    assert!(allowed(&route, "SELECT * FROM restaurants_restaurant"));
    assert!(allowed(
        &route,
        "SELECT st_distance(location, $1) FROM restaurants_restaurant WHERE id = $2"
    ));
    assert!(allowed(&route, "DELETE FROM orders_order"));
    assert_eq!(
        denial(&route, "SELECT total FROM orders_order").as_deref(),
        Some("select on orders_order.total not granted")
    );
    assert_eq!(
        denial(&route, "SELECT uuid_generate_v4() FROM orders_order").as_deref(),
        Some("the function uuid_generate_v4 is not granted")
    );
}

#[test]
fn a_covering_grant_writes_directives_and_never_widens_a_lock() {
    let directives = covering(&[
        r#"SELECT "notes_note"."id", "notes_note"."owner_id" FROM "notes_note" WHERE "notes_note"."owner_id" = $1"#,
        r#"INSERT INTO "notes_note" ("id", "owner_id") VALUES ($1, $2)"#,
    ]);
    assert_eq!(
        directives.to_blastfile("view notes postgres"),
        "# view notes postgres\n\
         select notes_note(id, owner_id)\n\
         insert notes_note(id, owner_id)\n"
    );
    assert_eq!(
        TableGrants::default().to_blastfile("view notes postgres"),
        "",
        "no directives is the empty string, never a bare header"
    );

    // A lock the statements already name a column for is written as that column.
    let sql = "SELECT t.id, t.status FROM t WHERE t.id = $1 FOR UPDATE";
    let named_lock = covering(&[sql]);
    let blastfile = named_lock.to_blastfile("view notes postgres");
    assert!(blastfile.contains("update t(id)"), "{blastfile}");
    assert!(!blastfile.contains("update t\n"), "{blastfile}");
    assert!(allowed(&named_lock, sql));

    // A lock no column can be named for is reported and refused, not widened.
    let sql = "SELECT 1 FROM (SELECT 1 FROM t) s FOR UPDATE";
    let unnamed_lock = covering(&[sql]);
    assert_eq!(
        unnamed_lock.locks_without_a_column().collect::<Vec<_>>(),
        ["t"]
    );
    let blastfile = unnamed_lock.to_blastfile("view notes postgres");
    assert!(!blastfile.contains("update"), "{blastfile}");
    assert!(blastfile.contains("name one here"), "{blastfile}");
    assert!(!allowed(&unnamed_lock, sql));
}

#[test]
fn a_denial_names_the_missing_access_and_an_allowlist_checks_tables() {
    let refused = denied(&selecting("t", &["id"]), "SELECT password FROM t");
    assert_eq!(refused.reason(), "select on t.password not granted");
    assert_eq!(refused.to_string(), refused.reason());

    // The runtime decision: an exact query, a table-granted statement and an
    // ungranted statement each get their own verdict, and the denial carries
    // the normalised text ready to paste into the manifest.
    let allowlist = Allowlist::new(["SELECT 1"], false).with_tables(selecting("t", &["id"]));
    assert!(allowlist.check("SELECT 1").is_allowed());
    let table_grant = allowlist.check("SELECT id FROM t WHERE id = $1");
    assert!(table_grant.is_allowed());
    match allowlist.check("SELECT secret FROM t WHERE id = $1") {
        Decision::Denied { normalized, reason } => {
            assert_eq!(reason, "select on t.secret not granted");
            assert!(normalized.contains("$?"), "{normalized:?}");
        }
        other => panic!("the statement must be denied: {other:?}"),
    }
}

#[test]
fn an_observation_spelling_is_restored_for_a_table_grant() {
    // `blastit_dbproxy`'s observation keeps `$?` placeholders and `…` where a
    // literal was redacted; a table grant restores one of the same shape, and
    // the access set — so the verdict — stays the wire statement's.
    let columns = Columns::list(["id"]);
    let allow = Allowlist::new(std::iter::empty::<&str>(), false).with_tables(TableGrants::new(
        [(
            "items".to_string(),
            TableGrant {
                select: Some(columns.clone()),
                insert: Some(columns),
                ..TableGrant::default()
            },
        )],
        std::iter::empty::<String>(),
    ));
    for observed in [
        r#"SELECT "items"."id" FROM "items" WHERE "items"."id" = $?"#,
        r#"SELECT "items"."id" FROM "items" WHERE "items"."id" = …"#,
        r#"SELECT "items"."id" FROM "items" WHERE "items"."id" IN ($?, ...)"#,
        r#"INSERT INTO "items" ("id") VALUES ($?), ..."#,
    ] {
        assert_eq!(allow.check(observed), Decision::Allowed, "{observed}");
    }
    // ... and a tail never widens a grant.
    for observed in [
        r#"SELECT "items"."secret" FROM "items" WHERE "items"."secret" = $?"#,
        r#"SELECT "items"."secret" FROM "items" WHERE "items"."secret" = …"#,
        r#"INSERT INTO "items" ("secret") VALUES ($?), ..."#,
    ] {
        assert!(!allow.check(observed).is_allowed(), "{observed}");
    }
}
