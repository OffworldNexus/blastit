//! Protected critical suite: `--learn` must never drop a route's executor block
//! (`R28`, `R29`; `docs/frameworks/php.md`, `docs/design/static-and-spa.md`).
//!
//! The manifest belongs to the author, and `[route.static]` (`root`, `index`,
//! `spa`, `immutable`) and `[route.php]` (`entry`, `workers`) decide what a
//! route *is*. An edit that adds one `queries` entry is allowed to splice that
//! entry and nothing else; if the rewriter cannot reproduce an executor block
//! it must refuse the edit rather than write a manifest with the block gone.
//!
//! Every case therefore asserts both directions:
//!
//! * the learned statement is in the file afterwards (the fix cannot be "make
//!   the rewriter a no-op"), and
//! * every other byte — both executor blocks included, in the same order, with
//!   the author's comments — is exactly the byte it was.
//!
//! A bare route is the negative twin: an edit must not invent a `[route.static]`
//! or `[route.php]` block a route never had, and a block the parser refuses is
//! refused by the editor too, before any write.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use blastit_manifest::edit::{
    EditError, ManifestEdits, ProposedGrant, ProposedRoute, apply_edits, write_atomic,
};
use blastit_manifest::{Manifest, PhpPolicy, StaticPolicy};

/// The documented PHP-shaped manifest (`docs/frameworks/php.md`): a front
/// controller, the static block beside it, and one grant on the same route.
///
/// It is hand-written on purpose: comments above and inside every block, both
/// quoting styles, and the executor tables *between* the route header and its
/// grant, which is the order the page shows.
const PHP_STATIC: &str = "\
# The shop: Symfony serves it from one front controller.
version = 1

# One database, one credential.
[proxy.db]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

[[route]]
name = \"app\"
path = ['.*']

# Only fingerprinted output is cached for a year.
[route.static]
root = \"public\"
index = \"index.html\"
spa = false
immutable = [\"/assets/*\", \"/build/*\"]

# The front controller; 4 is a cap, the budget is derived.
[route.php]
entry = \"public/index.php\"
workers = 4

[route.db]
queries = ['SELECT \"shop_product\".\"id\" FROM \"shop_product\"']
";

/// A route with no executor block at all: the shape the editor must leave bare.
const PLAIN: &str = "\
version = 1

[proxy.db]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

[[route]]
name = \"app\"
path = ['.*']
[route.db]
queries = ['SELECT \"shop_product\".\"id\" FROM \"shop_product\"']
";

/// The statement `PHP_STATIC` already grants.
const EXISTING_QUERY: &str = r#"SELECT "shop_product"."id" FROM "shop_product""#;

/// The statement a learning run adds, already normalised (`$?` for `$1`).
const LEARNED_QUERY: &str =
    r#"SELECT "shop_product"."sku" FROM "shop_product" WHERE "shop_product"."id" = $?"#;

fn grant_edit(route: &str, query: &str) -> ManifestEdits {
    ManifestEdits {
        routes: Vec::new(),
        grants: vec![ProposedGrant {
            route: route.to_string(),
            proxy: "db".to_string(),
            queries: vec![query.to_string()],
        }],
    }
}

/// The lines of one `[table]` block, from its header to the next header.
///
/// The comparison is on the raw lines, so a key reordered, a comment moved or
/// a value requoted is a difference, not an equal model.
fn block<'a>(document: &'a str, header: &str) -> Vec<&'a str> {
    let start = document
        .lines()
        .position(|line| line.trim() == header)
        .unwrap_or_else(|| panic!("{header} is in the document"));
    let rest: Vec<&str> = document.lines().skip(start).collect();
    rest.iter()
        .take(1)
        .chain(
            rest.iter()
                .skip(1)
                .take_while(|line| !line.trim().starts_with('[')),
        )
        .copied()
        .collect()
}

/// Everything except the `queries` assignment, which is the one line an edit
/// is allowed to touch.
fn without_queries(document: &str) -> String {
    document
        .lines()
        .filter(|line| !line.trim_start().starts_with("queries"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn route<'a>(manifest: &'a Manifest, name: &str) -> &'a blastit_manifest::Route {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .unwrap_or_else(|| panic!("route {name} is declared"))
}

/// `apply_edits` + `write_atomic`, the two calls `--learn` makes, byte for byte.
#[test]
fn a_learned_query_keeps_php_and_static_blocks_byte_for_byte_on_disk() {
    let dir = TempDir::new("executor-roundtrip");
    let manifest = dir.path.join("blastit.toml");
    std::fs::write(&manifest, PHP_STATIC).expect("write the fixture");

    let edited = apply_edits(PHP_STATIC, &grant_edit("app", LEARNED_QUERY))
        .expect("a route with an executor block is editable");
    write_atomic(&manifest, &edited).expect("the atomic write succeeds");
    let on_disk = std::fs::read_to_string(&manifest).expect("read the manifest back");

    assert_eq!(on_disk, edited, "the write is the edit, byte for byte");

    // The direction the fix must keep: the learned statement landed next to the
    // statement the author had already granted.
    assert!(on_disk.contains(LEARNED_QUERY), "{on_disk}");
    assert!(on_disk.contains(EXISTING_QUERY), "{on_disk}");

    // The direction the bug broke: every other line, both blocks included.
    assert_eq!(
        without_queries(&on_disk),
        without_queries(PHP_STATIC),
        "an edit changed a line it does not own"
    );
    assert_eq!(
        block(&on_disk, "[route.static]"),
        block(PHP_STATIC, "[route.static]"),
        "the [route.static] block moved or changed"
    );
    assert_eq!(
        block(&on_disk, "[route.php]"),
        block(PHP_STATIC, "[route.php]"),
        "the [route.php] block moved or changed"
    );
    assert_eq!(
        on_disk.matches("[route.static]").count(),
        1,
        "a static block was duplicated: {on_disk}"
    );
    assert_eq!(
        on_disk.matches("[route.php]").count(),
        1,
        "a php block was duplicated: {on_disk}"
    );

    // The written file is the manifest the page promises, and the model still
    // carries both policies, values and optional keys included.
    let (written, _warnings) =
        Manifest::parse_and_validate(&on_disk, 20_000).expect("the written manifest boots");
    let app = route(&written, "app");
    assert_eq!(
        app.static_policy(),
        Some(&StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            fallback: None,
            immutable: vec!["/assets/*".to_string(), "/build/*".to_string()],
        })
    );
    assert_eq!(
        app.php(),
        Some(&PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: Some(4),
        })
    );

    // Idempotence is part of the promise: a second learn of the same statement
    // returns the first output unchanged.
    assert_eq!(
        apply_edits(&on_disk, &grant_edit("app", LEARNED_QUERY)).expect("idempotent"),
        on_disk
    );
}

/// The same round trip without the disk: the edit is conservative on its own.
#[test]
fn an_executor_route_is_edited_only_on_its_queries_line() {
    let edited = apply_edits(PHP_STATIC, &grant_edit("app", LEARNED_QUERY))
        .expect("a route with an executor block is editable");

    assert_ne!(edited, PHP_STATIC, "the fix cannot be a no-op");
    assert_eq!(without_queries(&edited), without_queries(PHP_STATIC));
    assert_eq!(
        block(&edited, "[route.static]"),
        block(PHP_STATIC, "[route.static]")
    );
    assert_eq!(
        block(&edited, "[route.php]"),
        block(PHP_STATIC, "[route.php]")
    );
    // The author's comments are part of the block and survive with it.
    assert!(edited.contains("# Only fingerprinted output is cached for a year."));
    assert!(edited.contains("# The front controller; 4 is a cap, the budget is derived."));
}

/// The negative twin: a bare route gains no executor block, and still gets its
/// learned query.
#[test]
fn a_route_without_an_executor_block_never_gains_one() {
    let edited =
        apply_edits(PLAIN, &grant_edit("app", LEARNED_QUERY)).expect("a bare route is editable");

    assert!(edited.contains(LEARNED_QUERY), "{edited}");
    assert_eq!(
        without_queries(&edited),
        without_queries(PLAIN),
        "an edit changed a line it does not own"
    );
    assert!(
        !edited.contains("[route.static]"),
        "an edit invented a static block: {edited}"
    );
    assert!(
        !edited.contains("[route.php]"),
        "an edit invented a php block: {edited}"
    );

    let (written, _warnings) =
        Manifest::parse_and_validate(&edited, 20_000).expect("the written manifest boots");
    let app = route(&written, "app");
    assert_eq!(app.static_policy(), None);
    assert_eq!(app.php(), None);
}

/// An inserted route leaves a route it does not touch alone — executor blocks
/// included — and still lands above the catch-all.
#[test]
fn an_inserted_route_does_not_disturb_an_executor_route() {
    let edits = ManifestEdits {
        routes: vec![ProposedRoute {
            name: "assets".to_string(),
            path: vec!["/assets/.*".to_string()],
        }],
        grants: Vec::new(),
    };
    let edited = apply_edits(PHP_STATIC, &edits).expect("the insertion applies");

    assert!(edited.contains("name = \"assets\""), "{edited}");
    assert!(
        edited.find("name = \"assets\"").expect("inserted")
            < edited.find("name = \"app\"").expect("kept"),
        "the new route is above the catch-all: {edited}"
    );
    // Everything the edit did not insert is the source, byte for byte, from the
    // catch-all down; the blocks are inside that tail.
    let tail = |document: &str| -> String {
        document
            .find("[[route]]\nname = \"app\"")
            .map(|at| document[at..].to_string())
            .expect("the catch-all is still there")
    };
    assert_eq!(
        tail(&edited),
        tail(PHP_STATIC),
        "an insertion rewrote the route it did not touch"
    );
}

/// Fail closed: a document whose executor block does not parse is refused by
/// the editor before a writer is ever reached.
///
/// The honest level for this pair is `apply_edits`: it is the call that would
/// have produced the text, `--learn` only calls `write_atomic` on its `Ok`
/// output, and the refusal therefore leaves the file with no writer to invoke.
/// The `[route.php]` value is a type error (`entry = 4`), so the document never
/// becomes a [`blastit_manifest::Manifest`] and no route is rebuilt at all.
/// (The writer-level refusal is covered where a writer is actually driven:
/// `dev_learn::tests::a_learned_query_keeps_the_route_s_executor_blocks_on_disk`.)
#[test]
fn malformed_executor_state_is_refused_before_any_edit() {
    let broken = PHP_STATIC.replace("entry = \"public/index.php\"", "entry = 4");
    assert!(matches!(
        apply_edits(&broken, &grant_edit("app", LEARNED_QUERY)),
        Err(EditError::Source(_))
    ));
}

/// A directory unique to one test, removed however the case ends.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "blastit-mb321-{}-{tag}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a fresh fixture directory");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
