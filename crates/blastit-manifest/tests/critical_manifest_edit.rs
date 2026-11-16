//! Protected critical suite for the surgical `Blastfile` editor and the atomic
//! write (§14.5, `R28`, `R29`).
//!
//! `--learn` writes into a file a person wrote by hand, so two promises have to
//! be proven together. An edit keeps every comment, every quote and every
//! untouched byte, and it lands in the one place routing order requires: above
//! the method-unrestricted catch-all view, or at the end of the file.
//!
//! The negative half is the point too. A statement that inlines a literal, a
//! redacted `…`, a grant on an undeclared store, an edit that would collide with
//! an existing view name, and a write that fails halfway must all be refused
//! **and** leave no temporary file behind. Every allowed edit is round-tripped
//! through `Manifest::parse_blastfile`, so no edit can produce a manifest that
//! does not boot.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::{EditError, write_atomic};
use blastit_manifest::{Grant, Manifest, ProxyName, Route, UidLayout};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// A hand-written `Blastfile`: comments everywhere, both quoting styles, an
/// existing grant, and a catch-all last.
const SOURCE: &str = "\
# The shop's Blastfile.
app \"shop.wsgi:application\"

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

/// [`SOURCE`] after `comments_quoting_and_untouched_lines_survive_an_edit`'s
/// edit: the new view is the only added block, and every other byte is the
/// author's. The assertion is on the whole document, not a substring, because
/// "surgical" means the text is reproduced rather than mostly preserved.
const SOURCE_EDITED: &str = "\
# The shop's Blastfile.
app \"shop.wsgi:application\"

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
        query 'SELECT \"shop_order\".\"status\" FROM \"shop_order\"'
    }
}

# The catch-all renders the app's own 404s.
view fallback \"/*\" {
}
";

const PRODUCT_QUERY: &str =
    r#"SELECT "shop_product"."id" FROM "shop_product" WHERE "shop_product"."id" = $?"#;
/// The statement `SOURCE` already grants for `products`.
const EXISTING_QUERY: &str = r#"SELECT "shop_product"."slug" FROM "shop_product""#;
const ORDER_QUERY: &str =
    r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $?"#;
const PRICE_QUERY: &str =
    r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?"#;

/// The view names of a manifest, in document order.
fn view_names(manifest: &Manifest) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for route in manifest.routes() {
        if names.last().copied() != Some(route.name()) {
            names.push(route.name());
        }
    }
    names
}

fn route<'a>(manifest: &'a Manifest, name: &str) -> &'a Route {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .unwrap_or_else(|| panic!("view {name} should exist"))
}

fn queries(manifest: &Manifest, view: &str, store: &str) -> Vec<String> {
    route(manifest, view)
        .grant(&ProxyName::from(store))
        .and_then(Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default()
}

fn new_view(name: &str, pattern: &str) -> ProposedView {
    ProposedView {
        name: name.to_string(),
        methods: vec!["GET".to_string()],
        patterns: vec![pattern.to_string()],
    }
}

fn grant(view: &str, queries: &[&str]) -> ProposedGrant {
    ProposedGrant {
        view: view.to_string(),
        store: Some("db".to_string()),
        queries: queries.iter().map(|query| query.to_string()).collect(),
    }
}

//
// R29: an edit is surgical.
//

#[test]
fn comments_quoting_and_untouched_lines_survive_an_edit() {
    let edits = BlastfileEdits {
        views: vec![new_view("orders", "/orders/[0-9]+/")],
        grants: vec![grant(
            "orders",
            &[
                // The caller saw `$1` on the wire; the grant is normalised.
                r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $1"#,
                r#"SELECT "shop_order"."status" FROM "shop_order""#,
            ],
        )],
    };
    let edited = apply_edits(SOURCE, &edits).expect("the edit applies");

    for line in SOURCE.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            edited.contains(line),
            "the edit rewrote a line it does not own: {line}"
        );
    }
    assert!(
        edited.contains("# The catalogue's own statement."),
        "a comment inside a grant block was lost"
    );
    assert!(
        edited.contains("# The catch-all renders the app's own 404s."),
        "a section comment was lost"
    );
    assert!(
        edited.contains("query 'SELECT \"shop_product\".\"slug\" FROM \"shop_product\"'"),
        "the author's raw-string quoting was normalised away"
    );

    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it still boots");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        view_names(&manifest),
        ["catalogue", "products", "orders", "fallback"]
    );
    assert_eq!(
        queries(&manifest, "orders", "db"),
        [
            r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $?"#,
            r#"SELECT "shop_order"."status" FROM "shop_order""#,
        ]
    );

    let inserted = edited.find("view orders").expect("inserted");
    let fallback = edited.find("view fallback").expect("catch-all");
    assert!(inserted < fallback, "the new view is below the catch-all");
    assert_eq!(
        edited, SOURCE_EDITED,
        "the edit changed a byte it does not own"
    );
}

#[test]
fn a_view_is_inserted_above_both_catch_all_spellings() {
    // `/*` is the documented spelling and lowers to `/.*`; `regex '/.*'` is the
    // escape hatch and reaches the same fragment. Both are the catch-all.
    for spelling in ["\"/*\"", "regex '/.*'"] {
        let source = format!(
            "postgres $DATABASE_URL\n\
             \n\
             view about GET \"/about/\" {{\n\
             }}\n\
             \n\
             view fallback {spelling} {{\n\
             }}\n"
        );
        let edits = BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: Vec::new(),
        };
        let edited =
            apply_edits(&source, &edits).unwrap_or_else(|error| panic!("{spelling}: {error}"));
        let manifest = Manifest::from_blastfile_str(&edited).expect("it parses");
        assert_eq!(
            view_names(&manifest),
            ["about", "orders", "fallback"],
            "spelling {spelling}"
        );
        let inserted = edited.find("view orders").expect("inserted");
        let fallback = edited.find("view fallback").expect("catch-all");
        assert!(inserted < fallback, "spelling {spelling}");
    }
}

#[test]
fn a_method_restricted_catch_all_is_not_an_insertion_point() {
    // No trailing newline: the append still separates whole lines.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view fallback GET \"/*\" {\n\
                  }";
    let edits = BlastfileEdits {
        views: vec![new_view("orders", "/orders/")],
        grants: Vec::new(),
    };
    let edited = apply_edits(source, &edits).expect("the edit applies");
    let manifest = Manifest::from_blastfile_str(&edited).expect("it parses");

    // A `GET`-only catch-all lets other methods fall through, so a new view
    // belongs below it, not above: the caller's routing order is untouched.
    assert_eq!(view_names(&manifest), ["fallback", "orders"]);
    assert!(edited.ends_with("view orders GET \"/orders/\" {\n}\n"));
}

#[test]
fn applying_the_same_edits_twice_changes_nothing() {
    let edits = BlastfileEdits {
        views: vec![new_view("orders", "/orders/")],
        grants: vec![
            grant("orders", &[ORDER_QUERY]),
            grant("products", &[PRICE_QUERY]),
        ],
    };
    let once = apply_edits(SOURCE, &edits).expect("first application");
    let twice = apply_edits(&once, &edits).expect("second application");

    assert_eq!(once, twice, "repeated edits are not idempotent");
    assert_eq!(once.matches("view orders").count(), 1);
    assert_eq!(once.matches(ORDER_QUERY).count(), 1);
    assert_eq!(once.matches(PRICE_QUERY).count(), 1);
    assert_eq!(once.matches(EXISTING_QUERY).count(), 1);

    let (manifest, _) = Manifest::parse_blastfile(&once, BASE).expect("it boots");
    assert_eq!(
        queries(&manifest, "products", "db"),
        [EXISTING_QUERY, PRICE_QUERY]
    );

    // An edit with nothing to add is a byte-identical no-op.
    assert_eq!(
        apply_edits(&once, &BlastfileEdits::default()).expect("no-op"),
        once
    );
}

#[test]
fn a_grant_joins_an_existing_block_without_rewriting_it() {
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![grant("products", &[PRODUCT_QUERY, PRICE_QUERY])],
    };
    let edited = apply_edits(SOURCE, &edits).expect("the edit applies");

    // The already-granted statement is still there exactly once, on the line the
    // author wrote, and the block gained one line per new statement.
    assert_eq!(
        edited
            .matches(&format!("        query '{EXISTING_QUERY}'\n"))
            .count(),
        1,
        "the author's statement line was rewritten:\n{edited}"
    );
    assert_eq!(
        edited
            .matches(&format!("        query '{PRODUCT_QUERY}'\n"))
            .count(),
        1,
        "{edited}"
    );
    assert_eq!(
        edited
            .matches(&format!("        query '{PRICE_QUERY}'\n"))
            .count(),
        1,
        "{edited}"
    );
    assert_eq!(edited.matches("postgres {").count(), 1, "{edited}");

    let (manifest, _) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert_eq!(
        queries(&manifest, "products", "db"),
        [EXISTING_QUERY, PRODUCT_QUERY, PRICE_QUERY]
    );
}

//
// R28/R29: exact, normalised queries only, and every edit boots.
//

#[test]
fn every_edit_fixture_round_trips_through_parse_and_validate() {
    let fixtures: Vec<(&str, &str, BlastfileEdits)> = vec![
        (
            "a new view and a $1 statement",
            SOURCE,
            BlastfileEdits {
                views: vec![new_view("orders", "/orders/")],
                grants: vec![grant("orders", &[ORDER_QUERY])],
            },
        ),
        (
            "a grant into a view with an empty block",
            "postgres $DATABASE_URL\n\
             \n\
             view products GET \"/products/\" {\n\
             \x20   postgres {\n\
             \x20   }\n\
             }\n",
            BlastfileEdits {
                views: vec![],
                grants: vec![grant("products", &[PRODUCT_QUERY, PRICE_QUERY])],
            },
        ),
        (
            "a grant into a view with no block yet",
            "postgres $DATABASE_URL\n\
             \n\
             view products GET \"/products/\" {\n\
             }\n",
            BlastfileEdits {
                views: vec![],
                grants: vec![grant("products", &[PRODUCT_QUERY])],
            },
        ),
        (
            "a view appended when there is no catch-all",
            "app \"shop.wsgi:application\"\n\
             \n\
             view about GET \"/about/\" {\n\
             }\n",
            BlastfileEdits {
                views: vec![new_view("orders", "/orders/")],
                grants: Vec::new(),
            },
        ),
        (
            "the two documented setup statements",
            "postgres $DATABASE_URL\n\
             \n\
             view products GET \"/products/\" {\n\
             \x20   postgres {\n\
             \x20   }\n\
             }\n",
            BlastfileEdits {
                views: vec![],
                grants: vec![grant(
                    "products",
                    &[
                        "SET TIME ZONE 'UTC'",
                        "SELECT set_config('TimeZone', $1, false)",
                    ],
                )],
            },
        ),
    ];

    for (label, source, edits) in fixtures {
        let edited = apply_edits(source, &edits).unwrap_or_else(|error| panic!("{label}: {error}"));
        let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE)
            .unwrap_or_else(|error| panic!("{label}: {error}"));

        assert!(warnings.is_empty(), "{label}: {warnings:?}");
        for proposed in &edits.grants {
            let store = proposed.store.clone().unwrap_or_else(|| "db".to_string());
            let granted = queries(&manifest, &proposed.view, &store);
            for query in &proposed.queries {
                let expected = blastit_sqlpolicy::normalize(query);
                assert!(
                    granted.contains(&expected),
                    "{label}: the grant is missing {expected}"
                );
            }
        }
    }

    // The normalised spelling is what lands in the file: `$1` from the wire
    // becomes the `$?` a grant compares against.
    let edited = apply_edits(
        SOURCE,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant(
                "products",
                &[r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $1"#],
            )],
        },
    )
    .expect("the edit applies");
    assert!(edited.contains("$?"), "the written grant is not normalised");
    assert!(!edited.contains("$1"));
}

#[test]
fn a_proposed_view_has_to_be_routable() {
    let bad_name = BlastfileEdits {
        views: vec![new_view("Products", "/products/")],
        grants: Vec::new(),
    };
    let name_error = apply_edits(SOURCE, &bad_name).expect_err("the name is not routable");
    assert!(
        matches!(name_error, EditError::InvalidRouteName { .. }),
        "{name_error:?}"
    );

    let empty_path = BlastfileEdits {
        views: vec![ProposedView {
            name: "orders".to_string(),
            methods: Vec::new(),
            patterns: vec![],
        }],
        grants: Vec::new(),
    };
    assert!(matches!(
        apply_edits(SOURCE, &empty_path).expect_err("a view needs a pattern"),
        EditError::EmptyPath { .. }
    ));

    let bad_pattern = BlastfileEdits {
        views: vec![new_view("orders", "/orders/*/")],
        grants: Vec::new(),
    };
    let pattern_error = apply_edits(SOURCE, &bad_pattern).expect_err("the pattern is not a path");
    assert!(
        matches!(pattern_error, EditError::InvalidPattern { rule: 0, .. }),
        "{pattern_error:?}"
    );
}

//
// R24/R29: what may never be written.
//

#[test]
fn an_inline_literal_is_refused_without_echoing_it() {
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![grant(
            "products",
            &[
                r#"SELECT "shop_product"."id" FROM "shop_product" WHERE "shop_product"."token" = 'hunter2'"#,
            ],
        )],
    };
    let error = apply_edits(SOURCE, &edits).expect_err("a literal is not grantable");

    assert!(matches!(error, EditError::LiteralQuery { .. }), "{error:?}");
    let message = error.to_string();
    assert!(!message.contains("hunter2"), "the value leaked: {message}");
    assert!(
        message.contains("queries[0]"),
        "the message does not name the position: {message}"
    );
    assert!(
        message.contains("bind"),
        "the message does not name the fix: {message}"
    );
}

#[test]
fn a_redacted_placeholder_is_refused_without_echoing_it() {
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![grant(
            "products",
            &[
                r#"SELECT "shop_product"."body" FROM "shop_product" WHERE "shop_product"."slug" = …"#,
            ],
        )],
    };
    let error = apply_edits(SOURCE, &edits).expect_err("a redacted statement is not grantable");

    assert!(
        matches!(error, EditError::RedactedQuery { .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        !message.contains("\"shop_product\".\"body\""),
        "the statement leaked: {message}"
    );
    assert!(message.contains("bind"), "{message}");
}

#[test]
fn a_grant_on_an_undeclared_store_names_the_missing_declaration() {
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![ProposedGrant {
            view: "products".to_string(),
            store: Some("ghost".to_string()),
            queries: vec![PRODUCT_QUERY.to_string()],
        }],
    };
    let error = apply_edits(SOURCE, &edits).expect_err("the store is not declared");

    assert!(matches!(error, EditError::UnknownProxy { .. }), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains("postgres ghost"),
        "the message does not name the declaration to add: {message}"
    );
}

#[test]
fn an_unrestricted_grant_cannot_take_exact_queries() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view admin GET \"/admin/\" {\n\
                  \x20   postgres unrestricted\n\
                  }\n";
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![grant("admin", &[PRODUCT_QUERY])],
    };
    let error =
        apply_edits(source, &edits).expect_err("queries next to unrestricted is a boot error");

    assert!(
        matches!(error, EditError::UnrestrictedGrant { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("unrestricted"), "{error}");
}

#[test]
fn an_inferred_view_name_cannot_silently_change_a_pattern() {
    let edits = BlastfileEdits {
        views: vec![new_view("products", "/products/")],
        grants: Vec::new(),
    };
    let error = apply_edits(SOURCE, &edits).expect_err("the name is taken");

    assert!(
        matches!(error, EditError::RouteConflict { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("products"), "{error}");
}

/// Two `postgres` blocks for one store in one view is a parse refusal (§6.7),
/// so a second proposal for the same store has to merge into the block that is
/// there rather than write a duplicate the reader would refuse.
#[test]
fn a_grant_is_merged_into_the_one_block_for_its_store() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view products GET \"/products/\" {\n\
                  \x20   postgres {\n\
                  \x20       query 'SELECT \"t\".\"a\" FROM \"t\"'\n\
                  \x20   }\n\
                  }\n";
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![
            grant("products", &[PRODUCT_QUERY]),
            grant("products", &[PRICE_QUERY, PRODUCT_QUERY]),
        ],
    };
    let edited = apply_edits(source, &edits).expect("the edit applies");

    assert_eq!(
        edited.matches("postgres {").count(),
        1,
        "a second block was written for one store:\n{edited}"
    );
    assert_eq!(
        edited.matches(PRODUCT_QUERY).count(),
        1,
        "the statement was repeated:\n{edited}"
    );
    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        queries(&manifest, "products", "db"),
        [
            r#"SELECT "t"."a" FROM "t""#.to_string(),
            PRODUCT_QUERY.to_string(),
            PRICE_QUERY.to_string(),
        ]
    );
}

//
// A store may be named like a directive: names are names (§6.7).
//

#[test]
fn a_store_named_like_a_directive_still_boots() {
    let source = "postgres body $BODY_URL\n\
                  \n\
                  view catalogue GET \"/catalogue/\" {\n\
                  \x20   postgres body {\n\
                  \x20       query 'SELECT \"t\".\"a\" FROM \"t\"'\n\
                  \x20   }\n\
                  }\n";
    let edits = BlastfileEdits {
        views: vec![],
        grants: vec![ProposedGrant {
            view: "catalogue".to_string(),
            store: Some("body".to_string()),
            queries: vec![PRODUCT_QUERY.to_string()],
        }],
    };
    let edited = apply_edits(source, &edits).expect("the store name is legal");

    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        queries(&manifest, "catalogue", "body"),
        [
            r#"SELECT "t"."a" FROM "t""#.to_string(),
            PRODUCT_QUERY.to_string()
        ]
    );
    assert!(
        edited.contains("postgres body {"),
        "the store declaration was lost"
    );
}

//
// §14.5/`ADR-0011`: `--learn` never writes `body`.
//

/// The `body` directives of a document, trimmed, in the order they appear.
fn body_lines(document: &str) -> Vec<&str> {
    document
        .lines()
        .filter(|line| line.split_whitespace().next() == Some("body"))
        .map(|line| line.trim())
        .collect()
}

/// `ProposedView` and `ProposedGrant` have no `body` field, and the output
/// proves it: an edit that adds a view and a grant writes no `body` line, and a
/// document that declares one keeps exactly the line its author wrote.
#[test]
fn an_edit_never_writes_a_body_directive() {
    let edits = BlastfileEdits {
        views: vec![new_view("orders", "/orders/")],
        grants: vec![
            grant("orders", &[ORDER_QUERY]),
            grant("products", &[PRICE_QUERY]),
        ],
    };

    // A document with no `body`: none appears.
    let bare = "postgres $DATABASE_URL\n\
                \n\
                view products GET \"/products/\" {\n\
                }\n";
    let edited = apply_edits(bare, &edits).expect("the edit applies");
    assert!(
        body_lines(&edited).is_empty(),
        "the edit wrote a body directive: {:?}\n{edited}",
        body_lines(&edited)
    );

    // A document that declares one: the author's line survives, and no second
    // one is added anywhere.
    let declaring = "postgres $DATABASE_URL\n\
                     \n\
                     view products GET \"/products/\" {\n\
                     \x20   body json\n\
                     }\n";
    let edited = apply_edits(declaring, &edits).expect("the edit applies");
    assert_eq!(
        body_lines(&edited),
        ["body json"],
        "the author's body directive was rewritten or duplicated:\n{edited}"
    );
    assert!(
        edited.contains("    body json\n"),
        "the directive moved:\n{edited}"
    );
}

//
// R29: the atomic write.
//

/// A unique directory under `$TMPDIR`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "blastit-edit-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the directory reads")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn temp_files(dir: &Path) -> Vec<String> {
    entries(dir)
        .into_iter()
        .filter(|name| name.ends_with(".tmp"))
        .collect()
}

#[test]
fn the_atomic_write_leaves_no_temp_file_and_the_result_parses() {
    let dir = TempDir::new("write");
    let path = dir.path().join("Blastfile");

    write_atomic(&path, SOURCE).expect("the write succeeds");

    assert_eq!(std::fs::read_to_string(&path).expect("read back"), SOURCE);
    Manifest::parse_blastfile(SOURCE, BASE).expect("the written file boots");
    assert_eq!(entries(dir.path()), ["Blastfile"]);
    assert!(temp_files(dir.path()).is_empty());

    // A second write replaces the file in place, still leaving no temp behind.
    let replacement = format!("{SOURCE}\n");
    write_atomic(&path, &replacement).expect("the rewrite succeeds");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        replacement
    );
    assert_eq!(entries(dir.path()), ["Blastfile"]);
    assert!(temp_files(dir.path()).is_empty());

    // The replacement keeps the mode the author chose.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("chmod the manifest");
        write_atomic(&path, SOURCE).expect("the rewrite succeeds");
        let mode = std::fs::metadata(&path)
            .expect("the written file's metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the write widened the file mode");
    }
}

//
// P4: a postcondition mismatch writes nothing.
//

/// P4's other half: an edit whose result cannot reproduce the intended model is
/// `EditError::Verification` and the file is untouched.
///
/// The fixture grants `analytics` without declaring that store, so lowering
/// records an unresolved grant; [`BlastfileEdits::apply_to`] builds the intended
/// model with `Manifest::from_parts`, which carries no unresolved grant, so the
/// edited text and the intended model can never be equivalent and the edit must
/// refuse. The write goes through the same two calls `LearnRun::finish` makes —
/// `apply_edits`, then `write_atomic` only on `Ok` — so "nothing is written" is
/// the caller's contract, not just the editor's.
#[test]
fn a_postcondition_mismatch_writes_nothing() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view products GET \"/products/\" {\n\
                  \x20   postgres analytics {\n\
                  \x20       query 'SELECT \"t\".\"a\" FROM \"t\"'\n\
                  \x20   }\n\
                  }\n";
    let dir = TempDir::new("verification");
    let path = dir.path().join("Blastfile");
    std::fs::write(&path, source).expect("the fixture");

    let mut wrote = false;
    let outcome = apply_edits(
        source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[PRICE_QUERY])],
        },
    )
    .inspect(|edited| {
        write_atomic(&path, edited).expect("the write succeeds");
        wrote = true;
    });

    match outcome {
        Err(EditError::Verification { section }) => {
            // The unresolved grant is a top-level field, so that is the section
            // that diverged; the message must not blame the views.
            assert_eq!(
                section, "the top-level keys",
                "the message names the wrong section"
            );
        }
        other => panic!("the postcondition did not refuse: {other:?}"),
    }
    assert!(!wrote, "the refused edit was written");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        source,
        "the refused edit changed the file"
    );
    assert_eq!(
        entries(dir.path()),
        ["Blastfile"],
        "the refused edit left a temp file behind"
    );
}

#[test]
fn a_failed_write_removes_its_temp_file() {
    let dir = TempDir::new("failure");
    let path = dir.path().join("Blastfile");
    // A directory cannot be replaced by a file, so the final rename fails
    // after the temp file has been written and fsynced.
    std::fs::create_dir(&path).expect("a directory where the manifest goes");

    let error = write_atomic(&path, SOURCE).expect_err("the rename cannot succeed");
    assert_eq!(error.kind(), std::io::ErrorKind::IsADirectory);

    assert!(path.is_dir(), "the directory was touched");
    assert!(
        temp_files(dir.path()).is_empty(),
        "a temp file was left behind"
    );
    assert_eq!(entries(dir.path()), ["Blastfile"]);
}

/// `write_atomic`'s temp name is `.<name>.<pid>.tmp`, which anyone who can
/// write the directory can predict. A symlink planted there has to be removed,
/// never followed: the file is opened `O_CREAT | O_EXCL`, which refuses a name
/// that already exists — a symlink included.
#[cfg(unix)]
#[test]
fn a_planted_temp_name_symlink_is_never_written_through() {
    let dir = TempDir::new("planted-link");
    let path = dir.path().join("Blastfile");
    let victim = dir.path().join("victim.txt");
    let planted = dir
        .path()
        .join(format!(".Blastfile.{}.tmp", std::process::id()));

    std::fs::write(&path, SOURCE).expect("the manifest");
    std::fs::write(&victim, "victim\n").expect("the victim");
    // A relative target, exactly the shape that would be written through if the
    // temp file were opened by name without `O_EXCL`.
    std::os::unix::fs::symlink("victim.txt", &planted).expect("the planted symlink");

    let replacement = format!("{SOURCE}\n# learned one query\n");
    write_atomic(&path, &replacement).expect("the write succeeds");

    assert_eq!(
        std::fs::read_to_string(&victim).expect("the victim reads back"),
        "victim\n",
        "the write followed the planted symlink"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the manifest reads back"),
        replacement,
        "the manifest does not hold the new content"
    );
    assert!(
        std::fs::symlink_metadata(&planted).is_err(),
        "the planted symlink is still named in the directory"
    );
    assert!(
        temp_files(dir.path()).is_empty(),
        "a temp entry was left behind"
    );
    assert_eq!(entries(dir.path()), ["Blastfile", "victim.txt"]);
}

/// A regular file left at the temp name is the retry branch, and the retry has
/// to start from a fresh file: the stale bytes are unlinked, not truncated in
/// place or merged with the new content.
#[test]
fn a_stale_regular_file_at_the_temp_name_is_replaced() {
    use std::io::Read as _;

    let dir = TempDir::new("stale-temp");
    let path = dir.path().join("Blastfile");
    let planted = dir
        .path()
        .join(format!(".Blastfile.{}.tmp", std::process::id()));
    std::fs::write(&path, SOURCE).expect("the manifest");
    std::fs::write(&planted, "app \"stale:app\"\n").expect("the stale temp file");
    // Holding the old inode open proves which file received the write: an
    // unlinked inode keeps its own bytes, while a file opened in place changes
    // under this handle.
    let stale = std::fs::File::open(&planted).expect("open the stale temp file");

    write_atomic(&path, SOURCE).expect("the write succeeds");

    let mut old = String::new();
    (&stale)
        .read_to_string(&mut old)
        .expect("the stale file reads through the open handle");
    assert_eq!(
        old, "app \"stale:app\"\n",
        "the write went into the stale temp file instead of replacing it"
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read back"), SOURCE);
    assert!(
        std::fs::read_to_string(&planted).is_err(),
        "the stale temp name is still in the directory"
    );
    assert!(
        temp_files(dir.path()).is_empty(),
        "a temp entry was left behind"
    );
    assert_eq!(entries(dir.path()), ["Blastfile"]);
}

//
// Provenance: the two ways a patch can be well-formed and still wrong.
//
// The re-parse in `apply_edits` compares the document with the model the edit
// intended, so it catches a splice that corrupted the file. It cannot catch a
// splice that *agrees* with itself: a plan built from the wrong view's block, or
// from a `query` found inside a statement, re-parses to exactly the model it
// built. These cases state the intent — which view, which store, which line — so
// such a refactor cannot pass by being self-consistent.
//

/// Two views, only the second of which grants `warehouse`.
const TWO_VIEWS: &str = "postgres $DATABASE_URL\n\
    postgres warehouse $WAREHOUSE_URL\n\
    \n\
    view catalogue GET \"/catalogue/\" {\n\
    }\n\
    \n\
    view checkout GET \"/checkout/\" {\n\
    \x20   postgres warehouse {\n\
    \x20       query 'SELECT \"shop_order\".\"id\" FROM \"shop_order\"'\n\
    \x20   }\n\
    }\n";

/// `TWO_VIEWS`' existing grant, byte for byte.
const WAREHOUSE_LINES: &str = "\
    postgres warehouse {\n\
    \x20       query 'SELECT \"shop_order\".\"id\" FROM \"shop_order\"'\n\
    \x20   }\n";

#[test]
fn a_grant_lands_in_the_view_and_store_it_was_proposed_for() {
    let edited = apply_edits(
        TWO_VIEWS,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("catalogue", &[PRODUCT_QUERY])],
        },
    )
    .expect("the edit applies");
    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");

    // The proposal was for `catalogue` on `db`, and that is where the grant is.
    assert_eq!(queries(&manifest, "catalogue", "db"), [PRODUCT_QUERY]);
    assert!(
        route(&manifest, "catalogue")
            .grant(&ProxyName::from("warehouse"))
            .is_none(),
        "the grant was copied onto every store the view could have"
    );

    // The neighbour keeps exactly what it had: no grant was moved, shared or
    // copied, and its block is still the author's.
    assert!(
        queries(&manifest, "checkout", "db").is_empty(),
        "the proposal on catalogue was written into checkout"
    );
    assert_eq!(
        queries(&manifest, "checkout", "warehouse"),
        [r#"SELECT "shop_order"."id" FROM "shop_order""#]
    );
    assert!(
        edited.contains(WAREHOUSE_LINES),
        "the neighbour was rewritten"
    );
    assert_eq!(edited.matches(WAREHOUSE_LINES).count(), 1);
    assert_eq!(view_names(&manifest), ["catalogue", "checkout"]);

    // One block, inside the proposed view: above the next `view` header.
    assert_eq!(edited.matches("postgres {").count(), 1);
    let added = edited.find("postgres {").expect("the new block");
    assert!(
        added < edited.find("view checkout").expect("the next view"),
        "the new block was written below the view it belongs to:\n{edited}"
    );
    assert!(
        edited.contains(&format!(
            "    postgres {{\n        query '{PRODUCT_QUERY}'\n    }}\n"
        )),
        "the block does not hold the proposed statement:\n{edited}"
    );
}

/// Two `postgres` blocks in one view: the block a grant extends is the one for
/// the store it named, not the first block above it. Writing into the
/// neighbouring store's block would hand the view a capability the operator
/// granted to a different database.
#[test]
fn a_grant_is_never_written_into_the_other_store_s_block() {
    let source = "postgres $DATABASE_URL\n\
                  postgres warehouse $WAREHOUSE_URL\n\
                  \n\
                  view products GET \"/products/\" {\n\
                  \x20   postgres warehouse {\n\
                  \x20       query 'SELECT \"shop_order\".\"id\" FROM \"shop_order\"'\n\
                  \x20   }\n\
                  \x20   postgres {\n\
                  \x20   }\n\
                  }\n";
    let edited = apply_edits(
        source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[PRODUCT_QUERY])],
        },
    )
    .expect("the edit applies");

    assert!(
        edited.contains(&format!(
            "    postgres warehouse {{\n        query 'SELECT \"shop_order\".\"id\" FROM \
             \"shop_order\"'\n    }}\n    postgres {{\n        query '{PRODUCT_QUERY}'\n    }}\n"
        )),
        "the new statement did not go into the `db` block:\n{edited}"
    );
    let (manifest, warnings) = Manifest::parse_blastfile(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        queries(&manifest, "products", "warehouse"),
        [r#"SELECT "shop_order"."id" FROM "shop_order""#],
        "the other store's grant was extended"
    );
    assert_eq!(queries(&manifest, "products", "db"), [PRODUCT_QUERY]);
}

#[test]
fn a_grant_for_a_view_that_does_not_exist_is_refused_rather_than_attached() {
    // A stale or mistyped view name is not a new view: a grant has to name a
    // view the operator already approved, and nothing is written for a name
    // that matches none of them.
    let error = apply_edits(
        TWO_VIEWS,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("catalouge", &[PRODUCT_QUERY])],
        },
    )
    .expect_err("no view has that name");

    assert!(matches!(error, EditError::MissingRoute { .. }), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("catalouge"), "{message}");
    assert!(
        message.contains("add the view first"),
        "the message does not name the fix: {message}"
    );
}

/// A statement whose text reads exactly like the directive the editor looks
/// for. The tree is the only structure the editor reads, so the statement is one
/// entry and the block it sits in is the one that gets the new line.
const STATEMENT_HOLDS_A_DIRECTIVE: &str = "postgres $DATABASE_URL\n\
    \n\
    view products GET \"/products/\" {\n\
    \x20   postgres {\n\
    \x20       query \"SELECT 'postgres { query fake }'\"\n\
    \x20   }\n\
    }\n\
    \n\
    view fallback \"/*\" {\n\
    }\n";

#[test]
fn a_statement_that_reads_like_a_directive_is_never_edited_as_one() {
    let edited = apply_edits(
        STATEMENT_HOLDS_A_DIRECTIVE,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[PRODUCT_QUERY])],
        },
    )
    .expect("the edit applies");

    assert_eq!(
        edited.matches("\n    postgres {\n").count(),
        1,
        "the statement was read as a block:\n{edited}"
    );
    let manifest = Manifest::from_blastfile_str(&edited).expect("the edit parses");
    let granted = queries(&manifest, "products", "db");
    assert_eq!(
        granted.last(),
        Some(&PRODUCT_QUERY.to_string()),
        "{granted:?}"
    );
    assert_eq!(
        granted.len(),
        2,
        "the author's statement was split: {granted:?}"
    );
    assert!(
        granted
            .iter()
            .any(|query| query.contains("postgres { query fake }")),
        "the author's statement was lost: {granted:?}"
    );
    assert_eq!(view_names(&manifest), ["products", "fallback"]);
}
