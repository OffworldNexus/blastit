//! Protected critical tests for the SQL policy surface: the `normalize` shape
//! rules and the table/column/lock grant extents (ARCHITECTURE.md §6; ADR-0003).
//!
//! `docs/testing/AUDIT-MB.1.md` `F1`/`F2` found these oracles in the standard
//! tier, where they could be deleted together with the check they pin. The
//! reproduced escape: a widening `normalize` change (`is_placeholder_list ->
//! true`) makes a grant for `IN ($?)` accept `IN ('x')` — an inline literal the
//! manifest never approved — and both protected sqlpolicy suites stayed green.
//! This file is the oracle `tools/critical_guard.py` now protects, so that
//! mutant fails here instead.
//!
//! Every case is a pair: the text/grant that must be **allowed** and the twin
//! that must be **denied**. A rule that widens fails the denied half exactly as
//! a rule that narrows fails the allowed half, so neither direction of a change
//! to `normalize`, `tables` or the transaction grammar can pass unnoticed.
//!
//! The rendering/serde cases for the same modules stay in the standard tier
//! (`tests/policy_module_edges.rs`, `tests/policy_module_tables.rs`), which say
//! where each case moved from.

use std::collections::BTreeSet;

use blastit_sqlpolicy::{Allowlist, Columns, TableGrant, TableGrants, analyze, normalize};

//
// Helpers.
//

/// A Django `INSERT … RETURNING` the way the driver writes it.
const NOTES: &str =
    "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id";

/// An allowlist holding exactly `queries`, with no table grants.
fn grant(queries: &[&str]) -> Allowlist {
    Allowlist::new(queries.iter().copied(), false)
}

/// Asserts that `allow` allows `allowed_sql` and denies `denied_sql`.
fn pair(allow: &Allowlist, allowed_sql: &str, denied_sql: &str) {
    assert!(
        allow.check(allowed_sql).is_allowed(),
        "should be allowed: {allowed_sql:?} (normalises to {:?})",
        normalize(allowed_sql)
    );
    assert!(
        !allow.check(denied_sql).is_allowed(),
        "should be denied: {denied_sql:?} (normalises to {:?})",
        normalize(denied_sql)
    );
}

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
// `normalize`: reformatting versus a bypass (F1).
//

#[test]
fn reformatting_keeps_the_verdict_and_a_bypass_does_not() {
    let allow = grant(&[NOTES]);

    // The same statement, laid out by another formatter: whitespace runs of
    // every kind collapse to one space, comments stand for one, the trailing
    // separators are dropped and the placeholder numbers carry no meaning.
    for reformatted in [
        "INSERT   INTO notes_note (owner_id, body)\nVALUES ($1, $2)\tRETURNING notes_note.id",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING/* a */notes_note.id",
        "INSERT /* a */ INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id",
        "  INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id ;;  ",
        "INSERT INTO notes_note (owner_id, body) VALUES ($12, $13) RETURNING notes_note.id",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1,\t$2) RETURNING notes_note.id",
    ] {
        assert!(
            allow.check(reformatted).is_allowed(),
            "a reformatted statement is the same statement: {reformatted:?}"
        );
    }

    // A second statement is a second statement, however it is hidden. A `--`
    // comment that ends at a newline hides nothing: the `;` after it is code.
    pair(
        &allow,
        // ... while a comment that runs to the end of the text does hide it,
        // and PostgreSQL runs only the first statement.
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id --; DROP TABLE notes_note",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id --\n; DROP TABLE notes_note",
    );
    // A semicolon in code, with no comment in front of it.
    pair(
        &allow,
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id; DROP TABLE notes_note",
    );
    // A different column, a literal where the grant has a placeholder, casing,
    // and a no-break space where the grant has a space: scan.l does not treat
    // `\u{a0}` as whitespace, so it is part of the text.
    for denied in [
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.body",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, 'x') RETURNING notes_note.id",
        "insert into notes_note (owner_id, body) values ($1, $2) returning notes_note.id",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1,\u{a0}$2) RETURNING notes_note.id",
    ] {
        assert!(
            !allow.check(denied).is_allowed(),
            "not the granted statement: {denied:?}"
        );
    }
    // The same no-break-space text with a space in that position *is* the
    // grant, so the case above fails for the byte and for nothing else.
    assert!(allow.check(NOTES).is_allowed());
}

//
// `normalize`: the `IN` list shape is the grant surface (F1, the reproduced
// escape).
//

#[test]
fn an_in_placeholder_list_grant_does_not_admit_an_inline_literal() {
    // AUDIT-MB.1 `F1`, reproduced: with `is_placeholder_list` forced to `true`,
    // `IN ('x')` collapses to the same `IN ($?, ...)` shape as `IN ($1)`, so
    // this grant would allow the literal. Asserting the two texts normalise
    // apart names the mechanism, and the `Allowlist` check asserts the escape
    // itself.
    let allow = grant(&["SELECT id FROM notes_note WHERE owner_id IN ($1)"]);
    let granted = "SELECT id FROM notes_note WHERE owner_id IN ($1)";
    let denied = "SELECT id FROM notes_note WHERE owner_id IN ('x')";
    assert!(
        !normalize(denied).eq(&normalize(granted)),
        "widened: `normalize` collapsed an inline literal into the placeholder-list shape, so a \
         grant for `IN ($?)` admits `IN ('x')` (both normalise to {:?})",
        normalize(denied)
    );
    assert!(
        allow.check(granted).is_allowed(),
        "the granted placeholder list must stay allowed: {granted:?}"
    );
    assert!(
        !allow.check(denied).is_allowed(),
        "widened: a grant for `IN ($?)` allowed `IN ('x')`, an inline literal the manifest never \
         approved: {denied:?} normalises to {:?}",
        normalize(denied)
    );
    // The same hole with the literal beside a placeholder, in either order.
    for denied in [
        "SELECT id FROM notes_note WHERE owner_id IN ($1, 'x')",
        "SELECT id FROM notes_note WHERE owner_id IN ('x', $1)",
    ] {
        assert!(
            !allow.check(denied).is_allowed(),
            "widened: a literal in a placeholder list was granted: {denied:?}"
        );
    }
    // And the list is still one shape at any greater length (the allowed half).
    assert!(
        allow
            .check("SELECT id FROM notes_note WHERE owner_id IN ($1, $2, $3)")
            .is_allowed(),
        "a longer list of placeholders is the same shape"
    );
}

#[test]
fn an_in_list_is_one_shape_whatever_its_length() {
    // `prefetch_related` and `__in` grow the list per request, and every extra
    // placeholder renumbers the ones after it.
    let allow = grant(&["SELECT id FROM notes_note WHERE owner_id IN ($1)"]);

    for length in 1..=8 {
        let list = (1..=length)
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(", ");
        for statement in [
            format!("SELECT id FROM notes_note WHERE owner_id IN ({list})"),
            format!("SELECT id FROM notes_note WHERE owner_id IN ({list} )"),
            format!("SELECT id FROM notes_note WHERE owner_id IN ( {list} )"),
            format!("SELECT  id FROM notes_note WHERE owner_id IN ({list})"),
        ] {
            assert!(
                allow.check(&statement).is_allowed(),
                "{length} placeholders are one shape: {statement:?}"
            );
        }
    }

    // A list is a list of placeholders and commas. `IN ()` and `IN ($1,)` are
    // not lists of that shape, and anything else in the list — a literal, a
    // cast, a subquery — makes it a different statement.
    for denied in [
        "SELECT id FROM notes_note WHERE owner_id IN ()",
        "SELECT id FROM notes_note WHERE owner_id IN ($1,)",
        "SELECT id FROM notes_note WHERE owner_id IN ('x')",
        "SELECT id FROM notes_note WHERE owner_id IN ($1, 'x')",
        "SELECT id FROM notes_note WHERE owner_id IN ('x', $1)",
        "SELECT id FROM notes_note WHERE owner_id IN ($1::int, $2)",
        "SELECT id FROM notes_note WHERE owner_id IN (SELECT owner_id FROM t)",
        "SELECT id FROM notes_note WHERE owner_id NOT IN ($1, $2)",
    ] {
        assert!(
            !allow.check(denied).is_allowed(),
            "not a list of placeholders: {denied:?}"
        );
    }
}

#[test]
fn the_dot_is_the_guard_and_not_its_left_operand() {
    // A bare `in` in front of a parenthesised list is the reserved keyword
    // whatever expression precedes it, so a group or an arithmetic expression
    // in front of it still collapses.
    assert_eq!(
        normalize("SELECT 1 IN ($1, $2)"),
        normalize("SELECT 1 IN ($1)")
    );
    for sql in [
        "SELECT 1 IN ($1, $2)",
        "SELECT (1 + 2) IN ($1, $2)",
        "SELECT f(1) IN ($1, $2)",
    ] {
        assert!(
            normalize(sql).ends_with("IN ($?, ...)"),
            "the list collapses whatever precedes it: {sql:?} → {:?}",
            normalize(sql)
        );
    }
    assert_eq!(
        normalize("SELECT id FROM t WHERE (a, b) IN ($1, $2)"),
        normalize("SELECT id FROM t WHERE (a, b) IN ($1)")
    );

    // After a `.` the word is a member of a qualified name, so the parentheses
    // are an argument list and the arity picks the overload.
    let allow = grant(&["SELECT probe_s.in($1)"]);
    pair(
        &allow,
        "SELECT probe_s.in($12)",
        "SELECT probe_s.in($1, $2)",
    );
    for denied in [
        "SELECT probe_s . in($1, $2)",
        "SELECT probe_s.IN($1, $2)",
        "SELECT probe_s.in ($1, $2)",
        r#"SELECT probe_s."in"($1, $2)"#,
    ] {
        assert!(
            !allow.check(denied).is_allowed(),
            "a qualified call keeps its arity: {denied:?}"
        );
    }
    assert_eq!(
        normalize("SELECT s . IN ($1, $2)"),
        "SELECT s . IN ($?, $?)",
        "the `in` after a `.` is a member name and keeps its list"
    );
}

#[test]
fn in_needs_no_space_in_front_of_it() {
    // The marker keeps the whitespace that *was* in front of the keyword: the
    // shape rules never invent one and never delete one.
    assert_eq!(
        normalize("SELECT id FROM t WHERE (id)IN ($1, $2)"),
        "SELECT id FROM t WHERE (id)IN ($?, ...)"
    );
    assert_eq!(
        normalize("SELECT id FROM t WHERE (id)  IN ($1, $2)"),
        "SELECT id FROM t WHERE (id) IN ($?, ...)"
    );
    // ... and the spacing *inside* a collapsing list is not part of the shape,
    // so every spelling of it is one statement.
    for (statement, expected) in [
        (
            "SELECT id FROM t WHERE (id)IN ($1, $2)",
            "SELECT id FROM t WHERE (id)IN ($?, ...)",
        ),
        (
            "SELECT id FROM t WHERE (id) IN ($1)",
            "SELECT id FROM t WHERE (id) IN ($?, ...)",
        ),
        (
            "SELECT id FROM t WHERE (id) IN ($1,$2,$3,$4,$5,$6,$7,$8)",
            "SELECT id FROM t WHERE (id) IN ($?, ...)",
        ),
        (
            "SELECT id FROM t WHERE (id)IN($12)",
            "SELECT id FROM t WHERE (id)IN ($?, ...)",
        ),
    ] {
        assert_eq!(normalize(statement), expected, "{statement:?}");
    }
    // The text in front of the keyword is not re-spaced: `(id)IN` and
    // `(id) IN` stay two different spellings of the same shape, so a grant
    // written with the space does not cover the other.
    let allow = grant(&["SELECT id FROM t WHERE (id) IN ($1)"]);
    pair(
        &allow,
        "SELECT id FROM t WHERE (id) IN ($1, $2)",
        "SELECT id FROM t WHERE (id)IN ($1, $2)",
    );
}

#[test]
fn values_rows_collapse_only_as_identical_rows() {
    // `bulk_create` sends one identical row per object.
    let allow = grant(&["INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)"]);
    for rows in 2..=8 {
        let mut placeholders = 0;
        let tuples = (0..rows)
            .map(|_| {
                placeholders += 1;
                let first = placeholders;
                placeholders += 1;
                format!("(${first}, ${placeholders})")
            })
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            allow
                .check(&format!("INSERT INTO t (a, b) VALUES {tuples}"))
                .is_allowed(),
            "{rows} identical rows are one shape"
        );
    }
    // One row is its own statement, and so is a run of rows of another width.
    pair(
        &allow,
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
        "INSERT INTO t (a, b) VALUES ($1, $2)",
    );
    assert!(
        !allow
            .check("INSERT INTO t (a, b) VALUES ($1, $2, $3), ($4, $5, $6)")
            .is_allowed()
    );

    // A tuple list that does not follow the keyword is not a row list, and a
    // qualified `values` call is a call: neither loses a row.
    for unchanged in [
        "SELECT f(1), (1), (1)",
        "SELECT s.values(1), (1), (1)",
        "SELECT values FROM t",
        "VALUES",
        "SELECT (1), (1)",
    ] {
        assert_eq!(normalize(unchanged), unchanged, "left alone: {unchanged:?}");
    }
}

#[test]
fn a_separator_is_never_collapsed_away() {
    // Dropping a row that holds a `;` would change how many statements the
    // text has, and the transaction-control grammar counts them. The rule
    // therefore refuses to collapse a run where any row, however deeply
    // nested, holds a separator.
    for unchanged in [
        "VALUES ((1;2)), ((1;2))",
        "VALUES (f(1;2)), (f(1;2))",
        "VALUES ((1;2)), ((1;2)), ((1;2))",
    ] {
        assert_eq!(normalize(unchanged), unchanged, "kept whole: {unchanged:?}");
    }
    // The same rows without the separator are one shape.
    assert_eq!(normalize("VALUES ((1)), ((1))"), "VALUES ((1)), ...");
    assert_eq!(normalize("VALUES (f(1)), (f(1))"), "VALUES (f(1)), ...");

    // The denied twin a grant author cares about: a run that carries a
    // separator is never the collapsed shape of the run without one.
    let allow = grant(&["VALUES (f(1)), (f(1))"]);
    pair(
        &allow,
        "VALUES (f(1)), (f(1)), (f(1))",
        "VALUES (f(1;2)), (f(1;2))",
    );
}

#[test]
fn a_placeholder_list_is_only_a_shape_after_the_in_keyword() {
    // A parenthesised list of placeholders anywhere else keeps its arity: the
    // arity of a call picks the overload, and a tuple is a value.
    for unchanged in [
        "SELECT ($1, $2)",
        "SELECT f($1, $2)",
        "VALUES ($1, $2)",
        "SELECT $1, $2",
    ] {
        assert_eq!(
            normalize(unchanged),
            unchanged.replace("$1", "$?").replace("$2", "$?"),
            "left alone: {unchanged:?}"
        );
    }
    // An unterminated group is ordinary text, not a list: the text after it is
    // not the group the rules would have seen.
    assert_eq!(normalize("WHERE id IN ($1"), "WHERE id IN ($?");
    assert_eq!(normalize("WHERE id IN ($1,"), "WHERE id IN ($?,");
    assert_eq!(normalize("WHERE id IN ("), "WHERE id IN (");

    let allow = grant(&["SELECT f($1)"]);
    pair(&allow, "SELECT f($12)", "SELECT f($1, $2)");
    assert!(!allow.check("SELECT f($1").is_allowed());
}

#[test]
fn a_grant_matches_the_bulk_statement_and_its_shape_stays_put() {
    // The statement a real app sends, against the grant its manifest holds:
    // both sides go through the same rules, so they cannot be shaped apart.
    let grant_sql = "INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES ($1, $2), ($3, $4) RETURNING \"n\".\"id\"";
    let allow = grant(&[grant_sql]);
    for rows in [2, 4, 8] {
        let mut placeholders = 0;
        let tuples = (0..rows)
            .map(|_| {
                placeholders += 1;
                let first = placeholders;
                placeholders += 1;
                format!("(${first}, ${placeholders})")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let statement = format!(
            "INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES {tuples} RETURNING \"n\".\"id\""
        );
        assert!(allow.check(&statement).is_allowed(), "rows = {rows}");
        // Redaction and normalisation are both idempotent, which is what makes
        // a normalised text safe to log and to compare again.
        let once = normalize(&statement);
        assert_eq!(normalize(&once), once, "rows = {rows}");
    }
    // ... and a run of a different width is not that statement.
    assert!(
        !allow
            .check("INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES ($1, $2, $3), ($4, $5, $6) RETURNING \"n\".\"id\"")
            .is_allowed()
    );
}

//
// Table and column grant extents (F2).
//

#[test]
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

#[test]
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

#[test]
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

//
// The row-lock rule (§6, D3c).
//

#[test]
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

//
// The transaction-control grammar (§6): one flag, one closed statement set.
//

#[test]
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
