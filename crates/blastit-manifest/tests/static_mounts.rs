//! §9.4's `immutable` caps, per **mount**: more than 32 patterns in one mount,
//! or one pattern longer than 256 bytes, is refused, and the sentence names the
//! mount instead of a route.
//!
//! `critical_static_manifest.rs` pins the same two caps through the retired
//! reader's `[route.static]` shape, where the subject was a route. That subject
//! is gone: a `Blastfile` writes `immutable` inside `static "<prefix>" { … }`
//! and nothing else declares it (§9.1), so a cap has to be provable against the
//! declaration a user writes. The accepted twin is in each test, so a validator
//! that refused every list would fail here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::Manifest;

/// The first refusal `source` produces, rendered as one line (§18).
///
/// [`Manifest::parse_blastfile`] is the boot and `check` entry point, so the
/// caps are asserted where a user meets them: parse, lower **and** validate.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the file is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// The manifest `source` validates to, with the uid base every fixture uses.
fn accepted(source: &str) -> Manifest {
    let (manifest, _warnings) =
        Manifest::parse_blastfile(source, 20_000).expect("the file is accepted");
    manifest
}

/// A `static` block holding `patterns` as one `immutable` list.
fn mount(prefix: &str, patterns: &[String]) -> String {
    let list = patterns
        .iter()
        .map(|pattern| format!("{pattern:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("static {prefix:?} {{\n  root \"public\"\n  immutable {list}\n}}\n")
}

/// `count` distinct, valid globs: `/p0`, `/p1`, …
fn globs(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("/p{index}")).collect()
}

/// A `/`-anchored pattern of exactly `bytes` bytes: `/` then `a`s.
fn pattern_of(bytes: usize) -> String {
    format!("/{}", "a".repeat(bytes - 1))
}

/// 32 patterns are the documented ceiling: 32 parse, and the 33rd is refused.
#[test]
fn a_mount_may_declare_at_most_32_immutable_patterns() {
    // The accepted twin: exactly the cap.
    let allowed = globs(32);
    let manifest = accepted(&mount("/assets", &allowed));
    let declared = &manifest.mounts()[0];
    assert_eq!(declared.prefix, "/assets");
    assert_eq!(declared.policy.immutable, allowed, "every pattern survives");

    // One past the cap: the sentence names the mount and the pattern to delete.
    let mut too_many = globs(33);
    too_many.push("/p99".to_string());
    let rendered = refusal(&mount("/assets", &too_many));
    assert_eq!(
        rendered, "mount \"/assets\" (index 0): static immutable \"/p32\": at most 32 patterns",
        "the first pattern past the limit is the one an operator deletes"
    );
    assert!(!rendered.contains('\n'), "{rendered:?}");
}

/// A pattern longer than a path component cannot name a file: 256 bytes is the
/// ceiling and 257 is refused.
#[test]
fn a_mount_may_declare_patterns_of_at_most_256_bytes() {
    // The accepted twin: exactly the ceiling.
    let longest = pattern_of(256);
    let manifest = accepted(&mount("/assets", std::slice::from_ref(&longest)));
    assert_eq!(manifest.mounts()[0].policy.immutable, [longest]);

    // One byte over: refused, and the mount is the subject.
    let rendered = refusal(&mount("/assets", &[pattern_of(257)]));
    assert!(
        rendered.starts_with("mount \"/assets\" (index 0): static immutable "),
        "the mount is named: {rendered}"
    );
    assert!(
        rendered.contains("at most 256 bytes"),
        "the rule is named: {rendered}"
    );
    assert!(!rendered.contains('\n'), "one line (§18): {rendered:?}");
}

/// The index is the mount's own, not a route's: with two mounts, the second
/// one's refusal names `index 1` and its prefix.
#[test]
fn the_sentence_names_the_mount_that_broke_the_cap() {
    let source = format!(
        "{}{}",
        mount("/assets", &globs(2)),
        mount("/assets/v2", &globs(33))
    );
    let rendered = refusal(&source);
    assert_eq!(
        rendered,
        "mount \"/assets/v2\" (index 1): static immutable \"/p32\": at most 32 patterns"
    );
}
