//! The epic's freshness gate (`MB.67`): the generated PHP table equals a fresh
//! parse of `packaging/languages.tsv`.
//!
//! `build.rs` emits `LIBPHP_CANDIDATES`, `LIBPHP_PROBES`, `PER_MINOR_DIRS`,
//! `NEWEST_PHP`, `SUPPORTED_PHP`, `php_slug` and the `supported_php_minor`
//! helper from `packaging/languages.tsv`; this file parses the same file with the
//! same include-only fragment and asserts the emitted values equal it. This is
//! generator-output verification (the "one declaration" rule: no crate restates
//! a version), not a user-facing surface story, so it cites no
//! `TESTING_PLAN.md` section. A build script that emits a wrong minor turns this
//! red (verified by hand once, see `RESULT.md`).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

include!("../../../packaging/languages.rs.inc");

use blastit_view_php::discovery::{
    LIBPHP_CANDIDATES, LIBPHP_PROBES, NEWEST_PHP, PER_MINOR_DIRS, PhpVersion, SUPPORTED_PHP,
};

const TSV: &str = include_str!("../../../packaging/languages.tsv");

/// The declared `php` rows, newest first, as the TSV states them.
fn php() -> Vec<Row> {
    let rows = parse(TSV).expect("packaging/languages.tsv parses");
    rows_of(&rows, "php").into_iter().cloned().collect()
}

#[test]
fn the_candidates_are_the_tsv_newest_first() {
    let mut expected: Vec<String> = Vec::new();
    for row in php() {
        let version = &row.version;
        let digits = row.digits();
        expected.push(format!("libphp{version}.so"));
        expected.push(format!("libphp-{version}.so"));
        expected.push(format!("libphp{digits}.so"));
    }
    expected.push("libphp.so".to_string());
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(LIBPHP_CANDIDATES.as_slice(), expected.as_slice());
}

#[test]
fn the_probe_order_confines_each_minor_to_its_slug() {
    let mut expected: Vec<(String, Option<String>)> = Vec::new();
    for row in php() {
        let version = &row.version;
        let digits = row.digits();
        for name in [
            format!("libphp{version}.so"),
            format!("libphp-{version}.so"),
            format!("libphp{digits}.so"),
        ] {
            expected.push((name, None));
        }
        expected.push(("libphp.so".to_string(), Some(row.slug().to_string())));
    }
    expected.push(("libphp.so".to_string(), None));
    let actual: Vec<(String, Option<String>)> = LIBPHP_PROBES
        .iter()
        .map(|probe| {
            (
                probe.name().to_string(),
                probe.per_minor_dir().map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn the_per_minor_directories_are_the_tsv_slugs() {
    let expected: Vec<String> = php().iter().map(|row| row.slug().to_string()).collect();
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(PER_MINOR_DIRS.as_slice(), expected.as_slice());
}

#[test]
fn the_supported_sentence_names_the_declared_minors() {
    let versions: Vec<String> = php().iter().map(|row| row.version.clone()).collect();
    let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
    assert_eq!(SUPPORTED_PHP, sentence("PHP", &ascending(&versions)));
}

#[test]
fn the_newest_minor_is_the_first_row() {
    let rows = php();
    let newest = PhpVersion::from_dotted(&rows[0].version).expect("a declared major.minor");
    assert_eq!(NEWEST_PHP, newest);
}

#[test]
fn supported_follows_the_table() {
    for row in php() {
        let version = PhpVersion::from_dotted(&row.version).expect("a declared major.minor");
        assert!(
            version.supported(),
            "every declared PHP row is supported: {version:?}"
        );
    }
    assert!(
        !PhpVersion::new(7, 4).supported(),
        "a minor no row declares is refused"
    );
}
