//! The epic's freshness gate (`MB.67`): the generated PHP table equals a fresh
//! parse of `packaging/languages.tsv`.
//!
//! `build.rs` emits `Minor`, `SUPPORTED_MINORS`, `NEWEST_MINOR`, `CONFIG_NAMES`
//! and `SUPPORTED_MINORS_SENTENCE` from `packaging/languages.tsv`; this file
//! parses the same file with the same include-only fragment and asserts the
//! emitted values equal it. This is generator-output verification (the "one
//! declaration" rule: no crate restates a version), not a user-facing surface
//! story, so it cites no `TESTING_PLAN.md` section. A build script that emits a
//! wrong minor turns this red (verified by hand once, see `RESULT.md`).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

include!("../../../packaging/languages.rs.inc");

use blastit_php_sapi::{
    CONFIG_NAMES, Minor, NEWEST_MINOR, SUPPORTED_MINORS, supported_minors_sentence,
};

const TSV: &str = include_str!("../../../packaging/languages.tsv");

/// The declared `php` rows, newest first, as the TSV states them.
fn php() -> Vec<Row> {
    let rows = parse(TSV).expect("packaging/languages.tsv parses");
    rows_of(&rows, "php").into_iter().cloned().collect()
}

/// The `Minor` the TSV's `version` states.
fn minor_of(version: &str) -> Minor {
    Minor::parse(&format!("{version}.0")).expect("a declared major.minor parses")
}

#[test]
fn the_minors_are_the_tsv_newest_first() {
    let expected: Vec<Minor> = php().iter().map(|row| minor_of(&row.version)).collect();
    assert_eq!(SUPPORTED_MINORS.to_vec(), expected);
    assert_eq!(NEWEST_MINOR, expected[0]);
}

#[test]
fn the_config_names_are_the_tsv_newest_first() {
    let mut expected: Vec<String> = vec!["php-config".to_string()];
    expected.extend(php().iter().map(|row| format!("php-config{}", row.version)));
    let actual: Vec<String> = CONFIG_NAMES.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn each_minor_carries_its_own_code_digits_and_names() {
    for row in php() {
        let minor = minor_of(&row.version);
        let (major, number) = row.version.split_once('.').expect("a major.minor row");
        let major: u32 = major.parse().expect("a numeric major");
        let number: u32 = number.parse().expect("a numeric minor");
        let specific = format!("php-config{}", row.version);
        assert_eq!(minor.code(), major * 100 + number);
        assert_eq!(minor.digits(), row.digits());
        assert_eq!(minor.dotted(), row.version);
        assert_eq!(minor.slug(), row.slug(), "the shim infix is the TSV slug");
        assert_eq!(
            minor.shim_file(),
            format!(
                "libblastit_{}-{}-{}.so",
                row.slug(),
                std::env::consts::ARCH,
                if cfg!(target_env = "musl") {
                    "musl"
                } else {
                    "gnu"
                }
            )
        );
        assert_eq!(minor.config_names(), [specific.as_str(), "php-config"]);
    }
}

#[test]
fn the_supported_sentence_names_the_declared_minors() {
    let versions: Vec<String> = php().iter().map(|row| row.version.clone()).collect();
    let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
    assert_eq!(
        supported_minors_sentence(),
        sentence("PHP", &ascending(&versions))
    );
}

#[test]
fn an_undeclared_minor_is_refused() {
    assert_eq!(Minor::parse("7.4.0"), None);
    assert_eq!(Minor::parse("8"), None);
}
