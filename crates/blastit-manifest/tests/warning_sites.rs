//! The warning sites `blastit lsp` squiggles (`docs/design/blastfile-lsp-zed.md`
//! §5.4, §5.5; `MB.22.3`).
//!
//! `blastfile::check` is the editor's one entry point: the manifest, its
//! warnings and the CST site of each. The two fixtures below are the design's,
//! because no single file can make all six warnings fire
//! (`EntrypointNotReached` needs a manifest with no route; the other five need
//! views or a trait).
//!
//! Every assertion is written per warning and per fixture line — the line is
//! found by searching the fixture for the word the site is on — so a hand-count
//! can never drift and a `sites()` that returned one position for everything
//! fails the distinctness check.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::ManifestWarning;
use blastit_manifest::Position;
use blastit_manifest::blastfile::check;

/// Fixture A (§5.4): no view, no trait, so the two stores grant nothing and the
/// front controller is reached by no view.
const A: &str = concat!(
    "entrypoint php \"public/index.php\"\n",
    "\n",
    "postgres $DATABASE_URL\n",
    "postgres analytics $ANALYTICS_URL\n",
);

/// Fixture B (§5.4): three views and an orphan trait, so the four view/holder
/// warnings fire and no store is ungranted (the trait grants the default one).
const B: &str = concat!(
    "postgres $DATABASE_URL\n",
    "\n",
    "trait session {\n",
    "    postgres { transactions }\n",
    "}\n",
    "\n",
    "view open \"/a\" {\n",
    "    body raw\n",
    "}\n",
    "\n",
    "view wide \"/b\" {\n",
    "    postgres unrestricted\n",
    "}\n",
    "\n",
    "view outbound \"/c\" {\n",
    "    egress any\n",
    "}\n",
);

/// The 0-based line of the fixture line holding `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let at = text
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is on a line of the fixture"));
    u32::try_from(at).expect("a short fixture")
}

/// The site of the warning whose sentence holds `fragment`.
fn site_of(warnings: &[ManifestWarning], sites: &[Position], fragment: &str) -> Position {
    let index = warnings
        .iter()
        .position(|warning| warning.to_string().contains(fragment))
        .unwrap_or_else(|| panic!("a warning holds {fragment:?}"));
    *sites.get(index).expect("one site per warning")
}

/// Fixture A: three warnings, each on the line its declaration is written on,
/// and the whole-file default is what the two stores do not get.
#[test]
fn a_store_and_an_entrypoint_are_located_where_they_are_written() {
    let checked = check(A, 20_000).expect("fixture A parses and validates");
    assert_eq!(
        checked.warnings,
        vec![
            ManifestWarning::UngrantedProxy {
                proxy: blastit_manifest::ProxyName::from("analytics"),
            },
            ManifestWarning::UngrantedProxy {
                proxy: blastit_manifest::ProxyName::from("db"),
            },
            ManifestWarning::EntrypointNotReached {
                entry: "public/index.php".to_owned(),
            },
        ]
    );
    assert_eq!(checked.sites.len(), checked.warnings.len());

    let entry = site_of(&checked.warnings, &checked.sites, "is reached by no view");
    assert_eq!(entry.line(), line_of(A, "entrypoint php") + 1);
    let db = site_of(
        &checked.warnings,
        &checked.sites,
        "store \"db\" has no grant",
    );
    assert_eq!(db.line(), line_of(A, "postgres $DATABASE_URL") + 1);
    let analytics = site_of(
        &checked.warnings,
        &checked.sites,
        "store \"analytics\" has no grant",
    );
    assert_eq!(
        analytics.line(),
        line_of(A, "postgres analytics $ANALYTICS_URL") + 1
    );
    // The named store's site is its name token; the unnamed one's is its
    // `postgres` keyword, because that is the declaration either way.
    assert_eq!(db.col(), 1);
    assert_eq!(analytics.col(), "postgres ".len() as u32 + 1);
}

/// Fixture B: four warnings, one per view-level decision, each on the word that
/// is the decision — and seven distinct sites across both fixtures.
#[test]
fn four_view_warnings_are_located_at_the_word_that_decides_them() {
    let checked = check(B, 20_000).expect("fixture B parses and validates");
    assert_eq!(
        checked.warnings,
        vec![
            ManifestWarning::UnattachedGroup {
                group: "session".to_owned(),
            },
            ManifestWarning::UnrestrictedRoute {
                route: "wide".to_owned(),
                proxy: blastit_manifest::ProxyName::from("db"),
            },
            ManifestWarning::RawIngress {
                route: "open".to_owned(),
            },
            ManifestWarning::EgressRoute {
                route: "outbound".to_owned(),
            },
        ]
    );

    let trait_site = site_of(&checked.warnings, &checked.sites, "is attached to no view");
    assert_eq!(trait_site.line(), line_of(B, "trait session") + 1);
    let raw = site_of(
        &checked.warnings,
        &checked.sites,
        "passes the request body through unvalidated",
    );
    assert_eq!(raw.line(), line_of(B, "body raw") + 1);
    assert_eq!(raw.col(), "    body ".len() as u32 + 1);
    let unrestricted = site_of(&checked.warnings, &checked.sites, "may run any statement");
    assert_eq!(unrestricted.line(), line_of(B, "postgres unrestricted") + 1);
    assert_eq!(unrestricted.col(), "    postgres ".len() as u32 + 1);
    let egress = site_of(
        &checked.warnings,
        &checked.sites,
        "egress any opens any outbound address",
    );
    assert_eq!(egress.line(), line_of(B, "egress any") + 1);

    // A `sites()` that returned the first line for everything would pass every
    // line assertion above only if the fixture had one warning; the four sites
    // are distinct, which is the other half of the same proof.
    let distinct: std::collections::BTreeSet<(u32, u32)> = checked
        .sites
        .iter()
        .map(|at| (at.line(), at.col()))
        .collect();
    assert_eq!(distinct.len(), checked.sites.len());
}

/// A clean file with no warnings has no sites, and `check("")` is the same
/// shape — the fallback a caller zips over must be an empty list, not a
/// placeholder.
#[test]
fn a_file_with_no_warnings_has_no_sites() {
    let checked = check("", 20_000).expect("an empty file is valid");
    assert!(checked.warnings.is_empty());
    assert!(checked.sites.is_empty());

    let clean = "app \"shop\"\n\npostgres $DATABASE_URL\n\nview v \"/x\" {\n    \
                 postgres { transactions }\n}\n";
    let checked = check(clean, 20_000).expect("the clean fixture is valid");
    assert!(checked.warnings.is_empty(), "{:?}", checked.warnings);
    assert!(checked.sites.is_empty());
}

/// A parse refusal has no manifest and no warnings: the editor reports the
/// errors, and `check` is not the boot's second reader.
#[test]
fn a_broken_file_is_a_refusal_not_a_warning_list() {
    let errors =
        check("view v \"/x\" {\n    body =\n}\n", 20_000).expect_err("a bare `=` is refused");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].position(), Position::at(2, 10));
    assert_eq!(
        errors.errors()[0].message(),
        "`=` is not Blastfile syntax; write the value after the keyword: `body json`"
    );
    assert_eq!(
        errors.errors()[0].to_string(),
        "2:10: `=` is not Blastfile syntax; write the value after the keyword: `body json`"
    );
}
