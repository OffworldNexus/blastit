//! Protected critical suite for the surgical manifest editor and the atomic
//! write (`R28`, `R29`).
//!
//! `--learn` writes into a file a person wrote by hand, so two promises have to
//! be proven together. An edit keeps every comment, every quote and every
//! untouched byte, and it lands in the one place routing order requires: above
//! the method-unrestricted catch-all, in either spelling the documentation uses
//! (`path = ".*"` and `path = ['.*']`), or at the end of the file.
//!
//! The negative half is the point too. A statement that inlines a literal, a
//! redacted `…`, a grant on an undeclared proxy, an edit that would collide with
//! an existing route name, and a write that fails halfway must all be refused
//! **and** leave no temporary file behind. Every allowed edit is round-tripped
//! through `Manifest::parse_and_validate`, so no edit can produce a manifest
//! that does not boot.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use blastit_manifest::edit::{
    EditError, ManifestEdits, ProposedGrant, ProposedRoute, apply_edits, write_atomic,
};
use blastit_manifest::{Grant, Manifest, ProxyName, Route, UidLayout};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// A hand-written manifest: comments everywhere, both quoting styles, an
/// existing grant, and a catch-all last.
const SOURCE: &str = "\
# The shop's manifest.
version = 1
app = \"shop.wsgi:application\"

# One database, one credential.
[proxy.db]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

# The catalogue is matched first.
[[route]]
name = \"catalogue\"
path = [\"/catalogue/\"]
methods = [\"GET\"]          # a trailing comment

[[route]]
name = \"products\"
path = ['/products/[0-9]+/']
[route.db]
queries = ['SELECT \"shop_product\".\"slug\" FROM \"shop_product\"']

# The catch-all renders the app's own 404s.
[[route]]
name = \"fallback\"
path = \".*\"
";

const PRODUCT_QUERY: &str =
    r#"SELECT "shop_product"."id" FROM "shop_product" WHERE "shop_product"."id" = $?"#;
/// The statement `SOURCE` already grants for `products`.
const EXISTING_QUERY: &str = r#"SELECT "shop_product"."slug" FROM "shop_product""#;
const ORDER_QUERY: &str =
    r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $?"#;
const PRICE_QUERY: &str =
    r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?"#;

fn route_names(manifest: &Manifest) -> Vec<&str> {
    manifest.routes().iter().map(Route::name).collect()
}

fn route<'a>(manifest: &'a Manifest, name: &str) -> &'a Route {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .unwrap_or_else(|| panic!("route {name} should exist"))
}

fn queries(manifest: &Manifest, route_name: &str, proxy: &str) -> Vec<String> {
    route(manifest, route_name)
        .grant(&ProxyName::from(proxy))
        .and_then(Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default()
}

fn new_route(name: &str, path: &str) -> ProposedRoute {
    ProposedRoute {
        name: name.to_string(),
        path: vec![path.to_string()],
    }
}

fn grant(route: &str, proxy: &str, queries: &[&str]) -> ProposedGrant {
    ProposedGrant {
        route: route.to_string(),
        proxy: proxy.to_string(),
        queries: queries.iter().map(|query| query.to_string()).collect(),
    }
}

//
// R29: an edit is surgical.
//

#[test]
fn comments_quoting_and_untouched_lines_survive_an_edit() {
    let edits = ManifestEdits {
        routes: vec![new_route("orders", "/orders/[0-9]+/")],
        grants: vec![grant(
            "orders",
            "db",
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
        edited.contains("methods = [\"GET\"]          # a trailing comment"),
        "a trailing comment was lost"
    );
    assert!(
        edited.contains("# The catch-all renders the app's own 404s."),
        "a section comment was lost"
    );
    assert!(
        edited.contains("path = [\"/catalogue/\"]"),
        "the author's double quotes were normalised away"
    );

    let (manifest, warnings) = Manifest::parse_and_validate(&edited, BASE).expect("it still boots");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        route_names(&manifest),
        ["catalogue", "products", "orders", "fallback"]
    );
    assert_eq!(
        queries(&manifest, "orders", "db"),
        [
            r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $?"#,
            r#"SELECT "shop_order"."status" FROM "shop_order""#,
        ]
    );

    let inserted = edited.find("name = \"orders\"").expect("inserted");
    let fallback = edited.find("name = \"fallback\"").expect("catch-all");
    assert!(inserted < fallback, "the new route is below the catch-all");
}

#[test]
fn a_route_is_inserted_above_both_catch_all_spellings() {
    for spelling in ["\".*\"", "['.*']", "['^.*$']"] {
        let source = format!(
            "version = 1\n\
             [[route]]\nname = \"about\"\npath = ['/about/']\n\
             \n\
             [[route]]\nname = \"fallback\"\npath = {spelling}\n"
        );
        let edits = ManifestEdits {
            routes: vec![new_route("orders", "/orders/")],
            grants: vec![],
        };
        let edited =
            apply_edits(&source, &edits).unwrap_or_else(|error| panic!("{spelling}: {error}"));
        let manifest = Manifest::from_toml_str(&edited).expect("it parses");
        assert_eq!(
            route_names(&manifest),
            ["about", "orders", "fallback"],
            "spelling {spelling}"
        );
        let inserted = edited.find("name = \"orders\"").expect("inserted");
        let fallback = edited.find("name = \"fallback\"").expect("catch-all");
        assert!(inserted < fallback, "spelling {spelling}");
    }
}

#[test]
fn a_method_restricted_catch_all_is_not_an_insertion_point() {
    // No trailing newline: the append still separates whole lines.
    let source = "version = 1\n\
                  [[route]]\nname = \"fallback\"\npath = \".*\"\nmethods = [\"GET\"]";
    let edits = ManifestEdits {
        routes: vec![new_route("orders", "/orders/")],
        grants: vec![],
    };
    let edited = apply_edits(source, &edits).expect("the edit applies");
    let manifest = Manifest::from_toml_str(&edited).expect("it parses");

    // A `GET`-only catch-all lets other methods fall through, so a new route
    // belongs below it, not above: the caller's routing order is untouched.
    assert_eq!(route_names(&manifest), ["fallback", "orders"]);
    assert!(edited.ends_with("path = ['/orders/']\n"));
}

#[test]
fn applying_the_same_edits_twice_changes_nothing() {
    let edits = ManifestEdits {
        routes: vec![new_route("orders", "/orders/")],
        grants: vec![
            grant("orders", "db", &[ORDER_QUERY]),
            grant("products", "db", &[PRICE_QUERY]),
        ],
    };
    let once = apply_edits(SOURCE, &edits).expect("first application");
    let twice = apply_edits(&once, &edits).expect("second application");

    assert_eq!(once, twice, "repeated edits are not idempotent");
    assert_eq!(once.matches("name = \"orders\"").count(), 1);
    assert_eq!(once.matches(ORDER_QUERY).count(), 1);
    assert_eq!(once.matches(PRICE_QUERY).count(), 1);
    assert_eq!(once.matches(EXISTING_QUERY).count(), 1);

    let (manifest, _) = Manifest::parse_and_validate(&once, BASE).expect("it boots");
    assert_eq!(
        queries(&manifest, "products", "db"),
        [EXISTING_QUERY, PRICE_QUERY]
    );

    // An edit with nothing to add is a byte-identical no-op.
    assert_eq!(
        apply_edits(&once, &ManifestEdits::default()).expect("no-op"),
        once
    );
}

#[test]
fn a_grant_joins_an_existing_queries_list_without_rewriting_it() {
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant("products", "db", &[PRODUCT_QUERY, PRICE_QUERY])],
    };
    let edited = apply_edits(SOURCE, &edits).expect("the edit applies");

    // The already-granted statement is still there exactly once, and the
    // author's single-quoted list was extended rather than reformatted.
    let line = edited
        .lines()
        .find(|line| line.starts_with("queries = ["))
        .expect("a queries line");
    assert!(line.ends_with(']'), "the array stayed on one line: {line}");
    assert_eq!(line.matches(EXISTING_QUERY).count(), 1);
    assert_eq!(line.matches(PRODUCT_QUERY).count(), 1);
    assert_eq!(line.matches(PRICE_QUERY).count(), 1);

    let (manifest, _) = Manifest::parse_and_validate(&edited, BASE).expect("it boots");
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
    let fixtures: Vec<(&str, &str, ManifestEdits)> = vec![
        (
            "a new route and a $1 statement",
            SOURCE,
            ManifestEdits {
                routes: vec![new_route("orders", "/orders/")],
                grants: vec![grant("orders", "db", &[ORDER_QUERY])],
            },
        ),
        (
            "a grant into a route with an empty list",
            "version = 1\n\
             [proxy.db]\nprotocol = \"postgresql\"\n\
             [[route]]\nname = \"products\"\npath = ['/products/']\n\
             [route.db]\nqueries = []\n",
            ManifestEdits {
                routes: vec![],
                grants: vec![grant("products", "db", &[PRODUCT_QUERY, PRICE_QUERY])],
            },
        ),
        (
            "a grant into a route with no grant table yet",
            "version = 1\n\
             [proxy.db]\nprotocol = \"postgresql\"\n\
             [[route]]\nname = \"products\"\npath = ['/products/']\n",
            ManifestEdits {
                routes: vec![],
                grants: vec![grant("products", "db", &[PRODUCT_QUERY])],
            },
        ),
        (
            "a route appended when there is no catch-all",
            "version = 1\napp = \"shop.wsgi:application\"\n\
             [[route]]\nname = \"about\"\npath = ['/about/']\n",
            ManifestEdits {
                routes: vec![new_route("orders", "/orders/")],
                grants: vec![],
            },
        ),
        (
            "the two documented setup statements",
            "version = 1\n\
             [proxy.db]\nprotocol = \"postgresql\"\n\
             [[route]]\nname = \"products\"\npath = ['/products/']\n\
             [route.db]\nqueries = []\n",
            ManifestEdits {
                routes: vec![],
                grants: vec![grant(
                    "products",
                    "db",
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
        let (manifest, warnings) = Manifest::parse_and_validate(&edited, BASE)
            .unwrap_or_else(|error| panic!("{label}: {error}"));

        assert!(warnings.is_empty(), "{label}: {warnings:?}");
        for proposed in &edits.grants {
            let granted = queries(&manifest, &proposed.route, &proposed.proxy);
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
        &ManifestEdits {
            routes: vec![],
            grants: vec![grant(
                "products",
                "db",
                &[r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $1"#],
            )],
        },
    )
    .expect("the edit applies");
    assert!(edited.contains("$?"), "the written grant is not normalised");
    assert!(!edited.contains("$1"));
}

#[test]
fn a_proposed_route_has_to_be_routable() {
    let bad_name = ManifestEdits {
        routes: vec![new_route("Products", "/products/")],
        grants: vec![],
    };
    let name_error = apply_edits(SOURCE, &bad_name).expect_err("the name is not routable");
    assert!(
        matches!(name_error, EditError::InvalidRouteName { .. }),
        "{name_error:?}"
    );

    let empty_path = ManifestEdits {
        routes: vec![ProposedRoute {
            name: "orders".to_string(),
            path: vec![],
        }],
        grants: vec![],
    };
    assert!(matches!(
        apply_edits(SOURCE, &empty_path).expect_err("a route needs a path"),
        EditError::EmptyPath { .. }
    ));

    let bad_regex = ManifestEdits {
        routes: vec![new_route("orders", "/orders/([0-9]+/")],
        grants: vec![],
    };
    let regex_error = apply_edits(SOURCE, &bad_regex).expect_err("the regex does not compile");
    assert!(
        matches!(regex_error, EditError::InvalidPattern { rule: 0, .. }),
        "{regex_error:?}"
    );
}

//
// R24/R29: what may never be written.
//

#[test]
fn an_inline_literal_is_refused_without_echoing_it() {
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant(
            "products",
            "db",
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
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant(
            "products",
            "db",
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
fn a_grant_on_an_undeclared_proxy_names_the_missing_declaration() {
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant("products", "ghost", &[PRODUCT_QUERY])],
    };
    let error = apply_edits(SOURCE, &edits).expect_err("the proxy is not declared");

    assert!(matches!(error, EditError::UnknownProxy { .. }), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains("[proxy.ghost]"),
        "the message does not name the declaration to add: {message}"
    );
}

#[test]
fn an_unrestricted_grant_cannot_take_exact_queries() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/']\n\
                  [route.db]\nunrestricted = true\n";
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant("admin", "db", &[PRODUCT_QUERY])],
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
fn an_inferred_route_name_cannot_silently_change_a_path() {
    let edits = ManifestEdits {
        routes: vec![new_route("products", "/products/")],
        grants: vec![],
    };
    let error = apply_edits(SOURCE, &edits).expect_err("the name is taken");

    assert!(
        matches!(error, EditError::RouteConflict { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("products"), "{error}");
}

#[test]
fn an_inline_grant_table_is_named_rather_than_duplicated() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"products\"\npath = ['/products/']\n\
                  db = { queries = ['SELECT \"t\".\"id\" FROM \"t\"'] }\n";
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant("products", "db", &[PRODUCT_QUERY])],
    };
    let error = apply_edits(source, &edits).expect_err("an inline table cannot be patched");

    assert!(matches!(error, EditError::InlineGrant { .. }), "{error:?}");
    assert!(
        error.to_string().contains("[route.db]"),
        "the message does not name the table to write: {error}"
    );
}

//
// The follow-up from MA.2.2.1: a proxy may be named `app`, `version` or
// `preload` as long as its declaration exists.
//

#[test]
fn a_grant_on_a_reserved_proxy_name_still_boots() {
    let source = "version = 1\n\
                  app = \"shop.wsgi:application\"\n\
                  [[route]]\nname = \"catalogue\"\npath = ['/catalogue/']\n\
                  [route.app]\nqueries = []\n\
                  \n\
                  [proxy.app]\nprotocol = \"postgresql\"\ninput = { url = \"APP_DATABASE_URL\" }\n";
    let edits = ManifestEdits {
        routes: vec![],
        grants: vec![grant("catalogue", "app", &[PRODUCT_QUERY])],
    };
    let edited = apply_edits(source, &edits).expect("the declaration makes the name legal");

    let (manifest, warnings) = Manifest::parse_and_validate(&edited, BASE).expect("it boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(queries(&manifest, "catalogue", "app"), [PRODUCT_QUERY]);
    assert!(
        edited.contains("[proxy.app]"),
        "the proxy declaration was lost"
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
    let path = dir.path().join("blastit.toml");

    write_atomic(&path, SOURCE).expect("the write succeeds");

    assert_eq!(std::fs::read_to_string(&path).expect("read back"), SOURCE);
    Manifest::parse_and_validate(SOURCE, BASE).expect("the written file boots");
    assert_eq!(entries(dir.path()), ["blastit.toml"]);
    assert!(temp_files(dir.path()).is_empty());

    // A second write replaces the file in place, still leaving no temp behind.
    let replacement = format!("{SOURCE}\n");
    write_atomic(&path, &replacement).expect("the rewrite succeeds");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        replacement
    );
    assert_eq!(entries(dir.path()), ["blastit.toml"]);
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

#[test]
fn a_failed_write_removes_its_temp_file() {
    let dir = TempDir::new("failure");
    let path = dir.path().join("blastit.toml");
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
    assert_eq!(entries(dir.path()), ["blastit.toml"]);
}

/// `write_atomic`'s temp name is `.<name>.<pid>.tmp`, which anyone who can
/// write the directory can predict. A symlink planted there has to be removed,
/// never followed: the file is opened `O_CREAT | O_EXCL`, which refuses a name
/// that already exists — a symlink included.
#[cfg(unix)]
#[test]
fn a_planted_temp_name_symlink_is_never_written_through() {
    let dir = TempDir::new("planted-link");
    let path = dir.path().join("blastit.toml");
    let victim = dir.path().join("victim.txt");
    let planted = dir
        .path()
        .join(format!(".blastit.toml.{}.tmp", std::process::id()));

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
    assert_eq!(entries(dir.path()), ["blastit.toml", "victim.txt"]);
}

/// A regular file left at the temp name is the retry branch, and the retry has
/// to start from a fresh file: the stale bytes are unlinked, not truncated in
/// place or merged with the new content.
#[test]
fn a_stale_regular_file_at_the_temp_name_is_replaced() {
    use std::io::Read as _;

    let dir = TempDir::new("stale-temp");
    let path = dir.path().join("blastit.toml");
    let planted = dir
        .path()
        .join(format!(".blastit.toml.{}.tmp", std::process::id()));
    std::fs::write(&path, SOURCE).expect("the manifest");
    std::fs::write(&planted, "stale = 1\n").expect("the stale temp file");
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
        old, "stale = 1\n",
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
    assert_eq!(entries(dir.path()), ["blastit.toml"]);
}
