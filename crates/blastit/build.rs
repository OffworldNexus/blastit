//! Makes the binary able to find `libpython` with an empty environment.
//!
//! Every blastit process needs the dynamic loader to find `libpython`: one
//! binary plays every role, and the view role embeds CPython. dbproxy is
//! `execve`'d with an **empty** environment (§4), so the loader has no
//! `LD_LIBRARY_PATH` to fall back on there. When the interpreter is a bare
//! prefix outside the loader's default search path — a CI tool cache, a
//! `pyenv` build — dbproxy dies before it can report ready, and init fails the
//! whole boot.
//!
//! The fix belongs in the binary: an `rpath` pointing at the interpreter's
//! library directory. It is emitted only when `PYO3_PYTHON` names an
//! interpreter that actually ships a shared `libpython` under `<prefix>/lib` or
//! `<prefix>/lib64`, which is what a non-system distribution looks like, and a
//! path that does not exist at runtime is harmless: the loader then falls back
//! to its own search path.
//!
//! **The feature is the authority, not the variable.** `PYO3_PYTHON` is set per
//! *build*, not per crate: a workspace `cargo build`, a `make` target that
//! exports `RUST_ENV`, or a shell where the developer exports it for pyo3's
//! tests all hand this crate a path to an interpreter it does not link. The
//! standalone build (`--no-default-features --features php`) links no
//! `libpython` at all, so an `rpath` there is a build host's uv cache embedded
//! in a binary meant to run anywhere (`MB.17`, `ADR-0010` §4). `CARGO_FEATURE_PYTHON`
//! is the only signal that answers "does this artifact link `libpython`?", so it
//! gates the link arg; the variable merely says *where* a Python-enabled build
//! should look.
//!
//! `pyo3-build-config`'s `add_libpython_rpath_link_args()` does the same thing
//! more thoroughly, but taking it as a build dependency would touch
//! `Cargo.lock`, which every other crate in flight also edits.
//!
//! # The coverage gate
//!
//! `rustc -C instrument-coverage` emits **no** `cfg(coverage)` — that spelling is
//! a nightly compiler-internal switch, and
//! `rustc --print cfg -C instrument-coverage` lists no `coverage` at all. So the
//! flush in `src/coverage.rs` cannot be gated on it: an instrumented build would
//! compile the empty function and lose every profile to `_exit`. This script owns
//! the gate instead: the `coverage` cargo feature, or `BLASTIT_COVERAGE=1` for a
//! coverage target that passes `RUSTFLAGS` rather than a feature, emits
//! `cargo:rustc-cfg=coverage`. The decision is also exported as
//! `BLASTIT_COVERAGE_GATE` so a unit test can prove the cfg that selects the
//! extern block is exactly this decision.

use std::env;
use std::path::{Path, PathBuf};

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
    rpath();
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

/// The `libpython` search path a Python-enabled build needs (§4, `MB.17`).
fn rpath() {
    // No `python` capability, no `libpython`: the standalone artifact must stay
    // free of a search path into whoever built it. Cargo reruns this script when
    // the feature set changes, so turning `python` on still emits the rpath.
    if env::var_os("CARGO_FEATURE_PYTHON").is_none() {
        return;
    }
    let Some(python) = env::var_os("PYO3_PYTHON") else {
        return;
    };
    let interpreter = PathBuf::from(python);
    // <prefix>/bin/python → <prefix>.
    let Some(prefix) = interpreter.parent().and_then(Path::parent) else {
        return;
    };
    for candidate in ["lib", "lib64"] {
        let dir = prefix.join(candidate);
        if holds_libpython(&dir) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
    }
}

/// Whether `dir` holds a shared `libpython`.
fn holds_libpython(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("libpython") && name.contains(".so")
    })
}
