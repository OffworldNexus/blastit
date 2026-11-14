//! Edge cases for the three policy modules that no protected test owned
//! (MB.1.4): `normalize`, `literals` and — through the crate — `lru`.
//!
//! `src/normalize.rs`'s own tests pin the text rules one at a time and
//! `tests/critical_literals.rs` pins what the redaction *rules* are. What
//! follows pins the two things a grant must never get wrong, through the
//! public policy API ([`Allowlist::check`]) rather than through the text alone:
//!
//! * a statement that was only **reformatted** still matches its grant, and a
//!   **bypass** that leans on the same normalisation does not — a comment that
//!   does not hide a second statement, casing, a literal where the grant has a
//!   placeholder, a no-break space where the grant has a space;
//! * the two *shape* rules (`IN` lists of placeholders, `VALUES` row runs)
//!   collapse exactly the texts that are the same statement and nothing else.
//!
//! Every case is a pair: the allowed text, and the denied text that differs
//! from it in exactly one way. A rule that widens fails on the denied half just
//! as a rule that narrows fails on the allowed half, so neither direction of a
//! `normalize` change can pass unnoticed.

use blastit_sqlpolicy::{
    Allowlist, KNOWN_STATIC_SETUP_SQL, has_quoted_literal, normalize, redact_literals,
};

/// The redaction marker: one character, never a literal.
const ELLIPSIS: &str = "…";

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

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
fn every_literal_form_redacts_to_one_ellipsis_prefix_included() {
    // The prefix is part of the literal: `E'…'` is `…`, not `E…`, because a
    // half-literal is neither a statement nor a shape a grant author can read.
    for sql in [
        "SELECT 's3cr3t'",
        r"SELECT E's3cr3t'",
        "SELECT B'1'",
        "SELECT X'ff'",
        "SELECT N's3cr3t'",
        r"SELECT U&'\0073'",
        "SELECT $$s3cr3t$$",
        "SELECT $tag$s3cr3t$tag$",
        "SELECT 'a', 's3cr3t', $$b$$",
    ] {
        let once = redact_literals(sql);
        assert!(!once.contains("s3cr3t"), "leaked a literal: {once:?}");
        assert!(!once.contains("0073"), "leaked an escape: {once:?}");
        assert!(
            !once.contains("E'") && !once.contains("$tag$") && !once.contains("$$"),
            "the prefix of the literal survived: {once:?}"
        );
        assert_eq!(redact_literals(&once), once, "not idempotent: {sql:?}");
        assert!(has_quoted_literal(sql), "a literal: {sql:?}");
    }
    // Not literal data, and not redacted: identifiers, placeholders, comments.
    for sql in [
        r#"SELECT "s3cr3t" FROM "t""#,
        r#"SELECT U&"s3cr3t" FROM "t""#,
        "SELECT $1, $?",
        "SELECT 1 -- 's3cr3t'",
        "SELECT 1 /* 's3cr3t' */",
    ] {
        assert_eq!(redact_literals(sql), sql, "left alone: {sql:?}");
        assert!(!has_quoted_literal(sql), "no literal: {sql:?}");
    }
}

#[test]
fn known_static_setup_sql_is_exempt_only_as_an_exact_match() {
    // The two protocol constants keep their literals, because a grant has to be
    // able to spell them.
    assert!(!KNOWN_STATIC_SETUP_SQL.is_empty());
    for sql in KNOWN_STATIC_SETUP_SQL {
        assert_eq!(redact_literals(sql), *sql);
        assert!(has_quoted_literal(sql), "{sql:?} still holds its literal");
    }
    // Anything that is not that byte-for-byte is user text and is redacted:
    // a second space, another case, a trailing space, or the constant carried
    // as a literal of its own.
    assert_eq!(redact_literals("SET  TIME ZONE 'UTC'"), "SET  TIME ZONE …");
    assert_eq!(redact_literals("set time zone 'UTC'"), "set time zone …");
    assert_eq!(redact_literals("SET TIME ZONE 'UTC' "), "SET TIME ZONE … ");
    assert_eq!(
        redact_literals("SELECT 'SET TIME ZONE ''UTC'''"),
        "SELECT …"
    );
    // The second protocol statement is exempt only as the text blastit itself
    // sends, `$?` included: the wire form with a numbered placeholder is user
    // text and loses its literal.
    assert_eq!(
        redact_literals("SELECT set_config('TimeZone', $?, false)"),
        "SELECT set_config('TimeZone', $?, false)"
    );
    assert_eq!(
        redact_literals("SELECT set_config('TimeZone', $1, false)"),
        format!("SELECT set_config({ELLIPSIS}, $1, false)")
    );
}

#[test]
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
// Protected twin (MB.1.12, AUDIT-MB.1 F1/F2): tests/critical_policy_surface.rs.
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
        assert_eq!(
            redact_literals(&redact_literals(&once)),
            redact_literals(&once)
        );
    }
    // ... and a run of a different width is not that statement.
    assert!(
        !allow
            .check("INSERT INTO \"n\" (\"owner_id\", \"text\") VALUES ($1, $2, $3), ($4, $5, $6) RETURNING \"n\".\"id\"")
            .is_allowed()
    );
}
