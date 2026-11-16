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
//! alias §19.8 records); both go through `Manifest::parse_blastfile`. The pages
//! still carry nine `toml title="blastit.toml"` manifests until `MB.21.5.1`
//! migrates them, so that opening is read too — and each opening's count is
//! pinned per page and named in every failure, so `MB.21.5.2`'s strict flip is
//! one constant, not a rewrite.
//!
//! Partial fragments (`docs/reference/manifest.md`) are prose, not files —
//! except the `[route.static]` block `docs/frameworks/django.md` shows on its
//! own, which is parsed under a minimal header so its `immutable` depth cannot
//! drift.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt;
use std::path::{Path, PathBuf};

use blastit_manifest::{
    Manifest, ManifestError, ManifestErrors, ManifestWarning, StaticPolicy, UidLayout,
};

/// The document opener the public pages use: `text` renders the file verbatim.
const TEXT_DOCUMENT_OPENING: &str = "```text title=\"Blastfile\"";

/// The transitional document opener (§19.8); `MB.21.5.2` retires it.
const BLASTFILE_OPENING: &str = "```blastfile";

/// The opening line of a complete manifest fence, exactly as written.
const OPENING: &str = "```toml title=\"blastit.toml\"";

/// The opening line of a plain `toml` fence: a manifest fragment, no header.
const FRAGMENT_OPENING: &str = "```toml";

/// Both spellings of a complete document (§16.1).
const DOCUMENT_OPENINGS: [&str; 2] = [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING];

/// Every opening that carries a complete file, whichever spelling.
const COMPLETE_OPENINGS: [&str; 3] = [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING, OPENING];

/// `MB.21.5.2` flips this to `true` in the leaf that removes the last
/// `toml title="blastit.toml"` fence: the retired opening then has to be absent
/// from every pinned page.
const THE_RETIRED_OPENING_IS_GONE: bool = false;

/// One count per complete opening, in the order `text title="Blastfile"`,
/// `blastfile`, `toml title="blastit.toml"` (§16.2).
type Counts = [usize; 3];

/// Names every opening, so a drifted count says which one moved.
fn label(counts: Counts) -> String {
    format!(
        "{} `text title=\"Blastfile\"` + {} `blastfile` + {} `toml title=\"blastit.toml\"`",
        counts[0], counts[1], counts[2]
    )
}

/// Every page that carries a complete file, and how many of each opening it
/// carries, measured on the pages as they stand (2026-11-16) and then pinned.
///
/// The per-file rule: a page that gains a complete document is added here with
/// its count, and one that drops a document lowers it — `MB.21.5.1` moves the
/// nine TOML rows to documents, `MB.21.5.2` deletes the TOML column.
const PAGES: [(&str, Counts); 10] = [
    ("README.md", [0, 0, 1]),
    ("docs/index.md", [0, 0, 1]),
    ("docs/getting-started.md", [0, 0, 1]),
    ("docs/local.md", [0, 0, 1]),
    ("docs/frameworks/index.md", [0, 0, 1]),
    ("docs/frameworks/django.md", [0, 0, 1]),
    ("docs/frameworks/flask.md", [0, 0, 1]),
    ("docs/frameworks/php.md", [0, 0, 1]),
    ("docs/frameworks/static-and-spa.md", [0, 0, 1]),
    ("docs/blastfile.md", [1, 0, 0]),
];

/// The whole documented set: one `text title="Blastfile"` document today, and
/// the nine `toml title="blastit.toml"` manifests the migration replaces.
const TOTAL: Counts = [1, 0, 9];

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
        count(OPENING),
    ])
}

/// Every complete document in `document`, in page order, whichever spelling.
fn documents(document: &str) -> Result<Vec<Fence>, FenceError> {
    collect_fences(document, &DOCUMENT_OPENINGS)
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
            "{file}: still carries {} `toml title=\"blastit.toml\"` fence(s); the \
             retired opening is gone once `MB.21.5.2`'s strict flip lands (§16.2)",
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

/// The manifest a complete fence declares, through the entry point its opening
/// names: `parse_blastfile` for a document, `parse_and_validate` for the
/// retired `toml title="blastit.toml"` spelling.
fn parse_fence(
    opening: &str,
    body: &str,
) -> Result<(Manifest, Vec<ManifestWarning>), ManifestErrors> {
    if opening == OPENING {
        Manifest::parse_and_validate(body, UidLayout::DEFAULT_BASE)
    } else {
        Manifest::parse_blastfile(body, UidLayout::DEFAULT_BASE)
    }
}

/// The one plain `toml` fragment of `docs/frameworks/django.md`, its
/// `[route.static]` block.
fn django_static_fragment() -> String {
    let path = workspace_root().join("docs/frameworks/django.md");
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let fences = toml_fragments(&document)
        .unwrap_or_else(|error| panic!("docs/frameworks/django.md: {error}"));
    assert_eq!(
        fences.len(),
        1,
        "the Django page carries exactly one plain toml fragment"
    );
    fences.into_iter().next().expect("one fence").body
}

/// The `immutable` list `docs/frameworks/django.md` documents: one pattern per
/// depth, as deep as the deepest fingerprinted file Django's admin ships.
fn documented_immutable_depths() -> Vec<String> {
    [
        "/static/*",
        "/static/*/*",
        "/static/*/*/*",
        "/static/*/*/*/*",
        "/static/*/*/*/*/*",
        "/static/*/*/*/*/*/*",
    ]
    .map(str::to_string)
    .to_vec()
}

/// The one complete fence of `docs/frameworks/static-and-spa.md`, whichever
/// opening it uses, so the shape stays pinned across the page's migration.
fn static_page_fence() -> Fence {
    let path = workspace_root().join("docs/frameworks/static-and-spa.md");
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut fences = collect_fences(&document, &COMPLETE_OPENINGS)
        .unwrap_or_else(|error| panic!("docs/frameworks/static-and-spa.md: {error}"));
    assert_eq!(
        fences.len(),
        1,
        "the static page carries exactly one complete fence"
    );
    fences.remove(0)
}

/// The shape `docs/frameworks/static-and-spa.md` documents, as one predicate.
///
/// Design §7.2 requires the fence's shape to be pinned, not merely parsed: the
/// canary below fails if this predicate would accept a block that drifted.
fn is_the_documented_static_site(manifest: &Manifest) -> bool {
    manifest.is_all_static()
        && manifest.app().is_none()
        && manifest.route_count() == 1
        && manifest.route(0).is_some_and(|route| {
            route.name() == "site"
                && route.path().len() == 1
                && route.path()[0] == ".*"
                && route.static_policy()
                    == Some(&StaticPolicy {
                        root: "public".to_string(),
                        index: "index.html".to_string(),
                        // The page's `spa = true`, lowered to the file a miss
                        // is answered with (§9.3).
                        fallback: Some("index.html".to_string()),
                        immutable: vec!["/assets/*".to_string()],
                    })
        })
}

#[test]
fn every_documented_document_parses_and_validates() {
    let root = workspace_root();
    let mut total = [0, 0, 0];
    for (file, pinned) in PAGES {
        let path = root.join(file);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let actual = count_fences(&document).unwrap_or_else(|error| panic!("{file}: {error}"));
        check_counts(file, actual, pinned, THE_RETIRED_OPENING_IS_GONE)
            .unwrap_or_else(|message| panic!("{message}"));
        let complete = collect_fences(&document, &COMPLETE_OPENINGS)
            .unwrap_or_else(|error| panic!("{file}: {error}"));
        for fence in complete {
            let (manifest, _warnings) = parse_fence(fence.opening, &fence.body)
                .unwrap_or_else(|errors| panic!("{}", describe_failure(file, &fence, &errors)));
            assert!(
                manifest.route_count() > 0,
                "{file}:{}: the {} opened here declares a route",
                fence.opening_line,
                fence.opening
            );
        }
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
}

#[test]
fn the_static_page_manifest_has_its_documented_shape() {
    let fence = static_page_fence();
    let (manifest, warnings) = parse_fence(fence.opening, &fence.body).unwrap_or_else(|errors| {
        panic!(
            "{}",
            describe_failure("docs/frameworks/static-and-spa.md", &fence, &errors)
        )
    });
    assert_eq!(warnings, vec![], "the page's block warns about nothing");
    assert!(
        is_the_documented_static_site(&manifest),
        "the page's block is an app-less site serving `public` with `spa` and `/assets/*`:\n{}",
        fence.body
    );
}

#[test]
fn a_drifted_static_block_does_not_satisfy_the_documented_shape() {
    // The canary: the shape test above would pass on a predicate that always
    // answered true, so every promise the page makes is changed once and has to
    // stop satisfying it.
    let fence = static_page_fence();
    for (from, to) in [
        (r#"root = "public""#, r#"root = "dist""#),
        ("spa = true", "spa = false"),
        (r#"immutable = ["/assets/*"]"#, "immutable = []"),
        (r#"name = "site""#, r#"name = "web""#),
        ("path = ['.*']", "path = ['/assets/.*']"),
    ] {
        let drifted = fence.body.replace(from, to);
        assert_ne!(
            drifted, fence.body,
            "the canary must change the fence: {from}"
        );
        let (manifest, _) =
            parse_fence(fence.opening, &drifted).expect("the drifted block still boots");
        assert!(
            !is_the_documented_static_site(&manifest),
            "a block with {to} is no longer the documented shape"
        );
    }
}

#[test]
fn the_django_static_fragment_pins_its_documented_depth() {
    // The page shows `[route.static]` on its own; a reader pastes it under a
    // manifest that already has a header, so parse it that way.
    let fragment = django_static_fragment();
    let document = format!("version = 1\napp = \"app:app\"\n\n{fragment}");
    let (manifest, warnings) = Manifest::parse_and_validate(&document, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| {
            panic!("the Django static fragment does not boot:\n{errors}\n{document}")
        });
    assert_eq!(warnings, vec![], "the fragment warns about nothing");
    assert_eq!(
        manifest.route_count(),
        1,
        "the fragment documents one route"
    );
    let route = manifest.route(0).expect("the fragment documents a route");
    assert_eq!(route.name(), "static");
    assert_eq!(route.path(), ["/static/.*".to_string()]);
    let policy = route
        .static_policy()
        .expect("the fragment documents a [route.static] block");
    assert_eq!(policy.root, "staticfiles");
    assert_eq!(
        policy.immutable,
        documented_immutable_depths(),
        "the page lists one immutable glob per depth, as deep as the deepest \
         fingerprinted file Django's admin ships"
    );
}

#[test]
fn both_document_openings_are_collected_and_a_fragment_is_not() {
    // A plain `toml` fence is a fragment; both document spellings are files.
    let document = "text\n\
                    ```text title=\"Blastfile\"\napp \"shop\"\n```\n\n\
                    ```blastfile\napp \"shop\"\n```\n\n\
                    ```toml title=\"blastit.toml\"\nversion = 1\n```\n\n\
                    ```toml\nnot_a_manifest = true\n```\n";
    assert_eq!(
        count_fences(document).expect("every fence closes"),
        [1, 1, 1],
        "each complete opening is counted once"
    );
    let collected = documents(document).expect("every fence closes");
    let openings: Vec<_> = collected.iter().map(|fence| fence.opening).collect();
    assert_eq!(openings, [TEXT_DOCUMENT_OPENING, BLASTFILE_OPENING]);
    assert_eq!(collected[0].body, "app \"shop\"\n");
    assert_eq!(collected[0].opening_line, 2, "1-based page line");
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
    let errors = Manifest::parse_blastfile(&fence.body, UidLayout::DEFAULT_BASE)
        .expect_err("a directive that is not one must not parse");
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
        message.contains("fixture.md") && message.contains("`toml title=\"blastit.toml\"`"),
        "{message}"
    );
    assert_eq!(check_counts("fixture.md", actual, actual, true), Ok(()));
}

#[test]
fn an_invalid_regex_is_refused() {
    // The canary: the suite above would pass on a parser that accepts anything,
    // so one documented-shape manifest with a regex that cannot compile has to
    // fail the boot.
    let fence = "version = 1\n\
                 app = \"app:app\"\n\
                 [[route]]\nname = \"broken\"\npath = ['/products/(']\n";
    let errors = Manifest::parse_and_validate(fence, UidLayout::DEFAULT_BASE)
        .expect_err("a regex that cannot compile must not boot");
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::InvalidRegex { .. })),
        "{errors}"
    );
}
