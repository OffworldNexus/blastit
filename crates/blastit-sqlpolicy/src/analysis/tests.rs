//! The analysis against hostile statements (§6, D7).
//!
//! This is a security boundary, so the tests are as much a list of what must be
//! *refused* as of what the analysis reads: a column that is only compared, a
//! table reached through a CTE or a `*`, a function hidden behind a cast, a
//! second statement after a `;`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

/// Every access a statement makes, rendered the way a denial's reason does.
fn accesses(sql: &str) -> Vec<String> {
    analyze(sql)
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
        .accesses()
        .map(Access::to_string)
        .collect()
}

/// Every function a statement calls.
fn functions(sql: &str) -> Vec<String> {
    analyze(sql)
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
        .functions()
        .map(str::to_string)
        .collect()
}

/// Why a statement was refused.
fn refused(sql: &str) -> String {
    match analyze(sql) {
        Ok(analysis) => panic!("{sql}: expected a refusal, got {analysis:?}"),
        Err(error) => error.to_string(),
    }
}

/// The accesses a grant would have to cover, as one sorted list of
/// `op on table.column` strings.
fn covered(sql: &str) -> Vec<String> {
    let mut accesses = accesses(sql);
    accesses.sort();
    accesses
}

#[test]
fn a_select_reads_its_columns() {
    assert_eq!(
        covered(
            r#"SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1"#
        ),
        ["select on shop_product.id", "select on shop_product.name",]
    );
    assert_eq!(
        analyze("SELECT 1 FROM t").unwrap().kind(),
        StatementKind::Select
    );
    assert_eq!(
        covered("SELECT 1 FROM t"),
        ["select on t"],
        "naming a table needs a grant for it, even with a constant target list"
    );
}

#[test]
fn a_column_only_compared_in_a_where_is_read() {
    // The comparison itself is an oracle: a route that can ask `WHERE
    // password = $1` learns the password one guess at a time.
    assert_eq!(
        covered("SELECT 1 FROM auth_user WHERE password = $1"),
        ["select on auth_user.password"]
    );
}

#[test]
fn a_column_only_sorted_grouped_or_filtered_is_read() {
    assert_eq!(
        covered("SELECT id FROM t ORDER BY secret"),
        ["select on t.id", "select on t.secret"]
    );
    assert_eq!(
        covered("SELECT count(*) FROM t GROUP BY secret"),
        ["select on t.secret"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t HAVING max(secret) > 1"),
        ["select on t.secret"]
    );
    assert_eq!(
        covered("SELECT DISTINCT ON (secret) id FROM t"),
        ["select on t.id", "select on t.secret"]
    );
    assert_eq!(
        covered("SELECT id FROM t WHERE id IN (SELECT secret FROM u)"),
        ["select on t.id", "select on u.secret"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t GROUP BY GROUPING SETS ((secret), (id))"),
        ["select on t.id", "select on t.secret"]
    );
}

#[test]
fn a_correlated_subquery_reads_the_outer_column() {
    assert_eq!(
        covered("SELECT id FROM t WHERE EXISTS (SELECT 1 FROM u WHERE u.x = t.password)"),
        ["select on t.id", "select on t.password", "select on u.x"]
    );
    assert_eq!(
        covered("SELECT (SELECT max(u.x) FROM u WHERE u.tid = t.id) FROM t"),
        ["select on t.id", "select on u.tid", "select on u.x"]
    );
}

#[test]
fn a_cte_rename_does_not_hide_a_column() {
    // The whole point of resolving a derived relation's columns: `p` stands for
    // `auth_user.password`, and the body read it to begin with.
    assert_eq!(
        covered("WITH x AS (SELECT password AS p FROM auth_user) SELECT p FROM x"),
        ["select on auth_user.password"]
    );
    assert_eq!(
        covered("SELECT p FROM (SELECT password AS p FROM auth_user) x"),
        ["select on auth_user.password"]
    );
    // ... through two renames, too.
    assert_eq!(
        covered(
            "WITH a AS (SELECT password AS p FROM auth_user), b AS (SELECT p AS q FROM a) SELECT q FROM b"
        ),
        ["select on auth_user.password"]
    );
}

#[test]
fn a_star_through_a_subquery_reads_every_column() {
    assert_eq!(
        covered("SELECT * FROM (SELECT t.* FROM t) s"),
        ["select on t.*"]
    );
    assert_eq!(
        covered("SELECT * FROM (SELECT * FROM auth_user) s"),
        ["select on auth_user.*"]
    );
    // Even naming one column of an unnamed output reads the whole table: the
    // analysis cannot know which column `password` came out as.
    assert_eq!(
        covered("SELECT s.password FROM (SELECT * FROM auth_user) s"),
        ["select on auth_user.*", "select on auth_user.password"]
    );
    assert_eq!(
        covered("SELECT id FROM (SELECT * FROM t) s WHERE s.secret = $1"),
        ["select on t.*", "select on t.id", "select on t.secret"]
    );
    assert_eq!(
        covered("SELECT id, count(*) FROM (SELECT id FROM t) s GROUP BY id"),
        ["select on t.id"]
    );
}

#[test]
fn every_branch_of_a_union_is_read() {
    assert_eq!(
        covered("SELECT a FROM t1 UNION ALL SELECT b FROM t2"),
        ["select on t1.a", "select on t2.b"]
    );
    // A branch whose columns nobody can name (`*`) makes the whole output
    // unnamed, and everything any branch reads stays required.
    assert_eq!(
        covered("SELECT * FROM t1 UNION SELECT password FROM auth_user"),
        ["select on auth_user.password", "select on t1.*"]
    );
    assert_eq!(
        covered("(SELECT id FROM t LIMIT 1) UNION (SELECT id FROM u LIMIT 1)"),
        ["select on t.id", "select on u.id"]
    );
}

#[test]
fn a_lateral_subquery_reads_the_outer_relation() {
    assert_eq!(
        covered("SELECT x FROM t JOIN LATERAL (SELECT y FROM u WHERE u.id = t.id) v ON true"),
        [
            "select on t.id",
            "select on t.x",
            "select on u.id",
            "select on u.y",
        ]
    );
}

#[test]
fn a_values_list_reads_its_expressions() {
    assert_eq!(
        covered("SELECT * FROM (VALUES ((SELECT x FROM u)), (2)) v"),
        ["select on u.x"]
    );
    assert_eq!(covered("VALUES (1), (2)"), Vec::<String>::new());
    assert_eq!(
        covered("SELECT * FROM (VALUES (1, 2), (3, 4)) AS v(a, b)"),
        Vec::<String>::new()
    );
}

#[test]
fn a_function_anywhere_is_found() {
    assert_eq!(functions("SELECT id FROM t ORDER BY f(password)"), ["f"]);
    assert_eq!(
        functions("SELECT CAST(f(id) AS text) FROM t"),
        ["f"],
        "a function inside a cast"
    );
    assert_eq!(functions("SELECT id FROM t WHERE id = g()"), ["g"]);
    assert_eq!(functions("SELECT id FROM t GROUP BY f(id)"), ["f"]);
    assert_eq!(
        functions("SELECT count(*) FILTER (WHERE x > 1) FROM t"),
        ["count"]
    );
    assert_eq!(
        functions("SELECT sum(x) OVER (PARTITION BY y) FROM t"),
        ["sum"]
    );
    assert_eq!(
        functions("INSERT INTO t (a) VALUES (f($1))"),
        ["f"],
        "a function in an INSERT's values"
    );
    assert_eq!(
        functions("SELECT id FROM t WHERE id IN (SELECT f(id) FROM u)"),
        ["f"]
    );
}

#[test]
fn the_parser_generated_functions_are_safe() {
    for sql in [
        "SELECT trim(a), substring(a from 1 for 2), position('x' in a) FROM t",
        "SELECT a SIMILAR TO 'x', a AT TIME ZONE 'utc' FROM t",
        "SELECT extract(year from a), date_trunc('day', a) FROM t",
        "SELECT overlay(a placing 'x' from 1 for 2) FROM t",
    ] {
        let called = functions(sql);
        assert!(
            called.iter().all(|name| is_safe_function(name)),
            "{sql}: {called:?}"
        );
    }
    assert_eq!(
        functions("SELECT pg_catalog.lower(a) FROM t"),
        ["lower"],
        "a `pg_catalog.` prefix is dropped, so it matches the safe set"
    );
}

#[test]
fn operators_are_not_functions() {
    assert_eq!(
        functions("SELECT a || b, a -> 1, a #> '{x}' FROM t"),
        Vec::<String>::new()
    );
    assert_eq!(
        covered("SELECT a || b FROM t"),
        ["select on t.a", "select on t.b"]
    );
}

#[test]
fn a_function_the_grant_does_not_know_is_still_reported() {
    for (sql, name) in [
        ("SELECT nextval('s')", "nextval"),
        ("SELECT set_config('a', 'b', false)", "set_config"),
        ("SELECT current_setting('a')", "current_setting"),
        ("SELECT pg_read_file('x')", "pg_read_file"),
        ("SELECT lo_import('x')", "lo_import"),
        ("SELECT dblink('x', 'y')", "dblink"),
        ("SELECT pg_sleep(1)", "pg_sleep"),
        ("SELECT myschema.f(a) FROM t", "myschema.f"),
    ] {
        assert_eq!(functions(sql), [name], "{sql}");
        assert!(!is_safe_function(name));
    }
}

#[test]
fn the_system_catalogues_are_never_readable() {
    for sql in [
        "SELECT * FROM pg_catalog.pg_class",
        "SELECT * FROM information_schema.tables",
        "SELECT c.relname FROM t, pg_catalog.pg_class c",
        // `pg_catalog` is always first on the search path, so an unqualified
        // `pg_*` name resolves into it: it is the catalogue, not a table.
        "SELECT * FROM pg_class",
        "SELECT count(*) FROM pg_type",
        "SELECT * FROM pg_catalog.pg_type",
        // ... and a name that is one of the `information_schema` views is
        // refused too, so a grant key for it could never read as a capability.
        "SELECT count(*) FROM tables",
        "SELECT * FROM columns",
        // A schema of the app's own keeps its name: `audit.t` is a table like
        // any other and a grant can spell it out.
        "SELECT 1 FROM audit.users",
    ] {
        let name = sql.split_whitespace().last().unwrap_or(sql);
        if name == "audit.users" {
            assert_eq!(covered(sql), ["select on audit.users"], "{sql}");
            continue;
        }
        let reason = refused(sql);
        assert!(reason.contains("system catalogues"), "{sql}: {reason}");
    }
}

#[test]
fn schema_quoting_and_case_resolve_to_the_table_a_grant_names() {
    // `public` is where everything unqualified is, so both spellings are `t`.
    for sql in [
        "SELECT * FROM t",
        "SELECT * FROM public.t",
        r#"SELECT * FROM "public"."t""#,
        r#"SELECT * FROM "public".t"#,
    ] {
        assert_eq!(covered(sql), ["select on t.*"], "{sql}");
    }
    // Another schema keeps its name; PostgreSQL lowercases an unquoted one.
    assert_eq!(
        covered("SELECT x FROM audit.users"),
        ["select on audit.users.x"]
    );
    assert_eq!(
        covered(r#"SELECT * FROM "AUDIT".users"#),
        ["select on AUDIT.users.*"]
    );
    assert_eq!(
        covered("SELECT audit.users.x FROM audit.users"),
        ["select on audit.users.x"],
        "a three-part reference names a schema"
    );
    // A quoted identifier is not folded, and it is not the unquoted one.
    assert_eq!(covered(r#"SELECT "Id" FROM t"#), [r"select on t.Id"]);
    assert_eq!(covered("SELECT ID FROM t"), ["select on t.id"]);
}

#[test]
fn a_multi_statement_text_is_refused() {
    let reason = refused("SELECT 1; SELECT 2");
    assert!(reason.contains("2 statements"), "{reason}");
    assert!(refused("SELECT 1; DROP TABLE t").contains("2 statements"));
    assert!(refused("BEGIN; SELECT 1").contains("2 statements"));
    // A `;` inside a string or a comment is not a second statement.
    assert!(analyze("SELECT ';' FROM t").is_ok());
    assert!(analyze("SELECT 1 -- ; DROP TABLE t").is_ok());
}

#[test]
fn an_empty_statement_is_refused() {
    assert!(refused("").contains("no statement"));
    assert!(refused("   ").contains("no statement"));
    assert!(refused(";").contains("no statement"));
}

#[test]
fn a_writable_cte_is_checked_as_its_own_dml() {
    assert_eq!(
        covered("WITH d AS (DELETE FROM t RETURNING id) SELECT 1"),
        ["delete on t", "select on t.id"]
    );
    assert_eq!(
        covered("WITH i AS (INSERT INTO t (a) VALUES (1) RETURNING id) SELECT * FROM i"),
        ["insert on t.a", "select on t.id"],
        "RETURNING reads the target's own row"
    );
    assert_eq!(
        covered("WITH u AS (UPDATE t SET a = b WHERE c = 1 RETURNING *) SELECT 1"),
        [
            "select on t.*",
            "select on t.b",
            "select on t.c",
            "update on t.a"
        ]
    );
    assert_eq!(
        covered("WITH t2 AS (SELECT 1 AS n) INSERT INTO t (a) SELECT n FROM t2"),
        ["insert on t.a"]
    );
}

#[test]
fn select_into_is_refused() {
    let reason = refused("SELECT id INTO newt FROM t");
    assert!(reason.contains("INTO"), "{reason}");
}

#[test]
fn locking_a_row_needs_update() {
    assert_eq!(
        covered("SELECT id FROM t FOR UPDATE"),
        [
            "select on t.id",
            "update on t (any column, to lock its rows)"
        ]
    );
    assert_eq!(
        covered("SELECT id FROM t FOR SHARE OF t"),
        [
            "select on t.id",
            "update on t (any column, to lock its rows)"
        ]
    );
    assert_eq!(
        covered("SELECT id, u.id FROM t, u FOR UPDATE OF u"),
        [
            "select on t.id",
            "select on u.id",
            "update on u (any column, to lock its rows)"
        ],
        "only the named relation is locked"
    );
    assert!(
        refused("SELECT id FROM t FOR UPDATE OF other").contains("not read"),
        "PostgreSQL refuses a lock on a relation that is not in the FROM list"
    );
}

#[test]
fn an_insert_needs_insert_on_the_columns_it_writes() {
    assert_eq!(
        covered("INSERT INTO orders_order (status, accepted_at) VALUES ($1, $2)"),
        [
            "insert on orders_order.accepted_at",
            "insert on orders_order.status"
        ]
    );
    assert_eq!(
        covered("INSERT INTO orders_order (status) VALUES (DEFAULT)"),
        ["insert on orders_order.status"],
        "DEFAULT writes the column, not the schema's default expression"
    );
}

#[test]
fn an_insert_without_a_column_list_needs_every_column() {
    assert_eq!(
        covered("INSERT INTO t VALUES ($1, $2)"),
        ["insert on t.*"],
        "no column list means every column is written"
    );
    assert_eq!(
        covered("INSERT INTO t DEFAULT VALUES"),
        ["insert on t.*"],
        "DEFAULT VALUES writes whatever the defaults are"
    );
}

#[test]
fn an_insert_select_reads_its_source() {
    assert_eq!(
        covered("INSERT INTO t (a) SELECT b FROM u"),
        ["insert on t.a", "select on u.b"]
    );
    assert_eq!(
        covered("INSERT INTO t SELECT * FROM u"),
        ["insert on t.*", "select on u.*"]
    );
    assert_eq!(
        covered("INSERT INTO t (a) SELECT b FROM u WHERE u.c = 1"),
        ["insert on t.a", "select on u.b", "select on u.c"]
    );
}

#[test]
fn on_conflict_checks_both_sides_of_the_statement() {
    assert_eq!(
        covered("INSERT INTO t (id, a) VALUES ($1, $2) ON CONFLICT DO NOTHING"),
        ["insert on t.a", "insert on t.id"]
    );
    assert_eq!(
        covered(
            "INSERT INTO t (id, a) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET a = EXCLUDED.a"
        ),
        ["insert on t.a", "insert on t.id", "update on t.a"]
    );
    assert_eq!(
        covered(
            "INSERT INTO t (id, a) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET a = t.a + EXCLUDED.a WHERE t.a < $3"
        ),
        [
            "insert on t.a",
            "insert on t.id",
            "select on t.a",
            "update on t.a"
        ],
        "the proposed row and the existing one both read"
    );
}

#[test]
fn a_delete_needs_delete_and_select() {
    assert_eq!(
        covered("DELETE FROM t WHERE id = $1"),
        ["delete on t", "select on t.id"]
    );
    assert_eq!(
        covered("DELETE FROM t USING u WHERE t.id = u.tid"),
        ["delete on t", "select on t.id", "select on u.tid"]
    );
    assert_eq!(
        covered("DELETE FROM t WHERE id = $1 RETURNING t.password"),
        ["delete on t", "select on t.id", "select on t.password"]
    );
}

#[test]
fn an_update_needs_update_and_select() {
    assert_eq!(
        covered("UPDATE t SET a = $1 WHERE c = $2"),
        ["select on t.c", "update on t.a"]
    );
    assert_eq!(
        covered("UPDATE t SET a = b WHERE c = 1"),
        ["select on t.b", "select on t.c", "update on t.a"]
    );
    assert_eq!(
        covered("UPDATE t SET (a, b) = (1, 2)"),
        ["update on t.a", "update on t.b"]
    );
    assert_eq!(
        covered("UPDATE t SET a = u.v FROM u WHERE t.id = u.id"),
        [
            "select on t.id",
            "select on u.id",
            "select on u.v",
            "update on t.a"
        ]
    );
    assert_eq!(
        covered("UPDATE t SET a = 1 RETURNING *"),
        ["select on t.*", "update on t.a"]
    );
}

#[test]
fn returning_needs_select() {
    assert_eq!(
        covered("INSERT INTO t (a) VALUES ($1) RETURNING password"),
        ["insert on t.a", "select on t.password"]
    );
    assert_eq!(
        covered("UPDATE t SET a = $1 RETURNING id"),
        ["select on t.id", "update on t.a"]
    );
}

#[test]
fn an_unqualified_column_needs_every_relation_that_could_provide_it() {
    // No schema is known here, so `name` could be either table's.
    assert_eq!(
        covered("SELECT name FROM t, u"),
        ["select on t.name", "select on u.name"]
    );
    assert_eq!(
        covered("SELECT t.name FROM t, u"),
        ["select on t.name", "select on u"],
        "u is named even though no column of it is read"
    );
    // The innermost scope that provides it wins: the outer one is not read.
    assert_eq!(
        covered("SELECT id FROM (SELECT id FROM u) s, t"),
        ["select on t.id", "select on u.id"],
        "both relations of the level could provide the name"
    );
    assert_eq!(
        covered("SELECT name FROM t, (SELECT name FROM u) s"),
        ["select on t.name", "select on u.name"]
    );
}

#[test]
fn a_join_reads_its_condition_and_its_using_columns() {
    assert_eq!(
        covered("SELECT 1 FROM t JOIN u ON t.id = u.tid"),
        ["select on t.id", "select on u.tid"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t JOIN u USING (a)"),
        ["select on t.a", "select on u.a"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t CROSS JOIN u WHERE t.id = 1"),
        ["select on t.id", "select on u"],
        "a CROSS JOIN names u without reading a column of it"
    );
}

#[test]
fn a_join_reading_a_column_in_its_condition_is_read() {
    // `USING` and `ON` are both conditions the statement can learn from.
    assert_eq!(
        covered("SELECT 1 FROM t LEFT JOIN u ON u.secret = t.id"),
        ["select on t.id", "select on u.secret"]
    );
}

#[test]
fn a_window_clause_reads_its_partition_and_order() {
    assert_eq!(
        covered("SELECT sum(x) OVER (PARTITION BY y ORDER BY z) FROM t"),
        ["select on t.x", "select on t.y", "select on t.z"]
    );
    assert_eq!(
        covered("SELECT sum(x) OVER w FROM t WINDOW w AS (ORDER BY secret)"),
        ["select on t.secret", "select on t.x"]
    );
}

#[test]
fn count_star_reads_no_column_but_names_the_table() {
    // `count(*)` reads no column (§6), but the statement still asks after the
    // table: it learns that it exists and how many rows it has, so naming it
    // needs a grant (D7).
    assert_eq!(covered("SELECT count(*) FROM t"), ["select on t"]);
    assert_eq!(
        covered("SELECT count(*) FROM auth_user"),
        ["select on auth_user"]
    );
}

#[test]
fn a_table_named_without_a_column_read_still_needs_a_select_grant() {
    // A statement that names a table reads no column of it, but it tells the
    // statement whether the table exists and roughly how many rows it has:
    // `SELECT count(*) FROM auth_user`, `SELECT 1 FROM t`, `EXISTS (SELECT 1
    // FROM t …)` all need a `select` entry for the table (D7).
    assert!(
        !analyze("SELECT count(*) FROM auth_user")
            .unwrap()
            .touches_nothing(),
        "naming auth_user is touching it"
    );
    assert_eq!(
        covered("SELECT count(*) FROM auth_user"),
        ["select on auth_user"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t WHERE true"),
        ["select on t"],
        "a target list of constants still names t"
    );
    assert_eq!(
        covered("SELECT 1 FROM t WHERE EXISTS (SELECT 1 FROM u)"),
        ["select on t", "select on u"],
        "a subquery names its table too"
    );
    assert_eq!(
        covered("SELECT count(*) FROM t JOIN u ON t.id = u.tid"),
        ["select on t.id", "select on u.tid"],
        "the join condition reads both columns, which already covers the tables"
    );
    assert_eq!(
        covered("SELECT count(*) FROM t, u"),
        ["select on t", "select on u"],
        "a cross join names both tables without reading a column"
    );
    assert_eq!(
        covered("SELECT count(*) FROM auth_user WHERE password = $1"),
        ["select on auth_user.password"],
        "... and a filter is read as well (which names the table too)"
    );
    // Without a table, there is still nothing to grant.
    assert!(analyze("SELECT 1").unwrap().touches_nothing());
}

#[test]
fn an_array_index_and_a_field_read_their_column() {
    assert_eq!(covered("SELECT a[1] FROM t"), ["select on t.a"]);
    assert_eq!(covered("SELECT (b).c FROM t"), ["select on t.b"]);
    assert_eq!(
        covered("SELECT a[1:2], a[i] FROM t"),
        ["select on t.a", "select on t.i"]
    );
}

#[test]
fn a_table_sample_reads_the_table() {
    assert_eq!(
        covered("SELECT * FROM t TABLESAMPLE SYSTEM (10)"),
        ["select on t.*"]
    );
    assert_eq!(
        covered("SELECT id FROM t TABLESAMPLE BERNOULLI (10) WHERE secret = 1"),
        ["select on t.id", "select on t.secret"]
    );
}

#[test]
fn a_set_returning_function_reads_nothing_by_itself() {
    assert_eq!(
        covered("SELECT * FROM generate_series(1, 3) AS g(n)"),
        Vec::<String>::new()
    );
    assert_eq!(
        covered("SELECT n FROM unnest(ARRAY[1, 2]) AS u(n)"),
        Vec::<String>::new()
    );
    assert_eq!(
        covered("SELECT * FROM generate_series(1, (SELECT x FROM u)) g"),
        ["select on u.x"],
        "the arguments are still read"
    );
}

#[test]
fn a_table_alias_with_a_column_list_is_refused() {
    let reason = refused("SELECT id FROM t AS x(a)");
    assert!(reason.contains("column list on a table"), "{reason}");
    // ... and a star output has no countable column list either.
    let reason = refused("SELECT id FROM (SELECT * FROM t) AS v(id)");
    assert!(reason.contains("cannot be counted"), "{reason}");
    // A named output can be renamed positionally, and its body is still read.
    assert_eq!(
        covered("SELECT i FROM (SELECT id, name FROM t) AS v(i, n)"),
        ["select on t.id", "select on t.name"]
    );
}

#[test]
fn a_natural_join_is_refused() {
    let reason = refused("SELECT * FROM t1 NATURAL JOIN t2");
    assert!(reason.contains("NATURAL JOIN"), "{reason}");
}

#[test]
fn a_derived_relation_without_the_column_is_refused() {
    let reason = refused("SELECT nope FROM (SELECT id FROM t) s");
    assert!(reason.contains("not a column of anything"), "{reason}");
    let reason = refused("SELECT s.nope FROM (SELECT id FROM t) s");
    assert!(reason.contains("no column `nope`"), "{reason}");
    let reason = refused("SELECT missing FROM (SELECT id FROM t) s");
    assert!(reason.contains("not a column of anything"), "{reason}");
}

#[test]
fn a_reference_to_a_relation_that_is_not_in_scope_is_refused() {
    let reason = refused("SELECT other.id FROM t");
    assert!(reason.contains("not in its FROM clause"), "{reason}");
    // A base table's columns are not known, so any name on it is one of its
    // columns; a derived relation's are known, so a name it does not have is a
    // refusal.
    let reason = refused("SELECT 1 FROM (SELECT id FROM t) s WHERE nope = 1");
    assert!(reason.contains("not a column of anything"), "{reason}");
}

#[test]
fn only_the_four_dml_kinds_are_analysed() {
    for sql in [
        "SET TIME ZONE 'UTC'",
        "SET search_path = public",
        "SHOW all",
        "COPY t TO STDOUT",
        "COPY t FROM STDIN",
        "LOCK TABLE t",
        "DO $$ BEGIN END $$",
        "CALL p()",
        "EXPLAIN SELECT 1",
        "PREPARE p AS SELECT 1",
        "EXECUTE p",
        "CREATE TABLE x (a int)",
        "DROP TABLE t",
        "ALTER TABLE t ADD COLUMN a int",
        "TRUNCATE t",
        "VACUUM t",
        "GRANT SELECT ON t TO x",
        "REVOKE SELECT ON t FROM x",
        "LISTEN c",
        "NOTIFY c",
        "DISCARD ALL",
        "DECLARE c CURSOR FOR SELECT 1",
        "FETCH c",
        "BEGIN",
        "COMMIT",
        "ROLLBACK",
        "SAVEPOINT s1",
        "MERGE INTO t USING u ON t.id = u.id WHEN MATCHED THEN UPDATE SET a = 1",
        "SELECT 1 INTO t2",
    ] {
        let reason = refused(sql);
        assert!(
            reason.contains("only SELECT, INSERT, UPDATE and DELETE")
                || reason.contains("MERGE")
                || reason.contains("INTO"),
            "{sql}: {reason}"
        );
    }
}

#[test]
fn a_deeply_nested_statement_is_refused() {
    // 200 nested calls: the parser itself gives up (its own recursion limit),
    // and the analysis never has to walk it at all.
    let sql = format!("SELECT {}1{} FROM t", "f(".repeat(200), ")".repeat(200));
    let reason = refused(&sql);
    assert!(!reason.is_empty());
    assert!(
        reason.len() < 400,
        "a refusal is a line, not the whole parse path: {reason}"
    );
    // A depth the parser accepts is walked without trouble.
    let sql = format!("SELECT {}1{} FROM t", "f(".repeat(20), ")".repeat(20));
    assert!(analyze(&sql).is_ok());
}

#[test]
fn shapes_a_live_statement_changes_analyse_alike() {
    // The analysis is cached by the normalised text, so these have to be the
    // same statement for the policy as well as for the cache.
    let one = covered("SELECT id, name FROM t WHERE id IN ($1)");
    for sql in [
        "SELECT id, name FROM t WHERE id IN ($1, $2)",
        "SELECT id, name FROM t WHERE id IN ($1, $2, $3, $4)",
        "  select  id,\n name  from t where id in ($1) ; ",
    ] {
        assert_eq!(covered(sql), one, "{sql}");
    }
    // A different shape of VALUES is the same statement too.
    let one = covered("INSERT INTO t (a, b) VALUES ($1, $2)");
    for sql in [
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ($5, $6)",
    ] {
        assert_eq!(covered(sql), one, "{sql}");
    }
}

#[test]
fn the_cache_stays_bounded() {
    // More distinct statements than the cache holds: it must not grow with them.
    for index in 0..(CACHE_CAPACITY + 50) {
        let sql = format!("SELECT id FROM t{index} WHERE id = $1");
        assert!(analyze(&sql).is_ok());
    }
    assert!(
        cached_analyses() <= CACHE_CAPACITY,
        "the cache holds {} entries",
        cached_analyses()
    );
}

#[test]
fn a_refused_statement_names_what_it_could_not_read() {
    let reason = refused("SELECT * FROM (SELECT id FROM u) s WHERE x IN (SELECT y FROM u)");
    assert!(!reason.is_empty());
    let reason = refused("MERGE INTO t USING u ON true WHEN MATCHED THEN DELETE");
    assert!(reason.contains("MERGE"), "{reason}");
    let reason = refused("WITH x AS (SELECT id FROM t) SELECT * FROM x, pg_catalog.pg_class");
    assert!(reason.contains("system catalogues"), "{reason}");
}

#[test]
fn the_kind_of_a_statement_is_reported() {
    for (sql, kind) in [
        ("SELECT 1", StatementKind::Select),
        ("VALUES (1)", StatementKind::Select),
        ("INSERT INTO t (a) VALUES (1)", StatementKind::Insert),
        ("UPDATE t SET a = 1", StatementKind::Update),
        ("DELETE FROM t", StatementKind::Delete),
        (
            "WITH d AS (SELECT 1 AS n) SELECT n FROM d",
            StatementKind::Select,
        ),
    ] {
        assert_eq!(analyze(sql).unwrap().kind(), kind, "{sql}");
    }
}

#[test]
fn an_unrestricted_shaped_statement_is_still_analysed() {
    // `ANALYZE`/`EXPLAIN` are statement kinds of their own and are refused; a
    // `SELECT` that happens to be huge is not.
    assert!(refused("ANALYZE t").contains("only SELECT, INSERT, UPDATE and DELETE"));
}

#[test]
fn the_analysis_of_a_django_shaped_join_is_what_a_grant_has_to_cover() {
    // The shape a real list endpoint sends: a join, a filter, an order and a
    // limit, all qualified.
    let sql = r#"SELECT "restaurants_restaurant"."id", "restaurants_restaurant"."name", "restaurants_cuisinetype"."name" FROM "restaurants_restaurant" INNER JOIN "restaurants_cuisinetype" ON ("restaurants_restaurant"."cuisine_id" = "restaurants_cuisinetype"."id") WHERE ("restaurants_restaurant"."is_active" AND "restaurants_restaurant"."name" LIKE $1) ORDER BY "restaurants_restaurant"."id" ASC LIMIT 21"#;
    assert_eq!(
        covered(sql),
        [
            "select on restaurants_cuisinetype.id",
            "select on restaurants_cuisinetype.name",
            "select on restaurants_restaurant.cuisine_id",
            "select on restaurants_restaurant.id",
            "select on restaurants_restaurant.is_active",
            "select on restaurants_restaurant.name",
        ]
    );
    assert_eq!(functions(sql), Vec::<String>::new());
}

#[test]
fn an_alias_shadows_a_cte_name() {
    assert_eq!(
        covered(
            "WITH x AS (SELECT password FROM auth_user) SELECT 1 FROM (SELECT * FROM auth_user) x"
        ),
        ["select on auth_user.*", "select on auth_user.password"],
        "the CTE body still reads, and the derived alias is what is used"
    );
}

#[test]
fn a_cte_is_visible_inside_its_own_statement_only() {
    // A name no CTE defines is a table like any other.
    assert_eq!(covered("SELECT a FROM x"), ["select on x.a"]);
}

//
// D7 review round: the rules the first round missed.
//

#[test]
fn a_named_table_needs_a_select_grant_even_without_a_column_read() {
    // What the statement learns is that the table exists and roughly how many
    // rows it has; naming it is touching it (D7).
    assert_eq!(covered("SELECT 1 FROM t"), ["select on t"]);
    assert_eq!(covered("SELECT count(*) FROM t"), ["select on t"]);
    assert_eq!(covered("SELECT EXISTS (SELECT 1 FROM t)"), ["select on t"]);
    // Through a derived relation too: the inner statement is the one that names
    // `t`, and the derived relation carries that.
    assert_eq!(
        covered("SELECT 1 FROM (SELECT 1 FROM t) s"),
        ["select on t"]
    );
    assert_eq!(
        covered("WITH x AS (SELECT 1 FROM t) SELECT 1 FROM x"),
        ["select on t"]
    );
    assert_eq!(
        covered("SELECT 1 FROM t JOIN u ON true"),
        ["select on t", "select on u"],
        "a join with no column condition names both tables"
    );
    // A table a column is read from already needs a `select` entry, so the
    // weaker access is not reported on top of it.
    assert_eq!(
        covered("SELECT count(id) FROM t"),
        ["select on t.id"],
        "a column read is already more than naming the table"
    );
    assert!(
        analyze("SELECT 1").unwrap().touches_nothing(),
        "a statement with no table still touches nothing"
    );
}

#[test]
fn a_write_target_is_named_by_its_own_grant() {
    // `INSERT INTO t` names `t`, but the `insert` grant is what allows it: a
    // pure-insert grant is enough, with no `select` entry.
    assert_eq!(covered("INSERT INTO t (a) VALUES (1)"), ["insert on t.a"]);
    assert_eq!(covered("UPDATE t SET a = 1"), ["update on t.a"]);
    assert_eq!(covered("DELETE FROM t"), ["delete on t"]);
}

#[test]
fn locking_a_derived_relation_locks_the_tables_it_names() {
    // The statement reads no column of `auth_user`, but PostgreSQL locks its
    // rows through the subquery, so `update` is required — on any one column.
    assert_eq!(
        covered("SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR UPDATE"),
        [
            "select on auth_user",
            "update on auth_user (any column, to lock its rows)"
        ]
    );
    assert_eq!(
        covered("SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR SHARE"),
        [
            "select on auth_user",
            "update on auth_user (any column, to lock its rows)"
        ]
    );
    assert_eq!(
        covered("WITH x AS (SELECT 1 FROM auth_user) SELECT 1 FROM x FOR UPDATE"),
        [
            "select on auth_user",
            "update on auth_user (any column, to lock its rows)"
        ]
    );
    // `OF s` locks only `s`; `s` stands for `auth_user`.
    assert_eq!(
        covered("SELECT 1 FROM t, (SELECT 1 FROM auth_user) s FOR UPDATE OF s"),
        [
            "select on auth_user",
            "select on t",
            "update on auth_user (any column, to lock its rows)"
        ]
    );
    // ... and a join alias stands for both of its sides.
    assert_eq!(
        covered("SELECT 1 FROM t JOIN u ON true FOR UPDATE OF t"),
        [
            "select on t",
            "select on u",
            "update on t (any column, to lock its rows)"
        ]
    );
}

#[test]
fn an_unqualified_catalogue_name_is_refused() {
    for sql in [
        "SELECT * FROM pg_class",
        "SELECT count(*) FROM pg_type",
        "SELECT * FROM pg_catalog.pg_class",
        "SELECT * FROM information_schema.tables",
        "SELECT count(*) FROM tables",
        "SELECT * FROM columns",
    ] {
        let reason = refused(sql);
        assert!(reason.contains("system catalogues"), "{sql}: {reason}");
    }
    // Another schema keeps its name: `audit.pg_class` is the app's table, not
    // the catalogue, and a grant can spell it out.
    assert_eq!(
        covered("SELECT 1 FROM audit.pg_class"),
        ["select on audit.pg_class"]
    );
}

#[test]
fn a_whole_row_reference_to_a_relation_is_a_whole_table_read() {
    // The relation name, or its alias, is not a column: the statement reads
    // every column of the table, so the grant has to say `select = "*"`.
    for sql in [
        "SELECT x FROM t x",
        "SELECT t FROM t",
        "SELECT ROW(x) FROM t x",
        "SELECT to_jsonb(x) FROM t x",
        "SELECT x::text FROM t x",
        "SELECT (x).a FROM t x",
        "SELECT x['a'] FROM t x",
        "SELECT count(x) FROM t x",
    ] {
        assert_eq!(covered(sql), ["select on t.*"], "{sql}");
    }
    // A name that is not a relation in scope is a column like any other.
    assert_eq!(
        covered("SELECT x FROM t"),
        ["select on t.x"],
        "without an alias, x is a column of t"
    );
}

#[test]
fn a_cast_that_can_run_code_is_reported_like_a_function() {
    // Ordinary casts of the built-in scalar types, including arrays of them,
    // run no user code.
    for sql in [
        "SELECT a::int, a::integer, a::bigint, a::smallint FROM t",
        "SELECT a::text, a::varchar, a::char FROM t",
        "SELECT a::numeric, a::decimal, a::real, a::float8 FROM t",
        "SELECT a::timestamp, a::timestamptz, a::date FROM t",
        "SELECT a::boolean, a::uuid FROM t",
        "SELECT a::json, a::jsonb FROM t",
        "SELECT a::int[], a::text[] FROM t",
        "SELECT a::geometry, a::geography FROM t",
    ] {
        assert_eq!(functions(sql), Vec::<String>::new(), "{sql}");
    }
    // A `reg*` type looks a name up in the catalogue; a domain or a user type
    // can run code at input; any other type is simply not one this build knows.
    // Each is reported under its own name, so a grant has to list it.
    for (sql, name) in [
        ("SELECT a::regclass FROM t", "regclass"),
        ("SELECT a::regtype FROM t", "regtype"),
        ("SELECT a::regproc FROM t", "regproc"),
        ("SELECT a::regprocedure FROM t", "regprocedure"),
        ("SELECT a::regnamespace FROM t", "regnamespace"),
        ("SELECT a::regcollation FROM t", "regcollation"),
        ("SELECT a::regoper FROM t", "regoper"),
        ("SELECT a::regoperator FROM t", "regoperator"),
        ("SELECT a::regconfig FROM t", "regconfig"),
        ("SELECT a::regdictionary FROM t", "regdictionary"),
        ("SELECT a::regrole FROM t", "regrole"),
        ("SELECT a::regclass[] FROM t", "regclass"),
        ("SELECT a::myschema.mytype FROM t", "myschema.mytype"),
        ("SELECT a::money FROM t", "money"),
    ] {
        assert_eq!(functions(sql), [name], "{sql}");
        assert!(!is_safe_function(name), "{sql}");
    }
    // The cast's argument is still read.
    assert_eq!(covered("SELECT a::regclass FROM t"), ["select on t.a"]);
}

#[test]
fn the_pure_parser_nodes_are_allowed_and_xml_is_refused() {
    for sql in [
        "SELECT CURRENT_TIMESTAMP",
        "SELECT CURRENT_DATE, LOCALTIME",
        "SELECT CURRENT_USER, SESSION_USER",
        "SELECT greatest(1, 2), least(3, 4)",
        "SELECT coalesce(a, b) FROM t",
        "SELECT json_object('a': 1), json_array(1, 2)",
        "SELECT json_query('{}', '$.a')",
    ] {
        assert!(analyze(sql).is_ok(), "{sql}: {}", refused(sql));
    }
    for sql in [
        "SELECT xmlelement(name foo)",
        "SELECT xmlparse(document '<a/>')",
        "SELECT a IS DOCUMENT FROM t",
    ] {
        let reason = refused(sql);
        assert!(reason.contains("XML"), "{sql}: {reason}");
    }
}

#[test]
fn the_pure_window_and_text_functions_are_safe() {
    for name in [
        "lag",
        "lead",
        "ntile",
        "cume_dist",
        "percent_rank",
        "first_value",
        "last_value",
        "nth_value",
        "stddev",
        "stddev_pop",
        "stddev_samp",
        "variance",
        "var_pop",
        "var_samp",
        "mode",
        "percentile_cont",
        "percentile_disc",
        "to_tsvector",
        "plainto_tsquery",
        "websearch_to_tsquery",
        "ts_rank",
        "jsonb_extract_path_text",
        "json_extract_path_text",
        "jsonb_array_length",
    ] {
        assert!(is_safe_function(name), "{name} should be safe");
    }
    // `unaccent` reads a rule file, so it stays out of the safe set and a grant
    // has to name it in `functions`.
    assert!(!is_safe_function("unaccent"));
    for sql in [
        "SELECT lag(id) OVER (ORDER BY id) FROM t",
        "SELECT stddev(x), variance(x) FROM t",
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY x) FROM t",
        "SELECT mode() WITHIN GROUP (ORDER BY x) FROM t",
        "SELECT to_tsvector(body), ts_rank(body, plainto_tsquery($1)) FROM t",
        "SELECT jsonb_extract_path_text(data, 'a'), jsonb_array_length(data) FROM t",
    ] {
        let called = functions(sql);
        assert!(
            called.iter().all(|name| is_safe_function(name)),
            "{sql}: {called:?}"
        );
    }
}

/// The class of a statement, and what it is called (D3c).
///
/// [`classify`] answers where [`analyze`] cannot: a tool that offers to write a
/// grant into a manifest has to tell `SELECT … FROM pg_type` (which a `queries`
/// entry may hold) from `SET ROLE …` (which it may not), and neither can be
/// guessed at from the text.
fn class_of(sql: &str) -> (StatementClass, String) {
    let classified = classify(sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
    (classified.class(), classified.label().to_string())
}

#[test]
fn the_four_kinds_a_table_grant_covers_are_plain() {
    for (sql, class, label) in [
        ("SELECT 1", StatementClass::Select, "SELECT"),
        ("SELECT * FROM t", StatementClass::Select, "SELECT"),
        ("VALUES (1)", StatementClass::Select, "SELECT"),
        (
            "WITH x AS (SELECT 1) SELECT * FROM x",
            StatementClass::Select,
            "SELECT",
        ),
        (
            "WITH x AS (SELECT 1) UPDATE t SET a = 1 WHERE id IN (SELECT * FROM x)",
            StatementClass::Update,
            "UPDATE",
        ),
        (
            "INSERT INTO t (a) VALUES (1)",
            StatementClass::Insert,
            "INSERT",
        ),
        ("UPDATE t SET a = 1", StatementClass::Update, "UPDATE"),
        ("DELETE FROM t", StatementClass::Delete, "DELETE"),
    ] {
        let (kind, named) = class_of(sql);
        assert_eq!(kind, class, "{sql}");
        assert_eq!(named, label, "{sql}");
        assert!(kind.is_plain(), "{sql}");
    }
}

#[test]
fn every_other_kind_is_not_plain() {
    // The list a grant must refuse: DDL, the session commands, the transaction
    // commands, and everything that is not a statement about rows.
    for (sql, label) in [
        ("SET TIME ZONE 'UTC'", "SET"),
        ("SET ROLE admin", "SET"),
        ("RESET ALL", "RESET"),
        ("COPY t FROM STDIN", "COPY"),
        ("DO $$ BEGIN END $$", "DO"),
        ("CALL p()", "CALL"),
        ("LOCK TABLE t", "LOCK"),
        ("GRANT SELECT ON t TO x", "GRANT"),
        ("REVOKE SELECT ON t FROM x", "REVOKE"),
        ("ALTER TABLE t ADD COLUMN a int", "ALTER TABLE"),
        ("CREATE TABLE t (a int)", "CREATE TABLE"),
        ("CREATE INDEX i ON t (a)", "CREATE INDEX"),
        ("DROP TABLE t", "DROP"),
        ("TRUNCATE t", "TRUNCATE"),
        ("VACUUM t", "VACUUM"),
        ("LISTEN c", "LISTEN"),
        ("NOTIFY c", "NOTIFY"),
        ("BEGIN", "a transaction-control statement"),
        ("COMMIT", "a transaction-control statement"),
        ("SAVEPOINT s1", "a transaction-control statement"),
        (
            "MERGE INTO t USING u ON t.id = u.id WHEN MATCHED THEN DO NOTHING",
            "MERGE",
        ),
        ("DECLARE c CURSOR FOR SELECT 1", "DECLARE CURSOR"),
        ("FETCH c", "FETCH"),
        ("CLOSE c", "CLOSE"),
        ("EXPLAIN SELECT 1", "EXPLAIN"),
        ("PREPARE p AS SELECT 1", "PREPARE"),
        ("SHOW timezone", "SHOW"),
        ("SELECT 1 INTO t2", "SELECT"),
    ] {
        let (kind, named) = class_of(sql);
        assert_eq!(named, label, "{sql}");
        if sql == "SELECT 1 INTO t2" {
            // `SELECT … INTO` *is* a `SelectStmt` to the parser; the analysis is
            // what refuses it (`INTO` creates a table). `classify` says what the
            // statement is, not whether a table grant walks it.
            assert_eq!(kind, StatementClass::Select, "{sql}");
            continue;
        }
        assert_eq!(kind, StatementClass::Other, "{sql}");
        assert!(!kind.is_plain(), "{sql}");
    }
}

#[test]
fn a_statement_that_is_not_one_statement_is_refused_by_classify_too() {
    assert!(matches!(
        classify("SELECT 1; SELECT 2"),
        Err(AnalysisError::MultipleStatements(2))
    ));
    assert!(matches!(classify("  "), Err(AnalysisError::Empty)));
    assert!(matches!(
        classify("SELECT FROM"),
        Err(AnalysisError::Syntax(_))
    ));
}

#[test]
fn a_system_catalogue_read_is_still_a_plain_select() {
    // The analysis refuses it — a table grant must not name `pg_type` — but the
    // *kind* is a `SELECT`, which is what an exact `queries` entry holds.
    let sql = r#"SELECT "pg_type"."oid" FROM "pg_type""#;
    assert!(analyze(sql).is_err());
    let (kind, label) = class_of(sql);
    assert_eq!((kind, label.as_str()), (StatementClass::Select, "SELECT"));
    assert!(kind.is_plain());
}
