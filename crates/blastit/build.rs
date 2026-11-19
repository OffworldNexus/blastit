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
//! # The interpreter prefix (`MB.40.8`)
//!
//! The same gate records *where* that interpreter lives, as
//! `BLASTIT_PYTHON_HOME` (`<prefix>`) and `BLASTIT_PYTHON_MM` (`3.14`, asked of
//! the interpreter itself; `libpython3.14.so` is only the cross-compiling
//! fallback, because a prefix can hold more than one). Stage 0 hands stage 1 a
//! `PYTHONHOME` from them, so the
//! embedded interpreter finds `encodings` even under `env -i`, and matches a
//! project `.venv` against the CPython minor this binary really embeds. A build
//! without the `python` feature emits neither, so the standalone archive stays
//! free of the build host's paths (`MB.17`, `ADR-0010` §4).
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
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_PYTHON");
    if let Some((prefix, version)) = python_home() {
        println!("cargo:rustc-env=BLASTIT_PYTHON_HOME={}", prefix.display());
        println!("cargo:rustc-env=BLASTIT_PYTHON_MM={version}");
    }
    rpath();
}

/// The interpreter prefix and CPython minor a `python` build embeds (`MB.40.8`).
///
/// Emitted only when `CARGO_FEATURE_PYTHON` is set: a `--no-default-features`
/// build links no `libpython`, so recording where this host's interpreter lives
/// would only put a build-host path into an artifact meant to run anywhere.
fn python_home() -> Option<(PathBuf, String)> {
    // The feature is the authority: a `PYO3_PYTHON` exported for somebody
    // else's build must not put this host's prefix into a non-Python artifact.
    env::var_os("CARGO_FEATURE_PYTHON")?;
    let python = env::var_os("PYO3_PYTHON")?;
    let interpreter = PathBuf::from(python);
    // <prefix>/bin/python → <prefix>.
    let prefix = interpreter.parent().and_then(Path::parent)?;
    let mut from_file = None;
    for candidate in ["lib", "lib64"] {
        from_file = libpython_mm(&prefix.join(candidate));
        if from_file.is_some() {
            break;
        }
    }
    // The `libpython` has to be there for this to be an embedded interpreter at
    // all; its *name* is only the fallback version.
    from_file.as_ref()?;
    // The interpreter answers for itself, so the answer cannot depend on
    // `read_dir` order when a prefix ships several `libpython*.so` files. A
    // cross-compiled build cannot run the target interpreter at all, which is
    // the one case the file name is authoritative for.
    let version = interpreter_version(&interpreter).or(from_file);
    Some((prefix.to_path_buf(), version?))
}

/// The `major.minor` the interpreter itself reports (`MB.40.8`).
///
/// A build script that guessed from `read_dir` would pick whichever
/// `libpython*.so` the filesystem listed first — `libpython3.14.so` and a stale
/// `libpython3.13.so` in the same prefix is enough to record the wrong ABI. The
/// interpreter knows, so it is asked; `PYO3_PYTHON` is already the one this
/// build links.
fn interpreter_version(interpreter: &Path) -> Option<String> {
    let output = std::process::Command::new(interpreter)
        .arg("-c")
        .arg("import sys; print(f'{sys.version_info[0]}.{sys.version_info[1]}')")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let version = text.trim();
    is_major_minor(version).then(|| version.to_string())
}

/// The `major.minor` of the shared `libpython` in `dir`, if there is one.
///
/// `libpython3.14.so` → `3.14`; `libpython3.so` has no minor and is not a
/// version this script can match a virtualenv against, so it is skipped.
fn libpython_mm(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix("libpython") else {
            continue;
        };
        let Some((version, _)) = rest.split_once(".so") else {
            continue;
        };
        if is_major_minor(version) {
            return Some(version.to_string());
        }
    }
    None
}

/// Whether `text` is exactly `digits "." digits` (`3.14`, never `3` or `3.14t`).
fn is_major_minor(text: &str) -> bool {
    let mut parts = text.split('.');
    let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && minor.bytes().all(|byte| byte.is_ascii_digit())
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
