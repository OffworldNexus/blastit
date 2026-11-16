//! Protected critical suite: `--learn` must never drop a view's `static` mount
//! or the `entrypoint` (§14.5, `R28`, `R29`; `docs/frameworks/php.md`,
//! `docs/design/static-and-spa.md`).
//!
//! The `Blastfile` belongs to the author, and a `static` block (`root`, `index`,
//! `fallback`, `immutable`) and the `entrypoint php` decide what the application
//! *is*. An edit that adds one `query` entry is allowed to splice that entry and
//! nothing else; if the writer cannot reproduce a block it must refuse the edit
//! rather than write a document with the block gone.
//!
//! Every case therefore asserts both directions:
//!
//! * the learned statement is in the file afterwards (the fix cannot be "make
//!   the writer a no-op"), and
//! * every other byte — the mount and the entrypoint included, with the
//!   author's comments — is exactly the byte it was.
//!
//! A bare view is the negative twin: an edit must not invent a `static` block or
//! an `entrypoint` a document never had, and a block the parser refuses is
//! refused by the editor too, before any write.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::{EditError, write_atomic};
use blastit_manifest::{Manifest, PhpPolicy, StaticPolicy};

/// The documented PHP-shaped `Blastfile` (`docs/frameworks/php.md`): a front
/// controller, the static mount beside it, and one grant on the same view.
///
/// It is hand-written on purpose: comments above and inside every block, both
/// quoting styles, and the mount and the entrypoint *around* the view, which is
/// the order the page shows.
const PHP_STATIC: &str = "\
# The shop: Symfony serves it from one front controller.
app \"shop.wsgi:application\"

# One database, one credential.
postgres $DATABASE_URL

# Only fingerprinted output is cached for a year.
static \"/assets\" {
    root \"public\"
    index \"index.html\"
    immutable \"/assets/*\", \"/build/*\"
}

# The front controller.
entrypoint php \"public/index.php\"

view app \"/*\" {
    postgres {
        query 'SELECT \"shop_product\".\"id\" FROM \"shop_product\"'
    }
}
";

/// A view with no mount and no entrypoint at all: the shape the editor must
/// leave bare.
const PLAIN: &str = "\
app \"shop.wsgi:application\"

postgres $DATABASE_URL

view app \"/*\" {
    postgres {
        query 'SELECT \"shop_product\".\"id\" FROM \"shop_product\"'
    }
}
";

/// The statement `PHP_STATIC` already grants.
const EXISTING_QUERY: &str = r#"SELECT "shop_product"."id" FROM "shop_product""#;

/// The statement a learning run adds, already normalised (`$?` for `$1`).
const LEARNED_QUERY: &str =
    r#"SELECT "shop_product"."sku" FROM "shop_product" WHERE "shop_product"."id" = $?"#;

fn grant_edit(view: &str, query: &str) -> BlastfileEdits {
    BlastfileEdits {
        views: Vec::new(),
        grants: vec![ProposedGrant {
            view: view.to_string(),
            store: Some("db".to_string()),
            queries: vec![query.to_string()],
            tables: Default::default(),
            functions: Default::default(),
        }],
    }
}

/// The lines of one block, from its header to the line that closes it.
///
/// The comparison is on the raw lines, so a directive reordered, a comment moved
/// or a value requoted is a difference, not an equal model.
fn block<'a>(document: &'a str, header: &str) -> Vec<&'a str> {
    let start = document
        .lines()
        .position(|line| line.trim() == header)
        .unwrap_or_else(|| panic!("{header} is in the document"));
    let mut lines: Vec<&str> = Vec::new();
    for line in document.lines().skip(start) {
        lines.push(line);
        if line.trim() == "}" {
            break;
        }
    }
    lines
}

/// Everything except the `query` lines, which are the only lines an edit is
/// allowed to touch.
fn without_queries(document: &str) -> String {
    document
        .lines()
        .filter(|line| !line.trim_start().starts_with("query "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn route<'a>(manifest: &'a Manifest, name: &str) -> &'a blastit_manifest::Route {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .unwrap_or_else(|| panic!("view {name} is declared"))
}

/// `apply_edits` + `write_atomic`, the two calls `--learn` makes, byte for byte.
#[test]
fn a_learned_query_keeps_static_and_entrypoint_byte_for_byte_on_disk() {
    let dir = TempDir::new("executor-roundtrip");
    let manifest = dir.path.join("Blastfile");
    std::fs::write(&manifest, PHP_STATIC).expect("write the fixture");

    let edited = apply_edits(PHP_STATIC, &grant_edit("app", LEARNED_QUERY))
        .expect("a view beside a mount is editable");
    write_atomic(&manifest, &edited).expect("the atomic write succeeds");
    let on_disk = std::fs::read_to_string(&manifest).expect("read the manifest back");

    assert_eq!(on_disk, edited, "the write is the edit, byte for byte");

    // The direction the fix must keep: the learned statement landed next to the
    // statement the author had already granted.
    assert!(on_disk.contains(LEARNED_QUERY), "{on_disk}");
    assert!(on_disk.contains(EXISTING_QUERY), "{on_disk}");

    // The direction the bug broke: every other line, the mount and the
    // entrypoint included.
    assert_eq!(
        without_queries(&on_disk),
        without_queries(PHP_STATIC),
        "an edit changed a line it does not own"
    );
    assert_eq!(
        block(&on_disk, "static \"/assets\" {"),
        block(PHP_STATIC, "static \"/assets\" {"),
        "the static mount moved or changed"
    );
    assert_eq!(
        on_disk.matches("static \"/assets\"").count(),
        1,
        "a static mount was duplicated: {on_disk}"
    );
    assert_eq!(
        on_disk.matches("entrypoint php").count(),
        1,
        "the entrypoint was duplicated: {on_disk}"
    );
    assert!(
        on_disk.contains("# The front controller.\nentrypoint php \"public/index.php\"\n"),
        "the comment above the entrypoint was lost: {on_disk}"
    );

    // The written file is the manifest the page promises, and the model still
    // carries the mount and the front controller, values included.
    let (written, warnings) =
        Manifest::parse_blastfile(&on_disk, 20_000).expect("the written manifest boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        written.mounts(),
        [blastit_manifest::StaticMount {
            prefix: "/assets".to_string(),
            policy: StaticPolicy {
                root: "public".to_string(),
                index: "index.html".to_string(),
                fallback: None,
                immutable: vec!["/assets/*".to_string(), "/build/*".to_string()],
            },
        }]
    );
    assert_eq!(
        written.php(),
        Some(&PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: None,
        })
    );
    // The view itself carries no per-route executor block: the mount and the
    // entrypoint are the document's, not the route's (§12.2).
    assert_eq!(route(&written, "app").static_policy(), None);
    assert_eq!(route(&written, "app").php(), None);

    // Idempotence is part of the promise: a second learn of the same statement
    // returns the first output unchanged.
    assert_eq!(
        apply_edits(&on_disk, &grant_edit("app", LEARNED_QUERY)).expect("idempotent"),
        on_disk
    );
}

/// The same round trip without the disk: the edit is conservative on its own.
#[test]
fn an_executor_document_is_edited_only_on_its_query_lines() {
    let edited = apply_edits(PHP_STATIC, &grant_edit("app", LEARNED_QUERY))
        .expect("a view beside a mount is editable");

    assert_ne!(edited, PHP_STATIC, "the fix cannot be a no-op");
    assert_eq!(without_queries(&edited), without_queries(PHP_STATIC));
    assert_eq!(
        block(&edited, "static \"/assets\" {"),
        block(PHP_STATIC, "static \"/assets\" {")
    );
    // The author's comments are part of the document and survive with it.
    assert!(edited.contains("# Only fingerprinted output is cached for a year."));
    assert!(edited.contains("# The front controller."));
}

/// The negative twin: a bare view gains no mount and no entrypoint, and still
/// gets its learned query.
#[test]
fn a_view_without_a_mount_never_gains_one() {
    let edited =
        apply_edits(PLAIN, &grant_edit("app", LEARNED_QUERY)).expect("a bare view is editable");

    assert!(edited.contains(LEARNED_QUERY), "{edited}");
    assert_eq!(
        without_queries(&edited),
        without_queries(PLAIN),
        "an edit changed a line it does not own"
    );
    assert!(
        !edited.contains("static "),
        "an edit invented a static mount: {edited}"
    );
    assert!(
        !edited.contains("entrypoint "),
        "an edit invented an entrypoint: {edited}"
    );

    let (written, warnings) =
        Manifest::parse_blastfile(&edited, 20_000).expect("the written manifest boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(written.mounts().is_empty());
    assert_eq!(written.php(), None);
}

/// An inserted view leaves a view it does not touch alone — the mount and the
/// entrypoint included — and still lands above the catch-all.
#[test]
fn an_inserted_view_does_not_disturb_the_static_mount() {
    let edits = BlastfileEdits {
        views: vec![ProposedView {
            name: "assets".to_string(),
            methods: Vec::new(),
            patterns: vec!["/assets/".to_string()],
        }],
        grants: Vec::new(),
    };
    let edited = apply_edits(PHP_STATIC, &edits).expect("the insertion applies");

    assert!(edited.contains("view assets \"/assets/\""), "{edited}");
    assert!(
        edited.find("view assets").expect("inserted") < edited.find("view app").expect("kept"),
        "the new view is above the catch-all: {edited}"
    );
    // Everything the edit did not insert is the source, byte for byte, from the
    // catch-all down; the mount and the entrypoint are above it and untouched.
    let tail = |document: &str| -> String {
        document
            .find("view app \"/*\" {")
            .map(|at| document[at..].to_string())
            .expect("the catch-all is still there")
    };
    assert_eq!(
        tail(&edited),
        tail(PHP_STATIC),
        "an insertion rewrote the view it did not touch"
    );
    assert_eq!(
        block(&edited, "static \"/assets\" {"),
        block(PHP_STATIC, "static \"/assets\" {"),
        "an insertion rewrote the mount"
    );
}

/// Fail closed: a document whose static block does not parse is refused by the
/// editor before a writer is ever reached.
///
/// The honest level for this pair is `apply_edits`: it is the call that would
/// have produced the text, `--learn` only calls `write_atomic` on its `Ok`
/// output, and the refusal therefore leaves the file with no writer to invoke.
/// `root 4` is a type error, so the document never becomes a
/// [`blastit_manifest::Manifest`] and no view is rebuilt at all.
#[test]
fn malformed_executor_state_is_refused_before_any_edit() {
    let broken = PHP_STATIC.replace("root \"public\"", "root 4");
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
