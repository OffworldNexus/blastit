//! Protected critical suite for a `query`'s span (`R34`, `R35`, §13.4.5).
//!
//! A `query` is a string whose decoded text is not the file's text, so the
//! position of a refusal inside it is arithmetic nobody can eyeball: the origin
//! is the first character **inside** the opening quote, a `"…"` escape costs two
//! source columns for one decoded character, a raw newline moves to the next
//! line, and a `\n` *escape* does not. The four cases below are the contract
//! (§18.1's T1 table): a regression in that arithmetic has to fail here rather
//! than surface as a squiggle one column off in an editor.
//!
//! Every expectation is a `Position` computed by hand from the fixture, never a
//! value read back from the code: the point of the suite is to disagree.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use blastit_manifest::{
    Grant, Manifest, ManifestError, ManifestErrors, PgGrant, Position, ProxyName, Route, UidLayout,
};
use blastit_sqlpolicy::TemplateKind;

/// One `view` with one `postgres` grant whose lines are `grants`.
fn route_with_queries(grants: &str) -> String {
    format!(
        "postgres $DATABASE_URL\n\n\
         view notes \"/notes/\" {{\n    postgres {{\n        {grants}\n    }}\n}}\n"
    )
}

/// The one grant the fixtures below write, sites included.
fn grant_of(source: &str) -> PgGrant {
    let manifest = Manifest::from_blastfile_str(source)
        .expect("the file parses; a `query` is refused in validation, not in lowering");
    manifest
        .route(0)
        .expect("route 0")
        .grant(&ProxyName::from("db"))
        .expect("a grant on `db`")
        .as_postgresql()
        .expect("a postgresql grant")
        .clone()
}

/// The refusals `blastit check` reports for the fixture.
fn errors_of(source: &str) -> ManifestErrors {
    Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE).expect_err("the fixture is refused")
}

#[test]
fn the_origin_is_the_first_character_inside_the_quote() {
    // Five comment lines put the first `query` on line 9, eight spaces in: the
    // design's own site for this case.
    let header = "postgres $DATABASE_URL\n\
                  # one\n\
                  # two\n\
                  # three\n\
                  # four\n\
                  # five\n\
                  view notes \"/notes/\" {\n\
                      postgres {\n";
    let empty = format!("{header}        query ''\n    }}\n}}\n");

    // `query ''`: the origin is the **closing** quote — the empty statement's
    // only honest site — and `Position::at(9, 16)` is that quote's column.
    let grant = grant_of(&empty);
    assert_eq!(grant.queries(), [""], "one empty statement, one entry");
    assert_eq!(grant.query_sites().len(), 1, "one entry, one site");
    let site = &grant.query_sites()[0];
    assert_eq!(
        site.at,
        Position::at(9, 16),
        "the origin is the closing quote"
    );
    assert_eq!(site.raw, "", "the empty statement's decoded text");
    assert!(site.escapes.is_empty(), "a `'…'` string has no escapes");
    assert!(
        site.interior.is_empty(),
        "the interior of `''` is the empty range at the closing quote"
    );
    assert_eq!(
        empty.as_bytes().get(site.interior.start as usize),
        Some(&b'\''),
        "`interior.start` is the closing quote's byte"
    );
    assert_eq!(
        errors_of(&empty),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(9, 16),
            kind: TemplateKind::Empty,
        }),
        "an empty `query` is refused at the origin, never `0:0`"
    );

    // The same origin for a statement that opens with code: `Syntax` points at
    // the first non-trivia character, which is the statement's own first one.
    let typo = format!("{header}        query 'SELCT id FROM products'\n    }}\n}}\n");
    let grant = grant_of(&typo);
    assert_eq!(grant.query_sites()[0].at, Position::at(9, 16));
    assert_eq!(grant.query_sites()[0].raw, "SELCT id FROM products");
    assert_eq!(
        errors_of(&typo),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(9, 16),
            kind: TemplateKind::Syntax,
        })
    );
}

#[test]
fn the_escape_map_survives_a_multiline_double_quoted_query() {
    // A real line break inside a `"…"` string: the decoded text has raw
    // newlines, the source has line breaks, and the two `\"` pairs before them
    // cost two columns each. The `...` on the third line of the statement is
    // `MB.34.2`'s worked example.
    let source = route_with_queries(
        "query \"SELECT \\\"id\\\"\n\
         FROM t\n\
         WHERE prïx > $? AND ...\"",
    );
    let grant = grant_of(&source);
    assert_eq!(grant.query_sites().len(), 1);
    let site = &grant.query_sites()[0];
    assert_eq!(
        site.at,
        Position::at(5, 16),
        "eight spaces, `query`, space, quote"
    );
    assert_eq!(
        site.raw, "SELECT \"id\"\nFROM t\nWHERE prïx > $? AND ...",
        "the raw text keeps the decoded newlines and the decoded quotes"
    );
    assert_eq!(
        site.escapes,
        [7, 10],
        "the two quotes of `\"id\"`, at their decoded character indices"
    );
    // From (5, 16): `SELECT ` is 7 columns, `\"` two more, `\"` two more, the
    // raw newline moves to line 6, `FROM t` is 6 columns, the second newline
    // moves to line 7, and `WHERE prïx > $? AND ` is 20 characters — `ï` is one
    // column though it is two bytes — so the first `.` is column 21.
    assert_eq!(
        errors_of(&source),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(7, 21),
            kind: TemplateKind::StrayEllipsis,
        }),
        "the escape map and the raw newlines are both applied"
    );

    // The same statement written on **one** source line with `\n` escapes: the
    // escapes cost two columns and never move the line, so the `...` is at
    // column 16 + 8 + 2 + 6 + 2 + 10 = 44 on line 5.
    let escaped = route_with_queries("query \"SELECT 1\\nFROM t\\nWHERE x > ...\"");
    let grant = grant_of(&escaped);
    let site = &grant.query_sites()[0];
    assert_eq!(site.raw, "SELECT 1\nFROM t\nWHERE x > ...");
    assert_eq!(site.escapes, [8, 15], "the two `\\n` escapes");
    assert_eq!(
        errors_of(&escaped),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(5, 44),
            kind: TemplateKind::StrayEllipsis,
        }),
        "a `\\n` escape costs two columns on the same line, it is not a break"
    );
}

#[test]
fn a_duplicate_query_gets_one_site_and_one_sentence() {
    // Two spellings that normalise alike: whitespace is not part of a
    // statement, so this is one grant entry, written twice. The statement also
    // does not parse (`FROM` with nothing after it), so a broken
    // `if … else if … else` chain would print a second sentence for it.
    let source = route_with_queries(
        "query 'SELECT … FROM'\n\
                 query '  SELECT   …   FROM  '",
    );
    let grant = grant_of(&source);
    assert_eq!(
        grant.queries(),
        ["SELECT … FROM"],
        "one entry, deduplicated by the normalised text"
    );
    assert_eq!(grant.query_sites().len(), 1, "one entry, one site");
    assert_eq!(
        grant.query_sites()[0].at,
        Position::at(5, 16),
        "the first spelling's origin wins"
    );

    let errors = errors_of(&source);
    assert_eq!(errors.len(), 1, "one entry, one sentence: {errors}");
    assert_eq!(
        errors.errors(),
        [ManifestError::RedactedQueryPlaceholder {
            location: blastit_manifest::Location::Route {
                index: 0,
                name: "notes".to_string(),
            },
            proxy: ProxyName::from("db"),
            rule: 0,
            at: Position::at(5, 23),
        }],
        "the `…` is seven characters into the first statement"
    );
    assert_eq!(
        errors.errors()[0].position(),
        Position::at(5, 23),
        "the duplicate on line 6 has no site of its own, so it has no diagnostic"
    );
}

#[test]
fn a_programmatic_grant_keeps_none() {
    // A grant no document wrote still has one site per entry — the vectors are
    // always parallel — but nothing to point at.
    let grant = PgGrant::new(vec!["SELCT id FROM t".to_string()], false);
    assert_eq!(grant.queries().len(), 1);
    assert_eq!(grant.query_sites().len(), 1, "one entry, one site, always");
    assert_eq!(grant.query_sites()[0].at, Position::NONE);
    assert_eq!(grant.query_sites()[0].raw, "SELCT id FROM t");
    assert_eq!(grant.query_sites()[0].interior, 0..0);
    assert!(grant.query_sites()[0].escapes.is_empty());
    assert_eq!(
        grant.query_sites()[0].position_at(0),
        Position::NONE,
        "`NONE` means no site of its own, so no offset has one either"
    );

    let route = Route::new(
        "notes".to_string(),
        vec![".*".to_string()],
        None,
        Vec::new(),
        false,
        false,
        [(ProxyName::from("db"), Grant::Postgresql(grant))]
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
    );
    let errors = Manifest::from_parts(1, Vec::new(), BTreeMap::new(), BTreeMap::new(), vec![route])
        .validate(UidLayout::DEFAULT_BASE)
        .expect_err("`SELCT` is not a statement");

    assert_eq!(errors.len(), 1, "{errors}");
    assert_eq!(
        errors.errors()[0].position(),
        Position::NONE,
        "a value nobody wrote has no line to point at"
    );
    assert_eq!(
        errors.to_string(),
        TemplateKind::Syntax.sentence(),
        "no `0:0` prefix, and the statement is never rendered"
    );
    assert_eq!(
        errors.errors()[0].message(),
        TemplateKind::Syntax.sentence(),
        "the editor's message is the same sentence, prefixless"
    );
}
