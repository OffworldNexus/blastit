//! Protected critical suite: `--learn` must never drop a view's `static` mount
//! or the `entrypoint` (`R28`, `R29`; `docs/frameworks/php.md`,
//! `docs/design/static-and-spa.md`).
//!
//! This is the user-visible write path: `blastit dev --learn` records the
//! statements a request ran and the requests nothing routed, and hands the
//! `Blastfile` to [`blastit_manifest::blastfile::apply_edits`] and
//! `write_atomic` through the supervisor (`supervise.rs`,
//! `dev_learn::LearnRun::finish`). The manifest is the author's file, so a
//! learned `query` must land **and** every other line — the `static` mount and
//! the `entrypoint` included — must be the byte it was.
//!
//! The fixture is the documented shape (`docs/frameworks/php.md`,
//! `docs/frameworks/static-and-spa.md`): one static mount, one `entrypoint php`,
//! a catch-all view with `postgres { query '…' }` the author already granted, so
//! the request's statement is ungranted and `--learn` has to **extend** that
//! block — a fixture with no `postgres` block only proves a block can be
//! created. A document with a mount, an entrypoint and a view is legal
//! (`validate.rs`'s `check_static_declarations` returns early for a document
//! with a front controller), and needs no PHP binary.
//!
//! It pairs the promise with its twin: the learned query must be in the file
//! **as well as** the grant that was already there (the fix cannot be "make the
//! writer a no-op", nor "replace the block"), and the mount's lines must be
//! byte-identical (the bug was the rewriter rebuilding the view without the
//! document's executor state).
//!
//! `dev --learn` itself still reads the retired TOML document, because
//! `crates/blastit/src/dev_learn.rs` belongs to `MB.21.4.3`; this leaf replaces
//! the writer underneath it, so the suite drives the same two calls `finish`
//! makes (`apply_edits` + `write_atomic`) on a `Blastfile` and the end-to-end
//! `blastit dev --learn` driver returns with that leaf. The assertions are the
//! ones the driver made, at the level the writer is answerable for.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::write_atomic;
use blastit_manifest::{Manifest, ProxyName};

/// How long a write may take before the case gives up on it.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The manifest the page promises for a static surface served by the app.
///
/// The mount is written as the author wrote it (a comment between the mount and
/// the entrypoint), because the assertion below is on the raw lines, not on the
/// parsed model.
const BLASTFILE: &str = "\
# The shop: Symfony serves it from one front controller.
app \"shop.wsgi:application\"

postgres $DATABASE_URL

# Built assets are served by the responder; every other path is the view's.
static \"/assets\" {
    root \"public\"
    index \"index.html\"
    immutable \"/assets/*\"
}

entrypoint php \"public/index.php\"

# The catch-all renders the app's own 404s.
view app \"/*\" {
    # One statement is already granted, so the learned one has to be spliced onto
    # this block: a second block would be a duplicate the reader refuses, and
    # replacing the block would drop the grant the author wrote.
    postgres {
        query 'SELECT \"shop_product\".\"id\" FROM \"shop_product\"'
    }
}
";

/// The statement `BLASTFILE` already grants.
const EXISTING_QUERY: &str = r#"SELECT "shop_product"."id" FROM "shop_product""#;

/// The statement the request ran, as the wire spelled it (`$1`).
const WIRE_QUERY: &str =
    r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $1"#;

/// The normalised spelling the grant has to hold.
const LEARNED_QUERY: &str =
    r#"SELECT "shop_order"."id" FROM "shop_order" WHERE "shop_order"."id" = $?"#;

/// The edits one `dev --learn` run produces: the statement the routed request
/// ran, and a view for the request nothing routed.
fn learned() -> BlastfileEdits {
    BlastfileEdits {
        views: vec![ProposedView {
            name: "orders".to_string(),
            methods: vec!["GET".to_string()],
            patterns: vec!["/orders/[0-9]+/".to_string()],
        }],
        grants: vec![
            ProposedGrant {
                view: "app".to_string(),
                store: Some("db".to_string()),
                queries: vec![WIRE_QUERY.to_string()],
            },
            ProposedGrant {
                view: "orders".to_string(),
                store: Some("db".to_string()),
                queries: vec![WIRE_QUERY.to_string()],
            },
        ],
    }
}

/// Everything except the `query` lines, which are the only lines a learn run is
/// allowed to touch.
fn without_queries(document: &str) -> String {
    document
        .lines()
        .filter(|line| !line.trim_start().starts_with("query "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines of one `static` block, from its header to the line closing it.
fn mount_lines(document: &str) -> Vec<&str> {
    let start = document
        .lines()
        .position(|line| line.trim() == "static \"/assets\" {")
        .expect("the mount is in the document");
    let mut lines: Vec<&str> = Vec::new();
    for line in document.lines().skip(start) {
        lines.push(line);
        if line.trim() == "}" {
            break;
        }
    }
    lines
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
            "blastit-learn-{}-{tag}-{unique}",
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

#[test]
fn a_learned_query_keeps_the_mount_and_the_entrypoint_on_disk() {
    let dir = TempDir::new("learn");
    let manifest = dir.path.join("Blastfile");
    std::fs::write(&manifest, BLASTFILE).expect("write the fixture");

    let started = std::time::Instant::now();
    let edited = apply_edits(BLASTFILE, &learned()).expect("the learn run applies");
    write_atomic(&manifest, &edited).expect("the atomic write succeeds");
    assert!(
        started.elapsed() < WRITE_TIMEOUT,
        "the write took {:?}",
        started.elapsed()
    );
    let on_disk = std::fs::read_to_string(&manifest).expect("read the manifest back");
    assert_eq!(on_disk, edited, "the write is the edit, byte for byte");

    // The direction the fix must keep: the learned statement landed in the
    // block the author already granted, next to the statement it held.
    assert!(on_disk.contains(LEARNED_QUERY), "{on_disk}");
    assert!(on_disk.contains(EXISTING_QUERY), "{on_disk}");
    assert!(
        !on_disk.contains("$1"),
        "the wire spelling was written instead of the normalised one: {on_disk}"
    );

    // The direction the bug broke: every other line, the mount included. The
    // learn run adds one view above the catch-all, so the document splits at
    // the comment that documents the catch-all: everything above it is the
    // source's head plus the new view, and everything below it is the source's
    // tail with the learned query added.
    let marker = "# The catch-all renders the app's own 404s.";
    let (head, tail) = on_disk.split_once(marker).expect("the catch-all comment");
    let (source_head, source_tail) = BLASTFILE
        .split_once(marker)
        .expect("the fixture's catch-all comment");
    assert!(
        head.starts_with(source_head),
        "a learn run changed a line above the insertion point:\n{head}"
    );
    let added = head
        .strip_prefix(source_head)
        .expect("the head is the source's head plus the new view");
    assert!(
        added.contains("view orders GET \"/orders/[0-9]+/\" {"),
        "the new view is not the only thing above the catch-all:\n{added}"
    );
    assert_eq!(
        without_queries(tail),
        without_queries(source_tail),
        "a learn run changed a line it does not own"
    );
    assert_eq!(
        mount_lines(&on_disk),
        mount_lines(BLASTFILE),
        "the static mount moved or changed"
    );
    assert_eq!(
        on_disk.matches("static \"/assets\"").count(),
        1,
        "the mount was duplicated: {on_disk}"
    );
    assert_eq!(
        on_disk.matches("entrypoint php").count(),
        1,
        "the entrypoint was duplicated: {on_disk}"
    );

    // The written file is the manifest the page promises: the mount and the
    // front controller are intact, and the new view is above the catch-all.
    let (written, warnings) =
        Manifest::parse_blastfile(&on_disk, 20_000).expect("the written manifest boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        written.mounts().len(),
        1,
        "the mount is gone from the model: {:?}",
        written.mounts()
    );
    assert_eq!(
        written.php().map(|php| php.entry.as_str()),
        Some("public/index.php"),
        "the entrypoint is gone from the model"
    );
    let app = written
        .routes()
        .iter()
        .find(|route| route.name() == "app")
        .expect("the catch-all is still declared");
    assert_eq!(
        app.grant(&ProxyName::from("db"))
            .and_then(blastit_manifest::Grant::as_postgresql)
            .map(|grant| grant.queries().to_vec()),
        Some(vec![EXISTING_QUERY.to_string(), LEARNED_QUERY.to_string()]),
        "the learned statement did not join the author's block"
    );
    assert!(
        on_disk.find("view orders").expect("the new view")
            < on_disk.find("view app").expect("the catch-all"),
        "the new view is below the catch-all:\n{on_disk}"
    );

    // Idempotence is part of the promise: a second learn of the same run
    // returns the first output unchanged.
    assert_eq!(
        apply_edits(&on_disk, &learned()).expect("idempotent"),
        on_disk
    );
}

#[test]
fn a_learn_run_that_saw_nothing_writes_nothing() {
    let dir = TempDir::new("empty");
    let manifest = dir.path.join("Blastfile");
    std::fs::write(&manifest, BLASTFILE).expect("write the fixture");

    let edited = apply_edits(BLASTFILE, &BlastfileEdits::default()).expect("a no-op");
    assert_eq!(edited, BLASTFILE, "an empty learn run rewrote the document");
    write_atomic(&manifest, &edited).expect("the atomic write succeeds");
    assert_eq!(
        std::fs::read_to_string(&manifest).expect("read back"),
        BLASTFILE
    );
}
