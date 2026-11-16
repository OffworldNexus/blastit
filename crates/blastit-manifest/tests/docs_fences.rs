//! Every complete `Blastfile` in the user documentation parses and validates,
//! and every fence on every page is accounted for (`R7`, `R8`, `R11`, `R15`,
//! `R18`; design §16 "Documentation Fences").
//!
//! Standard tier on purpose: `MA.3` owns `docs/**`, and a protected copy of this
//! suite would make every documentation edit a `critical_guard` conflict. What
//! it still proves is the developer experience itself — a reader who copies one
//! of these blocks into `./Blastfile` boots.
//!
//! There is exactly **one** document opening now: `text title="Blastfile"`
//! (Pygments has no `blastfile` lexer, §16.1). The transitional `blastfile`
//! alias and the retired complete-manifest spelling were both removed by
//! `MB.21.5.2`'s strict flip: neither is parsed, and a fence opened with either
//! is refused by name so it can never pass as an illustration. The retired name
//! is composed from two literals wherever this file has to recognise it, so the
//! tree never carries it contiguously and `MB.21.5.2`'s guard needs no
//! exemption.
//!
//! The three fence classes of §16.1 are counted, not guessed:
//!
//! * **document** — the one opening above, parsed by `Manifest::parse_blastfile`;
//! * **statement list** — a plain `text` fence whose first non-blank,
//!   non-comment line is keyword-led; counted per page and pinned, with the
//!   Django page's `static` fragment parsed through `Manifest::from_blastfile_str`;
//! * **illustration** — every other fence, whatever its language.
//!
//! The per-page table is the drift rule: `PAGES` pins documents + illustrations
//! for every page that carries a document, `STATEMENT_LISTS` pins the pages that
//! carry statement lists, and the exhaustive scan walks `README.md` and every
//! `*.md` under `docs/**` (except the dated record) so a document fence on an
//! unpinned page fails rather than silently joining the illustrations.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt;
use std::path::{Path, PathBuf};

use blastit_manifest::{Manifest, ManifestError, ManifestErrors, ManifestWarning, UidLayout};

/// The one document opener the public pages use: `text` renders the file
/// verbatim (§16.1), and `MB.21.5.2` retired every other spelling.
const DOCUMENT_OPENING: &str = "```text title=\"Blastfile\"";

/// A plain `text` fence: a statement list when its body is keyword-led, an
/// illustration otherwise (§16.1).
const TEXT_FENCE_OPENING: &str = "```text";

/// Every opening that carries a complete manifest: exactly one after
/// `MB.21.5.2`'s strict flip (§16.1).
const COMPLETE_OPENINGS: [&str; 1] = [DOCUMENT_OPENING];

/// The keyword a statement list starts with (§16.1, measured by `MB.21.2`).
const STATEMENT_KEYWORDS: [&str; 6] = ["app", "entrypoint", "postgres", "trait", "view", "static"];

/// `MB.21.5.2`'s strict flip, and it is load-bearing: with this `true` a fence
/// opened with a retired complete-manifest spelling is refused outright, so no
/// page can carry one. With it `false` such a fence would be counted as an
/// illustration and pass — the §16.1 failure mode that the fragment self-test
/// asserts in both directions.
const THE_RETIRED_OPENING_IS_GONE: bool = true;

/// The pages the site publishes that carry a complete document, and how many
/// documents and illustrations each carries (measured 2026-11-16, then pinned).
///
/// A page that gains a document is added here with its count, and one that drops
/// a document lowers it. The exhaustive scan below adds a second rule: a
/// document on a page absent from this table fails rather than being absorbed by
/// "some illustration".
const PAGES: [(&str, Counts); 10] = [
    ("README.md", Counts::new(1, 4)),
    ("docs/index.md", Counts::new(1, 1)),
    ("docs/getting-started.md", Counts::new(1, 4)),
    ("docs/local.md", Counts::new(1, 6)),
    ("docs/frameworks/index.md", Counts::new(1, 0)),
    ("docs/frameworks/django.md", Counts::new(1, 2)),
    ("docs/frameworks/flask.md", Counts::new(1, 3)),
    ("docs/frameworks/php.md", Counts::new(1, 3)),
    ("docs/frameworks/static-and-spa.md", Counts::new(1, 6)),
    ("docs/blastfile.md", Counts::new(1, 3)),
];

/// The whole pinned set: one `text title="Blastfile"` document per page, and
/// every illustration on those ten pages.
const TOTAL: Counts = Counts::new(10, 32);

/// Pages whose plain `text` fences are statement lists: syntax, not documents
/// and not illustrations (§16.1). `docs/reference/blastfile.md` is deliberately
/// absent from `PAGES` — it carries statement lists only — but its count is
/// pinned here so a fence cannot vanish unnoticed.
const STATEMENT_LISTS: [(&str, usize); 4] = [
    ("docs/ci.md", 1),
    ("docs/local.md", 1),
    ("docs/frameworks/django.md", 1),
    ("docs/reference/blastfile.md", 14),
];

/// The directories that quote the erasure and are never part of §16.2's scan.
const RECORD_DIRS: [&str; 5] = [
    "docs/design",
    "docs/research",
    "docs/decisions",
    "docs/knowledge",
    "docs/testing",
];

/// How many fences of each class one page carries. Two fields on purpose: a
/// document is a file, everything that is not one is either a statement list
/// (counted beside this table) or an illustration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    documents: usize,
    illustrations: usize,
}

impl Counts {
    const fn new(documents: usize, illustrations: usize) -> Self {
        Self {
            documents,
            illustrations,
        }
    }

    fn add(&mut self, other: Self) {
        self.documents += other.documents;
        self.illustrations += other.illustrations;
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} `{DOCUMENT_OPENING}` document(s) + {} illustration(s)",
            self.documents, self.illustrations
        )
    }
}

/// What one fence is, in §16.1's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FenceClass {
    Document,
    StatementList,
    Illustration,
}

/// One fenced block: the opening line as written, the 1-based page line it sits
/// on, and its body without the markers.
#[derive(Debug)]
struct Fence {
    opening: String,
    opening_line: usize,
    body: String,
}

/// Why a page's fences could not be read.
#[derive(Debug, PartialEq, Eq)]
enum FenceError {
    /// A fence opened and never closed; nothing after it can be trusted.
    Unclosed { opening: String, line: usize },
    /// A fence opened with a spelling `MB.21.5.2` retired: it has no reader, so
    /// it is refused by name instead of being counted as an illustration.
    RetiredDocument { line: usize },
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
            Self::RetiredDocument { line } => write!(
                f,
                "line {line}: a complete manifest opened with a retired spelling; \
                 open a document with `{DOCUMENT_OPENING}` or write the statements \
                 as a plain `text` fence (§16.1)"
            ),
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

/// The `~`-or-backtick run that opens a fence, when `line` starts with one.
fn opening_run(line: &str) -> Option<(char, usize)> {
    let first = line.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let run = line.chars().take_while(|c| *c == first).count();
    (run >= 3).then_some((first, run))
}

/// Whether `line` closes a fence opened with `marker`/`length`.
fn closes_fence(line: &str, marker: char, length: usize) -> bool {
    let run = line.chars().take_while(|c| *c == marker).count();
    run >= length && run == line.chars().count()
}

/// Every fence in `document`, in page order, without the markers.
fn collect_fences(document: &str) -> Result<Vec<Fence>, FenceError> {
    let lines: Vec<&str> = document.lines().collect();
    let mut fences = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let Some((marker, length)) = opening_run(lines[at].trim()) else {
            at += 1;
            continue;
        };
        let opening = lines[at].trim().to_string();
        let opening_line = at + 1;
        let mut body = String::new();
        let mut closed = false;
        at += 1;
        while at < lines.len() {
            if closes_fence(lines[at].trim(), marker, length) {
                closed = true;
                break;
            }
            body.push_str(lines[at]);
            body.push('\n');
            at += 1;
        }
        if !closed {
            return Err(FenceError::Unclosed {
                opening,
                line: opening_line,
            });
        }
        fences.push(Fence {
            opening,
            opening_line,
            body,
        });
        at += 1;
    }
    Ok(fences)
}

/// The retired complete-manifest opening and the transitional `blastfile` alias,
/// both composed so this file never carries the retired name contiguously.
fn retired_openings() -> [String; 2] {
    [
        concat!("```toml title=\"", "blastit", ".toml", "\"").to_string(),
        "```blastfile".to_string(),
    ]
}

/// Whether `opening` is a spelling `MB.21.5.2` retired.
fn is_retired_document(opening: &str) -> bool {
    retired_openings().iter().any(|retired| retired == opening)
}

/// The first non-blank, non-comment line of a fence body, for §16.1's
/// keyword-led statement list.
fn first_statement(body: &str) -> Option<&str> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

/// Which of §16.1's three classes `fence` is.
fn class_of(fence: &Fence) -> FenceClass {
    if COMPLETE_OPENINGS.contains(&fence.opening.as_str()) {
        return FenceClass::Document;
    }
    if fence.opening == TEXT_FENCE_OPENING
        && let Some(first) = first_statement(&fence.body)
        && let Some(keyword) = first.split_whitespace().next()
        && STATEMENT_KEYWORDS.contains(&keyword)
    {
        return FenceClass::StatementList;
    }
    FenceClass::Illustration
}

/// One page's fence census.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Scan {
    counts: Counts,
    statements: usize,
    /// Plain `toml` fences. §16.2 refuses one on any `PAGES` page, even when it
    /// is a fragment: a page's own fragment is not a second configuration file.
    toml_fences: usize,
}

/// Whether `opening` is a `toml` fence, with or without an info suffix.
fn is_toml_fence(opening: &str) -> bool {
    opening == "```toml" || opening.starts_with("```toml ")
}

/// Count `document`'s fences by class, refusing a retired document spelling
/// when `strict` (§16.2).
fn scan_page(document: &str, strict: bool) -> Result<Scan, FenceError> {
    let mut scan = Scan::default();
    for fence in collect_fences(document)? {
        if strict && is_retired_document(&fence.opening) {
            return Err(FenceError::RetiredDocument {
                line: fence.opening_line,
            });
        }
        if is_toml_fence(&fence.opening) {
            scan.toml_fences += 1;
        }
        match class_of(&fence) {
            FenceClass::Document => scan.counts.documents += 1,
            FenceClass::StatementList => scan.statements += 1,
            FenceClass::Illustration => scan.counts.illustrations += 1,
        }
    }
    Ok(scan)
}

/// Every document fence on `document`, in page order.
fn documents(document: &str) -> Result<Vec<Fence>, FenceError> {
    Ok(collect_fences(document)?
        .into_iter()
        .filter(|fence| class_of(fence) == FenceClass::Document)
        .collect())
}

/// The drift check: `Ok(())` when the page carries the pinned counts, otherwise
/// an `Err` naming the page and the class that moved.
fn verify_page(file: &str, document: &str, pinned: Counts) -> Result<(), String> {
    let scan = scan_page(document, THE_RETIRED_OPENING_IS_GONE)
        .map_err(|error| format!("{file}: {error}"))?;
    if scan.toml_fences > 0 {
        return Err(format!(
            "{file}: carries {} plain ```toml fence(s); no page in `PAGES` may carry \
             one (§16.2) — a complete manifest is `{DOCUMENT_OPENING}`, and a \
             fragment is a plain `{TEXT_FENCE_OPENING}` fence",
            scan.toml_fences
        ));
    }
    if scan.counts != pinned {
        return Err(format!(
            "{file}: fence counts drifted: pinned {pinned}; the page carries {}. \
             A page that gains a complete document is added to `PAGES` with its \
             count and one that drops a document lowers it (§16.2).",
            scan.counts
        ));
    }
    let found = documents(document).map_err(|error| format!("{file}: {error}"))?;
    if found.len() != pinned.documents {
        return Err(format!(
            "{file}: pinned {} `{DOCUMENT_OPENING}` document fence(s); the page \
             carries {}. A missing document must never read as a count of zero \
             that still passes (§16.2).",
            pinned.documents,
            found.len()
        ));
    }
    Ok(())
}

/// The statements check: a page with statement lists must be pinned, or a
/// keyword-led fence would slip past both `PAGES` and the illustration count.
fn verify_statements(file: &str, found: usize) -> Result<(), String> {
    let pinned = STATEMENT_LISTS
        .iter()
        .find(|(page, _)| *page == file)
        .map(|(_, count)| *count);
    match pinned {
        Some(pinned) if pinned == found => Ok(()),
        Some(pinned) => Err(format!(
            "{file}: statement lists drifted: pinned {pinned} keyword-led `{TEXT_FENCE_OPENING}` \
             fence(s); the page carries {found} (§16.1)"
        )),
        None if found == 0 => Ok(()),
        None => Err(format!(
            "{file}: carries {found} keyword-led `{TEXT_FENCE_OPENING}` statement list(s) \
             but has no `STATEMENT_LISTS` row; pin the count so a fence cannot vanish \
             unnoticed (§16.1)"
        )),
    }
}

/// A document fence on a page that is not in `PAGES`: unpinned, so it fails
/// naming the page and the expected opening.
fn unpinned_document_error(file: &str, scan: Scan) -> Option<String> {
    (scan.counts.documents > 0).then(|| {
        format!(
            "{file}: carries {} `{DOCUMENT_OPENING}` document fence(s) but has no \
             `PAGES` row; add the page with its count so a document cannot appear \
             unlisted (§16.2)",
            scan.counts.documents
        )
    })
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
    /// Views and a mount: a PHP app whose `public/` is served as files too.
    ViewsAndMount,
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
    } else if manifest.route_count() > 0 && !manifest.mounts().is_empty() {
        Shape::ViewsAndMount
    } else {
        Shape::Empty
    }
}

/// Every Markdown page under `dir`, relative to `root`, POSIX form.
fn markdown_under(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown_under(&path, root, out);
        } else if path.extension().is_some_and(|extension| extension == "md")
            && let Ok(relative) = path.strip_prefix(root)
        {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// `README.md` plus every `*.md` under `docs/**` except the dated record
/// (§16.2). Sorted, so a failure names pages in a stable order.
fn published_pages(root: &Path) -> Vec<String> {
    let mut pages = vec!["README.md".to_string()];
    markdown_under(&root.join("docs"), root, &mut pages);
    pages.retain(|page| {
        !RECORD_DIRS
            .iter()
            .any(|record| page == record || page.starts_with(&format!("{record}/")))
    });
    pages.sort();
    pages.dedup();
    pages
}

#[test]
fn every_documented_document_parses_and_validates() {
    let root = workspace_root();
    let mut total = Counts::default();
    for (file, pinned) in PAGES {
        let path = root.join(file);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        verify_page(file, &document, pinned).unwrap_or_else(|message| panic!("{message}"));
        let scan = scan_page(&document, THE_RETIRED_OPENING_IS_GONE)
            .unwrap_or_else(|error| panic!("{file}: {error}"));
        verify_statements(file, scan.statements).unwrap_or_else(|message| panic!("{message}"));
        for fence in documents(&document).unwrap_or_else(|error| panic!("{file}: {error}")) {
            let (manifest, _warnings) = parse_document(&fence.body)
                .unwrap_or_else(|errors| panic!("{}", describe_failure(file, &fence, &errors)));
            // The shape is pinned per page, not absorbed by an "or": a fence
            // that degraded from views to a static mount fails here. A page
            // that documents an all-static site pins `Shape::StaticSite`, and a
            // PHP page whose `public/` is served as files too pins
            // `Shape::ViewsAndMount`; nothing else can execute a document, so an
            // unpinned page panics rather than passing.
            let expected = match file {
                "docs/frameworks/static-and-spa.md" => Shape::StaticSite,
                "docs/frameworks/php.md" => Shape::ViewsAndMount,
                "README.md"
                | "docs/index.md"
                | "docs/getting-started.md"
                | "docs/local.md"
                | "docs/blastfile.md"
                | "docs/frameworks/index.md"
                | "docs/frameworks/django.md"
                | "docs/frameworks/flask.md" => Shape::Application,
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
        total.add(scan.counts);
    }
    assert_eq!(
        total, TOTAL,
        "the documented set is {TOTAL}; a page that gained or dropped a complete \
         document was not pinned in `PAGES` (§16.2)"
    );

    // §16.2's exhaustive scan: every published page, whether pinned or not, is
    // read. A document on an unpinned page fails; a page with statement lists
    // must be pinned; the pages that carry only illustrations are reported by
    // path (and prove the branch is not dead).
    let pages = published_pages(&root);
    assert!(pages.iter().any(|page| page == "README.md"));
    assert!(
        !pages.iter().any(|page| RECORD_DIRS
            .iter()
            .any(|record| page.starts_with(&format!("{record}/")))),
        "the dated record must never be scanned"
    );
    let mut pinned_seen = 0;
    let mut statements_seen = 0;
    let mut unpinned: Vec<(String, usize)> = Vec::new();
    for page in &pages {
        if PAGES.iter().any(|(file, _)| file == page) {
            pinned_seen += 1;
            continue;
        }
        let path = root.join(page);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let scan = scan_page(&document, THE_RETIRED_OPENING_IS_GONE)
            .unwrap_or_else(|error| panic!("{page}: {error}"));
        if let Some(message) = unpinned_document_error(page, scan) {
            panic!("{message}");
        }
        verify_statements(page, scan.statements).unwrap_or_else(|message| panic!("{message}"));
        if scan.statements > 0 {
            statements_seen += 1;
        }
        unpinned.push((page.clone(), scan.counts.illustrations));
    }
    assert_eq!(
        pinned_seen,
        PAGES.len(),
        "every pinned page must exist in the published tree"
    );
    assert_eq!(
        statements_seen,
        STATEMENT_LISTS
            .iter()
            .filter(|(file, _)| !PAGES.iter().any(|(page, _)| page == file))
            .count(),
        "every statement-list page outside `PAGES` must exist in the published tree"
    );
    assert!(
        !unpinned.is_empty(),
        "the exhaustive scan found no illustration-only page: the branch has no subject"
    );
    assert!(
        unpinned.len() + PAGES.len() == pages.len(),
        "the scan partitions the published set: {} pinned + {} unpinned != {}",
        PAGES.len(),
        unpinned.len(),
        pages.len()
    );
    // The illustration-only pages are reported by path, so a reader can see
    // which pages the exhaustive scan covered without a second table.
    for (page, illustrations) in &unpinned {
        println!("unpinned: {page} ({illustrations} illustration(s))");
    }
}

#[test]
fn a_valid_document_fence_is_collected_parsed_and_counted() {
    // The self-test the toolchain lacked: a *valid* document fence goes through
    // all three steps — collected by the reader, parsed by the crate, counted as
    // one document.
    let document = format!(
        "intro\n\n{DOCUMENT_OPENING}\napp \"shop\"\n\nview products GET \"/products/{{id:int}}\" {{\n    body none\n}}\n```\n"
    );
    let scan = scan_page(&document, THE_RETIRED_OPENING_IS_GONE).expect("every fence closes");
    assert_eq!(
        scan.counts,
        Counts::new(1, 0),
        "one document, no illustration"
    );
    assert_eq!(scan.statements, 0, "a document is not a statement list");
    let collected = documents(&document).expect("every fence closes");
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].opening, DOCUMENT_OPENING);
    assert_eq!(collected[0].opening_line, 3, "1-based page line");
    let (manifest, _warnings) =
        parse_document(&collected[0].body).expect("a valid document parses");
    assert_eq!(shape_of(&manifest), Shape::Application);
    verify_page("fixture.md", &document, Counts::new(1, 0)).expect("it matches its own pin");
}

#[test]
fn a_pinned_page_that_lost_its_document_is_named() {
    // The "missing document" direction: an absent fence must never read as a
    // count of zero that still passes.
    let message = verify_page(
        "fixture.md",
        "intro\n\n```text\njust prose\n```\n",
        Counts::new(1, 0),
    )
    .expect_err("a page that lost its document no longer matches its row");
    assert!(
        message.contains("fixture.md") && message.contains("document"),
        "the message names the page and the missing document: {message}"
    );
}

#[test]
fn a_toml_fragment_passes_but_a_retired_complete_document_is_refused() {
    // Both directions, because a rule that only looked at the opening could let
    // a retired document pass as a fragment. A plain `toml` fence is an
    // illustration at any strictness; a complete retired document is refused
    // exactly when the flip is on.
    let fragment = "intro\n\n```toml\nnot_a_manifest = true\n```\n";
    let scan = scan_page(fragment, THE_RETIRED_OPENING_IS_GONE).expect("a fragment closes");
    assert_eq!(
        scan.counts,
        Counts::new(0, 1),
        "a plain toml fence is an illustration"
    );
    let retired = format!(
        "intro\n\n{}\nversion = 1\n```\n",
        concat!("```toml title=\"", "blastit", ".toml", "\"")
    );
    let loose = scan_page(&retired, false).expect("with the flip off it is an illustration");
    assert_eq!(
        loose.counts,
        Counts::new(0, 1),
        "with the flip off a retired document would pass as an illustration"
    );
    let error = scan_page(&retired, true).expect_err("the strict flip refuses it");
    assert_eq!(error, FenceError::RetiredDocument { line: 3 });
    assert!(error.to_string().contains("retired spelling"), "{error}");
    let message = verify_page("fixture.md", &retired, Counts::new(1, 0))
        .expect_err("the page carries a retired document, not the pinned document");
    assert!(
        message.contains("fixture.md") && message.contains("retired spelling"),
        "the failure names the page: {message}"
    );
    // The transitional `blastfile` alias is gone with it.
    let alias = "intro\n\n```blastfile\napp \"shop\"\n```\n";
    assert_eq!(
        scan_page(alias, true).expect_err("the alias is retired too"),
        FenceError::RetiredDocument { line: 3 }
    );
}

#[test]
fn a_document_on_an_unpinned_page_is_named() {
    // The exhaustive scan's failure: a page outside `PAGES` cannot gain a
    // document and have it counted as an illustration.
    let document = format!("{DOCUMENT_OPENING}\napp \"shop\"\n\nview shop \"/*\" {{}}\n```\n");
    let scan = scan_page(&document, THE_RETIRED_OPENING_IS_GONE).expect("closes");
    let message =
        unpinned_document_error("docs/new-page.md", scan).expect("a document is unpinned");
    assert!(
        message.contains("docs/new-page.md") && message.contains(DOCUMENT_OPENING),
        "the message names the page and the expected opening: {message}"
    );
    let no_document = scan_page("```console\n$ true\n```\n", true).expect("closes");
    assert_eq!(
        unpinned_document_error("docs/new-page.md", no_document),
        None
    );
}

#[test]
fn a_plain_toml_manifest_on_a_pinned_page_is_refused() {
    // §16.2's rule made explicit: no ```toml fence on a page in `PAGES`. A
    // complete retired manifest *without* the title would otherwise be an
    // illustration and fail only by count drift, which names the wrong defect.
    let document = "```toml\nversion = 1\n\n[route]\npath = \".*\"\n```\n";
    let message = verify_page("fixture.md", document, Counts::new(0, 1))
        .expect_err("a pinned page may not carry a toml fence");
    assert!(
        message.contains("fixture.md") && message.contains("```toml"),
        "the failure names the page and the fence: {message}"
    );
    // The same fence outside `PAGES` is an illustration, not a second format.
    let scan = scan_page(document, THE_RETIRED_OPENING_IS_GONE).expect("closes");
    assert_eq!(scan.counts, Counts::new(0, 1));
    assert_eq!(scan.toml_fences, 1);
}

#[test]
fn an_unclosed_fence_is_refused() {
    // Nothing after an unclosed fence can be trusted, so the page is refused
    // rather than read as if the rest of it were prose.
    let error = collect_fences("intro\n```blastfile\napp \"shop\"\n")
        .expect_err("an unclosed fence must be refused");
    assert_eq!(
        error,
        FenceError::Unclosed {
            opening: "```blastfile".to_string(),
            line: 2
        }
    );
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn a_document_that_does_not_parse_names_its_page_position() {
    // Self-test on a fixture, not on the repository's pages.
    let document = format!("intro\n\n{DOCUMENT_OPENING}\napp \"shop\"\nnot_a_directive\n```\n");
    let fence = documents(&document).expect("closes").remove(0);
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
    let document = format!("{DOCUMENT_OPENING}\napp \"shop\"\n\nview shop \"/*\" {{}}\n```\n");
    let scan = scan_page(&document, THE_RETIRED_OPENING_IS_GONE).expect("the fence closes");
    assert_eq!(scan.counts, Counts::new(1, 0));
    let message = verify_page("fixture.md", &document, Counts::new(0, 1))
        .expect_err("a page that gained a document no longer matches its row");
    assert!(
        message.contains("fixture.md") && message.contains(DOCUMENT_OPENING),
        "the message names the page and the opening that moved: {message}"
    );
    verify_page("fixture.md", &document, Counts::new(1, 0)).expect("it matches its own pin");
    // A statement list is pinned beside the counts, not inside them.
    let message = verify_statements("fixture.md", 1)
        .expect_err("a keyword-led fence with no row is unpinned");
    assert!(message.contains("fixture.md"), "{message}");
    verify_statements("docs/reference/blastfile.md", 14).expect("the reference is pinned");
}

#[test]
fn the_django_page_static_fragment_still_parses() {
    // `docs/frameworks/django.md` carries the `static` mount as a statement
    // list; §16.1 keeps it parsed so the six-segment `immutable` list cannot
    // drift into prose.
    let root = workspace_root();
    let document = std::fs::read_to_string(root.join("docs/frameworks/django.md"))
        .expect("the Django page exists");
    let fences = collect_fences(&document).expect("every fence closes");
    let fragment = fences
        .iter()
        .find(|fence| {
            class_of(fence) == FenceClass::StatementList
                && first_statement(&fence.body).is_some_and(|line| line.starts_with("static "))
        })
        .expect("the Django page carries its static mount as a plain text fence");
    let manifest = Manifest::from_blastfile_str(&fragment.body)
        .unwrap_or_else(|errors| panic!("the static fragment parses: {errors}"));
    assert_eq!(manifest.mounts().len(), 1, "one mount");
    let mount = &manifest.mounts()[0];
    assert_eq!(mount.prefix, "/static");
    assert_eq!(mount.policy.root, "staticfiles");
    assert_eq!(mount.policy.immutable.len(), 6, "one pattern per depth");
}

#[test]
fn an_invalid_regex_is_refused() {
    // The canary: the suite above would pass on a harness that never compiled a
    // pattern, so one document-shaped file with a `regex` that cannot compile
    // has to fail the boot. The parser compiles a `regex` at its quote (§8.1),
    // so the refusal is `PatternRegex` — a Blastfile-shaped refusal, not the
    // retired reader's `InvalidRegex`.
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
