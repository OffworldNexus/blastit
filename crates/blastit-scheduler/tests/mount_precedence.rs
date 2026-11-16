//! §9.2's four steps, end to end, over a `Blastfile` the product's own parser
//! read: a nested `static "/assets/v2"` beats `static "/assets"` on the longest
//! prefix, a mount that misses reaches the view, and the declared `fallback`
//! answers only a `GET`/`HEAD` no view claimed.
//!
//! The `critical_rewrite.rs` half asserts the same precedence as a total
//! function over one tree. This file asserts the **composition**: the manifest a
//! user writes, the scheduler's boot, its mount table and the wire — which is
//! what makes "a mount is the file's, not a route's" observable at all. Both
//! mounts below are declared at file level; no route carries a static policy,
//! and the view is only reachable because a mount miss falls through.
//!
//! The deny side is the same fixture: `static "/assets/v2"` has no `fallback`,
//! so `/assets/v2/gone.js` is a `404` even though `/assets` (which also matches
//! that path) declares one — the longest prefix is the one whose `fallback` may
//! answer (§9.2 step 3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use blastit_frame::Frame;
use common::{Options, Script, World, body_of, head, status_of};

/// The file a user writes: two mounts, one of them nested, with the fallback on
/// the outer one, and a view that claims a path under `/assets` the tree has no
/// file at, plus the `/app/` prefix.
const BLASTFILE: &str = concat!(
    "static \"/assets\" {\n",
    "  root \"public/assets\"\n",
    "  fallback \"index.html\"\n",
    "}\n",
    "\n",
    "static \"/assets/v2\" {\n",
    "  root \"public/v2\"\n",
    "}\n",
    "\n",
    "view app GET \"/app/*\", \"/assets/dyn\" {\n",
    "}\n",
);

/// The tree the mounts serve, removed on drop.
struct MountTree {
    base: PathBuf,
}

impl MountTree {
    /// `<base>/public/assets/{app.js,index.html,v2/app.js}` and
    /// `<base>/public/v2/app.js`.
    ///
    /// `v2/app.js` exists under **both** mounts with different bytes: that is
    /// what makes "the longest prefix answered" a body comparison instead of a
    /// header someone could set by accident.
    fn build() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "blastit-mount-precedence-{}-{nanos}",
            std::process::id()
        ));
        for (rel, body) in [
            ("public/assets/app.js", "the-outer-mount"),
            ("public/assets/index.html", "the-declared-fallback"),
            ("public/assets/v2/app.js", "the-outer-mount-relative"),
            ("public/v2/app.js", "the-nested-mount"),
        ] {
            let path = base.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        }
        let base = std::fs::canonicalize(&base).expect("canonical mount base");
        Self { base }
    }

    /// The base the mounts' `root`s are relative to.
    fn dir(&self) -> &Path {
        &self.base
    }
}

impl Drop for MountTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A world booting the `Blastfile` above over `tree`, with one view that answers
/// every request it receives with `view`.
fn world(tree: &MountTree) -> World {
    let mut options = Options::new().blastfile(BLASTFILE).script(
        0,
        Script::Answer {
            body: b"the-view".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.dir().to_path_buf();
    World::start(options)
}

/// Sends one request and returns its finished frames.
async fn round_trip(world: &mut World, id: u32, method: &str, target: &str) -> Vec<Frame> {
    world.http.send_request(id, head(method, target), b"").await;
    world.http.response(id).await
}

/// A response header's value, matched case-insensitively.
fn header_value<'a>(frames: &'a [Frame], name: &[u8]) -> Option<&'a [u8]> {
    match frames.first() {
        Some(Frame::RespHead(head)) => head
            .headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice()),
        other => panic!("no response head: {other:?}"),
    }
}

/// §9.2 step 1's "the longest prefix answers": `/assets/v2/app.js` exists under
/// **both** mounts (`public/assets/v2/app.js` and `public/v2/app.js`) and the
/// nested one serves it.
#[tokio::test]
async fn the_longest_mount_prefix_answers_a_file_both_mounts_could_serve() {
    tokio::time::pause();
    let tree = MountTree::build();
    let mut world = world(&tree);

    let frames = round_trip(&mut world, 1, "GET", "/assets/v2/app.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(
        body_of(&frames),
        b"the-nested-mount",
        "`static \"/assets/v2\"` is three segments deeper and must win"
    );

    // The same path under the outer mount alone is the outer mount's file: the
    // fixture is real, not a header trick, and both mounts really could answer.
    let frames = round_trip(&mut world, 2, "GET", "/assets/app.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-outer-mount");
    let frames = round_trip(&mut world, 3, "GET", "/assets/index.html").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-declared-fallback");
}

/// §9.2 step 2: a mount that answers no file must **not** answer `404`; the
/// view table is consulted, and it answers even though the mount declares a
/// `fallback`.
#[tokio::test]
async fn a_mount_miss_reaches_the_view_before_the_fallback() {
    tokio::time::pause();
    let tree = MountTree::build();
    let mut world = world(&tree);

    // `public/assets/dyn` does not exist, and the mount declares a `fallback`.
    // The view's pattern claims the path, so the view answers.
    let frames = round_trip(&mut world, 1, "GET", "/assets/dyn").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(
        body_of(&frames),
        b"the-view",
        "a missing file is step 2, not the mount's fallback"
    );
    assert!(
        world.log.lock().unwrap().request_ids(0) == [1],
        "the view really ran, one request"
    );

    // And the ordinary view path is untouched by the mounts beside it.
    let frames = round_trip(&mut world, 2, "GET", "/app/hello").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-view");
}

/// §9.2 step 3: no view claimed the path, so the **matching** mount's declared
/// `fallback` answers — on `GET` and `HEAD` only — and the nested mount, which
/// declares none, is a `404` instead of borrowing the outer one's.
#[tokio::test]
async fn the_declared_fallback_answers_after_the_views_and_only_get_and_head() {
    tokio::time::pause();
    let tree = MountTree::build();
    let mut world = world(&tree);

    // GET: the fallback file, and no-cache, which is what an `index`/`fallback`
    // answer is documented to carry (it is a file answer, never a redirect).
    let frames = round_trip(&mut world, 1, "GET", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-declared-fallback");
    assert_eq!(
        header_value(&frames, b"Cache-Control"),
        Some(&b"no-cache"[..]),
        "a fallback answer is always no-cache"
    );

    // HEAD: the same answer without a body frame, with the length it promised.
    let frames = round_trip(&mut world, 2, "HEAD", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"", "HEAD is GET without a body");
    assert_eq!(
        header_value(&frames, b"Content-Length"),
        Some(b"the-declared-fallback".len().to_string().as_bytes()),
        "the length of the file it did not write"
    );

    // POST: never the fallback (§9.2 step 3 is `GET`/`HEAD` only), and no view
    // pattern matched, so step 4 is a `404` — not a `405`, which needs a view
    // whose pattern matched.
    let frames = round_trip(&mut world, 3, "POST", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");
    assert!(
        world.log.lock().unwrap().received.is_empty(),
        "neither the fallback nor a view may answer a POST this way"
    );

    // The longest prefix is the mount whose fallback is consulted:
    // `/assets/v2/gone.js` matches `/assets/v2` (no fallback) and `/assets`
    // (which has one), and is a `404`.
    let frames = round_trip(&mut world, 4, "GET", "/assets/v2/gone.js").await;
    assert_eq!(
        status_of(&frames),
        404,
        "the outer mount's fallback must not answer a nested mount's miss: {frames:?}"
    );
    assert_ne!(
        body_of(&frames).as_slice(),
        b"the-declared-fallback",
        "the outer mount's fallback must not be read"
    );
}

/// The two mounts the scheduler boots are the file's, in file order, with their
/// own prefixes and policies: this is the manifest half of the test above, and
/// it fails if the boot ever folds a mount back into a route.
#[tokio::test]
async fn both_mounts_are_the_files_and_in_file_order() {
    tokio::time::pause();
    let tree = MountTree::build();
    let world = world(&tree);
    let mounts: Vec<(String, String)> = world
        .cfg
        .mounts
        .iter()
        .map(|mount| (mount.prefix.clone(), mount.policy.root.clone()))
        .collect();
    assert_eq!(
        mounts,
        vec![
            ("/assets".to_string(), "public/assets".to_string()),
            ("/assets/v2".to_string(), "public/v2".to_string()),
        ],
        "a mount is the file's, at the prefix it was written with"
    );
}
