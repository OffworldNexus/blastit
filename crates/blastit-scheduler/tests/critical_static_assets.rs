//! The protected response suite of design §4.5/§7.2 (`MB.2.4.2`): the header,
//! cache, conditional, `Content-Type` and `HEAD`/`304` tables, the `ETag`
//! identity, the bounded body and the statuses — the last through
//! `rewrite::decide`, never through a second copy of the precedence.
//!
//! The 40-row attack matrix's *resolution* half is `critical_static_resolve.rs`
//! (`MB.2.4.1`); this file owns what a resolved [`StaticFile`] turns into. Every
//! rule with two directions is asserted in both: an exact file earns the year
//! while the `index` it shares bytes with stays `no-cache`, a listed `ETag`
//! answers `304` while an absent one answers `200` with the body, and a `HEAD`
//! keeps the `GET` head while dropping only the bytes.
//!
//! Every positive row asserts its body's sha256 and the answering file's exact
//! `ETag`, so an empty or wrong `200` cannot pass, and every row runs over both
//! `Backend::Openat2` and `Backend::Walk`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/static_fixture.rs"]
mod static_fixture;

use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use blastit_scheduler::rewrite::{Rewrite, decide};
use blastit_scheduler::static_asset::{
    AnswerKind, Backend, STATIC_MAX_FILE_BYTES, StaticFile, StaticMeta, StaticOutcome, StaticRoot,
    head_segments,
};
use blastit_scheduler::static_response::{StaticResponse, http_date};
use static_fixture::{APP_JS, APP_JS_BYTES, Fixture, StaticTree, content_of};

const APP_URL: &str = "/assets/app.4f1c2a9b7d3e.js";
const TOKEN_URL: &str = "/.well-known/acme-challenge/token";
const INDEX_URL: &str = "/index.html";
const DOCS_URL: &str = "/docs/index.html";
const CAFE_URL: &str = "/café.js";

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NO_CACHE: &str = "no-cache";
const NOSNIFF: &[u8] = b"nosniff";

// ---------------------------------------------------------------------------
// The composition the scheduler runs: head → resolve → decide → respond.
// ---------------------------------------------------------------------------

fn outcome(root: &StaticRoot, target: &str) -> StaticOutcome {
    match head_segments(target) {
        Ok(owned) => {
            let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
            root.resolve(&parts)
        }
        // A refusal from the step-3/4 head is already the outcome.
        Err(outcome) => outcome,
    }
}

fn serve(
    root: &StaticRoot,
    target: &str,
    method: &[u8],
    if_none_match: Option<&[u8]>,
) -> StaticResponse {
    match decide(outcome(root, target), method, false, false) {
        Rewrite::Serve(file) => root.respond(file, method, if_none_match),
        Rewrite::Reject { status, .. } => panic!("`{target}` was rejected with {status}"),
        Rewrite::Forward => panic!("`{target}` forwarded to the app"),
    }
}

fn head<'a>(response: &'a StaticResponse, name: &str) -> Option<&'a [u8]> {
    response
        .headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name.as_bytes()))
        .map(|(_, value)| value.as_slice())
}

fn head_count(response: &StaticResponse, name: &str) -> usize {
    response
        .headers
        .iter()
        .filter(|(header, _)| header.eq_ignore_ascii_case(name.as_bytes()))
        .count()
}

fn read_body(body: blastit_scheduler::static_response::StaticBody) -> Vec<u8> {
    let mut file = std::fs::File::from(body.file);
    let mut bytes = vec![0u8; usize::try_from(body.len).expect("the body fits in memory")];
    file.read_exact(&mut bytes).expect("read the served body");
    bytes
}

/// The exact `ETag` of a file, recomputed from `stat(2)` by the suite: an
/// implementation that hashed something else cannot satisfy this comparison.
fn etag_from_path(path: &Path) -> String {
    let meta = std::fs::metadata(path).expect("fixture metadata");
    let modified = meta
        .modified()
        .expect("fixture mtime")
        .duration_since(UNIX_EPOCH)
        .expect("a post-epoch fixture mtime");
    format!("\"{:x}-{:x}\"", meta.len(), modified.as_nanos())
}

/// A forged [`StaticFile`] over `tree.path(rel)`, for the ceiling arm that the
/// resolver's own classification makes unreachable.
fn forged(tree: &StaticTree, rel: &str, url_path: &str, len: u64) -> StaticFile {
    forged_meta(tree, rel, url_path, len, len)
}

/// A forged [`StaticFile`] whose `len` and `meta.size` disagree — the only way
/// to reach each ceiling disjunct of `respond` on its own, since `resolve`
/// always sets `meta.size = len` (`G6`).
fn forged_meta(tree: &StaticTree, rel: &str, url_path: &str, len: u64, size: u64) -> StaticFile {
    let file = std::fs::File::open(tree.path(rel)).expect("forged file opens");
    let meta = file.metadata().expect("forged metadata");
    let modified = meta
        .modified()
        .expect("mtime")
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch mtime");
    StaticFile {
        url_path: url_path.to_string(),
        kind: AnswerKind::Exact,
        file: OwnedFd::from(file),
        len,
        meta: StaticMeta {
            size,
            mtime_secs: i64::try_from(modified.as_secs()).unwrap_or(0),
            mtime_nsec: modified.subsec_nanos(),
        },
    }
}

/// The one [`StaticFile`] `decide` hands the responder for a servable route.
fn resolved(root: &StaticRoot, target: &str) -> StaticFile {
    match decide(outcome(root, target), b"GET", false, false) {
        Rewrite::Serve(file) => file,
        Rewrite::Reject { status, .. } => panic!("`{target}` was rejected with {status}"),
        Rewrite::Forward => panic!("`{target}` forwarded to the app"),
    }
}

/// How many open descriptors point inside `base`: the root's one `O_PATH`
/// dirfd, plus one per `StaticFile`/`StaticBody` in flight (design §4.2, §7.1).
///
/// The filter is the fixture's own base, so descriptors other tests in this
/// binary hold cannot move the count: every test builds its own tree. A `dup`
/// in `respond` (or a body that outlives its response) adds exactly one here.
fn open_in_tree(base: &Path) -> usize {
    let base = std::fs::canonicalize(base).expect("canonical fixture base");
    std::fs::read_dir("/proc/self/fd")
        .expect("the descriptor oracle needs /proc/self/fd")
        .flatten()
        .filter(|entry| {
            std::fs::read_link(entry.path())
                .map(|target| target.starts_with(&base))
                .unwrap_or(false)
        })
        .count()
}

// ---------------------------------------------------------------------------
// The response table (design §4.5).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Case {
    fixture: Fixture,
    target: &'static str,
    url: &'static str,
    content_type: &'static str,
    cache: &'static str,
}

fn cases() -> Vec<Case> {
    let case = |fixture, target, url, content_type, cache| Case {
        fixture,
        target,
        url,
        content_type,
        cache,
    };
    vec![
        // Fixture S: hashed assets earn the year, `index` answers never do.
        case(Fixture::S, APP_URL, APP_URL, "text/javascript", IMMUTABLE),
        case(
            Fixture::S,
            "/assets/app.4f1c2a9b7d3e.js/",
            APP_URL,
            "text/javascript",
            IMMUTABLE,
        ),
        case(
            Fixture::S,
            "/assets/app.4f1c2a9b7d3e.js?v=1",
            APP_URL,
            "text/javascript",
            IMMUTABLE,
        ),
        case(
            Fixture::S,
            "/assets//app.4f1c2a9b7d3e.js",
            APP_URL,
            "text/javascript",
            IMMUTABLE,
        ),
        case(
            Fixture::S,
            INDEX_URL,
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/docs",
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/docs/",
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            DOCS_URL,
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/docs%2Findex.html",
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        // The page's deep-URL row and a hashed-looking miss: both answer
        // `index.html`, so the request path must not be globbed.
        case(
            Fixture::S,
            "/dashboard/settings",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/assets/missing.js",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            TOKEN_URL,
            TOKEN_URL,
            "application/octet-stream",
            NO_CACHE,
        ),
        case(
            Fixture::S,
            "/caf%C3%A9.js",
            CAFE_URL,
            "text/javascript",
            NO_CACHE,
        ),
        // Fixture D: the SAME file is `immutable` as an exact answer and
        // `no-cache` as every index answer (design §4.5 row 34/35).
        case(
            Fixture::D,
            INDEX_URL,
            INDEX_URL,
            "text/html; charset=utf-8",
            IMMUTABLE,
        ),
        case(
            Fixture::D,
            "/",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::D,
            "/dashboard",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(Fixture::D, APP_URL, APP_URL, "text/javascript", NO_CACHE),
        // Fixture F (front controller) and N (all-static, no app) keep the same
        // tree but declare no `immutable`, so their exact asset is `no-cache`:
        // the year is a configured glob, not a property of a hashed name.
        case(Fixture::F, APP_URL, APP_URL, "text/javascript", NO_CACHE),
        case(
            Fixture::F,
            "/docs",
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::F,
            INDEX_URL,
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(Fixture::N, APP_URL, APP_URL, "text/javascript", NO_CACHE),
        case(
            Fixture::N,
            "/docs",
            DOCS_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
        case(
            Fixture::N,
            "/",
            INDEX_URL,
            "text/html; charset=utf-8",
            NO_CACHE,
        ),
    ]
}

fn with_each_case(mut check: impl FnMut(&StaticTree, &StaticRoot, &Case)) {
    let tree = StaticTree::build(Fixture::S);
    for fixture in [Fixture::S, Fixture::D, Fixture::F, Fixture::N] {
        for backend in [Backend::Openat2, Backend::Walk] {
            let root = tree.open(fixture, backend);
            for case in cases().iter().filter(|case| case.fixture == fixture) {
                check(&tree, &root, case);
            }
        }
    }
}

fn check_case(tree: &StaticTree, root: &StaticRoot, case: &Case) {
    let mut response = serve(root, case.target, b"GET", None);
    let label = format!("{:?} `{}`", case.fixture, case.target);
    assert_eq!(response.status, 200, "{label}");
    assert_eq!(response.reason, "OK", "{label}");
    assert_eq!(
        head(&response, "Content-Type"),
        Some(case.content_type.as_bytes()),
        "{label}"
    );
    assert_eq!(
        head(&response, "Cache-Control"),
        Some(case.cache.as_bytes()),
        "{label}"
    );
    assert_eq!(
        head(&response, "X-Content-Type-Options"),
        Some(NOSNIFF),
        "{label}"
    );
    let path = tree.path(&case.url[1..]);
    assert_eq!(
        head(&response, "ETag"),
        Some(etag_from_path(&path).as_bytes()),
        "{label}: the ETag is the stat-derived strong value"
    );
    let mtime = std::fs::metadata(&path)
        .expect("fixture metadata")
        .modified()
        .expect("fixture mtime")
        .duration_since(UNIX_EPOCH)
        .expect("post-epoch mtime")
        .as_secs();
    assert_eq!(
        head(&response, "Last-Modified"),
        Some(http_date(i64::try_from(mtime).expect("mtime fits i64")).as_bytes()),
        "{label}: Last-Modified is the file's mtime"
    );

    let body = response.body.take().expect("a GET has a body");
    assert_eq!(body.offset, 0, "{label}");
    let bytes = read_body(body);
    assert_eq!(
        sha256_hex(&bytes),
        digest_of(case.url),
        "{label}: the served bytes' sha256"
    );
    assert_eq!(bytes, content_of(case.url), "{label}: the fixture's bytes");
    assert_eq!(
        head(&response, "Content-Length"),
        Some(bytes.len().to_string().as_bytes()),
        "{label}"
    );

    // Never set by this responder (design §4.5).
    for forbidden in [
        "Date",
        "Server",
        "Accept-Ranges",
        "Location",
        "Content-Encoding",
        "Transfer-Encoding",
    ] {
        assert_eq!(head(&response, forbidden), None, "{label}: {forbidden}");
    }
}

#[test]
fn every_response_table_row_answers_the_design() {
    with_each_case(check_case);
}

#[test]
fn head_answers_the_get_head_and_no_body() {
    with_each_case(|_, root, case| {
        let get = serve(root, case.target, b"GET", None);
        let head_response = serve(root, case.target, b"HEAD", None);
        assert_eq!(head_response.status, 200);
        assert_eq!(head_response.reason, "OK");
        assert_eq!(
            head_response.headers, get.headers,
            "`{}`: HEAD must answer GET's head",
            case.target
        );
        assert!(
            head_response.body.is_none(),
            "`{}`: HEAD has no body",
            case.target
        );
        assert_eq!(head_count(&head_response, "Content-Length"), 1);
        assert_eq!(
            head(&head_response, "Content-Length"),
            Some(content_of(case.url).len().to_string().as_bytes()),
            "`{}`: HEAD promises the would-be body's length",
            case.target
        );
        assert!(get.body.is_some());
    });
}

#[test]
fn the_etag_is_exact_stable_and_moves_with_the_mtime() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    let first = serve(&root, APP_URL, b"GET", None);
    let etag = head(&first, "ETag").expect("an ETag").to_vec();
    assert_eq!(
        etag,
        etag_from_path(&tree.path(APP_JS)).as_bytes(),
        "the one exact ETag of the fixture asset"
    );

    // Stable across two GETs and equal across every alias of the same file.
    for target in [
        APP_URL,
        "/assets/app.4f1c2a9b7d3e.js/",
        "/assets/app.4f1c2a9b7d3e.js?v=2",
        "/assets//app.4f1c2a9b7d3e.js",
    ] {
        let again = serve(&root, target, b"GET", None);
        assert_eq!(head(&again, "ETag"), Some(etag.as_slice()), "{target}");
        assert_eq!(
            head(&again, "Content-Length"),
            head(&first, "Content-Length")
        );
        assert_eq!(head(&again, "Cache-Control"), Some(IMMUTABLE.as_bytes()));
    }

    // The same file has the same ETag as an index, but not the same cache.
    let d = tree.open(Fixture::D, Backend::Openat2);
    let exact = serve(&d, INDEX_URL, b"GET", None);
    let index = serve(&d, "/", b"GET", None);
    assert_eq!(head(&exact, "ETag"), head(&index, "ETag"));
    assert_eq!(head(&index, "Cache-Control"), Some(NO_CACHE.as_bytes()));

    // A rewrite that moves the mtime (same bytes) moves the ETag.
    std::fs::File::options()
        .write(true)
        .open(tree.path(APP_JS))
        .expect("open the asset for utimensat")
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .expect("set a known future mtime");
    let rewritten = serve(&root, APP_URL, b"GET", None);
    assert_ne!(
        head(&rewritten, "ETag"),
        Some(etag.as_slice()),
        "the ETag must change with the mtime"
    );
    assert_eq!(
        head(&rewritten, "ETag"),
        Some(etag_from_path(&tree.path(APP_JS)).as_bytes())
    );
    assert_eq!(
        head(&rewritten, "Last-Modified"),
        Some(b"Tue, 14 Nov 2023 22:13:20 GMT".as_slice())
    );
}

#[test]
fn if_none_match_answers_304_and_repeats_the_head() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    let first = serve(&root, APP_URL, b"GET", None);
    let etag = String::from_utf8(head(&first, "ETag").expect("an ETag").to_vec()).expect("ASCII");
    let last_modified = head(&first, "Last-Modified")
        .expect("Last-Modified")
        .to_vec();

    let matching = [
        etag.clone(),
        format!("W/{etag}"),
        "*".to_string(),
        format!("\"other\", {etag} , W/{etag}"),
    ];
    for candidate in &matching {
        for method in [b"GET".as_slice(), b"HEAD".as_slice()] {
            let response = serve(&root, APP_URL, method, Some(candidate.as_bytes()));
            assert_eq!(response.status, 304, "{candidate}");
            assert_eq!(response.reason, "Not Modified");
            assert!(response.body.is_none(), "a 304 has no body");
            assert_eq!(head(&response, "ETag"), Some(etag.as_bytes()));
            assert_eq!(
                head(&response, "Last-Modified"),
                Some(last_modified.as_slice())
            );
            assert_eq!(head(&response, "Cache-Control"), Some(IMMUTABLE.as_bytes()));
            assert_eq!(head(&response, "X-Content-Type-Options"), Some(NOSNIFF));
            assert_eq!(
                head(&response, "Content-Length"),
                None,
                "a 304 has no Content-Length"
            );
            assert_eq!(
                head(&response, "Content-Type"),
                None,
                "a 304 repeats no Content-Type"
            );
        }
    }
    // An `index` answer's 304 repeats `no-cache`, not the year (fixture D).
    let d = tree.open(Fixture::D, Backend::Openat2);
    let index = serve(&d, "/dashboard", b"GET", None);
    let index_etag = head(&index, "ETag").expect("an ETag").to_vec();
    let conditional = serve(&d, "/dashboard", b"GET", Some(&index_etag));
    assert_eq!(conditional.status, 304);
    assert_eq!(
        head(&conditional, "Cache-Control"),
        Some(NO_CACHE.as_bytes())
    );

    // Absent or non-matching: a full 200 with the body.
    for candidate in [
        None,
        Some(b"\"not-the-tag\"".as_slice()),
        Some(b"W/\"not-the-tag\"".as_slice()),
    ] {
        let response = serve(&root, APP_URL, b"GET", candidate);
        assert_eq!(response.status, 200);
        assert!(response.body.is_some());
    }
}

#[test]
fn the_cache_header_is_keyed_on_the_answering_file() {
    let tree = StaticTree::build(Fixture::S);

    // S: `/index.html` is an exact-file answer that matches no pattern, so
    // `no-cache`; a hashed-looking miss answers the same file as a fallback.
    let s = tree.open(Fixture::S, Backend::Openat2);
    assert_eq!(
        head(&serve(&s, INDEX_URL, b"GET", None), "Cache-Control"),
        Some(NO_CACHE.as_bytes())
    );
    let fallback = serve(&s, "/assets/missing.js", b"GET", None);
    assert_eq!(
        head(&fallback, "ETag"),
        head(&serve(&s, INDEX_URL, b"GET", None), "ETag")
    );
    assert_eq!(head(&fallback, "Cache-Control"), Some(NO_CACHE.as_bytes()));

    // D flips the pattern to `/index.html`: the exact answer earns the year
    // while `/` and `/dashboard` (the same bytes) stay `no-cache`.
    let d = tree.open(Fixture::D, Backend::Openat2);
    assert_eq!(
        head(&serve(&d, INDEX_URL, b"GET", None), "Cache-Control"),
        Some(IMMUTABLE.as_bytes())
    );
    for index_answer in ["/", "/dashboard"] {
        let response = serve(&d, index_answer, b"GET", None);
        assert_eq!(response.status, 200);
        assert_eq!(
            head(&response, "Cache-Control"),
            Some(NO_CACHE.as_bytes()),
            "{index_answer}"
        );
    }
    // …and D gives the hashed asset `no-cache`: its pattern does not name it.
    assert_eq!(
        head(&serve(&d, APP_URL, b"GET", None), "Cache-Control"),
        Some(NO_CACHE.as_bytes())
    );

    // The other direction of the same rule: a front-controller policy that
    // *does* declare the page's pattern earns the year for the exact asset
    // (design §5.2 rows 32/33), while an index answer keeps `no-cache`.
    let mut front = Fixture::F.policy();
    front.immutable = vec!["/assets/*".to_string()];
    let front_root =
        StaticRoot::open_with(&tree.base, &front, Backend::Openat2).expect("root opens");
    assert_eq!(
        head(&serve(&front_root, APP_URL, b"GET", None), "Cache-Control"),
        Some(IMMUTABLE.as_bytes())
    );
    assert_eq!(
        head(&serve(&front_root, "/", b"GET", None), "Cache-Control"),
        Some(NO_CACHE.as_bytes())
    );
}

// ---------------------------------------------------------------------------
// The closed `Content-Type` table.
// ---------------------------------------------------------------------------

#[test]
fn the_content_type_table_is_closed() {
    let tree = StaticTree::build(Fixture::N);
    let root = tree.open(Fixture::N, Backend::Openat2);
    let rows: &[(&str, &str)] = &[
        ("page.html", "text/html; charset=utf-8"),
        ("page.htm", "text/html; charset=utf-8"),
        ("app.js", "text/javascript"),
        ("app.mjs", "text/javascript"),
        ("style.css", "text/css; charset=utf-8"),
        ("data.json", "application/json"),
        ("bundle.js.map", "application/json"),
        ("site.webmanifest", "application/manifest+json"),
        ("notes.txt", "text/plain; charset=utf-8"),
        ("feed.xml", "application/xml"),
        ("logo.svg", "image/svg+xml"),
        ("photo.PNG", "image/png"),
        ("photo.png", "image/png"),
        ("photo.jpg", "image/jpeg"),
        ("photo.jpeg", "image/jpeg"),
        ("photo.gif", "image/gif"),
        ("photo.webp", "image/webp"),
        ("photo.avif", "image/avif"),
        ("favicon.ico", "image/x-icon"),
        ("font.woff", "font/woff"),
        ("font.woff2", "font/woff2"),
        ("font.ttf", "font/ttf"),
        ("font.otf", "font/otf"),
        ("module.wasm", "application/wasm"),
        ("doc.pdf", "application/pdf"),
        ("clip.mp4", "video/mp4"),
        ("clip.webm", "video/webm"),
        ("sound.mp3", "audio/mpeg"),
        ("archive.zzz", "application/octet-stream"),
        ("Makefile", "application/octet-stream"),
    ];
    for (name, expected) in rows {
        std::fs::write(tree.path(name), b"fixture").expect("fixture content-type file");
        let target = format!("/{name}");
        let response = serve(&root, &target, b"GET", None);
        assert_eq!(response.status, 200, "{name}");
        assert_eq!(
            head(&response, "Content-Type"),
            Some(expected.as_bytes()),
            "{name}"
        );
    }
}

// ---------------------------------------------------------------------------
// The bodies, the ceiling and the statuses.
// ---------------------------------------------------------------------------

#[test]
fn a_zero_length_file_is_a_200_with_length_zero() {
    let tree = StaticTree::build(Fixture::N);
    std::fs::write(tree.path("empty.css"), b"").expect("fixture empty file");
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::N, backend);
        for method in [b"GET".as_slice(), b"HEAD".as_slice()] {
            let response = serve(&root, "/empty.css", method, None);
            assert_eq!(response.status, 200);
            assert_eq!(head(&response, "Content-Length"), Some(b"0".as_slice()));
            assert_eq!(
                head(&response, "Content-Type"),
                Some(b"text/css; charset=utf-8".as_slice())
            );
            match response.body {
                Some(body) => {
                    assert_eq!(method, b"GET");
                    assert_eq!(body.len, 0);
                    assert!(read_body(body).is_empty());
                }
                None => assert_eq!(method, b"HEAD"),
            }
        }
    }
}

#[test]
fn an_oversize_file_is_a_404_and_respond_never_promises_it() {
    let tree = StaticTree::build(Fixture::N);
    let root = tree.open(Fixture::N, Backend::Openat2);

    // The documented pipeline: `classify` refuses it before any header exists.
    assert!(matches!(outcome(&root, "/big.bin"), StaticOutcome::Hidden));
    assert!(matches!(
        decide(outcome(&root, "/big.bin"), b"GET", false, false),
        Rewrite::Reject {
            status: 404,
            allow: None
        }
    ));

    // The defence-in-depth arm: a file above `static_max_bytes` is refused,
    // never streamed under a promise this responder could not keep.
    let response = root.respond(
        forged(&tree, "big.bin", "/big.bin", STATIC_MAX_FILE_BYTES + 1),
        b"GET",
        None,
    );
    assert_eq!(response.status, 404);
    assert_eq!(response.reason, "Not Found");
    assert!(response.body.is_none());
    assert_eq!(head(&response, "Content-Length"), Some(b"0".as_slice()));
    assert_eq!(
        head(&response, "Cache-Control"),
        Some(b"no-store".as_slice())
    );

    // Its twin: the same construction at the ceiling's size is a normal 200.
    let served = root.respond(
        forged(&tree, APP_JS, APP_URL, APP_JS_BYTES.len() as u64),
        b"GET",
        None,
    );
    assert_eq!(served.status, 200);
    assert_eq!(
        head(&served, "Content-Length"),
        Some(APP_JS_BYTES.len().to_string().as_bytes())
    );
}

#[test]
fn a_response_moves_the_one_descriptor_and_never_duplicates_it() {
    // Design §4.2: "consumes the file: the descriptor moves into `StaticBody`,
    // never `dup`ed". A `dup` leaks one descriptor per static response and then
    // fd exhaustion under load, and no other suite can see it.
    let tree = StaticTree::build(Fixture::S);
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{backend:?}: the root holds exactly its one dirfd"
        );

        let file = resolved(&root, APP_URL);
        let raw = file.file.as_raw_fd();
        assert_eq!(
            open_in_tree(&tree.base),
            2,
            "{backend:?}: resolve hands back one read descriptor"
        );

        let mut response = root.respond(file, b"GET", None);
        assert_eq!(response.status, 200);
        let body = response.body.take().expect("a GET has a body");
        assert_eq!(
            body.file.as_raw_fd(),
            raw,
            "{backend:?}: the resolved descriptor itself moves into StaticBody"
        );
        assert_eq!(
            open_in_tree(&tree.base),
            2,
            "{backend:?}: a dup would make this three, and the original is gone"
        );

        drop(body);
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{backend:?}: the body's descriptor closes with the body"
        );
    }
}

#[test]
fn head_and_a_304_close_the_descriptor_instead_of_leaking_it() {
    // The other half of §4.2: with no body there is nothing to move the
    // descriptor into, so `respond` must drop it. Comparing the in-tree count
    // (not the raw number) keeps the oracle immune to descriptors other tests
    // in this binary open and close.
    let tree = StaticTree::build(Fixture::S);
    for backend in [Backend::Openat2, Backend::Walk] {
        let root = tree.open(Fixture::S, backend);
        let etag = head(&serve(&root, APP_URL, b"GET", None), "ETag")
            .expect("an ETag")
            .to_vec();

        let file = resolved(&root, APP_URL);
        assert_eq!(open_in_tree(&tree.base), 2, "{backend:?}");
        let head_response = root.respond(file, b"HEAD", None);
        assert_eq!(head_response.status, 200);
        assert!(head_response.body.is_none(), "HEAD has no body");
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{backend:?}: HEAD must drop the descriptor, not park it"
        );

        let file = resolved(&root, APP_URL);
        assert_eq!(open_in_tree(&tree.base), 2, "{backend:?}");
        let conditional = root.respond(file, b"GET", Some(&etag));
        assert_eq!(conditional.status, 304);
        assert!(conditional.body.is_none(), "a 304 has no body");
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{backend:?}: a 304 must drop the descriptor, not park it"
        );

        // …and the observable consequence: many responses in a row leave the
        // process exactly one descriptor richer in this tree, never two.
        for _ in 0..8 {
            let file = resolved(&root, APP_URL);
            drop(root.respond(file, b"HEAD", None));
            drop(root.respond(resolved(&root, APP_URL), b"GET", Some(&etag)));
        }
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{backend:?}: sixteen bodiless answers leak no descriptor"
        );
    }
}

#[test]
fn the_ceiling_admits_the_exact_size_and_refuses_either_half_over() {
    // `respond` refuses on `file.len > max_bytes || file.meta.size > max_bytes`
    // (§4.6's defence-in-depth arm). With `len == meta.size` — everything
    // `resolve` can produce — the `||` and the second disjunct are invisible,
    // so each half is forged on its own here.
    let tree = StaticTree::build(Fixture::N);
    let root = tree.open(Fixture::N, Backend::Openat2);
    let ceiling = STATIC_MAX_FILE_BYTES;

    // A real, sparse file **exactly** at the ceiling: no tmpfs cost, never read.
    std::fs::File::create(tree.path("at-cap.bin"))
        .expect("at-cap fixture")
        .set_len(ceiling)
        .expect("at-cap length");
    assert_eq!(open_in_tree(&tree.base), 1);

    // The accepted twin: `>` (not `>=`) on both halves, so the ceiling itself
    // is a normal 200 that promises exactly its size.
    let mut served = root.respond(
        forged(&tree, "at-cap.bin", "/at-cap.bin", ceiling),
        b"GET",
        None,
    );
    assert_eq!(
        served.status, 200,
        "a file exactly at static_max_bytes is served"
    );
    assert_eq!(
        head(&served, "Content-Length"),
        Some(ceiling.to_string().as_bytes())
    );
    let body = served.body.take().expect("a GET has a body");
    assert_eq!(body.len, ceiling);
    drop(body);
    assert_eq!(
        open_in_tree(&tree.base),
        1,
        "the served body's descriptor closes with it"
    );

    // The denied twins: one byte over, on both halves and on each alone.
    for (len, size, why) in [
        (ceiling + 1, ceiling + 1, "both halves over the ceiling"),
        (ceiling + 1, 0, "only len over the ceiling"),
        (0, ceiling + 1, "only meta.size over the ceiling"),
    ] {
        let response = root.respond(
            forged_meta(&tree, "big.bin", "/big.bin", len, size),
            b"GET",
            None,
        );
        assert_eq!(response.status, 404, "{why}");
        assert_eq!(response.reason, "Not Found", "{why}");
        assert!(response.body.is_none(), "{why}: a refusal has no body");
        assert_eq!(
            head(&response, "Content-Length"),
            Some(b"0".as_slice()),
            "{why}"
        );
        assert_eq!(
            head(&response, "Cache-Control"),
            Some(b"no-store".as_slice()),
            "{why}: a refusal is never cached"
        );
        assert_eq!(
            open_in_tree(&tree.base),
            1,
            "{why}: the refused file's descriptor is dropped"
        );
    }
}

#[test]
fn statuses_come_from_decide_not_a_second_mapping() {
    let tree = StaticTree::build(Fixture::S);
    let s = tree.open(Fixture::S, Backend::Openat2);
    let f = tree.open(Fixture::F, Backend::Openat2);
    let n = tree.open(Fixture::N, Backend::Openat2);

    // The responder's rows: exact file, directory index, SPA fallback.
    for target in [APP_URL, "/docs", "/docs/", "/dashboard"] {
        for method in [b"GET".as_slice(), b"HEAD".as_slice()] {
            assert!(
                matches!(
                    decide(outcome(&s, target), method, false, false),
                    Rewrite::Serve(_)
                ),
                "{target}"
            );
        }
    }

    // The method gate outranks the body gate: any other method is 405 with the
    // exact `Allow`, even when the head declares a body.
    for target in [APP_URL, "/dashboard"] {
        for method in [
            b"POST".as_slice(),
            b"PUT".as_slice(),
            b"OPTIONS".as_slice(),
            b"get".as_slice(),
        ] {
            match decide(outcome(&s, target), method, true, false) {
                Rewrite::Reject { status, allow } => {
                    assert_eq!(status, 405, "{target}");
                    assert_eq!(allow, Some(b"GET, HEAD".as_slice()), "{target}");
                }
                _ => panic!("{target}: {method:?} must be 405"),
            }
        }
        // A GET/HEAD that declares a body is 413 with no `Allow`.
        for method in [b"GET".as_slice(), b"HEAD".as_slice()] {
            match decide(outcome(&s, target), method, true, false) {
                Rewrite::Reject { status, allow } => {
                    assert_eq!(status, 413, "{target}");
                    assert_eq!(allow, None);
                }
                _ => panic!("{target}: a declared body must be 413"),
            }
        }
    }

    // Refusals: 400 for a malformed path, 404 for hidden existence and for a
    // file above the ceiling. Neither is ever masked by the SPA fallback.
    for target in [
        "/../etc/passwd",
        "/%2e%2e/%2e%2e/etc/passwd",
        "//etc/passwd",
        "/foo%5cbar",
    ] {
        assert!(
            matches!(
                decide(outcome(&s, target), b"GET", false, true),
                Rewrite::Reject { status: 400, .. }
            ),
            "{target}"
        );
    }
    for target in ["/.env", "/.git/config", "/link", "/mode000", "/big.bin"] {
        assert!(
            matches!(
                decide(outcome(&s, target), b"GET", false, true),
                Rewrite::Reject { status: 404, .. }
            ),
            "{target}"
        );
    }

    // A miss: forward with an app, 404 without, `index` under `spa = true`.
    assert!(matches!(
        decide(outcome(&f, "/missing"), b"GET", false, true),
        Rewrite::Forward
    ));
    assert!(matches!(
        decide(outcome(&f, "/missing"), b"GET", false, false),
        Rewrite::Reject {
            status: 404,
            allow: None
        }
    ));
    assert!(matches!(
        decide(outcome(&n, "/missing"), b"GET", false, false),
        Rewrite::Reject {
            status: 404,
            allow: None
        }
    ));
    assert!(matches!(
        decide(outcome(&s, "/missing"), b"GET", false, false),
        Rewrite::Serve(_)
    ));

    // `spa = true` with a missing `index` is `SpaLost`: a final 404.
    let mut lost = Fixture::S.policy();
    lost.index = "missing-index.html".to_string();
    let lost_root = StaticRoot::open_with(&tree.base, &lost, Backend::Openat2).expect("root opens");
    assert!(matches!(
        decide(lost_root.resolve(&["missing"]), b"GET", false, true),
        Rewrite::Reject {
            status: 404,
            allow: None
        }
    ));
}

// ---------------------------------------------------------------------------
// The pure halves this leaf owns.
// ---------------------------------------------------------------------------

#[test]
fn http_date_is_the_imf_fixdate_inverse() {
    assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
    assert_eq!(http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(http_date(1_700_000_000), "Tue, 14 Nov 2023 22:13:20 GMT");
    assert_eq!(http_date(1_000_000_000), "Sun, 09 Sep 2001 01:46:40 GMT");
    assert_eq!(http_date(-1), "Wed, 31 Dec 1969 23:59:59 GMT");
}

#[test]
fn the_suites_sha256_oracle_is_sha256() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ---------------------------------------------------------------------------
// The suite's own sha256 (no dependency is added for a test oracle).
// ---------------------------------------------------------------------------

fn digest_of(url: &str) -> &'static str {
    match url {
        INDEX_URL => "093ceb94fb025d430c803a6e80b527f0751398a6301cc7b858ff5b7cd2678a07",
        DOCS_URL => "a4cc4129469e9c9c0c21260186819be0a536d262a0d2c66fc4dde4f067c5f447",
        APP_URL => "8defcd4c749aad7502b77953048b491bab0f901c5ac926a17931266ae3c92637",
        TOKEN_URL => "cd9f809ff6c64eb628639a7c9beae7d06881cc36b0e6e3f07a9e857170f8b187",
        CAFE_URL => "30673163c6d5181509c7d046ac8023f5be0392d9fb6a5b026bf68b658daaf43c",
        _ => panic!("no fixture digest for {url}"),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (index, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[index] = u32::from_be_bytes(*word);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}
