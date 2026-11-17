//! The parser's budgets and its refusal matrix, through the public entry point
//! (`R4`).
//!
//! The grammar's own matrix is in `parser.rs`'s `mod tests`, because a
//! `pub(crate)` parser is the only thing that can see it (§4.1, TS-3). This
//! suite re-asserts the same rows from **outside** the crate, so a caller that
//! only has `Manifest::from_blastfile_str` sees the sentences too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::Manifest;

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// Two block levels are the grammar's maximum, in both shapes it allows (§4.5).
#[test]
fn depth_two_is_the_limit() {
    assert!(Manifest::from_blastfile_str("view v {\n  GET \"/x\" { body json }\n}\n").is_ok());
    assert!(
        Manifest::from_blastfile_str("view v {\n  GET \"/x\"\n  postgres { select a }\n}\n")
            .is_ok()
    );
    assert!(
        Manifest::from_blastfile_str("trait t {\n  postgres {\n    transactions\n  }\n}\n").is_ok()
    );
    // A third level is unreachable through the grammar: an endpoint block takes
    // `body` only, and a grant block takes grants only.
    assert_eq!(
        refusal("view v {\n  GET \"/x\" {\n    postgres { select a }\n  }\n}\n"),
        "3:5: a grant is per view; write the `postgres` block in the view body, where it applies to every endpoint"
    );
}

/// §6.1's legal continuations after a store name still parse — the unnamed
/// store, `unrestricted`, a named grant block, a trait's block, and the
/// one-line store — so the second-bare-word refusal is the only hole closed
/// (`MB.21.5.11`).
#[test]
fn every_legal_store_and_grant_form_parses() {
    for source in [
        "postgres $DATABASE_URL\n\nview v \"/v\" {\n  postgres db { transactions; query 'SELECT 1'; select t(id) }\n}\n",
        "postgres $DATABASE_URL\n\nview v \"/v\" {\n  postgres { transactions; query 'SELECT 1'; select t(id) }\n}\n",
        "postgres $DATABASE_URL\n\nview v \"/v\" {\n  postgres unrestricted\n}\n",
        "postgres $DATABASE_URL\n\nview v \"/v\" {\n  postgres db unrestricted\n}\n",
        "postgres $DATABASE_URL\n\ntrait t {\n  postgres {\n    transactions\n    select t(id)\n  }\n}\n\nview v \"/v\" {\n  use t\n}\n",
        "postgres analytics $ANALYTICS_URL\n\nview v \"/v\" {\n  postgres analytics unrestricted\n}\n",
    ] {
        assert!(Manifest::from_blastfile_str(source).is_ok(), "{source:?}");
    }
}

/// A wall of braces is refused at the first token that cannot start a section,
/// without recursion (`R4`).
#[test]
fn a_wall_of_braces_is_refused_without_recursion() {
    for source in [
        "{".repeat(100_000),
        "postgres { ".repeat(10_000),
        "view v { ".repeat(10_000),
        "trait t { ".repeat(10_000),
    ] {
        let rendered = refusal(&source);
        assert!(rendered.starts_with("1:"), "{rendered}");
        assert!(!rendered.contains('\n'), "{rendered}");
    }
}

/// A 512 KiB file of views lowers: the size is bounded by the reader, and the
/// lowering is one pass (`R4`).
#[test]
fn a_half_megabyte_of_views_lowers() {
    let mut source = String::with_capacity(512 * 1024 + 32);
    let mut views = 0u32;
    while source.len() < 512 * 1024 {
        source.push_str(&format!("view v{views} \"/v{views}\" {{\n}}\n"));
        views = views.saturating_add(1);
    }
    assert!(source.len() >= 512 * 1024);
    let manifest = Manifest::from_blastfile_str(&source).expect("lowers");
    assert_eq!(manifest.route_count(), views as usize);
    // Every view is its own sandbox, in file order.
    assert_eq!(manifest.route(0).map(|route| route.sandbox()), Some(0));
    assert_eq!(
        manifest
            .route(manifest.route_count() - 1)
            .map(|route| route.sandbox()),
        Some(views - 1)
    );
}

/// §6.7's rows, through the public entry point, each with its `line:col`.
#[test]
fn the_refusal_matrix() {
    let cases: &[(&str, &str)] = &[
        (
            "View products \"/x\" { }\n",
            "1:1: `View` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`",
        ),
        (
            "app \"a\"\napp \"b\"\n",
            "2:1: a file declares one `app`; the first is on line 1",
        ),
        (
            "view v {\n  fallback \"x\"\n}\n",
            "2:3: `fallback` is not a directive here; a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`",
        ),
        (
            "root \"public\"\n",
            "1:1: `root` is not a directive here; the top level takes `app`, `entrypoint`, `postgres`, `trait`, `view` and `static`",
        ),
        (
            "body json\n",
            "1:1: `body` is not a directive here; the top level takes `app`, `entrypoint`, `postgres`, `trait`, `view` and `static`",
        ),
        (
            "use session\n",
            "1:1: `use` is not a directive here; the top level takes `app`, `entrypoint`, `postgres`, `trait`, `view` and `static`",
        ),
        (
            "egress any\n",
            "1:1: `egress` is not a directive here; the top level takes `app`, `entrypoint`, `postgres`, `trait`, `view` and `static`",
        ),
        (
            "view v {\n  json\n}\n",
            "2:3: `json` is not a directive here; a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`",
        ),
        (
            "view v {\n  get \"/x\"\n}\n",
            "2:3: `get` is not a method: methods are upper-case, `GET`",
        ),
        (
            "view GET \"/x\" { }\n",
            "1:6: a view name cannot be `GET`; names start with a lower-case letter or a digit",
        ),
        (
            "view v GET\n",
            "1:11: `view \"v\"` lists methods but no pattern; write the path it answers",
        ),
        (
            "view v { }\n",
            "1:8: view \"v\" has no endpoint: declare at least `GET \"/products\"`",
        ),
        (
            "view p GET \"/p\" {\n  POST \"/x\"\n}\n",
            "2:3: view \"p\" declares its methods in the header; `POST \"/x\"` belongs in a view without one",
        ),
        (
            "postgres $URL { host $H }\n",
            "1:15: `postgres $DATABASE_URL` and a store block are two different forms; write one",
        ),
        (
            "postgres {}\n",
            "1:10: a store block declares at least one slot: `postgres { host $DB_HOST; database $DB_DATABASE }`",
        ),
        (
            "postgres db {\n  url $URL\n}\n",
            "2:3: `url` is the one-line form; write `postgres $DATABASE_URL` (or a named `postgres db $DATABASE_URL`)",
        ),
        (
            "postgres {\n  host $A\n  host $B\n}\n",
            "3:3: `host` is written twice; a store block names each slot once",
        ),
        (
            "trait s {\n  view cart \"/c\"\n}\n",
            "2:3: a trait holds `postgres` blocks only; `view` belongs at the top level",
        ),
        (
            "view v {\n  body json unvalidated\n}\n",
            "2:13: `unvalidated` acknowledges `raw` only; `body json` already validates the body",
        ),
        (
            "view v {\n  body\n}\n",
            "2:7: `body` needs a codec: `json`, `form`, `none` or `raw`",
        ),
        (
            "view v {\n  body xml\n}\n",
            "2:8: `xml` is not a codec; the codecs are `json`, `form`, `none`, `raw`",
        ),
        (
            "entrypoint python \"x\" {\n  body json\n}\n",
            "2:3: the `entrypoint python` block holds `preload` only",
        ),
        (
            "view v {\n  GET \"/x\"\n}\n}\n",
            "4:1: `}` closes nothing; the last open block is line 1",
        ),
        (
            "view v {\n  body json\n",
            "3:1: the block opened on line 1 is not closed before the end of the file",
        ),
        (
            "view v {\n  use , session\n}\n",
            "2:7: a list cannot start with `,`",
        ),
        (
            "view v {\n  use session,\n}\n",
            "2:14: a list has no trailing comma; delete the `,`",
        ),
        (
            "view _shop \"/x\" { }\n",
            "1:6: `_shop` is not a name; a view, trait or store name is `[a-z0-9][a-z0-9_-]*`",
        ),
        (
            "view v \"/x\" { }\nview v \"/y\" { }\n",
            "2:1: `view \"v\"` is declared twice; the first is on line 1",
        ),
        (
            "view v {\n  body json\n  body form\n}\n",
            "3:3: `body` is declared once per view; the one for a single method goes inside that endpoint",
        ),
        (
            "view v {\n  postgres { select a }\n  postgres { select b }\n}\n",
            "3:3: `postgres` is declared once per store in a view; put both grants in one block",
        ),
        (
            "view v {\n  GET \"/x\" {\n    body json\n    body form\n  }\n}\n",
            "4:5: `body` is declared once per endpoint",
        ),
        (
            "view v {\n  GET \"/x\" {\n    body json\n  }\n  .\n}\n",
            "5:3: `.` is only part of a qualified function name: `functions pg_catalog.st_distance`",
        ),
        (
            "postgres unrestricted\n",
            "1:10: `unrestricted` is a grant keyword, not a store name; write `postgres unrestricted` for the default store",
        ),
        // §6.1 `grant_body`: a second bare word after a named store is a
        // refusal at that word, not an empty grant (`MB.21.5.11`).
        (
            "view v {\n  postgres db bogus\n}\n",
            "2:15: expected `unrestricted` or `{`, found `bogus`",
        ),
        (
            "view v {\n  postgres db select\n}\n",
            "2:15: expected `unrestricted` or `{`, found `select`",
        ),
        (
            "view v {\n  body = \"json\"\n}\n",
            "2:8: `=` is not Blastfile syntax; write the value after the keyword: `body json`",
        ),
        (
            "postgres \"postgres://x\"\n",
            "1:10: a quoted string is not a variable name; a store reads `$IDENT`: `postgres $DATABASE_URL`",
        ),
        ("\u{1}b\n", "1:1: byte 0x1 is not Blastfile syntax"),
        ("\"x\"\n", "1:1: expected a section, found a string"),
    ];
    for (source, expected) in cases {
        assert_eq!(refusal(source), *expected, "{source:?}");
    }
}
