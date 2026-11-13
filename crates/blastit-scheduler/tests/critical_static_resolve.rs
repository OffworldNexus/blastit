//! The protected resolve suite of design §5.2/§7.2 (`MB.2.4.1`): every row of
//! the 40-row attack matrix that this leaf owns, the exact `StaticRefusal` (or
//! outcome) each one must produce, and the `openat2` **and** `Walk` backends
//! answering identically.
//!
//! Rows are asserted through the composition the scheduler's head path runs —
//! the production `head_segments` (query cut, one percent-decode, `segments`,
//! `is_dotfile`) then `resolve` — because that is the boundary this leaf owns:
//! `static_asset.rs` never builds a header and never decides a status, so the
//! suite asserts `StaticOutcome` values, not HTTP codes. The responder half is
//! `critical_static_assets.rs` (`MB.2.4.2`).
//!
//! Every refused row has an accepted twin in the same table (rows 14, 18–21,
//! 27–35, 37, 40), so a resolver that refuses everything cannot pass.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/static_fixture.rs"]
mod static_fixture;

use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use blastit_scheduler::static_asset::{
    AnswerKind, Backend, ImmutableGlob, MAX_SEGMENT_BYTES, MAX_STATIC_PATH_BYTES,
    STATIC_CHUNK_BYTES, STATIC_MAX_FILE_BYTES, STATIC_RESOLVE_FLAGS, StaticOutcome, StaticRefusal,
    StaticRoot, classify, head_segments, is_dotfile, same_inode, segments,
};
use rustix::fs::{FileType, Mode, OFlags, ResolveFlags};
use rustix::io::Errno;
use static_fixture::{
    APP_JS, CAFE_BYTES, Fixture, INDEX_HTML, SWAP_BYTES, SWAPPED_BYTES, StaticTree, content_of,
};

// ---------------------------------------------------------------------------
// The composition the scheduler's head path runs (design §4.3 steps 3–7).
// ---------------------------------------------------------------------------

/// Steps 1–7 of design §4.3 for one raw request target, through the production
/// head (`head_segments`) and then the filesystem half.
fn verdict(root: &StaticRoot, target: &str) -> StaticOutcome {
    let owned = match head_segments(target) {
        Ok(parts) => parts,
        Err(outcome) => return outcome,
    };
    let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
    root.resolve(&parts)
}

fn describe(outcome: &StaticOutcome) -> String {
    match outcome {
        StaticOutcome::File(file) => {
            format!("file:{:?}:{}:{}", file.kind, file.url_path, file.len)
        }
        StaticOutcome::Missing => "missing".to_string(),
        StaticOutcome::SpaLost => "spa-lost".to_string(),
        StaticOutcome::Hidden => "hidden".to_string(),
        StaticOutcome::Refused(refusal) => format!("refused:{refusal:?}"),
    }
}

fn read_all(fd: OwnedFd) -> Vec<u8> {
    let mut file = std::fs::File::from(fd);
    let mut body = Vec::new();
    file.read_to_end(&mut body).expect("read the served file");
    body
}

/// Whether this kernel has `openat2` with the resolve flags: the same probe
/// `StaticRoot::open_with` makes, recorded so the matrix log names the backend
/// the kernel actually exercised without widening `StaticRoot`'s public surface.
fn kernel_supports_openat2(root: &Path) -> bool {
    let Ok(dirfd) = rustix::fs::open(
        root,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return false;
    };
    match rustix::fs::openat2(
        &dirfd,
        ".",
        OFlags::PATH | OFlags::CLOEXEC,
        Mode::empty(),
        STATIC_RESOLVE_FLAGS,
    ) {
        Ok(_) => true,
        Err(Errno::NOSYS | Errno::INVAL) => false,
        // An unrelated errno says nothing about the syscall's existence.
        Err(_) => true,
    }
}

// ---------------------------------------------------------------------------
// The matrix.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Expected {
    Exact(&'static str),
    DirectoryIndex(&'static str),
    Spa,
    Missing,
    Hidden,
    Refused(StaticRefusal),
}

impl Expected {
    fn describe(&self) -> String {
        match self {
            Expected::Exact(url) => format!("file:Exact:{url}:{}", content_of(url).len()),
            Expected::DirectoryIndex(url) => {
                format!("file:DirectoryIndex:{url}:{}", content_of(url).len())
            }
            Expected::Spa => format!("file:SpaFallback:/index.html:{}", INDEX_HTML.len()),
            Expected::Missing => "missing".to_string(),
            Expected::Hidden => "hidden".to_string(),
            Expected::Refused(refusal) => format!("refused:{refusal:?}"),
        }
    }

    /// Fixtures F and N have no `spa`, so every miss is the caller's to
    /// forward; a file answer and a refusal are unchanged.
    fn without_spa(&self) -> Expected {
        match self {
            Expected::Spa => Expected::Missing,
            other => other.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    number: u8,
    target: String,
    expected: Expected,
    note: &'static str,
}

fn row(number: u8, target: &str, expected: Expected, note: &'static str) -> Row {
    Row {
        number,
        target: target.to_string(),
        expected,
        note,
    }
}

fn matrix() -> Vec<Row> {
    let assets = "/assets/app.4f1c2a9b7d3e.js";
    let mut rows = vec![
        // 1–13: the malformed paths, each with its own refusal.
        row(
            1,
            "/../etc/passwd",
            Expected::Refused(StaticRefusal::Traversal),
            "plain ..",
        ),
        row(
            2,
            "/%2e%2e/%2e%2e/etc/passwd",
            Expected::Refused(StaticRefusal::Traversal),
            "encoded ..",
        ),
        row(
            3,
            "/..%2f..%2fetc%2fpasswd",
            Expected::Refused(StaticRefusal::Traversal),
            "encoded separator + ..",
        ),
        row(
            4,
            "/assets/app.4f1c2a9b7d3e.js%2f..%2f..%2fetc%2fpasswd",
            Expected::Refused(StaticRefusal::Traversal),
            ".. after a decoded /",
        ),
        row(
            5,
            "/%2Fetc/passwd",
            Expected::Refused(StaticRefusal::Absolute),
            "decodes to //etc/passwd",
        ),
        row(
            6,
            "//etc/passwd",
            Expected::Refused(StaticRefusal::Absolute),
            "second leading /",
        ),
        row(
            7,
            "/foo%5c..%5cbar",
            Expected::Refused(StaticRefusal::Backslash),
            "backslash",
        ),
        row(
            8,
            "/%c0%ae%c0%ae/etc/passwd",
            Expected::Refused(StaticRefusal::Encoding),
            "overlong UTF-8 is not UTF-8",
        ),
        row(
            9,
            "/%00.js",
            Expected::Refused(StaticRefusal::Control),
            "NUL",
        ),
        row(
            10,
            "/%1f.js",
            Expected::Refused(StaticRefusal::Control),
            "C0 control",
        ),
        row(
            11,
            "/%ff.js",
            Expected::Refused(StaticRefusal::Encoding),
            "invalid UTF-8",
        ),
        row(
            12,
            "/%zz",
            Expected::Refused(StaticRefusal::Encoding),
            "malformed escape",
        ),
        // 14: double-encoded is inert data naming a literal `%2e…` file.
        row(
            14,
            "/%252e%252e/%252e%252e/etc/passwd",
            Expected::Spa,
            "one decode: literal %2e names, so a miss",
        ),
        // 15–17: dotfiles.
        row(15, "/.env", Expected::Hidden, "dotfile"),
        row(16, "/.git/config", Expected::Hidden, "dotfile"),
        row(17, "/.git/", Expected::Hidden, "dotfile directory"),
        // 18–21: the `.well-known` exemption and its limits.
        row(
            18,
            "/.well-known/acme-challenge/token",
            Expected::Exact("/.well-known/acme-challenge/token"),
            ".well-known is public",
        ),
        row(
            19,
            "/.well-known/missing",
            Expected::Spa,
            "a path it does not answer",
        ),
        row(
            20,
            "/.well-known/../.env",
            Expected::Refused(StaticRefusal::Traversal),
            ".. outranks the exemption",
        ),
        row(
            21,
            "/.well-known/.env",
            Expected::Hidden,
            "exemption covers the first segment only",
        ),
        // 22–26: hidden existence.
        row(22, "/link", Expected::Hidden, "symlink to /etc/passwd"),
        row(
            23,
            "/link2/x",
            Expected::Hidden,
            "traversal through a symlink",
        ),
        row(24, "/fifo", Expected::Hidden, "FIFO, never read"),
        row(24, "/sock", Expected::Hidden, "socket, never read"),
        row(25, "/mode000", Expected::Hidden, "unreadable regular file"),
        row(26, "/big.bin", Expected::Hidden, "oversize"),
        // 27–35, 37–38, 40: the positive twins.
        row(
            27,
            "/assets",
            Expected::Spa,
            "a directory with no index falls through",
        ),
        row(
            28,
            "/docs",
            Expected::DirectoryIndex("/docs/index.html"),
            "directory index",
        ),
        row(
            28,
            "/docs/",
            Expected::DirectoryIndex("/docs/index.html"),
            "the trailing / is the same directory",
        ),
        row(29, "/missing", Expected::Spa, "the SPA fallback"),
        row(
            30,
            "/docs%2Findex.html",
            Expected::Exact("/docs/index.html"),
            "%2F is a separator",
        ),
        row(31, "/DOCS", Expected::Spa, "no case folding"),
        row(
            32,
            assets_with_query(),
            Expected::Exact(assets),
            "query ignored",
        ),
        row(
            33,
            "/assets/app.4f1c2a9b7d3e.js/",
            Expected::Exact(assets),
            "trailing empty segment dropped",
        ),
        row(
            34,
            "/index.html",
            Expected::Exact("/index.html"),
            "fixture D: Exact, so /index.html is globbable",
        ),
        row(
            35,
            "/",
            Expected::DirectoryIndex("/index.html"),
            "fixture D: an index answer is never globbed",
        ),
        row(
            35,
            "/dashboard",
            Expected::Spa,
            "fixture D: an index answer is never globbed",
        ),
        row(
            37,
            "/caf%C3%A9.js",
            Expected::Exact("/café.js"),
            "the NFC bytes name café.js",
        ),
        row(
            38,
            "/cafe%CC%81.js",
            Expected::Spa,
            "NFD is a different filename",
        ),
        row(
            39,
            "/proclink/self/fd/0",
            Expected::Hidden,
            "magic link behind a root symlink",
        ),
        row(
            40,
            "/assets//app.4f1c2a9b7d3e.js",
            Expected::Exact(assets),
            "interior // collapses",
        ),
    ];
    rows.push(row(
        13,
        &format!("/{}.js", "a".repeat(4096)),
        Expected::Refused(StaticRefusal::TooLong),
        "over MAX_STATIC_PATH_BYTES",
    ));
    rows
}

fn assets_with_query() -> &'static str {
    "/assets/app.4f1c2a9b7d3e.js?v=1"
}

/// Asserts one row and returns its normalised answer, for the backend diff.
fn check(root: &StaticRoot, checked: &Row) -> String {
    let target = checked.target.as_str();
    let outcome = verdict(root, target);
    let got = describe(&outcome);
    let wanted = checked.expected.describe();
    assert_eq!(
        got, wanted,
        "row {} `{target}` ({}): expected `{wanted}`, got `{got}`",
        checked.number, checked.note
    );
    if let StaticOutcome::File(file) = outcome {
        let url = file.url_path.clone();
        let len = file.len;
        let body = read_all(file.file);
        assert_eq!(
            body,
            content_of(&url),
            "row {} `{target}` answered `{url}` with the wrong bytes",
            checked.number
        );
        assert_eq!(
            body.len() as u64,
            len,
            "row {} `{target}`: len disagrees with the bytes",
            checked.number
        );
        assert!(
            !body.is_empty(),
            "row {} `{target}` answered an empty file",
            checked.number
        );
    }
    got
}

#[test]
fn every_matrix_row_answers_the_same_on_both_backends() {
    let tree = StaticTree::build(Fixture::S);
    let rows = matrix();
    // The matrix names rows 1–40; the device row 36 is `MB.2.7`'s (needs root)
    // and is covered here only by `classify`'s `CharacterDevice` arm.
    for fixture in [Fixture::S, Fixture::F, Fixture::N, Fixture::D] {
        let mut answers: Vec<(Backend, Vec<String>)> = Vec::new();
        let openat2 = kernel_supports_openat2(&tree.root);
        for backend in [Backend::Openat2, Backend::Walk] {
            let root = tree.open(fixture, backend);
            // The effective backend is the same predicate `open_with` uses, so
            // the record below is honest without widening the type's surface.
            let effective = if backend == Backend::Walk || !openat2 {
                Backend::Walk
            } else {
                Backend::Openat2
            };
            println!("{fixture:?}: requested {backend:?}, kernel exercised {effective:?}");
            let mut seen = Vec::new();
            for checked in &rows {
                let mut checked = checked.clone();
                if matches!(fixture, Fixture::F | Fixture::N) {
                    checked.expected = checked.expected.without_spa();
                }
                seen.push(check(&root, &checked));
            }
            answers.push((effective, seen));
        }
        assert_eq!(
            answers[0].1, answers[1].1,
            "{fixture:?}: the {:?} and {:?} backends disagree",
            answers[0].0, answers[1].0
        );
    }
}

// ---------------------------------------------------------------------------
// The pure halves.
// ---------------------------------------------------------------------------

#[test]
fn the_documented_limits_and_flags_are_the_ones_asserted() {
    assert_eq!(MAX_STATIC_PATH_BYTES, 4096);
    assert_eq!(MAX_SEGMENT_BYTES, 255);
    assert_eq!(STATIC_CHUNK_BYTES, 64 * 1024);
    assert_eq!(STATIC_MAX_FILE_BYTES, 64 * 1024 * 1024);

    let expected = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS;
    assert_eq!(
        STATIC_RESOLVE_FLAGS, expected,
        "the exact RESOLVE_* composition of design §5.1"
    );
    assert!(
        STATIC_RESOLVE_FLAGS.contains(ResolveFlags::NO_MAGICLINKS),
        "the second wall against /proc/self/fd"
    );
    assert!(
        !STATIC_RESOLVE_FLAGS.contains(ResolveFlags::NO_XDEV),
        "mounts are followed: the boundary is the path namespace"
    );
    assert!(!STATIC_RESOLVE_FLAGS.contains(ResolveFlags::IN_ROOT));
    assert!(!STATIC_RESOLVE_FLAGS.contains(ResolveFlags::CACHED));
}

#[test]
fn segments_accepts_every_legal_shape() {
    assert_eq!(segments("/"), Ok(Vec::new()));
    assert_eq!(segments("/a/b"), Ok(vec!["a", "b"]));
    assert_eq!(
        segments("/a//b"),
        Ok(vec!["a", "b"]),
        "interior // collapses"
    );
    assert_eq!(
        segments("/x.js/"),
        Ok(vec!["x.js"]),
        "trailing / is dropped"
    );
    assert_eq!(
        segments("/./a/./b"),
        Ok(vec!["a", "b"]),
        ". segments are dropped"
    );
    assert_eq!(segments("/a/b/"), Ok(vec!["a", "b"]));
    assert_eq!(segments("/café.js"), Ok(vec!["café.js"]));
    assert_eq!(segments("/%2e"), Ok(vec!["%2e"]), "one decode: %2e is data");
    assert_eq!(
        segments("/\u{7f}.js"),
        Ok(vec!["\u{7f}.js"]),
        "DEL is not C0"
    );

    let longest_segment = "b".repeat(MAX_SEGMENT_BYTES);
    assert_eq!(
        segments(&format!("/{longest_segment}")),
        Ok(vec![longest_segment.as_str()])
    );
    assert_eq!(
        segments(&format!("/{longest_segment}/x")),
        Ok(vec![longest_segment.as_str(), "x"])
    );

    // Exactly MAX_STATIC_PATH_BYTES: 16 segments of 255 bytes = 1 + 16*255 + 15.
    let segment = "c".repeat(MAX_SEGMENT_BYTES);
    let longest_path = format!("/{}", vec![segment.as_str(); 16].join("/"));
    assert_eq!(longest_path.len(), MAX_STATIC_PATH_BYTES);
    assert_eq!(segments(&longest_path).map(|parts| parts.len()), Ok(16));
}

#[test]
fn segments_refuses_every_malformed_shape() {
    let cases: &[(&str, StaticRefusal)] = &[
        ("/../etc/passwd", StaticRefusal::Traversal),
        ("/a/../../etc", StaticRefusal::Traversal),
        ("/..", StaticRefusal::Traversal),
        ("/a/..", StaticRefusal::Traversal),
        ("//etc/passwd", StaticRefusal::Absolute),
        ("///x", StaticRefusal::Absolute),
        ("etc/passwd", StaticRefusal::Absolute),
        ("", StaticRefusal::Absolute),
        ("/foo\\bar", StaticRefusal::Backslash),
        ("/foo\\..\\bar", StaticRefusal::Backslash),
        ("/\\", StaticRefusal::Backslash),
        ("/\0.js", StaticRefusal::Control),
        ("/\u{1f}.js", StaticRefusal::Control),
        ("/a\nb", StaticRefusal::Control),
    ];
    for (decoded, refusal) in cases {
        assert_eq!(segments(decoded), Err(*refusal), "segments({decoded:?})");
    }

    let over_long = format!("/{}.js", "a".repeat(MAX_STATIC_PATH_BYTES));
    assert_eq!(segments(&over_long), Err(StaticRefusal::TooLong));
    assert_eq!(
        segments(&"a".repeat(MAX_STATIC_PATH_BYTES + 1)),
        Err(StaticRefusal::TooLong),
        "the length is checked before the leading /"
    );
    let huge_segment = "b".repeat(MAX_SEGMENT_BYTES + 1);
    assert_eq!(
        segments(&format!("/{huge_segment}")),
        Err(StaticRefusal::SegmentTooLong)
    );
}

#[test]
fn the_production_head_refuses_what_segments_never_sees() {
    for (raw, why) in [
        ("/%c0%ae%c0%ae/etc/passwd", "row 8: overlong UTF-8"),
        ("/%ff.js", "row 11: invalid UTF-8"),
        ("/%zz", "row 12: malformed escape"),
        ("/%", "row 12: truncated escape"),
        ("/%2", "row 12: truncated escape"),
    ] {
        assert!(
            matches!(
                head_segments(raw),
                Err(StaticOutcome::Refused(StaticRefusal::Encoding))
            ),
            "{why}"
        );
    }
    // Row 14: exactly one decode, so `%252e` becomes the literal text `%2e`.
    assert_eq!(
        head_segments("/%252e%252e/%252e%252e/etc/passwd").ok(),
        Some(vec![
            "%2e%2e".to_string(),
            "%2e%2e".to_string(),
            "etc".to_string(),
            "passwd".to_string(),
        ]),
        "one decode only"
    );
    // Rows 15–17, 21: the dotfile rule is the production one (`is_dotfile`,
    // which `head_segments` calls), not a copy of it in this suite.
    for raw in ["/.env", "/.git/config", "/.git/", "/.well-known/.env"] {
        assert!(
            matches!(head_segments(raw), Err(StaticOutcome::Hidden)),
            "{raw} must be hidden by the production dotfile rule"
        );
    }
    // Its positive twin: the exemption is still production behaviour.
    assert_eq!(
        head_segments("/.well-known/acme-challenge/token").ok(),
        Some(vec![
            ".well-known".to_string(),
            "acme-challenge".to_string(),
            "token".to_string(),
        ])
    );
    // Row 32: the query string is cut by the production decoder, not here.
    assert_eq!(
        head_segments("/assets/app.js?v=1").ok(),
        head_segments("/assets/app.js").ok()
    );

    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    for raw in ["/%c0%ae%c0%ae/etc/passwd", "/%ff.js", "/%zz"] {
        assert!(
            matches!(
                verdict(&root, raw),
                StaticOutcome::Refused(StaticRefusal::Encoding)
            ),
            "{raw} must be refused as an encoding error"
        );
    }
}

#[test]
fn classify_is_total_over_every_file_type() {
    assert!(classify(FileType::RegularFile, 10, 10).is_ok());
    assert!(
        classify(FileType::RegularFile, 0, 0).is_ok(),
        "a zero-length file is still a file"
    );
    assert!(matches!(
        classify(FileType::RegularFile, 11, 10),
        Err(StaticOutcome::Hidden)
    ));
    assert!(matches!(
        classify(
            FileType::RegularFile,
            STATIC_MAX_FILE_BYTES + 1,
            STATIC_MAX_FILE_BYTES
        ),
        Err(StaticOutcome::Hidden)
    ));
    assert!(matches!(
        classify(FileType::Directory, 0, u64::MAX),
        Err(StaticOutcome::Missing)
    ));
    for kind in [
        FileType::Fifo,
        FileType::Socket,
        FileType::CharacterDevice,
        FileType::BlockDevice,
        FileType::Symlink,
        FileType::Unknown,
    ] {
        assert!(
            matches!(classify(kind, 0, u64::MAX), Err(StaticOutcome::Hidden)),
            "{kind:?} must be hidden"
        );
    }
}

#[test]
fn same_inode_compares_device_and_inode() {
    assert!(same_inode((1, 2), (1, 2)));
    assert!(
        !same_inode((1, 2), (1, 3)),
        "a different inode is a different file"
    );
    assert!(
        !same_inode((1, 2), (2, 2)),
        "the same inode number on another device is another file"
    );
    assert!(same_inode((0, 0), (0, 0)));
    // A swapped leaf with a reused number still differs when the device does.
    assert!(!same_inode((0xfd, 42), (0xfe, 42)));
}

#[test]
fn immutable_globs_match_the_design_vectors() {
    let vectors: &[(&str, &str, bool)] = &[
        ("/assets/*", "/assets/app.4f1c2a9b7d3e.js", true),
        ("/assets/*", "/assets/", true),
        ("/assets/*", "/assets/x/y.js", false),
        ("/assets/*", "/x/assets/app.js", false),
        ("/assets/*/*", "/assets/chunks/x.js", true),
        ("/assets/*/*", "/assets/x.js", false),
        ("/assets/app.*", "/assets/app.js", true),
        ("/assets/app.*", "/assets/app.css", true),
        ("/index.html", "/index.html", true),
        ("/index.html", "/index.htm", false),
        ("/index.html", "/x/index.html", false),
        ("/assets/*", "", false),
    ];
    for (pattern, url, expected) in vectors {
        let glob = ImmutableGlob::new(pattern).expect("a validated pattern compiles");
        assert_eq!(glob.matches(url), *expected, "{pattern} against {url:?}");
    }

    // What MB.2.3 validation should have refused: `new` refuses it again.
    assert!(ImmutableGlob::new("").is_none());
    assert!(ImmutableGlob::new("assets/*").is_none(), "not /-anchored");
    assert!(ImmutableGlob::new("/a/../b").is_none(), "a .. segment");
    assert!(ImmutableGlob::new("/a\u{0}b").is_none(), "a control byte");
    assert!(
        ImmutableGlob::new(&format!("/{}", "a".repeat(256))).is_none(),
        "over 256 bytes"
    );
    assert!(ImmutableGlob::new(&format!("/{}", "a".repeat(255))).is_some());
}

#[test]
fn the_dotfile_rule_exempts_exactly_dot_well_known() {
    assert!(!is_dotfile(&[]));
    assert!(!is_dotfile(&[".well-known"]));
    assert!(!is_dotfile(&[".well-known", "acme-challenge", "token"]));
    assert!(is_dotfile(&[".well-known", ".env"]), "first segment only");
    assert!(
        is_dotfile(&["a", ".well-known"]),
        "only the first is exempt"
    );
    assert!(is_dotfile(&[".env"]));
    assert!(is_dotfile(&[".git", "config"]));
    assert!(is_dotfile(&[".well-knownx"]), "not exactly .well-known");
    assert!(!is_dotfile(&["assets", "app.js"]));
}

// ---------------------------------------------------------------------------
// The outcomes that need a bespoke tree or policy.
// ---------------------------------------------------------------------------

#[test]
fn an_spa_route_whose_index_is_missing_is_spa_lost() {
    let tree = StaticTree::build(Fixture::S);
    let mut policy = Fixture::S.policy();
    policy.index = "missing-index.html".to_string();
    for backend in [Backend::Openat2, Backend::Walk] {
        let root =
            StaticRoot::open_with(&tree.base, &policy, backend).expect("the fixture root opens");
        assert!(matches!(root.resolve(&[]), StaticOutcome::SpaLost));
        assert!(matches!(root.resolve(&["missing"]), StaticOutcome::SpaLost));
        assert!(matches!(root.resolve(&["docs"]), StaticOutcome::SpaLost));
        // The exact file is still served: `SpaLost` is only the fallback.
        let parts: Vec<&str> = APP_JS.split('/').collect();
        assert!(
            matches!(root.resolve(&parts), StaticOutcome::File(_)),
            "the exact asset is not a fallback"
        );
    }
}

#[test]
fn caller_supplied_segments_cannot_leave_the_root() {
    // `resolve` is `pub`, so the boundary must hold without `segments()`'s
    // caller: on `Walk` an `openat(dir, "..")` succeeds, which is exactly the
    // backend divergence this row pins. Both backends answer `Hidden`.
    let tree = StaticTree::build(Fixture::S);
    // `..` repeated to `/`, then `/etc/passwd`: without the guard this is the
    // literal root escape on `Walk` (and `Hidden` from `openat2`'s `EXDEV`).
    let mut to_root: Vec<&str> = vec![".."; tree.root.components().count().saturating_sub(1)];
    to_root.push("etc");
    to_root.push("passwd");
    // `Walk` first: on that backend the removed guard would serve
    // `/etc/passwd`, which is the failure this row exists to make visible.
    for backend in [Backend::Walk, Backend::Openat2] {
        let root = tree.open(Fixture::S, backend);
        for parts in [
            &to_root[..],
            &["..", "..", "etc", "passwd"][..],
            &[".."][..],
            &["assets", "..", "..", "etc", "passwd"][..],
            &[""][..],
            &["assets", "", "app.js"][..],
            &["assets/app.js"][..],
        ] {
            for outcome in [root.resolve(parts), root.resolve_with(parts, None)] {
                assert!(
                    matches!(outcome, StaticOutcome::Hidden),
                    "{backend:?}: resolve({parts:?}) must be Hidden, got {}",
                    describe(&outcome)
                );
            }
        }
        // "/" is the empty vector, and it is still the root directory: the
        // guard refuses empty *segments*, not the root itself.
        assert!(
            matches!(root.resolve(&[]), StaticOutcome::File(_)),
            "{backend:?}: the root itself must still resolve"
        );
        assert_eq!(
            describe(&root.resolve(&["..", "..", "etc", "passwd"])),
            "hidden",
            "{backend:?}: the walk must not be a bypass"
        );
    }
}

#[test]
fn a_leaf_swapped_between_the_probe_and_the_read_is_hidden() {
    for backend in [Backend::Openat2, Backend::Walk] {
        let tree = StaticTree::build(Fixture::S);
        let root = tree.open(Fixture::S, backend);

        // The control: with no hook the very same lookup serves the file, so
        // the substitution below is not a resolver that always answers Hidden.
        let served = root.resolve(&["swap.js"]);
        let described = describe(&served);
        let StaticOutcome::File(file) = served else {
            panic!("{backend:?}: swap.js must resolve: {described}");
        };
        assert_eq!(read_all(file.file), SWAP_BYTES);
        assert_eq!(file.url_path, "/swap.js");
        assert_eq!(file.kind, AnswerKind::Exact);

        // The substitution: rename the leaf (keeping the old inode alive under
        // the new name so its number cannot be reused) and write a new file in
        // its place, between the O_PATH probe and the read open.
        let swapped = AtomicBool::new(false);
        let hook = || {
            if !swapped.swap(true, Ordering::SeqCst) {
                std::fs::rename(tree.path("swap.js"), tree.path("swap.old.js"))
                    .expect("rename the probed inode away");
                std::fs::write(tree.path("swap.js"), SWAPPED_BYTES)
                    .expect("write the substituted inode");
            }
        };
        let outcome = root.resolve_with(&["swap.js"], Some(&hook));
        assert!(
            swapped.load(Ordering::SeqCst),
            "{backend:?}: the hook must run between the two opens"
        );
        assert!(
            matches!(outcome, StaticOutcome::Hidden),
            "{backend:?}: a swapped leaf must be Hidden, got {}",
            describe(&outcome)
        );
        // The old inode is still the original file under its new name, and the
        // path now names a different one: the identity check is what refused it.
        assert_eq!(
            std::fs::read(tree.path("swap.old.js")).expect("the probed file still exists"),
            SWAP_BYTES
        );
        assert_eq!(
            std::fs::read(tree.path("swap.js")).expect("the substituted file exists"),
            SWAPPED_BYTES
        );
    }
}

#[test]
fn an_unopenable_root_names_the_root_it_could_not_open() {
    let tree = StaticTree::build(Fixture::S);
    let mut policy = Fixture::S.policy();
    policy.root = "missing-root".to_string();
    let error = StaticRoot::open(&tree.base, &policy).expect_err("a missing root must not open");
    assert_eq!(error.root, "missing-root");
    assert!(
        error.source.raw_os_error().is_some(),
        "the source keeps the OS error"
    );
    assert!(
        error.to_string().starts_with("static root missing-root: "),
        "the boot message names the root: {error}"
    );
}

#[test]
fn the_cafe_file_is_the_nfc_bytes_and_nothing_else() {
    // Row 37/38's byte half, independent of the policy: the fixture really did
    // create NFC, and the served file has those bytes.
    let tree = StaticTree::build(Fixture::S);
    assert_eq!(
        std::fs::read(tree.path("café.js")).expect("café.js exists"),
        CAFE_BYTES
    );
    assert!(!tree.path("cafe\u{301}.js").exists());
}
