//! The rendered table grant is a fragment a developer pastes into a view.
//!
//! `TableGrants::to_blastfile` produces the report's `table_grants.blastfile` and
//! the pytest plugin's table-scope hint, and its whole purpose is that the value
//! can be pasted as it stands. This suite holds that promise mechanically: every
//! shape a grant can take is rendered, wrapped in a real
//! `view … { postgres { … } }` document, and read back by the same reader a boot
//! uses. A rendering that emitted a directive the reader refuses — `select t()`
//! for an empty column list, say — fails here instead of in a user's file.
//!
//! Two shapes need it most. An **empty column list** (`SELECT count(*) FROM t`
//! reads no column): the grammar's `table_grant` takes at least one, and
//! `select t` would grant every column, so the entry is a comment and the grant
//! is narrower than the covering grant — never wider. A **name the reader cannot
//! write** (`audit.users`, or a name carrying a newline): interpolated raw it is
//! a refusal or a new directive of its own, so the table is one comment and
//! nothing of it is pasted.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Columns, Grant, Manifest, ProxyName, TableGrant, TableGrants};

/// Pastes `grants`' rendered fragment into a view whose `postgres` block holds
/// it, reads the document back, and returns what the reader lowered.
///
/// The document is parsed **and** validated: a fragment only counts as pasteable
/// if a boot would accept the file it lands in.
fn pasted(grants: &TableGrants) -> Manifest {
    let rendered = grants.to_blastfile("view notes postgres");
    let source = format!(
        "postgres $DATABASE_URL\n\nview notes \"/notes/\" {{\n    postgres {{\n{rendered}    }}\n}}\n"
    );
    let manifest = Manifest::from_blastfile_str(&source).unwrap_or_else(|errors| {
        panic!("the rendered fragment must parse as a Blastfile:\n{errors}\n{source}")
    });
    manifest
        .validate(20_000)
        .unwrap_or_else(|errors| panic!("the pasted document must boot:\n{errors}\n{source}"));
    manifest
}

/// The grants one route of `manifest` holds on the unnamed store.
fn grants_of(manifest: &Manifest) -> TableGrants {
    manifest
        .routes()
        .first()
        .expect("one view")
        .grant(&ProxyName::from("db"))
        .and_then(Grant::as_postgresql)
        .expect("the view grants the store")
        .tables()
        .clone()
}

#[test]
fn a_column_grant_round_trips_exactly() {
    let grants = TableGrants::new(
        [(
            "notes_note".to_string(),
            TableGrant {
                select: Some(Columns::list(["id", "owner_id"])),
                insert: Some(Columns::list(["id", "owner_id"])),
                delete: true,
                ..TableGrant::default()
            },
        )],
        ["unaccent".to_string()],
    );
    // The rendered value names every directive the grant holds...
    let rendered = grants.to_blastfile("view notes postgres");
    for directive in [
        "select notes_note(id, owner_id)",
        "insert notes_note(id, owner_id)",
        "delete notes_note",
        "functions unaccent",
    ] {
        assert!(
            rendered.contains(directive),
            "{directive} missing:\n{rendered}"
        );
    }
    // ... and the reader builds back the grant it came from.
    assert_eq!(grants_of(&pasted(&grants)), grants);
}

#[test]
fn a_whole_table_grant_round_trips_as_a_bare_select() {
    let grants = TableGrants::new(
        [(
            "notes_note".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    // `*` is the one directive with no parentheses: a list here would grant
    // nothing the grant asked for.
    assert!(
        grants
            .to_blastfile("view notes postgres")
            .contains("select notes_note\n"),
        "{}",
        grants.to_blastfile("view notes postgres")
    );
    assert_eq!(grants_of(&pasted(&grants)), grants);
}

#[test]
fn a_grant_with_no_directive_at_all_renders_nothing_to_paste() {
    // The empty string, not a header alone: a value with no grant must not
    // become a `postgres { }` block a boot refuses.
    assert_eq!(
        TableGrants::default().to_blastfile("view notes postgres"),
        ""
    );
}

#[test]
fn a_qualified_relation_is_a_comment_and_the_document_still_boots() {
    // A relation outside the default schema keeps its `schema.` prefix, and
    // `audit.users` is not a table name the grammar takes: interpolated it is
    // the refusal `` `.` is only part of a qualified function name ``. The
    // fragment has to stay pasteable, so the table is one comment.
    let grants = TableGrants::new(
        [(
            "audit.users".to_string(),
            TableGrant {
                select: Some(Columns::list(["id", "name"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let rendered = grants.to_blastfile("view notes postgres");
    assert!(!rendered.contains("select audit.users"), "{rendered}");
    assert!(rendered.contains("\"audit.users\""), "{rendered}");

    let document = pasted(&grants);
    assert!(
        grants_of(&document).tables().is_empty(),
        "a name no directive can carry grants nothing"
    );
}

#[test]
fn a_name_with_a_newline_never_becomes_a_directive() {
    // A name interpolated raw would end the line: `select secret(id)` would
    // become a grant of its own. Quoted inside the comment, it cannot.
    let grants = TableGrants::new(
        [(
            "notes_note\nselect secret".to_string(),
            TableGrant {
                select: Some(Columns::list(["id"])),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let rendered = grants.to_blastfile("view notes postgres");
    assert_eq!(
        rendered.lines().count(),
        2,
        "one header and one comment, never a directive: {rendered:?}"
    );
    assert!(!rendered.contains("\nselect secret\n"), "{rendered:?}");

    let document = pasted(&grants);
    assert!(
        grants_of(&document).tables().is_empty(),
        "nothing got pasted"
    );
}

#[test]
fn an_empty_column_list_renders_advice_the_reader_accepts() {
    // `SELECT count(*) FROM t` reads no column: the covering grant is a naming
    // grant, and a `Blastfile` has no spelling for it.
    let grants = TableGrants::new(
        [(
            "notes_note".to_string(),
            TableGrant {
                select: Some(Columns::List(Default::default())),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let rendered = grants.to_blastfile("view notes postgres");
    assert!(
        rendered.contains("name one here"),
        "the author is told what to finish:\n{rendered}"
    );
    // Never widened to every column, and never the syntax the grammar lacks.
    assert!(!rendered.contains("select notes_note\n"), "{rendered}");
    assert!(!rendered.contains("notes_note()"), "{rendered}");
    // The document still parses and boots; the table simply has no grant until
    // the author names a column, which is the one line they add.
    let document = pasted(&grants);
    assert!(
        grants_of(&document).table("notes_note").is_none(),
        "an empty column list grants nothing rather than everything"
    );

    // The written `update = []` twin, next to a column grant that does render.
    let written = TableGrants::new(
        [(
            "notes_note".to_string(),
            TableGrant {
                select: Some(Columns::list(["id"])),
                update: Some(Columns::List(Default::default())),
                ..TableGrant::default()
            },
        )],
        [],
    );
    let rendered = written.to_blastfile("view notes postgres");
    assert!(rendered.contains("select notes_note(id)"), "{rendered}");
    assert!(!rendered.contains("update notes_note"), "{rendered}");
    assert_eq!(
        grants_of(&pasted(&written)).table("notes_note"),
        Some(&TableGrant {
            select: Some(Columns::list(["id"])),
            ..TableGrant::default()
        }),
        "only the column-read half survives: the empty entry has no spelling"
    );
}
