//! The epic's freshness gate (`MB.67`): the generated CPython table equals a
//! fresh parse of `packaging/languages.tsv`.
//!
//! `build.rs` emits `SUPPORTED_PY_MINORS` and `supports_py_minor` from
//! `packaging/languages.tsv`; this file parses the same file with the same
//! include-only fragment and asserts the emitted list equals it. This is
//! generator-output verification (the "one declaration" rule: no crate restates
//! a version), not a user-facing surface story, so it cites no
//! `TESTING_PLAN.md` section. A build script that emits a wrong minor turns this
//! red (verified by hand once, see `RESULT.md`).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

include!("../../../packaging/languages.rs.inc");

use blastit_py_shim::{SUPPORTED_PY_MINORS, supports_py_minor};

const TSV: &str = include_str!("../../../packaging/languages.tsv");

/// `"3.13"` as the `py_minor` encoding (`313`).
fn encoded(version: &str) -> u32 {
    let (major, minor) = version.split_once('.').expect("a major.minor row");
    let major: u32 = major.parse().expect("a numeric major");
    let minor: u32 = minor.parse().expect("a numeric minor");
    major * 100 + minor
}

#[test]
fn the_supported_minors_are_the_tsv_newest_first() {
    let rows = parse(TSV).expect("packaging/languages.tsv parses");
    let expected: Vec<u32> = rows_of(&rows, "python")
        .into_iter()
        .map(|row| encoded(&row.version))
        .collect();
    assert_eq!(SUPPORTED_PY_MINORS.to_vec(), expected);
}

#[test]
fn supports_py_minor_is_exactly_the_declared_set() {
    for minor in SUPPORTED_PY_MINORS {
        assert!(supports_py_minor(minor), "{minor} is a declared row");
    }
    // 3 is the oldest CPython a WSGI view could ever name; it is not declared.
    assert!(!supports_py_minor(3));
    assert!(!supports_py_minor(0));
}
