//! Fixtures shared by the integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestErrors, ManifestWarning, UidLayout};

/// Borrows a list of manifest strings for comparison with literal arrays.
pub fn texts(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

/// Borrows a string map for comparison with literal arrays.
pub fn text_map(map: &std::collections::BTreeMap<String, String>) -> Vec<(&str, &str)> {
    map.iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

/// The application of ARCHITECTURE.md §6, written as a `Blastfile` (§7.2).
///
/// It is the same four views and the same grants as the section's example: one
/// store, the shared `session` grant set, the shared `catalog` one, one inline
/// grant per view, `prefork` on the hot view and `egress any` on checkout.
///
/// Two rows of the example cannot be carried over, and neither is a way of
/// writing this file (§7.1): the example attaches `session` to every route with
/// a top-level `groups` list, but a `Blastfile` has no spelling for "attach this
/// trait to every view", so each view that wants `session` writes `use session`
/// and `fallback` — the view the example leaves without grants — writes none.
/// The other dropped row is the store's `ca_cert` slot, because `url` is the
/// one-line form only (§7.4).
pub const SECTION_6: &str = r#"app "shop"

postgres $DATABASE_URL

# Connection setup, plus what middleware does on every request.
trait session {
    postgres {
        transactions
        query "SET TIME ZONE 'UTC'"
        query 'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21'
    }
}

trait catalog {
    postgres {
        query 'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21'
    }
}

# Full match against PATH_INFO; the regex form is the one the section writes.
view product-detail GET regex '/products/[0-9]+/' {
    use session, catalog
    prefork
}

view checkout regex '/checkout/.*' {
    use session, catalog
    egress any
    postgres {
        query 'INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id"'
    }
}

view about "/about/" {
    use session
    postgres {
        query 'SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1 LIMIT 21'
    }
}

# Catch-all: the app renders its own 404s. It writes no `use`: a view that does
# not ask for a trait inherits nothing (§7.1).
view fallback regex '.*' {
}
"#;

/// The two stores of ARCHITECTURE.md §5.1, as a `Blastfile` (§7.1, §7.4, §7.5).
///
/// §5.1's `db` also carried `ca_cert` and its `analytics` carried an `output`
/// map. Neither has a spelling in this language: `url` is the one-line store
/// form (§7.4), so a store cannot hold it beside a second slot, and the `output`
/// map is dropped outright (§7.5, `ADR-0012`). The fixture therefore carries the
/// one-line form the language has, and the model's `output` maps are asserted
/// empty rather than absent.
pub const SECTION_5_1: &str = r#"postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL
"#;

/// The §6 application, parsed.
pub fn section_6() -> Manifest {
    Manifest::from_blastfile_str(SECTION_6).expect("§6 example should parse")
}

/// The §5.1 stores, parsed.
pub fn section_5_1() -> Manifest {
    Manifest::from_blastfile_str(SECTION_5_1).expect("§5.1 example should parse")
}

/// Parses a document that must be refused at parse or lower time, and returns
/// its problems.
pub fn parse_errors(source: &str) -> ManifestErrors {
    Manifest::from_blastfile_str(source).expect_err("manifest should not parse")
}

/// Validates a document that parses but must not validate.
pub fn validate_errors(source: &str, base: u32) -> ManifestErrors {
    let manifest = Manifest::from_blastfile_str(source).expect("manifest should parse");
    manifest
        .validate(base)
        .expect_err("manifest should not validate")
}

/// Validates a document and returns its warnings.
pub fn validate_warnings(source: &str) -> Vec<ManifestWarning> {
    let manifest = Manifest::from_blastfile_str(source).expect("manifest should parse");
    manifest
        .validate(UidLayout::DEFAULT_BASE)
        .expect("manifest should validate")
}

// ---- the Blastfile fixture corpus (§20 R8) ------------------------------

/// The fixture directory: one flat directory of `*.blastfile` documents, one
/// per row of §7.1/§7.2.
///
/// It is flat on purpose. §20 makes it the round-trip corpus `MB.21.3.4`
/// (P1–P3), `MB.21.3.6` (fence counts) and `MB.21.5.1` (docs) read, and each of
/// those globs `fixtures/*` and asserts a count — a directory per row would make
/// every one of them recurse and would lose the "15 fixtures" number §20 pins.
///
/// The expected model for a fixture is **not** a file: each test builds it field
/// by field, so both sides of the parity oracle are readable in one place and
/// neither comes from a document.
pub fn fixture_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture, as `(file name, source)`, in file-name order.
pub fn corpus() -> Vec<(String, String)> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(fixture_directory())
        .expect("the fixture directory is readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "blastfile")
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
            (name, source)
        })
        .collect()
}

/// One fixture's source, by file name.
pub fn fixture(name: &str) -> String {
    let path = fixture_directory().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

/// How many fixtures the corpus holds.
///
/// §20 pins the number: the corpus is the 15 rows of §7.1/§7.2, and a later
/// leaf that adds a file has to say so here as well.
pub const CORPUS_SIZE: usize = 15;

// ---- §20's round-trip corpus: the fixtures plus the documented documents --

/// §20's floor for the round-trip corpus: the fixtures plus the documents the
/// pages publish.
///
/// The number is the design's floor, not a measurement. What is measured is the
/// corpus: [`round_trip_corpus`] enumerates the fixture directory and reads
/// every document fence off the pages, and refuses to return a corpus smaller
/// than this. A corpus that silently shrank to three files cannot pass as
/// "some documents" any more (§20, `MB.21.5.7`).
pub const ROUND_TRIP_FLOOR: usize = 20;

/// The one document opening the public pages use: `text` renders the file
/// verbatim (§16.1).
pub const DOCUMENT_OPENING: &str = "```text title=\"Blastfile\"";

/// A plain `text` fence: a statement list when its body is keyword-led, an
/// illustration otherwise (§16.1).
pub const TEXT_FENCE_OPENING: &str = "```text";

/// The keyword a statement list starts with (§16.1).
pub const STATEMENT_KEYWORDS: [&str; 6] =
    ["app", "entrypoint", "postgres", "trait", "view", "static"];

/// The directories that quote the erasure and are never part of §16.2's scan.
pub const RECORD_DIRS: [&str; 5] = [
    "docs/design",
    "docs/research",
    "docs/decisions",
    "docs/knowledge",
    "docs/testing",
];

/// What one fence is, in §16.1's terms.
///
/// The three classes partition every fence on every published page, so a fence
/// is never skipped silently: a complete file is [`FenceClass::Document`], a
/// fragment of one is [`FenceClass::Fragment`], and everything else is an
/// [`FenceClass::Illustration`] (a column of a table or a one-line shape, never
/// a `Blastfile`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceClass {
    /// A complete `Blastfile`: `text title="Blastfile"` (§16.1).
    Document,
    /// A statement list: a plain `text` fence whose first non-blank,
    /// non-comment line is keyword-led. It is a fragment of a file — it has no
    /// `app` and binds no listener — and it is classified here so the
    /// round-trip oracle can treat it as one instead of dropping it.
    Fragment,
    /// Every other fence, whatever its language.
    Illustration,
}

/// One fenced block on one published page.
#[derive(Debug)]
pub struct PageFence {
    /// The page, POSIX-relative to the workspace root (`README.md`, `docs/local.md`).
    pub page: String,
    /// The 1-based page line the opening fence sits on.
    pub opening_line: usize,
    /// The opening line as written.
    pub opening: String,
    /// The body without the fence markers, one `\n` per line.
    pub source: String,
    /// The class §16.1 gives it.
    pub class: FenceClass,
}

impl PageFence {
    /// `page:line`, the name a failure prints.
    pub fn label(&self) -> String {
        format!("{}:{}", self.page, self.opening_line)
    }
}

/// The repository root, from this crate's manifest directory.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// One fenced block before it is classified.
struct RawFence {
    opening: String,
    opening_line: usize,
    body: String,
}

/// Every fence on `page`, in page order, without the markers.
///
/// The scanner is a copy of the one in `tests/docs_fences.rs` on purpose: a
/// test binary cannot depend on another test binary, and the protected suite
/// should not grow a Markdown reader of its own. An unclosed fence is refused
/// by name, because nothing after it can be trusted.
fn fences_on(page: &str, document: &str) -> Vec<RawFence> {
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
        assert!(
            closed,
            "{page}:{opening_line}: the {opening} fence opened here is never closed"
        );
        fences.push(RawFence {
            opening,
            opening_line,
            body,
        });
        at += 1;
    }
    fences
}

/// The first non-blank, non-comment line of a fence body, for §16.1's
/// keyword-led statement list.
fn first_statement(body: &str) -> Option<&str> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

/// Which of §16.1's three classes a fence is.
fn class_of(opening: &str, body: &str) -> FenceClass {
    if opening == DOCUMENT_OPENING {
        return FenceClass::Document;
    }
    if opening == TEXT_FENCE_OPENING
        && let Some(first) = first_statement(body)
        && let Some(keyword) = first.split_whitespace().next()
        && STATEMENT_KEYWORDS.contains(&keyword)
    {
        return FenceClass::Fragment;
    }
    FenceClass::Illustration
}

/// Every Markdown page under `dir`, relative to `root`, POSIX form.
fn markdown_under(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
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
pub fn published_pages() -> Vec<String> {
    let root = workspace_root();
    let mut pages = vec!["README.md".to_string()];
    markdown_under(&root.join("docs"), &root, &mut pages);
    pages.retain(|page| {
        !RECORD_DIRS
            .iter()
            .any(|record| page == record || page.starts_with(&format!("{record}/")))
    });
    pages.sort();
    pages.dedup();
    pages
}

/// Every fence on every published page, classified (§16.1).
///
/// Reading is exhaustive on purpose: a page whose fence never closes, or a page
/// that vanishes, fails here by name instead of shrinking the corpus by one.
pub fn documented_fences() -> Vec<PageFence> {
    let root = workspace_root();
    let mut fences = Vec::new();
    for page in published_pages() {
        let path = root.join(&page);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for raw in fences_on(&page, &document) {
            let class = class_of(&raw.opening, &raw.body);
            fences.push(PageFence {
                page: page.clone(),
                opening_line: raw.opening_line,
                opening: raw.opening,
                source: raw.body,
                class,
            });
        }
    }
    fences
}

/// The pages that publish a complete document today (`MB.21.5.7`'s review).
///
/// A total is not enough: with 27 documents against a floor of 20, a page that
/// silently stopped yielding its document still passes. So the pages that carry
/// one are pinned **as pages**, and [`documented_documents`] asserts every one of
/// them still yields at least one document — naming the page that dropped out.
/// The list is data, not a count: adding a page's document does not need an edit
/// here (the corpus grows either way), dropping one fails by name.
pub const DOCUMENTED_PAGES: [&str; 12] = [
    "README.md",
    "docs/blastfile.md",
    "docs/frameworks/php/index.md",
    "docs/frameworks/php/laravel.md",
    "docs/frameworks/php/symfony.md",
    "docs/frameworks/python/django.md",
    "docs/frameworks/python/flask.md",
    "docs/frameworks/python/index.md",
    "docs/frameworks/static-and-spa.md",
    "docs/getting-started.md",
    "docs/index.md",
    "docs/local.md",
];

/// Every complete documented document: the `text title="Blastfile"` fences.
///
/// The page set is checked here, not by the caller, so every suite that takes
/// the corpus inherits it: a page whose fence stops being a document fails the
/// count by name instead of being absorbed by the total.
pub fn documented_documents() -> Vec<PageFence> {
    let documents: Vec<PageFence> = documented_fences()
        .into_iter()
        .filter(|fence| fence.class == FenceClass::Document)
        .collect();
    assert!(
        !documents.is_empty(),
        "§20's corpus needs the documented documents, but no {DOCUMENT_OPENING} \
         fence was found on README.md or under docs/**"
    );
    for page in DOCUMENTED_PAGES {
        assert!(
            documents.iter().any(|fence| fence.page == page),
            "{page} publishes a {DOCUMENT_OPENING} document in §20's corpus and now \
             yields none: a page that fails to yield a document fails the count"
        );
    }
    documents
}

/// Every documented fragment: a keyword-led plain `text` fence, i.e. a
/// statement list. It is not a complete file, so it is named and classified
/// rather than silently skipped.
pub fn documented_fragments() -> Vec<PageFence> {
    documented_fences()
        .into_iter()
        .filter(|fence| fence.class == FenceClass::Fragment)
        .collect()
}

/// §20's round-trip corpus: the fixtures plus the documented documents,
/// labelled by where they come from.
///
/// Both halves are **enumerated**, never counted from a literal: the fixtures
/// come from [`corpus`] (the flat directory §20 names) and the documents from
/// [`documented_documents`] (every `text title="Blastfile"` fence on
/// `README.md` and `docs/**`, which also refuses a page that stopped yielding
/// one — a total of 26 would otherwise still clear a floor of 20). The floor is
/// asserted here, so every suite that takes the corpus inherits the
/// anti-vacuity check (`MB.21.5.7`); the fixture set keeps its own exact count
/// in [`CORPUS_SIZE`] for the fixture-only suites.
pub fn round_trip_corpus() -> Vec<(String, String)> {
    let fixtures = corpus();
    // `documented_documents` refuses a page that stopped yielding its document,
    // so the per-page floor is inherited here as well as by the fragment suite.
    let documents = documented_documents();
    let mut corpus: Vec<(String, String)> = fixtures
        .into_iter()
        .map(|(name, source)| (format!("fixtures/{name}"), source))
        .collect();
    let fixture_count = corpus.len();
    let document_count = documents.len();
    corpus.extend(
        documents
            .into_iter()
            .map(|fence| (fence.label(), fence.source)),
    );
    assert!(
        corpus.len() >= ROUND_TRIP_FLOOR,
        "§20's round-trip corpus holds {} document(s) ({fixture_count} fixtures + \
         {document_count} documented); the floor is {ROUND_TRIP_FLOOR}",
        corpus.len()
    );
    corpus
}
