//! `TESTING_PLAN.md` §7 — surgical, lossless `--learn` edits: an insertion
//! touches only the lines it owns, and a refused edit writes nothing.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::EditError;
use blastit_manifest::{Columns, Manifest, ProxyName, Route, TableGrant, UidLayout};

const BASE: u32 = UidLayout::DEFAULT_BASE;
/// `--learn`'s editing fixture: comments everywhere, both quoting styles, an
/// existing grant, and the catch-all last.
const SOURCE: &str = "\
# The shop's Blastfile.
app \"shop\"

# One database, one credential.
postgres $DATABASE_URL

# The catalogue is matched first.
view catalogue GET \"/catalogue/\" {
}

view products GET \"/products/[0-9]+/\" {
    # The catalogue's own statement.
    postgres {
        query 'SELECT \"shop_product\".\"slug\" FROM \"shop_product\"'
    }
}

# The catch-all renders the app's own 404s.
view fallback \"/*\" {
}
";

/// [`SOURCE`] after the edit below: the new view is the only added block.
const EDITED: &str = "\
# The shop's Blastfile.
app \"shop\"

# One database, one credential.
postgres $DATABASE_URL

# The catalogue is matched first.
view catalogue GET \"/catalogue/\" {
}

view products GET \"/products/[0-9]+/\" {
    # The catalogue's own statement.
    postgres {
        query 'SELECT \"shop_product\".\"slug\" FROM \"shop_product\"'
    }
}

view orders GET \"/orders/[0-9]+/\" {
    postgres {
        query 'SELECT \"shop_order\".\"id\" FROM \"shop_order\" WHERE \"shop_order\".\"id\" = $?'
    }
}

# The catch-all renders the app's own 404s.
view fallback \"/*\" {
}
";

/// The one view of `manifest` named `name`.
fn view<'a>(manifest: &'a Manifest, name: &str) -> &'a Route {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .unwrap_or_else(|| panic!("view {name} exists"))
}

/// The stored queries of one view's grant on one store.
fn queries(manifest: &Manifest, view_name: &str, store: &str) -> Vec<String> {
    view(manifest, view_name)
        .grant(&ProxyName::from(store))
        .and_then(blastit_manifest::Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default()
}

/// The order edit every assertion below starts from.
fn order_edit() -> BlastfileEdits {
    BlastfileEdits {
        views: vec![ProposedView {
            name: "orders".to_string(),
            methods: vec!["GET".to_string()],
            patterns: vec!["/orders/[0-9]+/".to_string()],
        }],
        grants: vec![ProposedGrant {
            view: "orders".to_string(),
            store: Some("db".to_string()),
            queries: vec![
                "SELECT \"shop_order\".\"id\" FROM \"shop_order\" WHERE \"shop_order\".\"id\" = $1"
                    .to_string(),
            ],
            tables: Default::default(),
            functions: Default::default(),
        }],
    }
}

//
// Surgical editing: `--learn` splices, it never re-serialises.
//

#[test]
fn an_edit_is_surgical_lossless_and_idempotent() {
    let edits = order_edit();
    let edited = apply_edits(SOURCE, &edits).expect("the edit applies");
    assert_eq!(edited, EDITED, "the edit changed a byte it does not own");

    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        queries(&manifest, "orders", "db"),
        ["SELECT \"shop_order\".\"id\" FROM \"shop_order\" WHERE \"shop_order\".\"id\" = $?"],
        "the wire's `$1` is written as `$?`"
    );
    let names: Vec<&str> = manifest.routes().iter().map(|route| route.name()).collect();
    assert_eq!(names, ["catalogue", "products", "orders", "fallback"]);

    // Repeating the edit, or applying an empty one, changes nothing.
    assert_eq!(apply_edits(&edited, &edits).expect("idempotent"), edited);
    assert_eq!(
        apply_edits(&edited, &BlastfileEdits::default()).expect("a no-op"),
        edited
    );
}

#[test]
fn an_edit_lands_above_the_catch_all_or_at_the_end() {
    for spelling in ["\"/*\"", "regex '/.*'"] {
        let source = format!(
            "postgres $DATABASE_URL\n\nview about GET \"/about/\" {{\n}}\n\nview fallback {spelling} {{\n}}\n"
        );
        let edits = BlastfileEdits {
            views: vec![ProposedView {
                name: "orders".to_string(),
                methods: vec!["GET".to_string()],
                patterns: vec!["/orders/".to_string()],
            }],
            grants: Vec::new(),
        };
        let edited =
            apply_edits(&source, &edits).unwrap_or_else(|error| panic!("{spelling}: {error}"));
        let manifest = Manifest::from_blastfile_str(&edited).expect("it parses");
        let names: Vec<&str> = manifest.routes().iter().map(|route| route.name()).collect();
        assert_eq!(names, ["about", "orders", "fallback"], "{spelling}");
        assert!(
            edited.contains("view orders GET \"/orders/\" {\n}\n\nview fallback"),
            "{spelling}: the catch-all's comment and blank line stay with it:\n{edited}"
        );
    }

    // A method-restricted catch-all lets other methods fall through, so a new
    // view belongs below it; a missing final newline still separates the lines.
    let source = "postgres $DATABASE_URL\n\nview fallback GET \"/*\" {\n}";
    let edited = apply_edits(source, &order_edit()).expect("it applies");
    let manifest = Manifest::from_blastfile_str(&edited).expect("it parses");
    let names: Vec<&str> = manifest.routes().iter().map(|route| route.name()).collect();
    assert_eq!(names, ["fallback", "orders"]);
    assert!(
        edited.contains("}\n\nview orders GET \"/orders/[0-9]+/\" {\n"),
        "{edited}"
    );

    // A CRLF document keeps its line endings.
    let source = SOURCE.replace('\n', "\r\n");
    let edited = apply_edits(&source, &order_edit()).expect("it applies");
    assert_eq!(
        edited.matches("\r\n").count(),
        edited.matches('\n').count(),
        "every newline is a CRLF"
    );
}

#[test]
fn a_grant_lands_in_the_view_and_store_it_was_proposed_for() {
    let source = concat!(
        "postgres $DATABASE_URL\n\n",
        "postgres analytics $ANALYTICS_URL\n\n",
        "view products GET \"/products/\" {\n",
        "    postgres {\n        select products(id)\n    }\n",
        "}\n",
    );
    let edits = BlastfileEdits {
        views: Vec::new(),
        grants: vec![
            ProposedGrant {
                view: "products".to_string(),
                store: Some("analytics".to_string()),
                queries: vec!["SELECT events.id FROM events".to_string()],
                tables: Default::default(),
                functions: Default::default(),
            },
            ProposedGrant {
                view: "products".to_string(),
                store: Some("db".to_string()),
                queries: Vec::new(),
                tables: [(
                    "products".to_string(),
                    TableGrant {
                        select: Some(Columns::list(["id", "name"])),
                        ..TableGrant::default()
                    },
                )]
                .into_iter()
                .collect(),
                functions: ["unaccent".to_string()].into_iter().collect(),
            },
        ],
    };
    let edited = apply_edits(source, &edits).expect("the edits apply");
    let manifest = Manifest::from_blastfile_str(&edited).expect("it boots");
    assert_eq!(
        queries(&manifest, "products", "analytics"),
        ["SELECT events.id FROM events"]
    );
    assert_eq!(queries(&manifest, "products", "db"), Vec::<String>::new());
    assert!(
        edited
            .contains("postgres analytics {\n        query 'SELECT events.id FROM events'\n    }"),
        "{edited}"
    );
    assert!(edited.contains("select products(id, name)"), "{edited}");
    assert!(edited.contains("functions unaccent"), "{edited}");
    // The table scope stays in the default store's block, the statement in the
    // named one: no grant is written into the other store's block.
    let analytics_at = edited.rfind("postgres analytics").expect("the named block");
    let id_at = edited
        .find("select products(id, name)")
        .expect("the table scope");
    assert!(id_at < analytics_at, "{edited}");
}

#[test]
fn an_edit_refuses_what_it_must_never_write() {
    let literal = BlastfileEdits {
        views: Vec::new(),
        grants: vec![ProposedGrant {
            view: "products".to_string(),
            store: Some("db".to_string()),
            queries: vec![
                "SELECT \"shop_product\".\"id\" FROM \"shop_product\" WHERE \"shop_product\".\"token\" = 'hunter2'"
                    .to_string(),
            ],
            tables: Default::default(),
            functions: Default::default(),
        }],
    };
    let error = apply_edits(SOURCE, &literal).expect_err("a literal is not grantable");
    assert!(matches!(error, EditError::LiteralQuery { .. }), "{error:?}");
    assert!(!error.to_string().contains("hunter2"), "the value leaked");

    let redacted = BlastfileEdits {
        views: Vec::new(),
        grants: vec![ProposedGrant {
            view: "products".to_string(),
            store: Some("db".to_string()),
            queries: vec![
                "SELECT \"shop_product\".\"body\" FROM \"shop_product\" WHERE x = …".to_string(),
            ],
            tables: Default::default(),
            functions: Default::default(),
        }],
    };
    let error = apply_edits(SOURCE, &redacted).expect_err("a redaction is not grantable");
    assert!(
        matches!(error, EditError::RedactedQuery { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains("shop_product"));

    let ghost = BlastfileEdits {
        views: Vec::new(),
        grants: vec![ProposedGrant {
            view: "products".to_string(),
            store: Some("ghost".to_string()),
            queries: vec!["SELECT 1".to_string()],
            tables: Default::default(),
            functions: Default::default(),
        }],
    };
    let error = apply_edits(SOURCE, &ghost).expect_err("the store is undeclared");
    assert!(matches!(error, EditError::UnknownProxy { .. }), "{error:?}");
    assert!(error.to_string().contains("postgres ghost"), "{error}");

    // A proposed view has to be routable before anything is written.
    let propose = |name: &str, methods: Vec<String>, patterns: Vec<String>| BlastfileEdits {
        views: vec![ProposedView {
            name: name.to_string(),
            methods,
            patterns,
        }],
        grants: Vec::new(),
    };
    let get = || vec!["GET".to_string()];
    assert!(matches!(
        apply_edits(
            SOURCE,
            &propose("Products", get(), vec!["/products/".into()])
        ),
        Err(EditError::InvalidRouteName { .. })
    ));
    assert!(matches!(
        apply_edits(SOURCE, &propose("orders", get(), Vec::new())),
        Err(EditError::EmptyPath { .. })
    ));
    assert!(matches!(
        apply_edits(SOURCE, &propose("orders", get(), vec!["/orders/*/".into()])),
        Err(EditError::InvalidPattern { rule: 0, .. })
    ));
}
