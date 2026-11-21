//! §7 "SQL grant matching & parameterization": the runtime allowlist, the shape
//! rules, the lexer boundaries a grant depends on, the `--learn` gate, and the
//! statements a table grant cannot cover at all.
//!
//! Every rule is asserted from both sides: the statement a grant allows **and**
//! the twin it has to refuse. The table/column/function extents live in
//! `sql_grants.rs`, the query-template half in `sql_templates.rs`; the case map
//! (old file → test here, and what was dropped) is in `RESULT.md`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use blastit_sqlpolicy::{
    Access, Allowlist, Columns, DANGEROUS_LEARN_FUNCTIONS, Decision, KNOWN_STATIC_SETUP_SQL, Op,
    RefuseReason, TableGrant, TableGrants, analyze, cached_analyses, check_learnable,
    has_quoted_literal, is_dangerous_learn_function, is_introspection_setup_sql, normalize,
    redact_literals,
};

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

/// A grant naming one table with `select = "*"`.
fn select_all(table: &str) -> TableGrants {
    TableGrants::new(
        [(
            table.to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    )
}

#[test]
fn an_exact_grant_matches_its_reformatted_self_and_nothing_else() {
    let allow = grant(&[NOTES]);
    // Whitespace runs collapse, a comment stands for one space, the trailing
    // separators are dropped and the placeholder numbers carry no meaning.
    for reformatted in [
        "INSERT   INTO notes_note (owner_id, body)\nVALUES ($1, $2)\tRETURNING notes_note.id",
        "  INSERT INTO notes_note (owner_id, body) VALUES ($12, $13) RETURNING notes_note.id ;;  ",
    ] {
        assert!(allow.check(reformatted).is_allowed(), "{reformatted:?}");
    }
    // A different column, a literal where the grant has a placeholder, casing,
    // and a no-break space where the grant has a space are all different text.
    for denied in [
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.body",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, 'x') RETURNING notes_note.id",
        "insert into notes_note (owner_id, body) values ($1, $2) returning notes_note.id",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1,\u{a0}$2) RETURNING notes_note.id",
    ] {
        assert!(!allow.check(denied).is_allowed(), "{denied:?}");
    }
    assert!(allow.check(NOTES).is_allowed());
}

#[test]
fn a_comment_never_hides_a_second_statement() {
    let allow = grant(&[NOTES]);
    pair(
        &allow,
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id --; DROP TABLE notes_note",
        "INSERT INTO notes_note (owner_id, body) VALUES ($1, $2) RETURNING notes_note.id --\n; DROP TABLE notes_note",
    );
    let allow = grant(&["SELECT 1"]);
    for sql in [
        "SELECT 1 -- c\n; DROP TABLE t",
        "SELECT 1 /* c */ ; DROP TABLE t",
        "SELECT 1 -- c\r; DROP TABLE t",
    ] {
        assert!(!allow.check(sql).is_allowed(), "{sql:?}");
    }
    // A CR-terminated comment on its own is still a fine `BEGIN`.
    let allow = Allowlist::new(std::iter::empty::<&str>(), true);
    for sql in ["BEGIN -- x\r", "BEGIN /* c */"] {
        assert!(allow.check(sql).is_allowed(), "{sql:?}");
    }
}

#[test]
fn quoted_regions_are_data_and_only_scan_l_whitespace_is_whitespace() {
    // Comment contents do not matter, so two spellings of one statement agree;
    // a block comment nests and an unbalanced close is ordinary code.
    let allow = grant(&["SELECT 1 /* ; */ DROP TABLE t"]);
    let comments = allow.check("SELECT 1 /*\nsome\nother\ncomment\n*/ DROP TABLE t");
    assert!(comments.is_allowed());
    assert_eq!(normalize("SELECT 1 /* /* */ */, 2"), "SELECT 1 , 2");
    assert_eq!(normalize("SELECT 1 */ 2"), "SELECT 1 */ 2");
    // A comment marker inside a quoted region is data, not a comment.
    let allow = grant(&["SELECT 1"]);
    for sql in ["SELECT '-- 1'", "SELECT $$-- 1$$", "SELECT E'-- 1'"] {
        assert!(!allow.check(sql).is_allowed(), "{sql:?}");
    }
    // scan.l's own whitespace is one space; anything else is text.
    for sql in ["SELECT\t1", " SELECT \t\r\n\x0b\x0c 1 ; "] {
        assert!(allow.check(sql).is_allowed(), "{sql:?}");
    }
    assert!(!allow.check("SELECT\u{a0}1").is_allowed());
}

#[test]
fn batches_and_empty_text_are_denied_and_grants_are_stored_normalised() {
    let allow = grant(&[NOTES, "SELECT 1"]);
    for sql in [
        format!("{NOTES}; SELECT 1"),
        format!("{NOTES}; DROP TABLE notes_note"),
    ] {
        assert!(!allow.check(&sql).is_allowed(), "{sql:?}");
    }
    // A bare trailing separator is normalised away, and a `;` in a string is
    // part of one statement.
    assert_eq!(allow.check(&format!("{NOTES};")), Decision::Allowed);
    let allow = grant(&["SELECT ';'"]);
    assert_eq!(allow.check("SELECT ';';"), Decision::Allowed);

    // Empty text is denied whatever the flag; an empty grant allows nothing.
    for transaction_control in [false, true] {
        let allow = Allowlist::new(["SELECT 1"], transaction_control);
        for sql in ["", " ", "\t", ";"] {
            assert!(!allow.check(sql).is_allowed(), "{sql:?}");
        }
        let empty = Allowlist::new(std::iter::empty::<&str>(), transaction_control);
        assert!(empty.is_empty());
        assert!(!empty.check("SELECT 1").is_allowed());
    }

    // Two manifest entries that differ only in formatting count once.
    let allow = Allowlist::new(["SELECT   1 ;", "SELECT 1", "SET TIME ZONE 'UTC'"], false);
    assert_eq!(allow.len(), 2);
    let mut stored: Vec<&str> = allow.queries().collect();
    stored.sort_unstable();
    assert_eq!(stored, ["SELECT 1", "SET TIME ZONE 'UTC'"]);
}

#[test]
fn transaction_control_allows_exactly_one_transaction_statement() {
    let on = Allowlist::new(std::iter::empty::<&str>(), true);
    for sql in [
        "BEGIN WORK",
        "START TRANSACTION READ WRITE",
        "COMMIT",
        "ROLLBACK",
        "SAVEPOINT \"s1_x\"",
        "ROLLBACK WORK TO SAVEPOINT s1",
    ] {
        assert!(on.check(sql).is_allowed(), "{sql:?}");
    }
    for sql in [
        "BEGIN READ ONLY EXTRA",
        "SAVEPOINT",
        "ROLLBACK TO 1",
        "COMMIT WORK EXTRA",
        "SET TIME ZONE 'UTC'",
    ] {
        assert!(!on.check(sql).is_allowed(), "{sql:?}");
    }
    for sql in ["BEGIN; DROP TABLE t", "SELECT 1; BEGIN"] {
        assert!(!on.check(sql).is_allowed(), "{sql:?}");
    }
    // Without the flag, even `BEGIN` is not granted — while a listed statement
    // still matches exactly.
    assert!(
        !Allowlist::new(std::iter::empty::<&str>(), false)
            .check("BEGIN")
            .is_allowed()
    );
    assert_eq!(grant(&["COMMIT"]).check("COMMIT;"), Decision::Allowed);
}

#[test]
fn an_unrestricted_allowlist_allows_everything_and_default_allows_nothing() {
    let allow = Allowlist::unrestricted();
    for sql in ["DROP TABLE t", "COPY (SELECT 1) TO STDOUT", ""] {
        assert_eq!(allow.check(sql), Decision::Allowed, "{sql:?}");
    }
    assert!(allow.is_unrestricted());
    assert!(
        !allow.is_empty(),
        "it allows everything, so it is not empty"
    );
    assert_eq!(allow.len(), 0);
    assert!(!allow.transaction_control());
    // An accidental `Default` can never open a route.
    let default = Allowlist::default();
    assert!(!default.is_unrestricted());
    assert!(!default.check("SELECT 1").is_allowed());
    assert!(!default.check("BEGIN").is_allowed());
}

#[test]
fn an_in_list_is_one_shape_and_a_literal_is_not_part_of_it() {
    let allow = grant(&["SELECT id FROM notes_note WHERE owner_id IN ($1)"]);
    let granted = "SELECT id FROM notes_note WHERE owner_id IN ($1)";
    let literal = "SELECT id FROM notes_note WHERE owner_id IN ('x')";
    assert_ne!(
        normalize(literal),
        normalize(granted),
        "an inline literal must not collapse into the placeholder-list shape"
    );
    assert!(allow.check(granted).is_allowed());
    for denied in [
        literal,
        "SELECT id FROM notes_note WHERE owner_id IN ($1, 'x')",
        "SELECT id FROM notes_note WHERE owner_id IN ('x', $1)",
        "SELECT id FROM notes_note WHERE owner_id IN ()",
        "SELECT id FROM notes_note WHERE owner_id IN ($1,)",
        "SELECT id FROM notes_note WHERE owner_id IN ($1::int, $2)",
        "SELECT id FROM notes_note WHERE owner_id IN (SELECT owner_id FROM t)",
        "SELECT id FROM notes_note WHERE owner_id NOT IN ($1, $2)",
    ] {
        assert!(!allow.check(denied).is_allowed(), "{denied:?}");
    }
    // ... while a longer list of placeholders is the same shape.
    for length in 1..=8 {
        let list = (1..=length)
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let statement = format!("SELECT id FROM notes_note WHERE owner_id IN ({list})");
        assert!(allow.check(&statement).is_allowed(), "{statement:?}");
    }
}

#[test]
fn a_qualified_in_call_keeps_its_arity_and_the_dot_is_the_guard() {
    for sql in ["SELECT 1 IN ($1, $2)", "SELECT f(1) IN ($1, $2)"] {
        assert!(normalize(sql).ends_with("IN ($?, ...)"), "{sql:?}");
    }
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
    ] {
        assert!(!allow.check(denied).is_allowed(), "{denied:?}");
    }
    // The whitespace in front of the keyword is kept, so the two spellings stay
    // two statements; an unterminated group is ordinary text.
    assert_eq!(
        normalize("SELECT id FROM t WHERE (id)IN ($1, $2)"),
        "SELECT id FROM t WHERE (id)IN ($?, ...)"
    );
    assert_eq!(normalize("WHERE id IN ($1"), "WHERE id IN ($?");
}

#[test]
fn identical_values_rows_collapse_but_a_width_or_a_separator_does_not() {
    let allow = grant(&["INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)"]);
    for rows in [2, 8] {
        let mut next = 0;
        let tuples = (0..rows)
            .map(|_| {
                next += 1;
                let first = next;
                next += 1;
                format!("(${first}, ${next})")
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
    // One row is its own statement, and so is a run of another width.
    pair(
        &allow,
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
        "INSERT INTO t (a, b) VALUES ($1, $2)",
    );
    let width = allow.check("INSERT INTO t (a, b) VALUES ($1, $2, $3), ($4, $5, $6)");
    assert!(!width.is_allowed());
    // A tuple list not after the keyword, or a qualified `values` call, is left
    // alone; a row carrying a `;` can never be dropped.
    for unchanged in ["SELECT s.values(1), (1), (1)", "VALUES ((1;2)), ((1;2))"] {
        assert_eq!(normalize(unchanged), unchanged, "{unchanged:?}");
    }
    assert_eq!(normalize("VALUES ((1)), ((1))"), "VALUES ((1)), ...");
}

#[test]
fn a_statement_a_table_grant_cannot_cover_is_refused_by_name() {
    let sentence = |label: &str| {
        format!("a table grant cannot cover {label}: only SELECT, INSERT, UPDATE and DELETE can be")
    };
    for (sql, label) in [
        ("SET ROLE admin", "SET"),
        ("RESET ALL", "SET"),
        ("GRANT SELECT ON t TO app", "GRANT"),
        ("REVOKE SELECT ON t FROM app", "GRANT"),
        ("TRUNCATE t", "TRUNCATE"),
        ("VACUUM t", "VACUUM"),
        ("LOCK TABLE t", "LOCK"),
        ("COPY t FROM STDIN", "COPY"),
        ("DO $$BEGIN NULL; END$$", "DO"),
        ("CALL p()", "CALL"),
        ("EXPLAIN SELECT 1", "EXPLAIN"),
        ("PREPARE p AS SELECT 1", "PREPARE"),
        ("CREATE TABLE t (a int)", "CREATE TABLE"),
        ("ALTER TABLE t ADD COLUMN a int", "ALTER TABLE"),
        ("DROP TABLE t", "DROP"),
        ("LISTEN c", "LISTEN"),
        ("NOTIFY c", "NOTIFY"),
        (
            "MERGE INTO t USING u ON true WHEN MATCHED THEN DELETE",
            "MERGE",
        ),
    ] {
        assert_eq!(
            analyze(sql).unwrap_err().to_string(),
            sentence(label),
            "{sql}"
        );
        // The runtime path: a grant with table rights still refuses it, with
        // exactly that sentence.
        let decision = Allowlist::new(std::iter::empty::<&str>(), false)
            .with_tables(select_all("t"))
            .check(sql);
        assert_eq!(
            decision,
            Decision::Denied {
                normalized: normalize(sql),
                reason: sentence(label),
            },
            "{sql}"
        );
    }
}

#[test]
fn every_quoted_form_closes_at_its_own_boundary() {
    // `''`/`""` escape by doubling; E-strings take a backslash; B/X take none,
    // so `\'` closes them and a doubled quote starts a second literal.
    assert_eq!(redact_literals("SELECT 'it''s'"), "SELECT …");
    assert_eq!(redact_literals(r#"SELECT "a""b""#), r#"SELECT "a""b""#);
    assert_eq!(redact_literals(r"SELECT E'a\'b'"), "SELECT …");
    assert_eq!(redact_literals(r"SELECT B'a''b'"), "SELECT ……");
    assert_eq!(redact_literals("SELECT $tag$a$other$"), "SELECT …");
    // `$1$` is the placeholder `$1` and a lone `$`, not a dollar quote.
    assert_eq!(redact_literals("SELECT $1$"), "SELECT $1$");
    assert!(!has_quoted_literal("SELECT $1$"));
    // A quote in a comment never opens a literal, and an unterminated literal is
    // one opaque region to the end.
    assert_eq!(redact_literals("SELECT '--x' -- 'y'"), "SELECT … -- 'y'");
    assert_eq!(redact_literals("SELECT E'a"), "SELECT …");
    // The protocol's own setup statements keep their fixed literal, and only as
    // an exact match.
    for sql in KNOWN_STATIC_SETUP_SQL {
        assert_eq!(redact_literals(sql), *sql, "{sql:?}");
        assert!(has_quoted_literal(sql));
    }
}

#[test]
fn numeric_and_placeholder_extents_decide_where_the_next_token_starts() {
    // The number ends after its last digit, so `$1` behind it is a parameter; an
    // identifier glued to it is one token instead.
    assert_eq!(normalize("SELECT 0x1F$1"), "SELECT 0x1F$?");
    assert_eq!(normalize("SELECT 0x1Fg$1"), "SELECT 0x1Fg$1");
    assert_eq!(normalize("SELECT 0x_$1"), "SELECT 0x_$?");
    assert_eq!(normalize("SELECT 1_0$1"), "SELECT 1_0$?");
    assert_eq!(normalize("SELECT 1_$1"), "SELECT 1_$1");
    // The `E` opens an E-string only at a token start.
    assert_eq!(normalize(r"SELECT 1..E'a\'b  c'"), r"SELECT 1..E'a\'b  c'");
    assert_eq!(normalize(r"SELECT 1.E'a\'b  c'"), r"SELECT 1.E'a\'b c'");
    assert_eq!(normalize("SELECT $1E'a'"), "SELECT $?E'a'");
    // A `$` inside an identifier is data, and `$1$` keeps the trailing `$`.
    assert_eq!(normalize("SELECT a$1, a$b$c, x$"), "SELECT a$1, a$b$c, x$");
}

#[test]
fn a_large_hostile_text_is_scanned_in_one_pass() {
    let body = "a".repeat(200_000);
    for sql in [format!("SELECT '{body}"), format!("SELECT $tag${body}")] {
        assert_eq!(normalize(&sql), sql, "a {}-byte statement", sql.len());
    }
    for sql in [format!("SELECT --{body}"), format!("SELECT /*{body}")] {
        assert_eq!(normalize(&sql), "SELECT");
    }
    assert_eq!(
        check_learnable(&format!("SELECT '{body}'")).reason(),
        Some(&RefuseReason::InlineLiteral)
    );
}

#[test]
fn learn_is_grantable_only_for_parameterised_single_statements() {
    let verdict = check_learnable(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $1"#);
    assert!(verdict.is_query(), "{verdict:?}");
    assert_eq!(
        verdict.normalized(),
        Some(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#)
    );
    // Every inline literal form is refused, and only the redacted shape is
    // carried.
    for sql in ["SELECT 'x'", r"SELECT E'\n'", r"SELECT U&'\0041'"] {
        let verdict = check_learnable(sql);
        assert_eq!(
            verdict.reason(),
            Some(&RefuseReason::InlineLiteral),
            "{sql:?}"
        );
        assert_eq!(verdict.redacted(), Some("SELECT …"), "{sql:?}");
        assert_eq!(verdict.normalized(), None, "{sql:?}");
    }
    // A log's `…` marker is not a grantable statement, and neither is SQL that
    // does not parse — and a refusal never prints the literal it choked on.
    assert_eq!(
        check_learnable("SELECT … FROM t WHERE id = $?").reason(),
        Some(&RefuseReason::RedactedPlaceholder)
    );
    for sql in ["SELEC 1", "", ";"] {
        assert!(
            matches!(
                check_learnable(sql).reason(),
                Some(RefuseReason::Unparseable(_))
            ),
            "{sql:?}"
        );
    }
    let verdict = check_learnable("SELECT 's3cr3t");
    assert!(!format!("{verdict:?}").contains("s3cr3t"), "{verdict:?}");
    assert!(
        !verdict.redacted().unwrap_or_default().contains("s3cr3t"),
        "{verdict:?}"
    );
    // One grant is one statement, whatever the count.
    for (sql, count) in [
        ("SELECT 1; SELECT 2", 2usize),
        ("SELECT 1; SELECT 2; SELECT 3", 3),
    ] {
        assert_eq!(
            check_learnable(sql).reason(),
            Some(&RefuseReason::MultipleStatements(count)),
            "{sql:?}"
        );
    }
}

#[test]
fn a_refusal_never_renders_a_literal_a_writer_built_by_hand() {
    // `Display` strips the token again, for a record written by another build:
    // both the ` at or near ` tail and a quoted span in the middle are removed.
    for message in [
        r#"syntax error at or near "hunter2""#,
        r#"invalid Unicode escape value at or near ""s3cr3t"""#,
        r#"unterminated quoted string at or near "'s3cr3t"#,
        r#"column "s3cr3t" does not exist"#,
    ] {
        let rendered = RefuseReason::Unparseable(message.to_string()).to_string();
        for needle in ["hunter2", "s3cr3t"] {
            assert!(!rendered.contains(needle), "{message} -> {rendered}");
        }
        assert!(
            rendered.starts_with("the statement is not valid SQL: "),
            "{rendered}"
        );
    }
}

#[test]
fn learn_refuses_a_server_side_command_and_the_dangerous_functions() {
    for sql in [
        "COPY t FROM PROGRAM 'cmd-from-the-wire'",
        "COPY (SELECT 1) TO PROGRAM 'cmd-from-the-wire'",
    ] {
        let verdict = check_learnable(sql);
        assert_eq!(
            verdict.reason(),
            Some(&RefuseReason::CopyProgram),
            "{sql:?}"
        );
        assert!(
            !verdict
                .redacted()
                .unwrap_or_default()
                .contains("cmd-from-the-wire"),
            "{verdict:?}"
        );
    }
    // A `COPY … TO STDOUT` runs no command and stays grantable.
    for sql in ["COPY t TO STDOUT", "COPY (SELECT 1) TO STDOUT"] {
        assert!(check_learnable(sql).is_query(), "{sql:?}");
    }
    // Every name on the list is refused, including through `CALL` and a
    // three-part name — while another schema's function is a different one.
    for name in DANGEROUS_LEARN_FUNCTIONS {
        assert!(is_dangerous_learn_function(name));
        assert!(
            matches!(
                check_learnable(&format!("SELECT {name}($1)")).reason(),
                Some(RefuseReason::DangerousFunction(seen)) if seen.as_str() == *name
            ),
            "{name}"
        );
    }
    for sql in [
        "CALL pg_catalog.dblink_exec($1)",
        "SELECT pg_catalog.pg_catalog.pg_read_file($1)",
    ] {
        assert!(
            matches!(
                check_learnable(sql).reason(),
                Some(RefuseReason::DangerousFunction(_))
            ),
            "{sql:?}"
        );
    }
    assert!(check_learnable("SELECT public.pg_read_file($1)").is_query());
}

#[test]
fn session_setup_is_replayed_only_for_the_catalogue() {
    for sql in [
        "SET TIME ZONE 'UTC'",
        "SHOW standard_conforming_strings",
        "SELECT pg_catalog.version()",
        "SELECT to_regtype($?)",
        "SELECT * FROM pg_type",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?}");
    }
    for sql in [
        "SELECT * FROM auth_user",
        "SELECT * FROM otherdb.pg_catalog.pg_type",
        "SELECT pg_get_userbyid($?)",
        "DROP TABLE t",
        "SET x; DROP TABLE t",
        "",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?}");
    }
    // A `$` that opens no placeholder is left alone, so the statement stays
    // unparseable instead of being repaired into a replayable one.
    assert!(!is_introspection_setup_sql(
        "SELECT pg_catalog.version(), $a, $?"
    ));
    assert!(is_introspection_setup_sql(
        "SELECT pg_catalog.version(), $?, $?"
    ));
}

#[test]
fn the_analysis_reports_exactly_the_accesses_a_grant_is_checked_against() {
    let analysis = analyze(r#"SELECT "shop_product"."id" FROM "shop_product""#)
        .expect("a plain SELECT is analysed");
    let accesses: Vec<&Access> = analysis.accesses().collect();
    assert_eq!(
        accesses,
        [&Access::new(Op::Select, "shop_product", Some("id"))]
    );
    assert_eq!(analysis.functions().count(), 0);
    assert!(cached_analyses() <= 512, "the memo is bounded");
    // What the analysis does not understand is refused, never allowed.
    assert_eq!(
        analyze("SET search_path = public").unwrap_err().to_string(),
        "a table grant cannot cover SET: only SELECT, INSERT, UPDATE and DELETE can be"
    );
    assert_eq!(
        analyze("SELECT 1; SELECT 2").unwrap_err().to_string(),
        "the text holds 2 statements, and a table grant covers exactly one"
    );
}
