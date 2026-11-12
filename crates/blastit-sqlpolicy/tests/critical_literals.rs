//! Protected critical tests for SQL literal redaction and `--learn` verdicts
//! (R24, R25, R31; ADR-0003).
//!
//! Every rule has a positive and a negative twin, so a change that widens what
//! is grantable and a change that narrows it both fail here. These tests are
//! guarded by `tools/critical_guard.py`: do not weaken them.

use blastit_sqlpolicy::{
    DANGEROUS_LEARN_FUNCTIONS, KNOWN_STATIC_SETUP_SQL, RefuseReason, check_learnable,
    has_quoted_literal, is_dangerous_learn_function, is_introspection_setup_sql, redact_literals,
};

/// The redaction marker: one character, never a literal.
const ELLIPSIS: &str = "…";

//
// R24: redaction.
//

#[test]
fn every_literal_form_redacts_to_exactly_one_ellipsis() {
    // The four pinned in §7.2, then the rest of the eight forms.
    for sql in [
        r"SELECT E'\n'",
        "SELECT B'1'",
        r"SELECT U&'\0041'",
        "SELECT $t$x$t$",
        "SELECT 'x'",
        "SELECT X'ff'",
        "SELECT N'x'",
        "SELECT $$x$$",
    ] {
        assert_eq!(
            redact_literals(sql),
            format!("SELECT {ELLIPSIS}"),
            "redaction of {sql:?}"
        );
    }
}

#[test]
fn redaction_replaces_the_whole_token_including_its_prefix() {
    // A half-literal (`SELECT E…`) is neither a statement nor a shape a grant
    // can hold, so the prefix goes with the literal.
    for (sql, prefix) in [
        (r"SELECT E'\n'", 'E'),
        ("SELECT B'1'", 'B'),
        (r"SELECT U&'\0041'", 'U'),
        ("SELECT $t$x$t$", '$'),
        ("SELECT N'x'", 'N'),
        ("SELECT X'ff'", 'X'),
    ] {
        let redacted = redact_literals(sql);
        assert_eq!(redacted, "SELECT …", "for {sql:?}");
        let tail = redacted.strip_prefix("SELECT ").unwrap_or_default();
        assert!(
            !tail.contains(prefix) && !tail.contains('&'),
            "a {prefix:?} literal prefix survived {sql:?}: {redacted:?}"
        );
    }
}

#[test]
fn identifiers_placeholders_and_static_setup_survive_redaction() {
    for sql in [
        r#"SELECT "quoted_identifiers" FROM "t""#,
        r#"SELECT U&"quoted_identifiers" FROM "t""#,
        "SELECT $1, $? FROM t",
        r#"SELECT "a""b" FROM "t""#,
    ] {
        assert_eq!(redact_literals(sql), sql, "for {sql:?}");
        assert!(!has_quoted_literal(sql), "for {sql:?}");
    }
    for sql in KNOWN_STATIC_SETUP_SQL {
        assert_eq!(redact_literals(sql), *sql, "for {sql:?}");
        assert!(has_quoted_literal(sql), "{sql:?} keeps its fixed literal");
    }
}

#[test]
fn a_quote_in_a_comment_is_not_a_literal() {
    for sql in ["SELECT 1 -- 'x'", "SELECT 1 /* 'x' */"] {
        assert!(!has_quoted_literal(sql), "for {sql:?}");
        assert_eq!(redact_literals(sql), sql, "for {sql:?}");
    }
}

#[test]
fn redaction_is_idempotent_and_never_emits_a_literal_byte() {
    for sql in [
        "SELECT 's3cr3t' FROM t WHERE id = $1",
        r"SELECT E'a\'s3cr3t'",
        "SELECT N's3cr3t', \"col\"",
        r"SELECT U&'\0073\0063\0072\0074'",
        "SELECT $t$s3cr3t$t$",
        "SELECT 'a', \"col\", 'b'",
        "SET TIME ZONE 'UTC'",
    ] {
        let once = redact_literals(sql);
        assert_eq!(redact_literals(&once), once, "not idempotent for {sql:?}");
        assert!(
            !once.contains("s3cr3t"),
            "leaked a literal byte from {sql:?}: {once:?}"
        );
        assert!(
            !once.contains("\\0073"),
            "leaked an escape from {sql:?}: {once:?}"
        );
    }
}

#[test]
fn comments_are_not_literals_and_are_left_to_normalisation() {
    // `sql.denied` and `obs_fd` carry the *normalised* text, where every
    // comment is already a space; redaction only owns quoted literals.
    let sql = "SELECT 's3cr3t' -- a comment";
    assert_eq!(redact_literals(sql), "SELECT … -- a comment");
}

//
// R25: what `--learn` may propose.
//

#[test]
fn a_parameterised_statement_is_grantable() {
    // `$1` becomes `$?`, and the text a grant has to hold is returned.
    let verdict = check_learnable(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $1"#);
    assert!(verdict.is_query(), "{verdict:?}");
    assert_eq!(
        verdict.normalized(),
        Some(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#)
    );
    assert!(verdict.reason().is_none());
}

#[test]
fn quoted_and_unicode_identifiers_are_grantable() {
    for sql in [
        r#"SELECT "quoted_identifiers" FROM "t" WHERE id = $1"#,
        r#"SELECT U&"quoted_identifiers" FROM "t" WHERE id = $1"#,
    ] {
        let verdict = check_learnable(sql);
        assert!(verdict.is_query(), "{sql:?} -> {verdict:?}");
    }
}

#[test]
fn the_known_static_setup_statements_are_grantable() {
    for sql in KNOWN_STATIC_SETUP_SQL {
        let verdict = check_learnable(sql);
        assert!(verdict.is_query(), "{sql:?} -> {verdict:?}");
        assert_eq!(verdict.normalized(), Some(*sql));
    }
}

#[test]
fn catalogue_introspection_and_show_are_grantable() {
    for sql in [
        "SELECT pg_catalog.version()",
        "SELECT version()",
        "SELECT current_schema()",
        "SHOW standard_conforming_strings",
        "SHOW ALL",
    ] {
        let verdict = check_learnable(sql);
        assert!(verdict.is_query(), "{sql:?} -> {verdict:?}");
    }
}

#[test]
fn every_inline_literal_form_is_refused() {
    for sql in [
        "SELECT 'x'",
        r"SELECT E'\n'",
        "SELECT B'1'",
        "SELECT X'ff'",
        "SELECT N'x'",
        r"SELECT U&'\0041'",
        "SELECT $$x$$",
        "SELECT $t$x$t$",
    ] {
        let verdict = check_learnable(sql);
        assert!(
            matches!(verdict.reason(), Some(RefuseReason::InlineLiteral)),
            "{sql:?} -> {verdict:?}"
        );
        assert_eq!(verdict.redacted(), Some("SELECT …"), "for {sql:?}");
    }
}

#[test]
fn a_redacted_placeholder_is_refused() {
    let verdict = check_learnable("SELECT … FROM t WHERE id = $?");
    assert!(
        matches!(verdict.reason(), Some(RefuseReason::RedactedPlaceholder)),
        "{verdict:?}"
    );
}

#[test]
fn unparseable_sql_is_refused() {
    for sql in ["SELEC 1", "SELECT 'unterminated", "", "   ", ";"] {
        let verdict = check_learnable(sql);
        assert!(
            matches!(verdict.reason(), Some(RefuseReason::Unparseable(_))),
            "{sql:?} -> {verdict:?}"
        );
    }
}

#[test]
fn no_refusal_prints_a_literal_byte() {
    // PostgreSQL's parser quotes the bytes it choked on — `syntax error at or
    // near "s3cr3t"` — and `--learn` prints the refusal to a human. Every
    // variant is exercised, because the guarantee is about the type, not about
    // one code path.
    let refusals = [
        RefuseReason::Unparseable(
            "Invalid statement: unterminated quoted string at or near \"'s3cr3t\"".to_string(),
        ),
        RefuseReason::Unparseable(
            "Invalid statement: syntax error at or near \"hunter2\"".to_string(),
        ),
        RefuseReason::Unparseable("invalid input syntax for type integer: \"s3cr3t\"".to_string()),
        RefuseReason::MultipleStatements(2),
        RefuseReason::InlineLiteral,
        RefuseReason::RedactedPlaceholder,
        RefuseReason::CopyProgram,
        RefuseReason::DangerousFunction("pg_read_file".to_string()),
    ];
    for reason in &refusals {
        let rendered = reason.to_string();
        assert!(!rendered.contains("s3cr3t"), "{reason:?} -> {rendered:?}");
        assert!(!rendered.contains("hunter2"), "{reason:?} -> {rendered:?}");
    }
    // End to end: the parser's own report about the wire text must not carry
    // the literal, in the reason or in the redacted statement.
    for sql in ["SELECT 's3cr3t", "SELECT $t$hunter2"] {
        let verdict = check_learnable(sql);
        let Some(reason) = verdict.reason() else {
            panic!("{sql:?} must be refused: {verdict:?}");
        };
        assert!(
            matches!(reason, RefuseReason::Unparseable(_)),
            "{sql:?} -> {verdict:?}"
        );
        let rendered = reason.to_string();
        assert!(!rendered.contains("s3cr3t"), "{sql:?} -> {rendered:?}");
        assert!(!rendered.contains("hunter2"), "{sql:?} -> {rendered:?}");
        let debug = format!("{verdict:?}");
        assert!(!debug.contains("s3cr3t"), "{sql:?} -> {debug:?}");
        assert!(!debug.contains("hunter2"), "{sql:?} -> {debug:?}");
        let redacted = verdict.redacted().unwrap_or_default();
        assert!(!redacted.contains("s3cr3t"), "{sql:?} -> {redacted:?}");
        assert!(!redacted.contains("hunter2"), "{sql:?} -> {redacted:?}");
    }
}

#[test]
fn multi_statement_text_is_refused() {
    for sql in [
        "SELECT 1; SELECT 2",
        "SELECT 1; DROP TABLE t",
        "BEGIN; COMMIT",
        "SELECT 1; -- c\nSELECT 2",
    ] {
        let verdict = check_learnable(sql);
        assert!(
            matches!(
                verdict.reason(),
                Some(RefuseReason::MultipleStatements(count)) if *count >= 2
            ),
            "{sql:?} -> {verdict:?}"
        );
    }
}

#[test]
fn copy_program_is_refused() {
    for sql in [
        "COPY t FROM PROGRAM 'id'",
        "COPY (SELECT 1) TO PROGRAM 'id'",
        "copy t from program 'id'",
    ] {
        let verdict = check_learnable(sql);
        assert!(
            matches!(verdict.reason(), Some(RefuseReason::CopyProgram)),
            "{sql:?} -> {verdict:?}"
        );
    }
}

#[test]
fn every_dangerous_function_is_refused() {
    assert!(
        DANGEROUS_LEARN_FUNCTIONS.contains(&"pg_read_file"),
        "the list must hold the functions the docs name"
    );
    assert!(DANGEROUS_LEARN_FUNCTIONS.contains(&"lo_import"));
    assert!(DANGEROUS_LEARN_FUNCTIONS.contains(&"dblink"));
    for name in DANGEROUS_LEARN_FUNCTIONS {
        assert!(is_dangerous_learn_function(name), "{name:?}");
        let sql = format!("SELECT {name}($1)");
        let verdict = check_learnable(&sql);
        assert!(
            matches!(
                verdict.reason(),
                Some(RefuseReason::DangerousFunction(seen)) if seen.as_str() == *name
            ),
            "{sql:?} -> {verdict:?}"
        );
    }
}

#[test]
fn a_dangerous_function_in_another_schema_is_a_different_function() {
    // `public.pg_read_file` is not the server's `pg_read_file`, so refusing it
    // would be a false positive; the route's own `functions` grant covers it.
    let verdict = check_learnable("SELECT public.pg_read_file($1)");
    assert!(verdict.is_query(), "{verdict:?}");
}

#[test]
fn a_call_and_a_three_part_name_cannot_hide_a_dangerous_function() {
    // `CALL p(...)` holds its `FuncCall` directly in `CallStmt.funccall`, not
    // in the one-key node wrapper a tree walk sees, and PostgreSQL accepts one
    // more qualifier than the two-part resolution checked before.
    for sql in [
        "CALL pg_read_file($1)",
        "CALL pg_catalog.pg_read_file($1)",
        "CALL pg_catalog.dblink_exec($1)",
        "SELECT pg_catalog.pg_catalog.pg_read_file($1)",
        "SELECT pg_catalog.pg_catalog.dblink_exec($1)",
    ] {
        let verdict = check_learnable(sql);
        assert!(
            matches!(verdict.reason(), Some(RefuseReason::DangerousFunction(_))),
            "{sql:?} -> {verdict:?}"
        );
    }
}

//
// R31: session-init replay.
//

#[test]
fn session_setup_is_replayed() {
    for sql in [
        "SET TIME ZONE 'UTC'",
        "SET search_path = public",
        "SET application_name = 'app'",
        "SHOW standard_conforming_strings",
        "SHOW ALL",
        "SELECT pg_catalog.version()",
        "SELECT current_schema()",
        "SELECT to_regtype($?)",
        "SELECT * FROM pg_catalog.pg_type WHERE oid = $?",
        "SELECT * FROM pg_type",
        "SELECT current_setting('server_version_num')",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?}");
    }
}

#[test]
fn only_qualified_information_schema_tables_are_replayed() {
    // `tables`, `columns` and `schemata` are ordinary names a `public` table
    // can take. Only `pg_catalog` is first on every search path, so only its
    // entries may be matched unqualified; `information_schema` needs its
    // schema spelled out.
    for sql in [
        "SELECT * FROM tables",
        "SELECT * FROM columns",
        "SELECT * FROM schemata",
        "SELECT * FROM information_schema.pg_type",
        "SELECT * FROM public.pg_type",
        "SELECT * FROM public.tables",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?}");
    }
    for sql in [
        "SELECT pg_catalog.version()",
        "SELECT * FROM information_schema.tables",
        "SELECT * FROM information_schema.columns",
        "SELECT * FROM information_schema.schemata",
        "SELECT * FROM pg_catalog.pg_type",
        "SELECT * FROM pg_type",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?}");
    }
}

#[test]
fn pg_get_userbyid_is_not_replayed() {
    // It resolves role names out of `pg_authid`, which no route is granted.
    assert!(!is_introspection_setup_sql("SELECT pg_get_userbyid($?)"));
}

#[test]
fn anything_else_is_not_replayed() {
    for sql in [
        "SELECT * FROM auth_user",
        "SELECT pg_read_file('/etc/passwd')",
        "SELECT * FROM pg_catalog.pg_authid",
        "SELECT nextval('s')",
        "SELECT 1 INTO t",
        "SELECT 1 FOR UPDATE",
        "DROP TABLE t",
        "INSERT INTO t VALUES ($1)",
        "SET x; DROP TABLE t",
        "SELECT 1; SHOW x",
        "",
        "   ",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?}");
    }
}

#[test]
fn only_pg_catalog_qualified_functions_are_replayed() {
    // A replay runs once under a routed target, without a grant, so the name
    // has to resolve to the server's function and nothing else. A two-part
    // call resolves in the schema that is spelled out: `public.version()` is
    // the route's own function, not the one `pg_catalog` holds.
    for sql in [
        "SELECT public.version()",
        "SELECT app.current_schema()",
        "SELECT app.to_regtype($?)",
        "SELECT public.pg_catalog.version()",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?}");
    }
    // The same calls under `pg_catalog`, and the bare name (`pg_catalog` is
    // first on every search path), are the server's functions.
    for sql in [
        "SELECT version()",
        "SELECT pg_catalog.version()",
        "SELECT pg_catalog.current_schema()",
        "SELECT pg_catalog.to_regtype($?)",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?}");
    }
}

#[test]
fn three_part_relations_are_not_replayed() {
    // `database.schema.relation` is a name PostgreSQL accepts, and the schema
    // part alone does not identify the catalogue: `otherdb.pg_catalog.pg_type`
    // is another database's `pg_type`. Only the one- and two-part forms name a
    // relation this session can vouch for.
    for sql in [
        "SELECT * FROM otherdb.pg_catalog.pg_type",
        "SELECT * FROM otherdb.information_schema.tables",
    ] {
        assert!(!is_introspection_setup_sql(sql), "{sql:?}");
    }
    for sql in [
        "SELECT * FROM pg_catalog.pg_type",
        "SELECT * FROM information_schema.tables",
    ] {
        assert!(is_introspection_setup_sql(sql), "{sql:?}");
    }
}
