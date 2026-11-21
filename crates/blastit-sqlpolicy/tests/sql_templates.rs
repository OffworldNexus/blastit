//! §7 "SQL grant matching & parameterization", the query-template half: the
//! `$?`/`...` rewrite that decides whether a grant's text parses, the seven
//! refusals and their character offsets, the Quick Fix's one rewrite, and the
//! literal site it points at.
//!
//! The rewrite is the contract a `query` grant rests on, so each of its rows is
//! asserted twice: the written text is accepted *and* the documented expansion
//! is exactly one statement to PostgreSQL (and, wherever the rewrite changed
//! anything, the text as written is not SQL — so an `Ok` can only be the
//! rewrite's doing).
//!
//! The matching/table-grant half of §7 lives beside this file in
//! `sql_policy.rs`; `RESULT.md` holds the case map.

use blastit_sqlpolicy::{
    MAX_TEMPLATE_CHARS, MAX_TEMPLATE_PARAMS, MAX_TEMPLATE_PARSES, TemplateError, TemplateKind,
    check_template, literal_offset, normalize, parameterize, redact_literals,
};

/// The refusal `sql` earns, as its kind and its character offset.
fn refusal(sql: &str) -> (TemplateKind, u32) {
    match check_template(sql) {
        Ok(()) => panic!("{sql:?} should have been refused"),
        Err(error) => (error.kind(), error.offset()),
    }
}

/// Asserts that `sql` is accepted, naming the refusal when it is not.
fn accepted(sql: &str) {
    if let Err(error) = check_template(sql) {
        panic!(
            "{sql:?} should be accepted, refused as {:?} at {}",
            error.kind(),
            error.offset()
        );
    }
}

/// The character offset `needle` starts at, which is the unit every refusal uses.
fn character_offset(sql: &str, needle: &str) -> u32 {
    sql.find(needle)
        .and_then(|byte| sql.get(..byte))
        .map_or(0, |head| head.chars().count() as u32)
}

//
// §13.4.2 — the expansion table.
//

/// `(written, what PostgreSQL is handed)`.
const REWRITE_ROWS: &[(&str, &str)] = &[
    // `$?` is the next `$N`, in statement order.
    (
        "SELECT id FROM t WHERE a = $? AND b = $?",
        "SELECT id FROM t WHERE a = $1 AND b = $2",
    ),
    // PostgreSQL's own numbering is legal as written, and may be reused.
    (
        "SELECT id FROM t WHERE a = $1 AND b = $?",
        "SELECT id FROM t WHERE a = $1 AND b = $1",
    ),
    // `IN ($?)` and `IN ($?, ...)` are one-element lists to the parser.
    (
        "SELECT id FROM t WHERE id IN ($?)",
        "SELECT id FROM t WHERE id IN ($1)",
    ),
    (
        "SELECT id FROM t WHERE id NOT IN ($?, ...)",
        "SELECT id FROM t WHERE id NOT IN ($1)",
    ),
    (
        "SELECT id FROM t WHERE id IN ($?, $?)",
        "SELECT id FROM t WHERE id IN ($1, $2)",
    ),
    // One `VALUES` row reaches the parser, whatever the row count.
    (
        "INSERT INTO t (a, b) VALUES ($?, $?), ...",
        "INSERT INTO t (a, b) VALUES ($1, $2)",
    ),
    (
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ...",
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
    ),
    // Numbering is statement order across every shape at once.
    (
        "SELECT $? AS a, (SELECT 1 FROM u WHERE u.id IN ($?, ...)) AS b, $? AS c",
        "SELECT $1 AS a, (SELECT 1 FROM u WHERE u.id IN ($2)) AS b, $3 AS c",
    ),
    // Nothing else moves: a quoted region keeps its `$?`, its `;` and its
    // spacing, and a trailing separator stays.
    (
        "SELECT '$? and  spaces', ';' FROM t WHERE a = $?",
        "SELECT '$? and  spaces', ';' FROM t WHERE a = $1",
    ),
    ("SELECT 1;", "SELECT 1;"),
    ("SET TIME ZONE 'UTC'", "SET TIME ZONE 'UTC'"),
];

#[test]
fn every_documented_rewrite_is_accepted_and_expands_to_one_statement() {
    for (written, expanded) in REWRITE_ROWS {
        assert_eq!(check_template(written), Ok(()), "for {written:?}");
        match pg_query::parse(expanded) {
            Ok(parsed) => assert_eq!(
                parsed.protobuf.stmts.len(),
                1,
                "the documented expansion of {written:?} is not one statement"
            ),
            Err(error) => panic!("the documented expansion {expanded:?} is not SQL: {error}"),
        }
        // A row the rewrite leaves alone has to parse both ways; a row it
        // rewrites must not parse as written.
        if written == expanded {
            assert!(pg_query::parse(written).is_ok(), "{written:?}");
        } else {
            assert!(
                pg_query::parse(written).is_err(),
                "{written:?} is SQL as written, so accepting it proves no rewrite"
            );
        }
    }
}

#[test]
fn a_rewrite_never_swallows_a_separator_or_a_statement() {
    // A code `;` between the comma and the marker is not the last item of an
    // `IN` list, so the sentence is the ellipsis one.
    assert_eq!(
        refusal("SELECT id FROM t WHERE id IN ($?, ; ...)"),
        (TemplateKind::StrayEllipsis, 36)
    );
    // A `;` hidden in a subquery inside a repeated `VALUES` row is two
    // statements, at the first code character after it.
    let sql = "INSERT INTO t (a) VALUES ((SELECT 1; SELECT 2)), ...";
    assert_eq!(
        refusal(sql),
        (
            TemplateKind::MultipleStatements,
            character_offset(sql, "SELECT 2")
        )
    );
    // A `;` in a quoted region is data, and one trailing separator is allowed.
    accepted("SELECT ';';");
    let sql = "SELECT ';'; SELECT 2";
    assert_eq!(
        refusal(sql),
        (
            TemplateKind::MultipleStatements,
            character_offset(sql, "SELECT 2")
        )
    );
}

#[test]
fn a_marker_inside_a_string_or_a_comment_is_not_a_marker() {
    assert_eq!(
        check_template("SELECT '$?', '...' FROM t WHERE a = $?"),
        Ok(())
    );
    assert_eq!(
        check_template("SELECT id FROM t WHERE id IN ($?, ...) -- $?"),
        Ok(())
    );
    // A `...` inside a string is data, so the refusal names the code dots.
    let data = "SELECT '...' FROM t WHERE a = ...";
    assert_eq!(refusal(data), (TemplateKind::StrayEllipsis, 30));
    assert_eq!(character_offset(data, "..."), 8);
    // A longer run of dots, an unterminated group and the first stray marker in
    // the text all earn the same sentence.
    for sql in [
        "SELECT id FROM ....",
        "SELECT id FROM t WHERE id IN ($?, ....)",
    ] {
        assert_eq!(refusal(sql).0, TemplateKind::StrayEllipsis, "{sql:?}");
    }
    assert_eq!(
        refusal("SELECT id FROM t WHERE id IN ($?, ..."),
        (TemplateKind::StrayEllipsis, 34)
    );
    let nested = "SELECT id FROM t WHERE s.in ($?, ...) AND a = ...";
    assert_eq!(
        refusal(nested),
        (TemplateKind::StrayEllipsis, character_offset(nested, "..."))
    );
}

#[test]
fn a_qualified_in_or_values_is_not_the_keyword() {
    assert_eq!(
        refusal("SELECT s.in ($?, ...) FROM t"),
        (TemplateKind::StrayEllipsis, 17)
    );
    assert_eq!(
        refusal("SELECT f($?, ...) FROM t"),
        (TemplateKind::StrayEllipsis, 13)
    );
    // A two-letter word is not `in`; the whole token is compared.
    let two = "SELECT id FROM t WHERE fn ($?, ...)";
    assert_eq!(
        refusal(two),
        (TemplateKind::StrayEllipsis, character_offset(two, "..."))
    );
    // Two placeholders before the marker name both legal positions.
    assert_eq!(
        refusal("SELECT id FROM t WHERE id IN ($?, $?, ...)"),
        (TemplateKind::StrayEllipsis, 38)
    );
    accepted("SELECT id FROM t WHERE id IN ($?, $?)");
}

//
// §13.4.3 — the refusals, each with its offset; §13.4.5 — offsets are characters.
//

#[test]
fn a_refusal_carries_its_kind_and_its_character_offset() {
    for sql in ["", "   ", " -- x", "  ;  ", "/* nothing */"] {
        assert_eq!(refusal(sql), (TemplateKind::Empty, 0), "for {sql:?}");
    }
    assert_eq!(
        refusal("SELECT 1; SELECT 2"),
        (TemplateKind::MultipleStatements, 10)
    );
    let sql = "SELECT 1;\n  SELECT 2";
    assert_eq!(
        refusal(sql),
        (
            TemplateKind::MultipleStatements,
            character_offset(sql, "SELECT 2")
        )
    );
    assert_eq!(refusal("; SELECT 1"), (TemplateKind::MultipleStatements, 2));
    // A parse error lands on the first non-trivia code character.
    assert_eq!(refusal("SELCT id FROM t"), (TemplateKind::Syntax, 0));
    assert_eq!(refusal("\nSELECT id FROM"), (TemplateKind::Syntax, 1));
    assert_eq!(
        refusal("/* c */ SELCT id FROM t"),
        (TemplateKind::Syntax, 8)
    );
    // A non-ASCII identifier costs one character and two bytes, and the offset
    // is the character.
    let sql = "SELECT \"café\", ... FROM t";
    assert_eq!(refusal(sql), (TemplateKind::StrayEllipsis, 15));
    assert_eq!(sql.find("..."), Some(16));
    accepted("SELECT name FROM \"café\"");
}

#[test]
fn the_caps_refuse_rather_than_grow() {
    let filler = MAX_TEMPLATE_CHARS - "-- \nSELECT 1".chars().count();
    let at_cap = format!("-- {}\nSELECT 1", "é".repeat(filler));
    assert_eq!(at_cap.chars().count(), MAX_TEMPLATE_CHARS);
    assert!(at_cap.len() > MAX_TEMPLATE_CHARS, "characters, not bytes");
    assert_eq!(check_template(&at_cap), Ok(()));
    let over = format!("-- {}\nSELECT 1", "é".repeat(filler + 1));
    assert_eq!(refusal(&over), (TemplateKind::TooLong, 0));

    // The 1025th placeholder is refused at its own position.
    let mut parameters = String::from("SELECT ");
    for index in 0..MAX_TEMPLATE_PARAMS {
        if index > 0 {
            parameters.push_str(", ");
        }
        parameters.push_str("$?");
    }
    assert_eq!(check_template(&parameters), Ok(()));
    let offset = parameters.chars().count() as u32 + 2;
    let over = format!("{parameters}, $?");
    assert_eq!(refusal(&over), (TemplateKind::TooManyParameters, offset));
    assert_eq!(
        over.chars()
            .skip(offset as usize)
            .take(2)
            .collect::<String>(),
        "$?"
    );

    // The file-wide refusal names the trait that fixes it.
    let error = TemplateError::too_many_queries();
    assert_eq!(error.kind(), TemplateKind::TooManyQueries);
    assert_eq!(error.offset(), 0);
    assert!(
        error
            .to_string()
            .contains("fold the shared statements into a `trait`")
    );
    assert_eq!(MAX_TEMPLATE_PARSES, 4_096);
    // Every sentence is one line.
    for kind in [
        TemplateKind::Empty,
        TemplateKind::MultipleStatements,
        TemplateKind::StrayEllipsis,
        TemplateKind::Syntax,
        TemplateKind::TooLong,
        TemplateKind::TooManyParameters,
        TemplateKind::TooManyQueries,
    ] {
        let sentence = kind.sentence();
        assert_eq!(sentence, kind.to_string(), "{kind:?}");
        assert!(!sentence.contains('\n'), "one line only: {sentence:?}");
    }
}

#[test]
fn a_refusal_never_renders_the_statement() {
    // An unterminated literal is exactly where PostgreSQL quotes the value, so
    // a refusal that rendered the message would print the bound address.
    let unterminated = "SELECT id FROM t WHERE email = 'a@b.com";
    let error = match check_template(unterminated) {
        Ok(()) => panic!("{unterminated:?} should have been refused"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), TemplateKind::Syntax);
    assert_eq!(error.to_string(), TemplateKind::Syntax.sentence());
    for needle in ["a@b.com", "@", "syntax", "near"] {
        assert!(!error.to_string().contains(needle), "{error}");
        assert!(!format!("{error:?}").contains(needle), "{error:?}");
    }
}

//
// R36 — `parameterize`, the Quick Fix's one rewrite, and `literal_offset`.
//

#[test]
fn the_quick_fix_is_canonical_and_idempotent() {
    let sql = "SELECT id FROM t WHERE email = 'a@b.com'";
    assert_eq!(
        parameterize(sql),
        Some("SELECT id FROM t WHERE email = $?".to_string())
    );
    for (written, expected) in [
        ("SELECT upper('x') FROM t", "SELECT upper($?) FROM t"),
        (
            "SELECT id FROM t WHERE a = $1",
            "SELECT id FROM t WHERE a = $?",
        ),
        (
            "SELECT id FROM t WHERE id IN ($1, $2, $3)",
            "SELECT id FROM t WHERE id IN ($?, ...)",
        ),
        (
            "INSERT INTO t (a) VALUES ($1), ($2)",
            "INSERT INTO t (a) VALUES ($?), ...",
        ),
    ] {
        assert_eq!(
            parameterize(written),
            Some(expected.to_string()),
            "for {written:?}"
        );
        // Every result passes the template check and is already normalised.
        assert_eq!(check_template(expected), Ok(()), "for {expected:?}");
        assert_eq!(normalize(expected), expected, "for {expected:?}");
    }
    // Idempotent: the canonical text has nothing left to change.
    for canonical in [
        "SELECT id FROM t WHERE email = $?",
        "SELECT id FROM t WHERE id IN ($?, ...)",
    ] {
        assert_eq!(parameterize(canonical), None, "for {canonical:?}");
    }
}

#[test]
fn the_quick_fix_is_never_offered_where_it_would_widen_a_capability() {
    for sql in [
        "SELECT pg_read_file('/etc/passwd')",
        "SELECT lo_import('/etc/shadow')",
        "SELECT dblink('host=attacker.example', 'select 1')",
        "COPY t FROM PROGRAM 'sh'",
        // A paste that would not re-parse clean.
        "SELECT 'x'; SELECT 2",
        "SELCT 'x'",
        // Nothing to parameterise: a redacted `…`, a clean query, and the two
        // protocol statements that keep their literals.
        "SELECT id FROM t WHERE id = …",
        "SELECT id FROM t WHERE id = $?",
        "SET TIME ZONE 'UTC'",
    ] {
        assert_eq!(parameterize(sql), None, "for {sql:?}");
    }
    // The cap is the page's: 65536 characters is a `query`, 65537 is not.
    let filler = MAX_TEMPLATE_CHARS - "-- \nSELECT 'x'".chars().count();
    let at_cap = format!("-- {}\nSELECT 'x'", "x".repeat(filler));
    assert_eq!(parameterize(&at_cap), Some("SELECT $?".to_string()));
    let over = format!("-- {}\nSELECT 'x'", "x".repeat(filler + 1));
    assert_eq!(parameterize(&over), None);
}

#[test]
fn the_literal_offset_is_the_whole_token_including_its_prefix() {
    for (sql, offset) in [
        ("SELECT E'x'", 7),
        ("SELECT B'1'", 7),
        ("SELECT U&'\\0041'", 7),
        ("SELECT $tag$x$tag$", 7),
        ("SELECT 'x'", 7),
    ] {
        assert_eq!(literal_offset(sql), Some(offset), "for {sql:?}");
        // The same character `redact_literals` replaces.
        assert_eq!(redact_literals(sql), "SELECT …", "for {sql:?}");
    }
    for sql in [
        "SELECT id FROM t",
        "SELECT \"a\" FROM \"t\"",
        "SELECT $1 FROM t -- 'x'",
    ] {
        assert_eq!(literal_offset(sql), None, "for {sql:?}");
    }
}
