//! `TESTING_PLAN.md` §7 — `Blastfile` formatting is a comment-preserving fixed
//! point, and every `Blastfile` the user documentation publishes or fragments
//! parses and survives `blastit fmt --check`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use blastit_manifest::blastfile::fmt;
use blastit_manifest::{Manifest, UidLayout};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// Every documented opening starts its title with `Blastfile` (Pygments has no
/// `blastfile` lexer, so `text` renders the file verbatim).
const OPENING: &str = "Blastfile";
/// One document per shape the reference documents: the formatter's corpus.
const CORPUS: [(&str, &str); 5] = [
    (
        "app-and-entrypoint",
        "app \"shop\"\n\nentrypoint python \"shop.wsgi:application\" {\n    preload \"shop.urls\", \"shop.signals\"\n}\n",
    ),
    (
        "stores-and-trait",
        "postgres $DATABASE_URL\n\npostgres analytics $ANALYTICS_URL\n\ntrait session {\n    postgres {\n        transactions\n        query \"SET TIME ZONE 'UTC'\"\n    }\n}\n\nview account GET \"/account\" {\n    use session\n}\n",
    ),
    (
        "grants",
        "postgres $DATABASE_URL\n\nview catalog GET \"/catalog\", \"/catalog/{id:int}\" {\n    postgres {\n        select products(id, name, price)\n        update products(price)\n        delete archived_products\n        query 'SELECT count(*) FROM products WHERE price > $?'\n    }\n}\n",
    ),
    (
        "bodies",
        "view cart {\n    GET \"/cart\"\n    POST \"/cart/items\" {\n        body json\n    }\n    PUT \"/cart/items/{id:uuid}\" {\n        body form\n    }\n    DELETE \"/cart/items/{id}\" {\n        body raw unvalidated\n    }\n}\n",
    ),
    (
        "static-only",
        "static \"/\" {\n    root \"public\"\n    index \"index.html\"\n    immutable \"/assets/*\", \"/fonts/*\"\n    fallback \"index.html\"\n}\n",
    ),
];
//
// Formatting: a comment-preserving fixed point that keeps the meaning.
//

#[test]
fn formatting_is_a_comment_preserving_fixed_point() {
    for (name, source) in CORPUS {
        let once = fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let twice = fmt(&once).unwrap_or_else(|error| panic!("{name} twice: {error}"));
        assert_eq!(twice, once, "{name} is not a fixed point");
        assert!(
            once.ends_with('\n') && !once.ends_with("\n\n"),
            "{name} does not end with exactly one newline"
        );

        let (before, before_warnings) = Manifest::parse_blastfile(source, BASE)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let (after, after_warnings) = Manifest::parse_blastfile(&once, BASE)
            .unwrap_or_else(|error| panic!("{name} formatted: {error}"));
        assert!(after.equivalent(&before), "{name} changed meaning");
        assert_eq!(after_warnings, before_warnings, "{name} changed warnings");
    }

    // A one-line block is expanded, a `;` becomes a newline, and both comments
    // survive exactly once.
    let source = "# top\napp \"shop\"; postgres $DATABASE_URL # trailing\n\n# the view\nview v GET \"/x\" { body json }\n";
    let formatted = fmt(source).expect("the messy document formats");
    assert!(formatted.starts_with("# top\n"), "{formatted:?}");
    assert!(
        formatted.contains("\npostgres $DATABASE_URL # trailing\n"),
        "{formatted:?}"
    );
    assert!(
        formatted.contains("\n# the view\nview v GET \"/x\" {\n    body json\n}\n"),
        "{formatted:?}"
    );
    assert_eq!(formatted.matches("body json").count(), 1);
    assert_eq!(fmt(&formatted).expect("a fixed point"), formatted);
}

#[test]
fn a_canonical_file_comes_back_byte_for_byte() {
    let canonical = concat!(
        "app \"shop\"\n",
        "\n",
        "postgres $DATABASE_URL\n",
        "\n",
        "view products GET \"/products\" {\n",
        "    body json\n",
        "    postgres {\n",
        "        select products(id, name)\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(fmt(canonical).expect("canonical parses"), canonical);
}

#[test]
fn formatting_refuses_what_the_reader_refuses() {
    for source in [
        "view products GET \"/products\" {\n    body xml\n}\n",
        "view v \"/x/{id:int\" {\n}\n",
        "version = 1\n",
    ] {
        let errors = fmt(source).expect_err("the document is refused");
        let rendered = errors.errors()[0].to_string();
        assert!(
            rendered.starts_with(|c: char| c.is_ascii_digit()),
            "{rendered}"
        );
        assert!(!rendered.contains('\n'), "{rendered:?}");
    }
    // `body xml` is refused at the codec, not at the block.
    let rendered = fmt("view products GET \"/products\" {\n    body xml\n}\n")
        .expect_err("`xml` is not a codec")
        .errors()[0]
        .to_string();
    assert!(
        rendered.starts_with("2:10: ") && rendered.contains("not a codec"),
        "{rendered}"
    );

    // A file the formatter would grow past the reader's cap is refused rather
    // than written.
    let mut source = String::from("view v \"/x\" {\npostgres {\n");
    for _ in 0..1_000_000 {
        source.push_str("delete t\n");
    }
    source.push_str("}\n}\n");
    assert!(source.len() < 16_777_216);
    let rendered = fmt(&source)
        .expect_err("the canonical form is over the cap")
        .to_string();
    assert!(rendered.contains("16777216"), "{rendered}");
}

//
// The documentation is the contract: every published `Blastfile` fence parses
// and is already canonical, so `blastit fmt --check` passes on a copied
// example. A *document* is a complete file; a *fragment* is a lone store block
// or statement list, and is only parsed.
//

/// The directories and pages whose fences quote the design record rather than
/// the public contract.
const EXCLUDED: [&str; 6] = [
    "docs/design",
    "docs/decisions",
    "docs/research",
    "docs/knowledge",
    "docs/testing",
    "docs/VISION.md",
];

/// What one documented fence is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A complete file: `title="Blastfile"`.
    Document,
    /// A fragment: a `blastfile` fence, or a `Blastfile: …` titled one.
    Fragment,
}

/// Classifies a fence by its info string, from the opening line alone.
fn kind(info: &str) -> Option<Kind> {
    if info.split_whitespace().next() == Some("blastfile") {
        return Some(Kind::Fragment);
    }
    let title = info
        .split_once("title=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(title, _)| title)?;
    if title == OPENING {
        Some(Kind::Document)
    } else {
        title.starts_with(OPENING).then_some(Kind::Fragment)
    }
}

/// Every `Blastfile` fence of `page`: its 1-based opening line, its info string
/// and its body, de-indented by the opening's own indentation (a content tab
/// indents its fence four or eight spaces).
fn fences(page: &str) -> Vec<(usize, String, String)> {
    let lines: Vec<&str> = page.lines().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let opening = lines[at].trim();
        let Some(info) = opening.strip_prefix("```") else {
            at += 1;
            continue;
        };
        if kind(info).is_none() {
            at += 1;
            continue;
        }
        let line = at + 1;
        let indent = lines[at].len() - lines[at].trim_start().len();
        let mut body = String::new();
        at += 1;
        while at < lines.len() && !closes(lines[at]) {
            body.push_str(deindent(lines[at], indent));
            body.push('\n');
            at += 1;
        }
        assert!(
            at < lines.len(),
            "the fence opened on line {line} is never closed"
        );
        found.push((line, info.trim().to_string(), body));
        at += 1;
    }
    found
}

/// Whether `line` is a bare closing fence.
fn closes(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3 && trimmed.bytes().all(|byte| byte == b'`')
}

/// `line` without up to `width` leading space or tab characters.
fn deindent(line: &str, width: usize) -> &str {
    let bytes = line.as_bytes();
    let mut cut = 0;
    while cut < width && cut < bytes.len() && (bytes[cut] == b' ' || bytes[cut] == b'\t') {
        cut += 1;
    }
    &line[cut..]
}

/// Every Markdown page under `dir`.
fn markdown_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown_under(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            out.push(path);
        }
    }
}

/// How many documented `(documents, fragments)` `page` carries, or the first
/// fence that fails to boot or is not canonical, named by page and line.
fn check_page(file: &str, page: &str) -> Result<(usize, usize), String> {
    let (mut documents, mut fragments) = (0, 0);
    for (line, info, body) in fences(page) {
        match kind(&info).expect("the fence was classified above") {
            Kind::Document => {
                let (manifest, _) = Manifest::parse_blastfile(&body, BASE).map_err(|errors| {
                    format!("{file}:{line}: the documented Blastfile does not boot:\n{errors}")
                })?;
                if manifest.route_count() + manifest.mounts().len() == 0 {
                    return Err(format!(
                        "{file}:{line}: the documented Blastfile reaches no view and no mount"
                    ));
                }
                documents += 1;
            }
            Kind::Fragment => {
                Manifest::from_blastfile_str(&body).map_err(|errors| {
                    format!("{file}:{line}: the documented fragment does not parse:\n{errors}")
                })?;
                fragments += 1;
            }
        }
        let formatted =
            fmt(&body).map_err(|errors| format!("{file}:{line}: it does not format:\n{errors}"))?;
        if formatted != body {
            return Err(format!(
                "{file}:{line}: the documented fence is not canonical; `blastit fmt` would rewrite it"
            ));
        }
    }
    Ok((documents, fragments))
}

#[test]
fn every_documented_blastfile_fence_parses_and_is_canonical() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists");
    let mut pages = vec![root.join("README.md")];
    markdown_under(&root.join("docs"), &mut pages);
    pages.sort();
    let (mut documents, mut fragments, mut checked) = (0, 0, 0);
    for page in &pages {
        let file = page
            .strip_prefix(&root)
            .unwrap_or(page)
            .to_string_lossy()
            .replace('\\', "/");
        if EXCLUDED
            .iter()
            .any(|excluded| file == *excluded || file.starts_with(&format!("{excluded}/")))
        {
            continue;
        }
        let text = std::fs::read_to_string(page).unwrap_or_else(|error| panic!("{file}: {error}"));
        let (found, parts) = check_page(&file, &text).unwrap_or_else(|message| panic!("{message}"));
        if found + parts > 0 {
            checked += 1;
        }
        documents += found;
        fragments += parts;
    }
    assert!(pages.iter().any(|page| page.ends_with("README.md")));
    assert!(
        checked >= 10,
        "the walk covers the published pages: {checked}"
    );
    assert!(
        documents >= 10,
        "the published pages carry complete files: {documents}"
    );
    assert!(
        fragments >= 1,
        "the store-block fragment is checked: {fragments}"
    );
}

#[test]
fn a_broken_published_example_fails_this_suite() {
    let document = |body: &str| format!("intro\n\n```text title=\"{OPENING}\"\n{body}\n```\n");
    let broken = document("app \"shop\"\nview v GET \"/x\" {\n    body xml\n}");
    let message = check_page("docs/fixture.md", &broken).expect_err("a broken document fails");
    assert!(message.starts_with("docs/fixture.md:3: "), "{message}");
    assert!(message.contains("not a codec"), "{message}");

    let fixed = document("app \"shop\"\n\nview v GET \"/x\" {\n    body json\n}");
    assert_eq!(check_page("docs/fixture.md", &fixed), Ok((1, 0)));

    // A content tab indents its fence; the body is de-indented once.
    let tabbed = format!(
        "=== \"PHP\"\n    ```text title=\"{OPENING}\"\n    app \"shop\"\n\n    view v GET \"/x\" {{\n    }}\n    ```\n"
    );
    assert_eq!(check_page("docs/tabbed.md", &tabbed), Ok((1, 0)));

    // A fragment is a lone store block: parsed, canonical, no view required.
    let fragment = "intro\n\n```text title=\"Blastfile: the store block\"\npostgres {\n    host $DB_HOST\n}\n```\n";
    assert_eq!(check_page("docs/store.md", fragment), Ok((0, 1)));
    let bare = "```blastfile\npostgres {\n    host $DB_HOST\n}\n```\n";
    assert_eq!(check_page("docs/bare.md", bare), Ok((0, 1)));
    let broken =
        "```text title=\"Blastfile: the store block\"\npostgres {\n    bogus $DB_HOST\n}\n```\n";
    let message = check_page("docs/store.md", broken).expect_err("a broken fragment fails");
    assert!(
        message.starts_with("docs/store.md:1: ") && message.contains("does not parse"),
        "{message}"
    );
}
