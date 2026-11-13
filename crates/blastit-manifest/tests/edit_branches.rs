//! Standard-tier coverage for the line patcher behind `--learn`: the shapes a
//! hand-written `queries` list can have, where a new `[route.<proxy>]` table
//! lands, which comment belongs to the catch-all, and the refusals that happen
//! before anything is written.
//!
//! `critical_manifest_edit.rs` proves the promises (surgical, idempotent, every
//! output boots). This file walks the branches of the scanner that decides
//! them, because every one of those branches either edits a file a person wrote
//! or refuses to.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use blastit_manifest::edit::{
    EditError, ManifestEdits, ProposedGrant, ProposedRoute, apply_edits, write_atomic,
};
use blastit_manifest::{Grant, Manifest, ProxyName};

/// The declarations every fixture shares: one proxy, no routes yet.
const HEAD: &str = "version = 1\n\
                    [proxy.db]\n\
                    protocol = \"postgresql\"\n";

/// The statement a route already grants in the fixtures below.
const KEPT: &str = r#"SELECT "t"."a" FROM "t""#;
/// The statement the edits add.
const ADDED: &str = r#"SELECT "t"."b" FROM "t""#;
/// The normalised spelling of [`ADDED`].
const ADDED_NORMALISED: &str = r#"SELECT "t"."b" FROM "t""#;

fn grant(route: &str, queries: &[&str]) -> ProposedGrant {
    ProposedGrant {
        route: route.to_string(),
        proxy: "db".to_string(),
        queries: queries.iter().map(|query| query.to_string()).collect(),
    }
}

fn new_route(name: &str, path: &str) -> ProposedRoute {
    ProposedRoute {
        name: name.to_string(),
        path: vec![path.to_string()],
    }
}

/// The route names of a manifest, in document order.
fn route_names(manifest: &Manifest) -> Vec<&str> {
    manifest.routes().iter().map(|route| route.name()).collect()
}

/// The `queries` one route grants on `db`, as written.
fn queries_in(manifest: &Manifest, name: &str) -> Vec<String> {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .and_then(|route| route.grant(&ProxyName::from("db")))
        .and_then(Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default()
}

/// The edited text, parsed strictly: every branch below has to produce a
/// document that still reads back.
fn edited_and_parsed(source: &str, edits: &ManifestEdits) -> (String, Manifest) {
    let edited = apply_edits(source, edits).expect("the edit applies");
    let manifest = Manifest::from_toml_str(&edited).expect("the edit parses");
    (edited, manifest)
}

//
// The shapes of an existing `queries` list.
//

#[test]
fn a_list_spread_over_several_lines_is_written_back_on_one() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n\
         [route.db]\nqueries = [\n    '{KEPT}',\n]\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains(
            "queries = ['SELECT \"t\".\"a\" FROM \"t\"', 'SELECT \"t\".\"b\" FROM \"t\"']\n"
        ),
        "the multi-line list was not folded into one assignment:\n{edited}"
    );
    // The original lines are gone: no stray opening bracket or closing one.
    assert_eq!(edited.matches("queries = [").count(), 1, "{edited}");
    assert!(
        !edited.contains("\n    '"),
        "an entry line was left behind:\n{edited}"
    );
    assert!(
        !edited.contains("\n]\n"),
        "a closing bracket line was left behind:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products"),
        [KEPT.to_string(), ADDED_NORMALISED.to_string()]
    );
}

#[test]
fn a_one_line_list_keeps_the_authors_trailing_comment() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n\
         [route.db]\nqueries = ['{KEPT}']  # learned by --learn, edited by hand\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    let line = edited
        .lines()
        .find(|line| line.starts_with("queries = ["))
        .expect("the queries line");
    assert!(
        line.ends_with("  # learned by --learn, edited by hand"),
        "the author's note was lost: {line}"
    );
    assert_eq!(line.matches(KEPT).count(), 1);
    assert_eq!(line.matches(ADDED).count(), 1);
    assert_eq!(
        queries_in(&manifest, "products"),
        [KEPT.to_string(), ADDED_NORMALISED.to_string()]
    );
}

#[test]
fn an_empty_list_and_a_list_the_author_ended_with_a_comma_both_extend_cleanly() {
    for (list, expected) in [
        (
            "queries = []\n",
            "queries = ['SELECT \"t\".\"b\" FROM \"t\"']\n",
        ),
        (
            "queries = ['SELECT \"t\".\"a\" FROM \"t\"', ]\n",
            "queries = ['SELECT \"t\".\"a\" FROM \"t\"', 'SELECT \"t\".\"b\" FROM \"t\"']\n",
        ),
    ] {
        let source = format!(
            "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n[route.db]\n{list}"
        );
        let (edited, manifest) = edited_and_parsed(
            &source,
            &ManifestEdits {
                routes: vec![],
                grants: vec![grant("products", &[ADDED])],
            },
        );

        let line = edited
            .lines()
            .find(|line| line.starts_with("queries = ["))
            .expect("the queries line");
        assert_eq!(line, expected.trim_end(), "extending {list:?}");
        assert_eq!(
            queries_in(&manifest, "products").last(),
            Some(&ADDED_NORMALISED.to_string())
        );
    }
}

#[test]
fn a_grant_table_without_an_assignment_gets_one_written_under_its_header() {
    let source =
        format!("{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n[route.db]\n");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains("[route.db]\nqueries = ['SELECT \"t\".\"b\" FROM \"t\"']\n"),
        "the assignment does not follow its header:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products"),
        [ADDED_NORMALISED.to_string()]
    );
}

//
// Where a patched table lands.
//

#[test]
fn a_new_grant_table_goes_after_the_routes_last_line_not_after_its_blank_run() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n\n\
         [[route]]\nname = \"fallback\"\npath = \".*\"\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains("path = ['/products/']\n\n[route.db]\nqueries = "),
        "the table was not separated from the route's fields:\n{edited}"
    );
    assert!(
        edited.contains("\n\n[[route]]\nname = \"fallback\""),
        "{edited}"
    );
    assert_eq!(route_names(&manifest), ["products", "fallback"]);
    assert_eq!(
        queries_in(&manifest, "products"),
        [ADDED_NORMALISED.to_string()]
    );
}

#[test]
fn a_grant_table_after_a_foreign_table_still_belongs_to_its_route() {
    // TOML lets a root table sit between `[[route]]` and `[route.db]`, so the
    // header is found past the end of the block the route's own tables suggest.
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n\n\
         [proxy.backup]\nprotocol = \"postgresql\"\n\n\
         [route.db]\nqueries = []\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert_eq!(
        edited.matches("[route.db]").count(),
        1,
        "the table was duplicated instead of extended:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products"),
        [ADDED_NORMALISED.to_string()]
    );
}

//
// Where a new route lands.
//

#[test]
fn a_new_route_goes_above_the_comment_run_that_documents_the_catch_all() {
    // No blank line above the comments: the run documents the catch-all, so the
    // new block goes above it and the whole insertion site is re-spaced.
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n\
         # The first comment.\n# The second comment.\n[[route]]\nname = \"fallback\"\npath = \".*\"\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![new_route("orders", "/orders/")],
            grants: vec![],
        },
    );

    let inserted = edited.find("name = \"orders\"").expect("the new route");
    for kept in ["# The first comment.", "# The second comment."] {
        let at = edited.find(kept).expect("the comment");
        assert!(
            inserted < at,
            "{kept} is no longer attached to the catch-all"
        );
    }
    // One blank line separates the block from the route above it and from the
    // comment run below it: no more, no fewer.
    assert!(
        edited.contains(
            "path = ['/about/']\n\n[[route]]\nname = \"orders\"\npath = ['/orders/']\n\n\
             # The first comment.\n"
        ),
        "the insertion site is not separated by exactly one blank line:\n{edited}"
    );
    assert!(
        !edited.contains("\n\n\n"),
        "a doubled blank line:\n{edited}"
    );
    assert_eq!(route_names(&manifest), ["about", "orders", "fallback"]);
}

#[test]
fn a_bracket_inside_a_quoted_statement_does_not_hide_the_next_route() {
    // The `[[` belongs to the statement, not to the document: if the scanner
    // counted it, the catch-all below would not be seen and the new route would
    // land after it.
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n\
         [route.db]\nqueries = ['SELECT \"t\".\"a\" FROM \"t\" WHERE \"t\".\"b\" = \"[[\"']\n\n\
         [[route]]\nname = \"fallback\"\npath = \".*\"\n"
    );
    let (_, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![new_route("orders", "/orders/")],
            grants: vec![],
        },
    );

    assert_eq!(route_names(&manifest), ["products", "orders", "fallback"]);
}

#[test]
fn two_proposed_routes_are_written_in_order_with_a_blank_line_between_them() {
    let source = format!("{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![
                new_route("carts", "/carts/"),
                new_route("orders", "/orders/"),
            ],
            grants: vec![grant("carts", &[ADDED])],
        },
    );

    assert_eq!(route_names(&manifest), ["about", "carts", "orders"]);
    // The appended block starts one blank line below the existing route, which
    // already ended with a newline.
    assert!(
        edited.contains("path = ['/about/']\n\n[[route]]\nname = \"carts\""),
        "the append re-spaced the existing route:\n{edited}"
    );
    assert!(
        edited.contains(
            "path = ['/carts/']\n\n[route.db]\nqueries = ['SELECT \"t\".\"b\" FROM \"t\"']\n\n\
             [[route]]\nname = \"orders\""
        ),
        "the proposed routes and their grant table are not laid out as documented:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "carts"),
        [ADDED_NORMALISED.to_string()]
    );
}

//
// Refusals that never reach the document.
//

#[test]
fn duplicate_proposed_route_names_are_refused() {
    let error = apply_edits(
        &format!("{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n"),
        &ManifestEdits {
            routes: vec![
                new_route("orders", "/orders/"),
                new_route("orders", "/sales/"),
            ],
            grants: vec![],
        },
    )
    .expect_err("the two proposals collide");

    assert!(
        matches!(error, EditError::DuplicateProposedRoute { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("orders"), "{error}");
}

#[test]
fn an_empty_pattern_is_refused_by_its_rule_index() {
    let error = apply_edits(
        &format!("{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n"),
        &ManifestEdits {
            routes: vec![ProposedRoute {
                name: "orders".to_string(),
                path: vec!["/orders/".to_string(), String::new()],
            }],
            grants: vec![],
        },
    )
    .expect_err("an empty pattern matches nothing");

    assert!(
        matches!(error, EditError::EmptyPattern { rule: 1, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("path[1]"), "{error}");
}

#[test]
fn an_empty_statement_is_refused_without_quoting_it() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n[route.db]\nqueries = []\n"
    );
    let error = apply_edits(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &["   "])],
        },
    )
    .expect_err("a blank statement is not grantable");

    assert!(
        matches!(error, EditError::EmptyQuery { index: 0, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("queries[0]"), "{error}");
}

#[test]
fn a_path_without_a_file_name_is_refused_before_anything_is_written() {
    let error = write_atomic(Path::new("/"), "version = 1\n").expect_err("a root has no name");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("file name"), "{error}");
}

#[test]
fn two_proposals_for_one_new_route_share_a_single_grant_table() {
    let source = format!("{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![new_route("carts", "/carts/")],
            // The same statement twice, then a different one: one table, two
            // entries, in the order they were proposed.
            grants: vec![grant("carts", &[ADDED]), grant("carts", &[ADDED, KEPT])],
        },
    );

    assert_eq!(
        edited.matches("[route.db]").count(),
        1,
        "the grant was written as more than one table:\n{edited}"
    );
    assert_eq!(
        edited.matches(ADDED).count(),
        1,
        "the statement was repeated:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "carts"),
        [ADDED_NORMALISED.to_string(), KEPT.to_string()]
    );
}

#[test]
fn a_line_that_looks_like_an_assignment_inside_a_string_is_not_the_assignment() {
    // The grant's own `queries` value holds the text `queries = ['fake']`. The
    // patcher has to edit the real assignment, four lines above it.
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n[route.db]\n\
         queries = ['''\nqueries = ['fake']\n''']\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert_eq!(
        edited
            .lines()
            .filter(|line| line.starts_with("queries = ["))
            .count(),
        1,
        "the edit landed inside the string value:\n{edited}"
    );
    let granted = queries_in(&manifest, "products");
    assert_eq!(
        granted.last(),
        Some(&ADDED_NORMALISED.to_string()),
        "{granted:?}"
    );
    assert!(
        granted
            .iter()
            .any(|query| query.contains("queries = ['fake']")),
        "the author's statement was lost: {granted:?}"
    );
}

#[test]
fn a_replaced_list_does_not_add_a_newline_the_file_did_not_have() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"products\"\npath = ['/products/']\n[route.db]\n\
         queries = [\n    '{KEPT}',\n]"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.ends_with("']"),
        "a newline was added to a file that ended without one:\n{edited:?}"
    );
    assert_eq!(
        queries_in(&manifest, "products"),
        [KEPT.to_string(), ADDED_NORMALISED.to_string()]
    );
}

#[test]
fn a_comment_run_that_already_has_a_blank_line_above_it_keeps_exactly_one() {
    let source = format!(
        "{HEAD}\n[[route]]\nname = \"about\"\npath = ['/about/']\n\n\
         # The comment for the catch-all.\n[[route]]\nname = \"fallback\"\npath = \".*\"\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &ManifestEdits {
            routes: vec![new_route("orders", "/orders/")],
            grants: vec![],
        },
    );

    assert!(
        edited.contains("path = ['/about/']\n\n[[route]]\nname = \"orders\""),
        "a blank line was added above a block that already had one:\n{edited}"
    );
    assert!(
        !edited.contains("\n\n\n"),
        "a doubled blank line:\n{edited}"
    );
    assert_eq!(route_names(&manifest), ["about", "orders", "fallback"]);
}
