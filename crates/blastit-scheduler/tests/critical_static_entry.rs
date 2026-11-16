//! The protected entry-script suite (`MB.3.33`): a static root never answers
//! `.php`/`.phtml`/`.phar` source, whatever the case, the method or the
//! declared body, and never opens the file for reading.
//!
//! The canonical manifest declares `entrypoint php "public/index.php"` beside a
//! `static "/" { root "public" }` mount, so the front controller legitimately
//! lives under the served root. The owner's decision (2026-11-14) is a per-request
//! `404` rather than a boot refusal: `root` may contain `entry`, and nothing
//! `.php`/`.phar` is ever served. Every row runs over both backends, and the
//! suite asserts the file was not read-opened through the `resolve_with` hook
//! (which fires between the `O_PATH` probe and the `O_RDONLY` open) plus a
//! `000`-mode source that must still be `404`, never a read failure.
//!
//! `GET /assets/app.css` is the positive twin: exact bytes, the CSS content
//! type, and the untouched `405`/`413` gates on a servable asset.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/static_fixture.rs"]
mod static_fixture;

use std::io::Read;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use blastit_scheduler::rewrite::{Rewrite, decide};
use blastit_scheduler::static_asset::{
    AnswerKind, Backend, StaticFile, StaticMeta, StaticOutcome, StaticRoot, head_segments,
    is_php_source,
};
use blastit_scheduler::static_response::{StaticBody, StaticResponse};
use rustix::fs::{Mode, chmod};
use static_fixture::{Fixture, StaticTree};

/// The bytes a PHP source file must never leak.
const SOURCE_BYTES: &[u8] = b"<?php echo 'fixture-source-secret';\n";
/// A `.phar` is an executable PHP archive: same rule, same bytes.
const PHAR_BYTES: &[u8] = b"<?php __HALT_COMPILER(); ?>\n";
/// The positive twin's exact body.
const CSS_BYTES: &[u8] = b"body{margin:0}\n";
/// A name that merely *contains* `.php` is still an ordinary asset.
const MANUAL_BYTES: &[u8] = b"not php source\n";
/// The file a directory literally named `dir.php` serves; the directory name
/// must not hide its non-source child.
const DIR_HTML: &[u8] = b"<!doctype html><title>dir</title>\n";

/// The fixture tree plus the source files this suite needs.
fn tree_with_sources() -> StaticTree {
    let tree = StaticTree::build(Fixture::S);
    std::fs::create_dir_all(tree.path("dir.php")).expect("fixture dir.php");
    for (rel, bytes) in [
        ("index.php", SOURCE_BYTES),
        ("INDEX.PHP", SOURCE_BYTES),
        ("index.Php", SOURCE_BYTES),
        ("shell.PhAr", PHAR_BYTES),
        ("x.phar", PHAR_BYTES),
        ("x.phtml", SOURCE_BYTES),
        ("INDEX.PHTML", SOURCE_BYTES),
        ("mode000.php", SOURCE_BYTES),
        ("dir.php/index.html", DIR_HTML),
        ("assets/app.css", CSS_BYTES),
        ("docs/manual.php.txt", MANUAL_BYTES),
    ] {
        std::fs::write(tree.path(rel), bytes).unwrap_or_else(|error| {
            panic!("fixture {}: {error}", rel);
        });
    }
    chmod(tree.path("mode000.php"), Mode::from_bits_truncate(0o000)).expect("fixture mode000.php");
    tree
}

/// The production static composition: the head (`head_segments`) then the
/// filesystem half (`resolve`).
fn head_outcome(root: &StaticRoot, target: &str) -> StaticOutcome {
    match head_segments(target) {
        Ok(owned) => {
            let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
            root.resolve(&parts)
        }
        Err(outcome) => outcome,
    }
}

/// The status the whole static decision answers for one request.
fn reject_status(root: &StaticRoot, target: &str, method: &[u8], has_body: bool) -> u16 {
    match decide(head_outcome(root, target), method, has_body, true) {
        Rewrite::Reject { status, .. } => status,
        Rewrite::Serve(_) => panic!("{target}: served"),
        Rewrite::Forward => panic!("{target}: forwarded to the app"),
    }
}

/// `head -> resolve -> decide -> respond` for the positive rows.
fn serve(root: &StaticRoot, target: &str) -> StaticResponse {
    match decide(head_outcome(root, target), b"GET", false, false) {
        Rewrite::Serve(file) => root.respond(file, b"GET", None),
        Rewrite::Reject { status, .. } => panic!("{target}: rejected with {status}"),
        Rewrite::Forward => panic!("{target}: forwarded"),
    }
}

fn body_of(body: StaticBody) -> Vec<u8> {
    let mut file = std::fs::File::from(body.file);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("read the served body");
    bytes
}

fn header<'a>(response: &'a StaticResponse, name: &str) -> Option<&'a [u8]> {
    response
        .headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name.as_bytes()))
        .map(|(_, value)| value.as_slice())
}

/// A [`StaticFile`] over a real descriptor, with an invented `url_path`: the
/// only way to reach `respond`'s defence-in-depth source arm, which `resolve`
/// makes unreachable.
fn forged(tree: &StaticTree, rel: &str, url_path: &str) -> StaticFile {
    let file = std::fs::File::open(tree.path(rel)).expect("forged file opens");
    let metadata = file.metadata().expect("forged metadata");
    let modified = metadata
        .modified()
        .expect("forged mtime")
        .duration_since(UNIX_EPOCH)
        .expect("a post-epoch fixture mtime");
    let len = metadata.len();
    StaticFile {
        url_path: url_path.to_string(),
        kind: AnswerKind::Exact,
        file: OwnedFd::from(file),
        len,
        meta: StaticMeta {
            size: len,
            mtime_secs: i64::try_from(modified.as_secs()).unwrap_or(0),
            mtime_nsec: modified.subsec_nanos(),
        },
    }
}

#[test]
fn php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        for target in [
            "/index.php",
            "/INDEX.PHP",
            "/index.Php",
            "/shell.PhAr",
            "/x.phar",
            "/x.phtml",
            "/INDEX.PHTML",
            "/index%2Ephp",
            "/index.php?x=1",
            "/index.php/",
        ] {
            // Step 3/4 of the head refuses the request before `resolve`,
            // whatever the file's mode or existence.
            assert!(
                matches!(head_segments(target), Err(StaticOutcome::Hidden)),
                "{backend:?} {target}: the head must hide every source path"
            );
            // The resolver owns the same rule, so a direct caller cannot
            // bypass the head.
            assert!(
                matches!(head_outcome(&root, target), StaticOutcome::Hidden),
                "{backend:?} {target}: the resolver must hide every source path"
            );
            for (method, has_body) in [
                (b"GET".as_slice(), false),
                (b"HEAD".as_slice(), false),
                (b"POST".as_slice(), false),
                (b"POST".as_slice(), true),
            ] {
                // 404, not 405/413: the source gate outranks the method and
                // body gates, and a route with an `app` never forwards it.
                assert_eq!(
                    reject_status(&root, target, method, has_body),
                    404,
                    "{backend:?} {target} with {method:?} has_body={has_body}"
                );
            }
        }

        // The last segment decides: a `.php` earlier in the path or inside the
        // name is not source under this rule, and must not be over-blocked.
        assert!(matches!(
            head_segments("/docs/manual.php.txt"),
            Ok(parts) if parts == ["docs", "manual.php.txt"]
        ));
        assert!(is_php_source("index.php") && is_php_source("x.phar"));
        assert!(is_php_source("x.phtml") && is_php_source("INDEX.PHTML"));
        assert!(!is_php_source("manual.php.txt") && !is_php_source("php"));
    }
}

#[test]
fn a_source_file_is_hidden_at_resolve_without_any_read_open() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        for segments in [
            vec!["index.php"],
            vec!["INDEX.PHP"],
            vec!["index.Php"],
            vec!["shell.PhAr"],
            vec!["x.phar"],
            vec!["x.phtml"],
            vec!["INDEX.PHTML"],
        ] {
            let read_open = AtomicBool::new(false);
            let hook = || read_open.store(true, Ordering::SeqCst);
            // The hook fires in `finish`, between the `O_PATH` probe and the
            // `O_RDONLY` open. A source path must never reach it.
            assert!(
                matches!(
                    root.resolve_with(&segments, Some(&hook)),
                    StaticOutcome::Hidden
                ),
                "{backend:?} {segments:?}"
            );
            assert!(
                !read_open.load(Ordering::SeqCst),
                "{backend:?} {segments:?}: a source descriptor reached the read open"
            );
        }
    }
}

#[test]
fn an_unreadable_php_file_is_404_not_a_read_failure() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        let read_open = AtomicBool::new(false);
        let hook = || read_open.store(true, Ordering::SeqCst);
        // `000` would be `EACCES` at a read open; the gate answers before it.
        assert!(matches!(
            root.resolve_with(&["mode000.php"], Some(&hook)),
            StaticOutcome::Hidden
        ));
        assert!(
            !read_open.load(Ordering::SeqCst),
            "{backend:?}: the 000-mode source reached the read open"
        );
        assert!(matches!(
            head_outcome(&root, "/mode000.php"),
            StaticOutcome::Hidden
        ));
        assert_eq!(reject_status(&root, "/mode000.php", b"GET", false), 404);
        assert_eq!(reject_status(&root, "/mode000.php", b"POST", true), 404);
    }
}

#[test]
fn a_forged_source_url_path_is_never_streamed() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        // `resolve` makes these unreachable, so a forged `StaticFile` is the
        // only way to prove the responder's own arm: a trailing `/` or a query
        // must not hide the final segment.
        for url_path in ["/x.php/", "/x.php?y=1", "/INDEX.PHP/", "/x.phtml"] {
            let response = root.respond(forged(&tree, "assets/app.css", url_path), b"GET", None);
            assert_eq!(response.status, 404, "{backend:?} {url_path}");
            assert_eq!(response.reason, "Not Found", "{backend:?} {url_path}");
            assert!(
                response.body.is_none(),
                "{backend:?} {url_path}: a refusal has no body"
            );
            assert_eq!(
                header(&response, "Content-Length"),
                Some(b"0".as_slice()),
                "{backend:?} {url_path}"
            );
            assert_eq!(
                header(&response, "Cache-Control"),
                Some(b"no-store".as_slice()),
                "{backend:?} {url_path}"
            );
        }
    }
}

#[test]
fn a_normal_asset_is_still_served_exactly_and_its_gates_are_unchanged() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);

        // The positive twin: exact bytes, the CSS content type.
        assert!(matches!(
            head_segments("/assets/app.css"),
            Ok(parts) if parts == ["assets", "app.css"]
        ));
        let response = serve(&root, "/assets/app.css");
        assert_eq!(response.status, 200, "{backend:?}");
        assert_eq!(
            header(&response, "Content-Type"),
            Some(b"text/css; charset=utf-8".as_slice()),
            "{backend:?}"
        );
        assert_eq!(
            body_of(response.body.expect("a GET body")),
            CSS_BYTES,
            "{backend:?}"
        );

        // A suffix-adjacent name is not PHP source.
        let response = serve(&root, "/docs/manual.php.txt");
        assert_eq!(response.status, 200, "{backend:?}");
        assert_eq!(
            body_of(response.body.expect("a GET body")),
            MANUAL_BYTES,
            "{backend:?}"
        );

        // Dotfiles keep their own `Hidden` rule, and traversal keeps its `400`.
        assert!(matches!(
            head_outcome(&root, "/.env"),
            StaticOutcome::Hidden
        ));
        assert_eq!(reject_status(&root, "/.env", b"GET", false), 404);
        assert_eq!(reject_status(&root, "/../etc/passwd", b"GET", false), 400);

        // A directory literally named `dir.php` is not served itself, but its
        // non-source child is: the rule is on the final segment, not a path
        // substring.
        assert_eq!(reject_status(&root, "/dir.php", b"GET", false), 404);
        assert_eq!(reject_status(&root, "/dir.php/", b"GET", false), 404);
        let response = serve(&root, "/dir.php/index.html");
        assert_eq!(response.status, 200, "{backend:?}");
        assert_eq!(
            header(&response, "Content-Type"),
            Some(b"text/html; charset=utf-8".as_slice()),
            "{backend:?}"
        );
        assert_eq!(
            body_of(response.body.expect("a GET body")),
            DIR_HTML,
            "{backend:?}"
        );

        // The method and body gates still apply to a servable asset: the
        // source gate did not move or weaken them.
        assert_eq!(reject_status(&root, "/assets/app.css", b"POST", false), 405);
        assert_eq!(reject_status(&root, "/assets/app.css", b"GET", true), 413);
    }
}

#[test]
fn a_source_index_never_answers_the_directory_or_the_spa_fallback() {
    let tree = tree_with_sources();
    for backend in [Backend::Openat2, Backend::Walk] {
        let mut policy = Fixture::S.policy();
        // The manifest permits any non-dotfile `index`; `.php` is the one that
        // would publish source at `/` and `/docs`. `fallback` is its own file
        // now (§9.3), so both names have to name the source to prove every
        // answer the root controls is guarded.
        policy.index = "index.php".to_string();
        policy.fallback = Some("index.php".to_string());
        let root =
            StaticRoot::open_with(&tree.base, &policy, backend).expect("the fixture root opens");
        for target in ["/", "/docs", "/dashboard"] {
            let outcome = head_outcome(&root, target);
            assert!(
                !matches!(outcome, StaticOutcome::File(_)),
                "{backend:?} {target}: a .php index answered"
            );
            assert_eq!(reject_status(&root, target, b"GET", false), 404);
        }
    }
}
