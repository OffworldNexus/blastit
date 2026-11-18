//! Protected critical tests for the `query` template validator (§13.4; R31–R33,
//! R36).
//!
//! A `query` is the one grant that carries SQL text, so the rewrite that decides
//! whether it parses is a contract: the ten rows of §13.4.2, the seven refusals
//! of §13.4.3 with their exact character offsets, and the promise that a refusal
//! never prints the statement. Every case here is the page's own text.
//!
//! The rewrite itself is asserted in `template.rs`'s own tests through the
//! crate-private pass both the production path and those tests run
//! (`expand_template`), so no lookalike expansion exists anywhere; this file is
//! the *public* oracle, and it pins the rewrite's observable consequences: a row
//! that is not SQL as written (it holds `$?` or `...`) is accepted anyway, and
//! the documented expansion is exactly one statement to PostgreSQL.
//!
//! The file is guarded by `tools/critical_guard.py`: do not weaken it.

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

/// The character offset `needle` starts at, which is the unit every refusal and
/// every site uses.
fn character_offset(sql: &str, needle: &str) -> u32 {
    let byte = sql.find(needle).unwrap_or_default();
    sql.get(..byte)
        .map_or(0, |head| head.chars().count() as u32)
}

//
// §13.4.2 — the expansion table, row for row.
//

/// `(written, what PostgreSQL is handed)`: the text and its expanded form, so a
/// row that stops expanding, or expands differently, reds here.
const REWRITE_ROWS: &[(&str, &str)] = &[
    // Row 1 — `$?` is the next `$N`, in statement order.
    (
        "SELECT id FROM products WHERE price > $?",
        "SELECT id FROM products WHERE price > $1",
    ),
    (
        "SELECT id FROM t WHERE a = $? AND b = $?",
        "SELECT id FROM t WHERE a = $1 AND b = $2",
    ),
    (
        "SELECT id FROM t WHERE a = $? AND b = $? AND c = $?",
        "SELECT id FROM t WHERE a = $1 AND b = $2 AND c = $3",
    ),
    // Row 2 — PostgreSQL's own numbering is legal as written.
    (
        "SELECT id FROM t WHERE a = $1 AND b = $2",
        "SELECT id FROM t WHERE a = $1 AND b = $2",
    ),
    // ... and it may be reused: the expanded text is a validation artefact.
    (
        "SELECT id FROM t WHERE a = $1 AND b = $?",
        "SELECT id FROM t WHERE a = $1 AND b = $1",
    ),
    // Row 3 — `IN ($?)` and `IN ($?, ...)` are one-element lists to the parser.
    (
        "SELECT id FROM t WHERE id IN ($?)",
        "SELECT id FROM t WHERE id IN ($1)",
    ),
    (
        "SELECT id FROM t WHERE id IN ($?, ...)",
        "SELECT id FROM t WHERE id IN ($1)",
    ),
    // A one-line grant written by `check` reads the same way.
    (
        "SELECT \"t\".\"id\" FROM \"t\" WHERE \"t\".\"id\" IN ($?, ...)",
        "SELECT \"t\".\"id\" FROM \"t\" WHERE \"t\".\"id\" IN ($1)",
    ),
    // The deleted span is exact: the whitespace and the comment between the
    // comma and the dots go with it, and nothing else does.
    (
        "SELECT id FROM t WHERE id IN ($?, /* ; */ ...)",
        "SELECT id FROM t WHERE id IN ($1)",
    ),
    // Row 4 — `NOT IN` is row 3's rule, and a hand-written `$N` may survive.
    (
        "SELECT id FROM t WHERE id NOT IN ($?, ...)",
        "SELECT id FROM t WHERE id NOT IN ($1)",
    ),
    (
        "SELECT id FROM t WHERE id NOT IN ($1, ...)",
        "SELECT id FROM t WHERE id NOT IN ($1)",
    ),
    // A list with no `...` is legal at any length, and needs no row of its own.
    (
        "SELECT id FROM t WHERE id IN ($?, $?)",
        "SELECT id FROM t WHERE id IN ($1, $2)",
    ),
    (
        "SELECT id FROM t WHERE id IN ($1, $2, $3)",
        "SELECT id FROM t WHERE id IN ($1, $2, $3)",
    ),
    // Row 5 — one `VALUES` row reaches the parser, whatever the row count.
    (
        "INSERT INTO t (a, b) VALUES ($?, $?)",
        "INSERT INTO t (a, b) VALUES ($1, $2)",
    ),
    (
        "INSERT INTO t (a, b) VALUES ($?, $?), ...",
        "INSERT INTO t (a, b) VALUES ($1, $2)",
    ),
    (
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
    ),
    (
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ...",
        "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
    ),
    (
        "INSERT INTO \"notes_note\" (\"owner_id\", \"text\") VALUES ($?, $?), ... \
         RETURNING \"notes_note\".\"id\"",
        "INSERT INTO \"notes_note\" (\"owner_id\", \"text\") VALUES ($1, $2) \
         RETURNING \"notes_note\".\"id\"",
    ),
    // Numbering is statement order, across every shape at once: a `$?` before
    // an `IN` list, the list's own placeholder, and one after it.
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
    ("SELECT 1 -- ...\n", "SELECT 1 -- ...\n"),
    ("SET TIME ZONE 'UTC'", "SET TIME ZONE 'UTC'"),
    (
        "SELECT set_config('TimeZone', $?, false)",
        "SELECT set_config('TimeZone', $1, false)",
    ),
    // A parse, not a policy: a capability template is accepted here and refused
    // by the literal or `--learn` gate, never by a second allowlist.
    ("SELECT pg_read_file($?)", "SELECT pg_read_file($1)"),
    ("SELECT dblink($?)", "SELECT dblink($1)"),
];

#[test]
fn every_rewrite_row_is_accepted_and_its_documented_expansion_parses() {
    for (written, expanded) in REWRITE_ROWS {
        assert_eq!(check_template(written), Ok(()), "for {written:?}");
        // The documented expansion is one statement to PostgreSQL ...
        match pg_query::parse(expanded) {
            Ok(parsed) => assert_eq!(
                parsed.protobuf.stmts.len(),
                1,
                "the documented expansion of {written:?} is not one statement"
            ),
            Err(error) => panic!("the documented expansion {expanded:?} is not SQL: {error}"),
        }
        // ... and where the row is rewritten, the text **as written** is not
        // SQL: PostgreSQL refuses `$?` and `...`, so an `Ok` from
        // `check_template` can only be the rewrite's doing. A row the rewrite
        // leaves alone has to parse both ways, which pins the other direction.
        if written == expanded {
            assert!(
                pg_query::parse(written).is_ok(),
                "{written:?} is unchanged by the rewrite, so it must be SQL as written"
            );
        } else {
            assert!(
                pg_query::parse(written).is_err(),
                "{written:?} is SQL as written, so accepting it proves no rewrite happened"
            );
        }
    }
}

#[test]
fn a_two_element_in_list_is_legal_and_a_second_placeholder_before_the_marker_is_not() {
    // The developer who deleted the `...` gets a parseable statement, and the
    // one who wrote two placeholders before it gets the sentence that names both
    // legal positions.
    assert_eq!(
        check_template("SELECT id FROM t WHERE id IN ($?, $?)"),
        Ok(())
    );
    assert!(
        pg_query::parse("SELECT id FROM t WHERE id IN ($?, $?)").is_err(),
        "the written form is not SQL, so the acceptance above is the rewrite's"
    );
    assert_eq!(
        refusal("SELECT id FROM t WHERE id IN ($?, $?, ...)"),
        (TemplateKind::StrayEllipsis, 38)
    );
}

//
// §13.4.2 rules 6–8 — a malformed text is refused as written, never rewritten.
//

#[test]
fn a_rewrite_never_swallows_a_separator_or_a_second_statement() {
    // A code `;` between the comma and the marker: the marker is not the last
    // item of an `IN` list, so the sentence is the ellipsis one — and the `;` is
    // still there for the statement count to find.
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
    // A `;` inside a quoted region is data, and one `;` in code at the end is
    // the page's allowed trailing separator.
    accepted("SELECT ';'");
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
    // A `$?` or a `...` inside a string, a quoted identifier or a comment is
    // data: the text parses as written once the one real placeholder is
    // numbered, so it is accepted and nothing else moves.
    assert_eq!(
        check_template("SELECT '$?', '...' FROM t WHERE a = $?"),
        Ok(())
    );
    assert_eq!(
        check_template("SELECT id FROM t WHERE id IN ($?, ...) -- $?"),
        Ok(())
    );
    // A `...` inside a string is not the marker: the statement is refused only
    // for what is genuinely wrong with it, never for its data.
    let data = "SELECT '...' FROM t WHERE a = ...";
    assert_eq!(
        refusal(data),
        (TemplateKind::StrayEllipsis, 30),
        "the code dots are the marker"
    );
    assert_eq!(
        character_offset(data, "..."),
        8,
        "the first dots are the string's, and they are data"
    );
}

#[test]
fn a_qualified_in_or_values_is_not_the_keyword() {
    // `s.in(…)` is the function `in` of the schema `s`: its argument list is an
    // argument list, so a marker inside it is stray — even a placeholder list,
    // which `normalize` never collapses for a qualified name.
    assert_eq!(
        refusal("SELECT s.in (1, ...) FROM t"),
        (TemplateKind::StrayEllipsis, 16)
    );
    assert_eq!(
        refusal("SELECT s.in ($?, ...) FROM t"),
        (TemplateKind::StrayEllipsis, 17)
    );
    assert_eq!(
        refusal("SELECT f($?, ...) FROM t"),
        (TemplateKind::StrayEllipsis, 13)
    );
    // A two-letter word is not `in`: the whole token is compared.
    let two = "SELECT id FROM t WHERE fn ($?, ...)";
    assert_eq!(
        refusal(two),
        (TemplateKind::StrayEllipsis, character_offset(two, "..."))
    );
    assert_eq!(
        refusal("SELECT s.values (1), ... FROM t"),
        (TemplateKind::StrayEllipsis, 21)
    );
}

#[test]
fn the_first_stray_marker_in_the_text_is_the_one_reported() {
    let first = "SELECT ... FROM t WHERE a = ...";
    assert_eq!(
        refusal(first),
        (TemplateKind::StrayEllipsis, character_offset(first, "..."))
    );
    // A marker settled when its list closes is still reported at the earlier
    // character when another follows it.
    let nested = "SELECT id FROM t WHERE s.in ($?, ...) AND a = ...";
    assert_eq!(
        refusal(nested),
        (TemplateKind::StrayEllipsis, character_offset(nested, "..."))
    );
}

#[test]
fn a_longer_run_of_dots_is_a_stray_marker_too() {
    for sql in [
        "SELECT id FROM ....",
        "SELECT id FROM t WHERE id IN ($?, ....)",
        "VALUES (1), ....",
    ] {
        assert_eq!(refusal(sql).0, TemplateKind::StrayEllipsis, "for {sql:?}");
    }
}

#[test]
fn an_unterminated_group_holds_no_legal_marker() {
    assert_eq!(
        refusal("SELECT id FROM t WHERE id IN ($?, ..."),
        (TemplateKind::StrayEllipsis, 34)
    );
}

//
// §13.4.3 — the refusals, each with its offset.
//

#[test]
fn an_empty_query_is_refused_at_zero() {
    for sql in [
        "",
        "   ",
        " -- x",
        "-- x",
        "  ;  ",
        "\t\n\r",
        "/* nothing */",
        " ; ",
    ] {
        assert_eq!(refusal(sql), (TemplateKind::Empty, 0), "for {sql:?}");
    }
}

#[test]
fn a_second_statement_is_refused_at_its_first_character() {
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
    // A leading separator starts a second statement at the first code
    // character after it, and a trailing one is legal.
    assert_eq!(refusal("; SELECT 1"), (TemplateKind::MultipleStatements, 2));
    accepted("SELECT 1;");
}

#[test]
fn a_stray_marker_is_refused_at_its_first_dot() {
    assert_eq!(
        refusal("SELECT id FROM ..."),
        (TemplateKind::StrayEllipsis, 15)
    );
    assert_eq!(refusal("..."), (TemplateKind::StrayEllipsis, 0));
    let sql = "SELECT id FROM t WHERE a = 1 AND b = '...' OR c = ...";
    assert_eq!(
        refusal(sql),
        (
            TemplateKind::StrayEllipsis,
            character_offset(sql, "c = ...") + 4
        )
    );
}

#[test]
fn a_parse_error_is_refused_at_the_first_non_trivia_code_character() {
    assert_eq!(refusal("SELCT id FROM t"), (TemplateKind::Syntax, 0));
    assert_eq!(refusal("SELECT id FROM"), (TemplateKind::Syntax, 0));
    assert_eq!(refusal("SELECT id FROM t WHERE"), (TemplateKind::Syntax, 0));
    assert_eq!(refusal("SELECT ) FROM t"), (TemplateKind::Syntax, 0));
    // Space, a newline and a comment are trivia, so the squiggle lands on the
    // statement's first token, not on the whitespace in front of it.
    assert_eq!(refusal("\nSELECT id FROM"), (TemplateKind::Syntax, 1));
    assert_eq!(
        refusal("/* c */ SELCT id FROM t"),
        (TemplateKind::Syntax, 8)
    );
    let sql = "\n\n\t/* c */\n  SELCT 1";
    assert_eq!(
        refusal(sql),
        (TemplateKind::Syntax, character_offset(sql, "SELCT"))
    );
}

#[test]
fn a_statement_over_the_cap_is_refused_not_grown() {
    let filler = MAX_TEMPLATE_CHARS - "-- \nSELECT 1".chars().count();
    let at_cap = format!("-- {}\nSELECT 1", "é".repeat(filler));
    assert_eq!(at_cap.chars().count(), MAX_TEMPLATE_CHARS);
    // Characters, not bytes: this text is at the cap in characters and over it
    // in bytes, and it is accepted — so the red side of this assertion is
    // `Err(TemplateError { kind: TooLong, .. })`, not a silent pass.
    assert!(at_cap.len() > MAX_TEMPLATE_CHARS);
    assert_eq!(check_template(&at_cap), Ok(()));

    let over = format!("-- {}\nSELECT 1", "é".repeat(filler + 1));
    assert_eq!(over.chars().count(), MAX_TEMPLATE_CHARS + 1);
    assert_eq!(refusal(&over), (TemplateKind::TooLong, 0));
}

#[test]
fn the_1025th_placeholder_is_refused_at_its_own_position() {
    let mut parameters = String::from("SELECT ");
    for index in 0..MAX_TEMPLATE_PARAMS {
        if index > 0 {
            parameters.push_str(", ");
        }
        parameters.push_str("$?");
    }
    assert_eq!(check_template(&parameters), Ok(()));
    assert_eq!(
        parameters.matches("$?").count(),
        MAX_TEMPLATE_PARAMS as usize
    );

    let offset = parameters.chars().count() as u32 + 2;
    let over = format!("{parameters}, $?");
    assert_eq!(refusal(&over), (TemplateKind::TooManyParameters, offset));
    assert_eq!(char_offset_is(&over, offset), "$?");
}

/// The two characters at `offset`, as a string, to show the site is the `$?`.
fn char_offset_is(sql: &str, offset: u32) -> String {
    sql.chars()
        .skip(offset as usize)
        .take(2)
        .collect::<String>()
}

#[test]
fn the_file_wide_refusal_is_constructible_and_says_the_pages_sentence() {
    let error = TemplateError::too_many_queries();
    assert_eq!(error.kind(), TemplateKind::TooManyQueries);
    assert_eq!(error.offset(), 0);
    assert_eq!(
        error.to_string(),
        "a `Blastfile` validates 4096 `query` entries at most; fold the shared statements into a \
         `trait` that many views `use`"
    );
    assert_eq!(MAX_TEMPLATE_PARSES, 4_096);
}

#[test]
fn every_sentence_is_the_reference_pages_and_one_line() {
    let kinds = [
        TemplateKind::Empty,
        TemplateKind::MultipleStatements,
        TemplateKind::StrayEllipsis,
        TemplateKind::Syntax,
        TemplateKind::TooLong,
        TemplateKind::TooManyParameters,
        TemplateKind::TooManyQueries,
    ];
    let expected = [
        "this `query` is empty; write the one statement it grants",
        "a `query` is one statement; this one starts a second",
        "`...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row",
        "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`",
        "a `query` is 65536 characters at most; split it, or grant the tables and columns instead",
        "a `query` holds 1024 bind placeholders at most; grant the tables and columns instead",
        "a `Blastfile` validates 4096 `query` entries at most; fold the shared statements into a \
         `trait` that many views `use`",
    ];
    assert_eq!(kinds.len(), expected.len());
    for (kind, sentence) in kinds.iter().zip(expected) {
        assert_eq!(kind.sentence(), sentence, "for {kind:?}");
        assert_eq!(kind.to_string(), sentence, "for {kind:?}");
        assert!(!sentence.contains('\n'), "one line only: {sentence:?}");
    }
}

//
// §13.4.4 — the statement is never printed.
//

#[test]
fn a_refusal_never_renders_the_statement() {
    // The parse message quotes the token it stopped at — `syntax error at or
    // near "…"` — and a `query` may hold a value, so the sentence is fixed and
    // the message is classification only.
    //
    // The strongest case first: an unterminated literal is exactly where
    // PostgreSQL quotes the **value** itself (`unterminated quoted string at or
    // near "'a@b.com"`), so a refusal that rendered the message would print the
    // address the author was trying to bind.
    let unterminated = "SELECT id FROM t WHERE email = 'a@b.com";
    let error = match check_template(unterminated) {
        Ok(()) => panic!("{unterminated:?} should have been refused"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), TemplateKind::Syntax);
    assert_eq!(error.to_string(), TemplateKind::Syntax.sentence());
    for needle in ["a@b.com", "b.com", "@"] {
        assert!(
            !error.to_string().contains(needle),
            "the refusal printed {needle:?}: {error}"
        );
        assert!(!format!("{error:?}").contains(needle), "leaked {needle:?}");
    }

    // A statement that stops at the end of its input is the same sentence.
    let sql = "SELECT id FROM t WHERE email = 'a@b.com' AND";
    let error = match check_template(sql) {
        Ok(()) => panic!("{sql:?} should have been refused"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), TemplateKind::Syntax);
    assert_eq!(error.to_string(), TemplateKind::Syntax.sentence());
    for needle in [
        "a@b.com",
        "b.com",
        "@",
        "email",
        "SELECT",
        "syntax",
        "near",
        "end of input",
    ] {
        assert!(
            !error.to_string().contains(needle),
            "the refusal printed {needle:?}: {error}"
        );
        // `Debug` too: a failed assertion renders it.
        assert!(
            !format!("{error:?}").contains(needle),
            "the Debug form printed {needle:?}"
        );
    }

    // A bare token that is not a literal is quoted by PostgreSQL just the same.
    let error = match check_template("SELECT s3cr3t FROM") {
        Ok(()) => panic!("the statement should have been refused"),
        Err(error) => error,
    };
    assert!(!error.to_string().contains("s3cr3t"));
    assert!(!format!("{error:?}").contains("s3cr3t"));
}

//
// §13.4.5 — offsets are characters.
//

#[test]
fn offsets_are_counted_in_characters_and_the_byte_distance_is_asserted() {
    let sql = "SELECT \"café\", ... FROM t";
    assert_eq!(refusal(sql), (TemplateKind::StrayEllipsis, 15));
    let byte = sql.find("...").unwrap_or_default();
    let character = sql.get(..byte).map_or(0, |head| head.chars().count());
    assert_eq!(character, 15);
    assert_eq!(byte, 16);
    assert_eq!(byte - character, 1, "`é` costs one character and two bytes");
    // The identifier itself is ordinary SQL.
    accepted("SELECT name FROM \"café\"");
    accepted("SELECT id FROM t WHERE name = 'héllo' OR id = $?");
}

//
// The property: every accepted fixture is exactly one statement to PostgreSQL,
// through the shipped rewrite.
//

const ACCEPTED_FIXTURES: &[&str] = &[
    "SELECT 1",
    "SELECT 1;",
    // One code token is a statement, not an empty `query`.
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "SELECT id FROM products WHERE price > $?",
    "SELECT id FROM products WHERE email = $? AND id > $?",
    "SELECT id FROM t WHERE a = $1 AND b = $2",
    "SELECT id FROM t WHERE a = $1 AND b = $?",
    "SELECT id FROM t WHERE id IN ($?)",
    "SELECT id FROM t WHERE id IN ($?, ...)",
    "SELECT id FROM t WHERE id NOT IN ($?, ...)",
    "SELECT id FROM t WHERE id IN ($1, $2, $3)",
    "SELECT id FROM t WHERE id IN ($?, $?)",
    "INSERT INTO t (a, b) VALUES ($?, $?)",
    "INSERT INTO t (a, b) VALUES ($?, $?), ...",
    "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4)",
    "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ...",
    "UPDATE t SET a = $? WHERE b = $?",
    "DELETE FROM t WHERE a = $? RETURNING id",
    "SELECT \"t\".\"id\" FROM \"t\" WHERE \"t\".\"id\" IN ($?, ...)",
    "SELECT '$?', '$' FROM t WHERE a = $?",
    "SELECT 1 -- a comment with a ; in it\n",
    "SET TIME ZONE 'UTC'",
    "SELECT set_config('TimeZone', $?, false)",
    "SELECT pg_read_file($?)",
    "SELECT dblink($?)",
    "SELECT name FROM \"café\"",
    "SELECT $? AS a, (SELECT 1 FROM u WHERE u.id IN ($?, ...)) AS b, $? AS c",
];

#[test]
fn every_accepted_fixture_is_accepted() {
    // The same list `template.rs`'s unit tests run the one-statement property
    // over: a shape that stops being accepted reds in both tiers.
    for sql in ACCEPTED_FIXTURES {
        assert_eq!(check_template(sql), Ok(()), "for {sql:?}");
    }
}

//
// R36 — `parameterize`, the Quick Fix's one rewrite.
//

#[test]
fn the_quick_fix_rewrite_is_canonical() {
    // A quoted literal in the `"…"` spelling, which is the only spelling a
    // statement holding a `'` can have.
    let sql = "SELECT id FROM t WHERE email = 'a@b.com'";
    let new = match parameterize(sql) {
        Some(new) => new,
        None => panic!("{sql:?} should have been parameterised"),
    };
    assert_eq!(new, "SELECT id FROM t WHERE email = $?");
    assert!(!new.contains('@'), "the value survived: {new:?}");
    accepted(&new);

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
        (
            "SELECT E'x' FROM t WHERE a = 1",
            "SELECT $? FROM t WHERE a = 1",
        ),
    ] {
        assert_eq!(
            parameterize(written),
            Some(expected.to_string()),
            "for {written:?}"
        );
        accepted(expected);
    }

    // Idempotent: the canonical text has nothing left to change, so a second
    // apply offers no edit.
    for canonical in [
        "SELECT id FROM t WHERE email = $?",
        "SELECT id FROM t WHERE id IN ($?, ...)",
        "SELECT id FROM t WHERE id = $?",
    ] {
        assert_eq!(parameterize(canonical), None, "for {canonical:?}");
        accepted(canonical);
    }
}

#[test]
fn the_quick_fix_is_never_offered_where_it_would_widen_a_capability() {
    // `--learn` refuses these, and the Quick Fix is a grant producer: none of
    // them may become a `$?` grant.
    for sql in [
        "SELECT pg_read_file('/etc/passwd')",
        "SELECT pg_read_file('/etc/passwd', 0, 100)",
        "SELECT lo_import('/etc/shadow')",
        "SELECT dblink('host=attacker.example', 'select 1')",
        "COPY t FROM PROGRAM 'sh'",
        // A paste that would not re-parse clean.
        "SELECT 'x'; SELECT 2",
        "SELCT 'x'",
        "DO $$BEGIN NULL; END$$",
        // Nothing to parameterise: a redacted `…`, a clean query with no
        // placeholder, and the two protocol statements that keep their literals.
        "SELECT id FROM t WHERE id = …",
        "SELECT id FROM t WHERE id = $?",
        "SELECT id FROM t",
        "SET TIME ZONE 'UTC'",
        "SELECT set_config('TimeZone', $?, false)",
    ] {
        assert_eq!(parameterize(sql), None, "for {sql:?}");
    }

    // Over the cap: refused before any allocation.
    let long = format!("SELECT '{}'", "x".repeat(MAX_TEMPLATE_CHARS));
    assert_eq!(parameterize(&long), None);
}

#[test]
fn the_quick_fix_is_offered_at_the_cap_and_refused_one_character_over_it() {
    // The boundary is the page's: 65536 characters is a `query`, 65537 is not.
    let filler = MAX_TEMPLATE_CHARS - "-- \nSELECT 'x'".chars().count();
    let at_cap = format!("-- {}\nSELECT 'x'", "x".repeat(filler));
    assert_eq!(at_cap.chars().count(), MAX_TEMPLATE_CHARS);
    assert_eq!(parameterize(&at_cap), Some("SELECT $?".to_string()));

    let over = format!("-- {}\nSELECT 'x'", "x".repeat(filler + 1));
    assert_eq!(over.chars().count(), MAX_TEMPLATE_CHARS + 1);
    assert_eq!(parameterize(&over), None);
}

#[test]
fn every_quick_fix_result_passes_the_template_check() {
    // `Some` is asserted, never skipped: a regression that stopped offering the
    // fix for one of these shapes has to red here, not vanish into an `if let`.
    for sql in [
        "SELECT id FROM t WHERE email = 'a@b.com'",
        "SELECT upper('x') FROM t",
        "SELECT id FROM t WHERE id = $1",
        "SELECT id FROM t WHERE id IN ($1, $2)",
        "INSERT INTO t (a) VALUES ($1), ($2)",
        "SET TIME ZONE 'Europe/Paris'",
    ] {
        let new = match parameterize(sql) {
            Some(new) => new,
            None => panic!("parameterize refused {sql:?}, so the Quick Fix vanished"),
        };
        assert_eq!(check_template(&new), Ok(()), "for {sql:?} -> {new:?}");
        // ... and the canonical text is what matching compares.
        assert_eq!(normalize(&new), new, "for {sql:?}");
    }
}

//
// `literal_offset` — the site `LiteralQuery` points its squiggle at.
//

#[test]
fn the_literal_offset_is_the_whole_token_including_its_prefix() {
    for (sql, offset) in [
        ("SELECT E'x'", 7),
        ("SELECT B'1'", 7),
        ("SELECT X'ff'", 7),
        ("SELECT N'x'", 7),
        (r"SELECT U&'\0041'", 7),
        ("SELECT $$x$$", 7),
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
