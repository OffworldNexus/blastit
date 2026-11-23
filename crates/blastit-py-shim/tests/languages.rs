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

/// The epic's split (`MB.75`): one binary accepts a nightly-only minor, and a
/// stable package set never names it.
#[test]
fn a_nightly_only_minor_is_selectable_but_never_in_the_stable_package_set() {
    let rows = parse(TSV).expect("packaging/languages.tsv parses");
    let python = rows_of(&rows, "python");

    // The runtime table is the whole file, newest first: the same shipped binary
    // serves a nightly install, so 3.16 is selectable even though only the
    // nightly channel packages it.
    let declared: Vec<u32> = python.iter().map(|row| encoded(&row.version)).collect();
    assert_eq!(SUPPORTED_PY_MINORS.to_vec(), declared);
    assert!(
        supports_py_minor(316),
        "the runtime accepts the nightly-only minor"
    );

    // Exactly one CPython row is nightly-only, and it is 3.16.
    let nightly_only: Vec<&Row> = python
        .iter()
        .copied()
        .filter(|row| !in_channel(row, "stable"))
        .collect();
    assert_eq!(nightly_only.len(), 1, "one nightly-only CPython row");
    assert_eq!(nightly_only[0].version, "3.16");
    assert!(in_channel(nightly_only[0], "nightly"));

    // The stable set is the *packaging* selection over the same declaration.
    let stable: Vec<&str> = python
        .iter()
        .filter(|row| in_channel(row, "stable"))
        .map(|row| row.version.as_str())
        .collect();
    assert_eq!(stable, ["3.15", "3.14", "3.13"], "stable ships 3.13-3.15");

    // And the real derivation agrees: `package-set.sh --channel stable` names no
    // 3.16 package.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root is two directories above this crate")
        .to_path_buf();
    let output = std::process::Command::new("bash")
        .arg(root.join("packaging/package-set.sh"))
        .args(["--channel", "stable", "--names"])
        .output()
        .expect("bash runs packaging/package-set.sh");
    assert!(
        output.status.success(),
        "package-set.sh --channel stable failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names = String::from_utf8(output.stdout).expect("package names are UTF-8");
    assert!(
        names.lines().any(|name| name == "blastit-python-3.15"),
        "a stable set ships 3.15: {names}"
    );
    assert!(
        !names.lines().any(|name| name == "blastit-python-3.16"),
        "a stable set never names a nightly-only package: {names}"
    );
}
