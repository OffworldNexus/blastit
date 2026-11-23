//! The epic's freshness gate (`MB.67`): the generated CPython table equals a
//! fresh parse of `packaging/languages.tsv`.
//!
//! `build.rs` emits `SUPPORTED_MINORS`, `DEFAULT_MINOR`, `PY_MINOR_<digits>`,
//! `py_slug` and `SUPPORTED_MINORS_SENTENCE` from `packaging/languages.tsv`;
//! this file parses the same file with the same include-only fragment and asserts
//! the emitted values equal it. This is generator-output verification (the "one
//! declaration" rule: no crate restates a version), not a user-facing surface
//! story, so it cites no `TESTING_PLAN.md` section. A build script that emits a
//! wrong minor turns this red (verified by hand once, see `RESULT.md`).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

include!("../../../packaging/languages.rs.inc");

use blastit_py_sapi::{
    DEFAULT_MINOR, PY_MINOR_313, PY_MINOR_314, SUPPORTED_MINORS, default_minor, encode_py_minor,
    shim_file, supported_minors_sentence,
};

const TSV: &str = include_str!("../../../packaging/languages.tsv");

/// The declared `python` rows, newest first, as the TSV states them.
fn python() -> Vec<Row> {
    let rows = parse(TSV).expect("packaging/languages.tsv parses");
    rows_of(&rows, "python").into_iter().cloned().collect()
}

/// The minor of `"3.13"` (`13`).
fn minor_of(version: &str) -> u32 {
    version
        .split_once('.')
        .expect("a major.minor row")
        .1
        .parse()
        .expect("a numeric minor")
}

#[test]
fn the_supported_minors_are_the_tsv_newest_first() {
    let expected: Vec<u32> = python().iter().map(|row| minor_of(&row.version)).collect();
    assert_eq!(SUPPORTED_MINORS.to_vec(), expected);
}

#[test]
fn the_default_minor_is_the_newest_row() {
    let rows = python();
    assert_eq!(DEFAULT_MINOR, rows[0].version.as_str());
    assert_eq!(default_minor(), DEFAULT_MINOR);
}

#[test]
fn the_py_minor_constants_are_the_rows() {
    // One generated constant per declared row. A withdrawn row removes its line
    // here together with the constant (see `MB.67`); a wrong value on either
    // side turns this red.
    let published = [("3.13", PY_MINOR_313), ("3.14", PY_MINOR_314)];
    let rows = python();
    assert_eq!(published.len(), rows.len(), "one constant per declared row");
    for (version, value) in published {
        assert!(
            rows.iter().any(|row| row.version == version),
            "{version} is a declared row"
        );
        let (major, number) = version.split_once('.').expect("a major.minor row");
        let major: u32 = major.parse().expect("a numeric major");
        let number: u32 = number.parse().expect("a numeric minor");
        assert_eq!(
            value,
            encode_py_minor(major, number),
            "PY_MINOR_{major}{number} is the row's encoding"
        );
    }
}

#[test]
fn the_shim_name_uses_the_declared_slug() {
    for row in python() {
        let name = shim_file(minor_of(&row.version));
        assert!(
            name.starts_with(&format!("libblastit_{}-", row.slug())),
            "the shim infix is the TSV slug: {name}"
        );
    }
}

#[test]
fn the_supported_sentence_names_the_declared_minors() {
    let versions: Vec<String> = python().iter().map(|row| row.version.clone()).collect();
    let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
    assert_eq!(
        supported_minors_sentence(),
        sentence("CPython", &ascending(&versions))
    );
}
