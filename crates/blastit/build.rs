//! The one build-time switch left: the coverage runtime's flush.
//!
//! `rustc -C instrument-coverage` emits **no** `cfg(coverage)` — that spelling is
//! a nightly compiler-internal switch, and `rustc --print cfg -C
//! instrument-coverage` lists no `coverage` at all. So the flush in
//! `src/coverage.rs` cannot be gated on it: an instrumented build would compile
//! the empty function and lose every profile to `_exit`. This script owns the
//! gate instead: the `coverage` cargo feature, or `BLASTIT_COVERAGE=1` for a
//! coverage target that passes `RUSTFLAGS` rather than a feature, emits
//! `cargo:rustc-cfg=coverage`. The decision is also exported as
//! `BLASTIT_COVERAGE_GATE` so a unit test can prove the cfg that selects the
//! extern block is exactly this decision.
//!
//! # No interpreter prefix, no `rpath` (design §1.1)
//!
//! This crate used to record the linked interpreter's prefix and to emit an
//! `rpath` into it, because the binary linked `libpython`. It links neither now:
//! stage 1 `dlopen`s the host's `libpython` and the per-minor shim from pinned
//! descriptors, so a build-host path in the artifact would be exactly the
//! `MB.17` defect (a binary that only runs in the checkout that produced it).
//! `PYO3_PYTHON`, `BLASTIT_PYTHON_HOME` and `BLASTIT_PYTHON_MM` are therefore
//! **not** read here at all.

use std::env;

fn main() {
    // The crate uses this cfg whether or not it is set, so declare it always.
    println!("cargo:rustc-check-cfg=cfg(coverage)");
    println!("cargo:rerun-if-env-changed=BLASTIT_COVERAGE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_COVERAGE");
    let coverage = coverage_requested();
    println!(
        "cargo:rustc-env=BLASTIT_COVERAGE_GATE={}",
        if coverage { "on" } else { "off" }
    );
    if coverage {
        println!("cargo:rustc-cfg=coverage");
    }
}

/// Whether this build wants the coverage runtime's flush compiled in.
///
/// Two spellings, both deliberate: the `coverage` feature for
/// `cargo … --features coverage`, and `BLASTIT_COVERAGE` for a target that
/// already passes `RUSTFLAGS=-C instrument-coverage` and cannot add a feature to
/// every crate in the workspace.
fn coverage_requested() -> bool {
    if env::var_os("CARGO_FEATURE_COVERAGE").is_some() {
        return true;
    }
    env::var_os("BLASTIT_COVERAGE").is_some_and(|value| value == "1" || value == "true")
}
