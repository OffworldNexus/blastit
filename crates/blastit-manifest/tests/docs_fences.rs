//! Every complete `blastit.toml` in the user documentation parses and validates
//! (`R7`, `R8`, `R11`, `R15`; design §7.1 "Docs Fence Suite").
//!
//! Standard tier on purpose: `MA.3` owns `docs/**`, and a protected copy of this
//! suite would make every documentation edit a `critical_guard` conflict. What
//! it still proves is the developer experience itself — a reader who copies one
//! of these seven blocks into `./blastit.toml` boots.
//!
//! Only the `toml title="blastit.toml"` fences are complete manifests. Partial
//! fragments (`docs/reference/manifest.md`) are prose, not files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use blastit_manifest::{Manifest, ManifestError, StaticPolicy, UidLayout};

/// The seven files that carry a complete `blastit.toml`, relative to the
/// workspace root.
const FILES: [&str; 7] = [
    "README.md",
    "docs/index.md",
    "docs/getting-started.md",
    "docs/local.md",
    "docs/frameworks/django.md",
    "docs/frameworks/flask.md",
    "docs/frameworks/static-and-spa.md",
];

/// The opening line of a complete manifest fence, exactly as written.
const OPENING: &str = "```toml title=\"blastit.toml\"";

/// The repository root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

/// Every complete `blastit.toml` fence in `document`, in order, without the
/// fence markers.
fn blastit_fences(document: &str) -> Vec<String> {
    let mut fences = Vec::new();
    let mut lines = document.lines();
    while let Some(line) = lines.next() {
        if line.trim() != OPENING {
            continue;
        }
        let mut fence = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line.trim() == "```" {
                closed = true;
                break;
            }
            fence.push_str(line);
            fence.push('\n');
        }
        assert!(closed, "unclosed {OPENING} fence");
        fences.push(fence);
    }
    fences
}

/// The one complete manifest fence of `docs/frameworks/static-and-spa.md`.
fn static_page_fence() -> String {
    let path = workspace_root().join("docs/frameworks/static-and-spa.md");
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let fences = blastit_fences(&document);
    assert_eq!(
        fences.len(),
        1,
        "the static page carries exactly one complete blastit.toml fence"
    );
    fences.into_iter().next().expect("one fence")
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
                        spa: true,
                        immutable: vec!["/assets/*".to_string()],
                    })
        })
}

#[test]
fn every_documented_manifest_parses_and_validates() {
    let root = workspace_root();
    let mut total = 0;
    for file in FILES {
        let path = root.join(file);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let fences = blastit_fences(&document);
        assert_eq!(
            fences.len(),
            1,
            "{file} carries exactly one complete blastit.toml fence"
        );
        for fence in fences {
            let (manifest, _warnings) =
                Manifest::parse_and_validate(&fence, UidLayout::DEFAULT_BASE)
                    .unwrap_or_else(|errors| panic!("{file} does not boot:\n{errors}\n{fence}"));
            assert!(
                manifest.route_count() > 0,
                "{file} documents a manifest with a route"
            );
            total += 1;
        }
    }
    assert_eq!(total, 7, "the documented set is exactly seven manifests");
}

#[test]
fn the_static_page_manifest_has_its_documented_shape() {
    let fence = static_page_fence();
    let (manifest, warnings) = Manifest::parse_and_validate(&fence, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| panic!("the static page does not boot:\n{errors}\n{fence}"));
    assert_eq!(warnings, vec![], "the page's block warns about nothing");
    assert!(
        is_the_documented_static_site(&manifest),
        "the page's block is an app-less site serving `public` with `spa` and `/assets/*`:\n{fence}"
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
        let drifted = fence.replace(from, to);
        assert_ne!(drifted, fence, "the canary must change the fence: {from}");
        let (manifest, _) = Manifest::parse_and_validate(&drifted, UidLayout::DEFAULT_BASE)
            .expect("the drifted block still boots");
        assert!(
            !is_the_documented_static_site(&manifest),
            "a block with {to} is no longer the documented shape"
        );
    }
}

#[test]
fn only_a_complete_blastit_fence_is_collected() {
    // A plain `toml` fence is a fragment; only the titled header is a file.
    let document = "text\n\
                    ```toml title=\"blastit.toml\"\nversion = 1\n```\n\n\
                    ```toml\nnot_a_manifest = true\n```\n";
    assert_eq!(blastit_fences(document), ["version = 1\n"]);
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
