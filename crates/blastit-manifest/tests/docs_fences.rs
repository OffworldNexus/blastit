//! Every complete `Blastfile` in the user documentation parses and validates
//! (`R7`, `R8`, `R11`, `R15`, `R18`; design §16 "Documentation Fences").
//!
//! Standard tier on purpose: `MA.3` owns `docs/**`, and a protected copy of this
//! suite would make every documentation edit a `critical_guard` conflict. What
//! it still proves is the developer experience itself — a reader who copies one
//! of these blocks into `./Blastfile` boots.
//!
//! A complete document opens with `text title="Blastfile"` (what the pages ship:
//! Pygments has no `blastfile` lexer, §16.1) or `blastfile` (the transitional
//! alias §19.8 records); both go through `Manifest::parse_blastfile`.
//!
//! The pages still carry manifests opened with the retired file name until
//! `MB.21.5.1` migrates them. This crate no longer has a parser for that
//! spelling, so those fences are **counted and named but never executed**: each
//! page's count stays pinned (the retired column included) and every failure
//! names it, so `MB.21.5.1` rewrites the pages and `MB.21.5.2`'s strict flip is
//! one constant, not a rewrite. What the retired fences stopped proving is
//! re-homed at model level in the same commit:
//!
//! * the `InvalidRegex` canary is a `view` whose `regex` cannot compile (below);
//! * the documented static site and the Django page's `immutable` depths are
//!   `static` mount fixtures in `tests/static_mounts.rs`. The Django page's
//!   `[route.static]` fragment test is therefore **deleted, not translated**: the
//!   fragment has no spelling here any more, `MB.21.5.1` rewrites the page as a
//!   `static` mount, and the mount fixture already pins the `root` and the
//!   one-glob-per-depth list that test protected.
//!
//! Every message that spells the retired file name composes it from two literals,
//! so the tree never carries the contiguous name and `MB.21.5.2`'s guard needs no
//! exemption.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt;
use std::path::{Path, PathBuf};

use blastit_manifest::{Manifest, ManifestError, ManifestErrors, ManifestWarning, UidLayout};

/// The document opener the public pages use: `text` renders the file verbatim.
const TEXT_DOCUMENT_OPENING: &str = "```text title=\"Blastfile\"";

/// The transitional document opener (§19.8); `MB.21.5.2` retires it.
const BLASTFILE_OPENING: &str = "```blastfile";

/// The opening line of a retired complete manifest, exactly as the pages write
/// it. Such a fence is counted, never parsed.
///
/// The file's name is composed from two literals, so the tree never carries it
/// contiguously: it is only ever recognised and named here, and `MB.21.5.2`'s
/// guard then needs no exemption.
const RETIRED_OPENING: &str = concat!(r#"```toml title=""#, "blastit", ".toml", "\"");

/// The opening line of a plain `toml` fence: a retired manifest fragment.
const FRAGMENT_OPENING: &str = "```toml";

/// Both spellings of a complete document (§16.1) — the ones this crate executes.
const DOCUMENT_OPENINGS: [&str; 2] = [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING];

/// Every opening that carries a complete file, whichever spelling.
const COMPLETE_OPENINGS: [&str; 3] = [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING, RETIRED_OPENING];

/// `MB.21.5.2` flips this to `true` in the leaf that removes the last retired
/// fence: the retired opening then has to be absent from every pinned page.
const THE_RETIRED_OPENING_IS_GONE: bool = false;

/// One count per complete opening, in the order `text title="Blastfile"`,
/// `blastfile`, and the retired opening (§16.2).
type Counts = [usize; 3];

/// Names every opening, so a drifted count says which one moved.
fn label(counts: Counts) -> String {
    format!(
        "{} `text title=\"Blastfile\"` + {} `blastfile` + {} `{RETIRED_OPENING}`",
        counts[0], counts[1], counts[2]
    )
}

/// Every page that carries a complete file, and how many of each opening it
/// carries, measured on the pages as they stand (2026-11-16) and then pinned.
///
/// The per-file rule: a page that gains a complete document is added here with
/// its count, and one that drops a document lowers it — `MB.21.5.1` moves the
/// eight retired rows to `Blastfile` documents, `MB.21.5.2` deletes the retired
/// column.
const PAGES: [(&str, Counts); 10] = [
    ("README.md", [0, 0, 1]),
    ("docs/index.md", [0, 0, 1]),
    ("docs/getting-started.md", [0, 0, 1]),
    ("docs/local.md", [1, 0, 0]),
    ("docs/frameworks/index.md", [0, 0, 1]),
    ("docs/frameworks/django.md", [0, 0, 1]),
    ("docs/frameworks/flask.md", [0, 0, 1]),
    ("docs/frameworks/php.md", [0, 0, 1]),
    ("docs/frameworks/static-and-spa.md", [0, 0, 1]),
    ("docs/blastfile.md", [1, 0, 0]),
];

/// The whole documented set: two `text title="Blastfile"` documents today
/// (`docs/blastfile.md`'s and `docs/local.md`'s), and the eight retired
/// manifests the migration replaces.
const TOTAL: Counts = [2, 0, 8];

/// One fenced block: the opening it uses, the 1-based page line that opening
/// sits on, and its body without the markers.
#[derive(Debug)]
struct Fence {
    /// The opening, exactly as written; one of the constants above.
    opening: &'static str,
    /// The 1-based page line the opening sits on.
    opening_line: usize,
    /// The body, one page line per line, trailing newline included.
    body: String,
}

/// Why a page's fences could not be read.
#[derive(Debug, PartialEq, Eq)]
enum FenceError {
    /// A fence opened and never closed; nothing after it can be trusted.
    Unclosed { opening: &'static str, line: usize },
}

impl fmt::Display for FenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed { opening, line } => {
                write!(
                    f,
                    "the {opening} fence opened on line {line} is never closed"
                )
            }
        }
    }
}

/// The repository root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

/// Every fence in `document` whose opening line trims to one of `openings`, in
/// page order, without the fence markers.
fn collect_fences(document: &str, openings: &[&'static str]) -> Result<Vec<Fence>, FenceError> {
    let mut fences = Vec::new();
    let mut lines = document.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let trimmed = line.trim();
        let Some(&opening) = openings.iter().find(|opening| **opening == trimmed) else {
            continue;
        };
        let mut body = String::new();
        let mut closed = false;
        for (_, line) in lines.by_ref() {
            if line.trim() == "```" {
                closed = true;
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        if !closed {
            return Err(FenceError::Unclosed {
                opening,
                line: index + 1,
            });
        }
        fences.push(Fence {
            opening,
            opening_line: index + 1,
            body,
        });
    }
    Ok(fences)
}

/// How many fences of each complete opening `document` carries.
fn count_fences(document: &str) -> Result<Counts, FenceError> {
    let fences = collect_fences(document, &COMPLETE_OPENINGS)?;
    let count = |opening: &str| {
        fences
            .iter()
            .filter(|fence| fence.opening == opening)
            .count()
    };
    Ok([
        count(TEXT_DOCUMENT_OPENING),
        count(BLASTFILE_OPENING),
        count(RETIRED_OPENING),
    ])
}

/// Every complete document this crate can read, in page order.
fn documents(document: &str) -> Result<Vec<Fence>, FenceError> {
    collect_fences(document, &DOCUMENT_OPENINGS)
}

/// Every retired complete manifest `document` carries, in page order.
///
/// It is only ever counted and named: this crate has no parser for that
/// spelling, and it must not grow one back (`MB.21.5.1`, `MB.21.5.2`).
fn retired_documents(document: &str) -> Result<Vec<Fence>, FenceError> {
    collect_fences(document, &[RETIRED_OPENING])
}

/// Every plain `toml` fragment in `document`, in order, without the markers.
fn toml_fragments(document: &str) -> Result<Vec<Fence>, FenceError> {
    collect_fences(document, &[FRAGMENT_OPENING])
}

/// The drift check: `Ok(())` when the page carries the pinned counts, otherwise
/// an `Err` naming the page and every opening, so a red test says what moved.
fn check_counts(
    file: &str,
    actual: Counts,
    pinned: Counts,
    retired_gone: bool,
) -> Result<(), String> {
    if actual != pinned {
        return Err(format!(
            "{file}: fence counts drifted: pinned {}; the page carries {}. A page \
             that gains a complete document is added to `PAGES` with its count \
             and one that drops a document lowers it (§16.2).",
            label(pinned),
            label(actual)
        ));
    }
    if retired_gone && actual[2] > 0 {
        return Err(format!(
            "{file}: still carries {} `{RETIRED_OPENING}` fence(s); the retired \
             opening is gone once `MB.21.5.2`'s strict flip lands (§16.2)",
            actual[2]
        ));
    }
    Ok(())
}

/// The 1-based `line:col` a rendered refusal starts with, if it has one.
fn leading_position(rendered: &str) -> Option<(usize, usize)> {
    let (line, rest) = rendered.split_once(':')?;
    let (col, _) = rest.split_once(": ")?;
    let line: usize = line.parse().ok()?;
    let col: usize = col.parse().ok()?;
    (line > 0).then_some((line, col))
}

/// The failure a page's fence produces, every refusal prefixed with the page's
/// own `file:line:col` (§18): a fence body's lines are the page's lines, shifted
/// by the opening line.
fn describe_failure(file: &str, fence: &Fence, errors: &ManifestErrors) -> String {
    let mut message = format!(
        "{file}:{}: the {} document opened here does not boot",
        fence.opening_line, fence.opening
    );
    for error in errors.errors() {
        match leading_position(&error.to_string()) {
            Some((line, col)) => message.push_str(&format!(
                "\n  {file}:{}:{col}: {error}",
                fence.opening_line + line
            )),
            None => message.push_str(&format!("\n  {file}: {error}")),
        }
    }
    message
}

/// The manifest a complete fence declares, through the crate's one boot entry
/// point.
fn parse_document(body: &str) -> Result<(Manifest, Vec<ManifestWarning>), ManifestErrors> {
    Manifest::parse_blastfile(body, UidLayout::DEFAULT_BASE)
}

/// The shape an executed document declares, as one value.
///
/// A page promises one of these, so it is pinned per page instead of being
/// absorbed by an "or": a fence that degrades from views to a static mount does
/// not pass as "some document".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Views and no mount: the application a reader copies to run their app.
    Application,
    /// At least one mount, no view, no app: an app-less static site (§9.5).
    StaticSite,
    /// Neither a view nor a mount: the document reaches nothing.
    Empty,
}

/// What `manifest` is, in the terms the pages promise.
fn shape_of(manifest: &Manifest) -> Shape {
    if manifest.route_count() > 0 && manifest.mounts().is_empty() {
        Shape::Application
    } else if manifest.route_count() == 0
        && !manifest.mounts().is_empty()
        && manifest.is_all_static()
    {
        Shape::StaticSite
    } else {
        Shape::Empty
    }
}

#[test]
fn every_documented_document_parses_and_validates() {
    let root = workspace_root();
    let mut total = [0, 0, 0];
    let mut retired = 0;
    for (file, pinned) in PAGES {
        let path = root.join(file);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let actual = count_fences(&document).unwrap_or_else(|error| panic!("{file}: {error}"));
        check_counts(file, actual, pinned, THE_RETIRED_OPENING_IS_GONE)
            .unwrap_or_else(|message| panic!("{message}"));
        let complete = documents(&document).unwrap_or_else(|error| panic!("{file}: {error}"));
        for fence in complete {
            let (manifest, _warnings) = parse_document(&fence.body)
                .unwrap_or_else(|errors| panic!("{}", describe_failure(file, &fence, &errors)));
            // The shape is pinned per page, not absorbed by an "or": both pages
            // that carry a document today show an application — one or more
            // views and no mount — so a fence that degraded from views to a
            // static mount fails here. A page that documents an all-static site
            // adds its own arm with `Shape::StaticSite`; nothing else can
            // execute a document, so an unpinned page panics rather than
            // passing.
            let expected = match file {
                "docs/local.md" | "docs/blastfile.md" => Shape::Application,
                other => panic!("{other}: a page with an executed document needs its shape pinned"),
            };
            assert_eq!(
                shape_of(&manifest),
                expected,
                "{file}:{}: the {} opened here documents {expected:?}",
                fence.opening_line,
                fence.opening
            );
        }
        // The retired fences are counted and named, never parsed: this crate has
        // no reader for that spelling, and `MB.21.5.1` rewrites the pages so
        // `docs_fences` can execute them again. The count is cross-checked
        // against the page's own text — an independent scan, not the same
        // collector over the same constant — so the two cannot drift together.
        let named = document
            .lines()
            .filter(|line| line.trim() == RETIRED_OPENING)
            .count();
        assert_eq!(
            actual[2], named,
            "{file}: the pinned retired count and the page's own `{RETIRED_OPENING}` lines agree"
        );
        let collected =
            retired_documents(&document).unwrap_or_else(|error| panic!("{file}: {error}"));
        assert_eq!(
            collected.len(),
            named,
            "{file}: every `{RETIRED_OPENING}` fence is named, not skipped"
        );
        retired += named;
        total = [
            total[0] + actual[0],
            total[1] + actual[1],
            total[2] + actual[2],
        ];
    }
    assert_eq!(
        total,
        TOTAL,
        "the documented set is {}; a page that gained or dropped a complete \
         document was not pinned in `PAGES` (§16.2)",
        label(TOTAL)
    );
    assert_eq!(
        retired, TOTAL[2],
        "the retired opening is counted on every page that still carries it"
    );
}

#[test]
fn both_document_openings_are_collected_and_a_fragment_is_not() {
    // A plain `toml` fence is a fragment; both document spellings are files, and
    // the retired opening is only counted.
    let document = format!(
        "text\n\
         ```text title=\"Blastfile\"\napp \"shop\"\n```\n\n\
         ```blastfile\napp \"shop\"\n```\n\n\
         {RETIRED_OPENING}\nversion = 1\n```\n\n\
         ```toml\nnot_a_manifest = true\n```\n"
    );
    assert_eq!(
        count_fences(&document).expect("every fence closes"),
        [1, 1, 1],
        "each complete opening is counted once"
    );
    let collected = documents(&document).expect("every fence closes");
    let openings: Vec<_> = collected.iter().map(|fence| fence.opening).collect();
    assert_eq!(openings, [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING]);
    assert_eq!(collected[0].body, "app \"shop\"\n");
    assert_eq!(collected[0].opening_line, 2, "1-based page line");
    // The retired fence is reachable by its own collector, and only that way.
    assert_eq!(
        retired_documents(&document).expect("closes").len(),
        1,
        "the retired opening is counted and named"
    );
    assert_eq!(
        toml_fragments(&document).expect("closes").len(),
        1,
        "a plain toml fence is still not a complete file"
    );
}

#[test]
fn an_unclosed_fence_is_refused() {
    // Nothing after an unclosed fence can be trusted, so the page is refused
    // rather than read as if the rest of it were prose.
    let error = count_fences("intro\n```blastfile\napp \"shop\"\n")
        .expect_err("an unclosed fence must be refused");
    assert_eq!(
        error,
        FenceError::Unclosed {
            opening: BLASTFILE_OPENING,
            line: 2
        }
    );
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn a_document_that_does_not_parse_names_its_page_position() {
    // Self-test on a fixture, not on the repository's pages, so it stays green
    // while `MB.21.5.1` migrates them.
    let document = "intro\n\n```blastfile\napp \"shop\"\nnot_a_directive\n```\n";
    let fence = documents(document).expect("closes").remove(0);
    let errors =
        parse_document(&fence.body).expect_err("a directive that is not one must not parse");
    let first = errors.errors().first().expect("one refusal");
    assert!(
        first.to_string().starts_with("2:1: "),
        "the refusal points at the body's second line: {first}"
    );
    // The body starts on page line 4, so the body's 2:1 is the page's 5:1.
    assert!(
        describe_failure("fixture.md", &fence, &errors).contains("fixture.md:5:1: "),
        "the failure names the page position"
    );
}

#[test]
fn a_page_whose_count_drifted_is_named() {
    let actual =
        count_fences("```text title=\"Blastfile\"\napp \"shop\"\n```\n").expect("the fence closes");
    let message = check_counts("fixture.md", actual, [0, 0, 0], false)
        .expect_err("a page that gained a document no longer matches its row");
    assert!(
        message.contains("fixture.md") && message.contains("1 `text title=\"Blastfile\"`"),
        "the message names the page and the opening that moved: {message}"
    );
    assert_eq!(check_counts("fixture.md", actual, actual, false), Ok(()));
    // The strict flip's one assertion, which `MB.21.5.2` turns on.
    let retired = [0, 0, 1];
    let message = check_counts("fixture.md", retired, retired, true)
        .expect_err("a page that still carries the retired opening fails the flip");
    assert!(
        message.contains("fixture.md") && message.contains(&format!("`{RETIRED_OPENING}`")),
        "{message}"
    );
    assert_eq!(check_counts("fixture.md", actual, actual, true), Ok(()));
}

#[test]
fn an_invalid_regex_is_refused() {
    // The canary: the suite above would pass on a harness that never compiled a
    // pattern, so one document-shaped file with a `regex` that cannot compile
    // has to fail the boot. The parser compiles a `regex` at its quote (§8.1),
    // so the refusal is `PatternRegex` — the `InvalidRegex` a *route* would
    // report is the validator's, and no document can reach it any more.
    let fence = "app \"shop\"\n\nview products GET regex '/products/(' {\n    body none\n}\n";
    let errors = Manifest::parse_blastfile(fence, UidLayout::DEFAULT_BASE)
        .expect_err("a regex that cannot compile must not boot");
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::PatternRegex { .. })),
        "{errors}"
    );
    assert!(
        errors.to_string().contains("/products/("),
        "the refusal quotes the pattern: {errors}"
    );
}
