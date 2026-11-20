//! `TESTING_PLAN.md` §2/§4 — the two discovery sentences no unprivileged boot can
//! print, asserted where they are produced.
//!
//! Every other interpreter refusal is driven end-to-end by
//! `crates/blastit/tests/e2e_dev.rs` against the real binary. `Production`
//! discovery is the exception: `serve` refuses an unprivileged caller **before**
//! `Config::from_env` (`MB.1.6`), so a relative `BLASTIT_PYTHON` and an absolute
//! path that is not an executable file have no reachable boot. They are one
//! sentence each, written in `discovery/error.rs`, and this file asserts them
//! against the discovery the deployment boot calls — no `Mock*`, no skip.
//!
//! The accepted twin is here too: an absolute executable interpreter is accepted
//! by the same call, so the refusal is about the value and not about discovery
//! being unable to resolve anything in a test fixture.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use blastit_py_sapi::discovery::{DiscoveryEnv, ProductionEnv, discover};

/// A directory with a name of its own, removed when dropped.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let base = Path::new("/tmp/opencode");
        let base = if std::fs::create_dir_all(base).is_ok() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let dir = base.join(format!("blastit-py-sentences-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory");
        Self { dir }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a fixture parent directory");
        }
        std::fs::write(&path, bytes).expect("a fixture file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// §2: a deployment reads only absolute paths, and says so with the fix.
#[test]
fn a_relative_blastit_python_is_refused_with_the_absolute_fix() {
    let scratch = Scratch::new("relative");
    let error = discover(
        &DiscoveryEnv::production(ProductionEnv {
            blastit_python: Some(PathBuf::from("bin/python3")),
            wheel_python: None,
            exe_dir: scratch.dir.clone(),
        }),
        "serve",
        &scratch.dir,
    )
    .expect_err("a relative BLASTIT_PYTHON is refused in a deployment");
    assert_eq!(
        error.to_string(),
        format!(
            "blastit serve: BLASTIT_PYTHON (bin/python3) is not an absolute path; a deployment \
             reads only absolute paths — use an absolute path such as {}/bin/python3",
            scratch.dir.join(".venv").display()
        ),
        "the sentence names the value and the absolute fix"
    );
}

/// §2: an absolute path that is not an executable file is refused with the same
/// fix, and an executable one is accepted (so the refusal is about the value).
#[test]
fn an_absolute_path_that_is_no_executable_is_refused_and_an_executable_one_is_accepted() {
    let scratch = Scratch::new("absolute");
    let missing = scratch.dir.join("missing").join("python3");
    let error = discover(
        &DiscoveryEnv::production(ProductionEnv {
            blastit_python: Some(missing.clone()),
            wheel_python: None,
            exe_dir: scratch.dir.clone(),
        }),
        "serve",
        &scratch.dir,
    )
    .expect_err("a missing absolute BLASTIT_PYTHON is refused");
    assert_eq!(
        error.to_string(),
        format!(
            "blastit serve: BLASTIT_PYTHON ({}) is not an executable file; unset BLASTIT_PYTHON, \
             or point it at {}/bin/python3",
            missing.display(),
            scratch.dir.join(".venv").display()
        ),
        "the sentence names the value and the fix"
    );

    // The accepted twin: an absolute, executable, regular file passes the
    // interpreter step, so the refusal is about the *value* and not about a
    // fixture discovery cannot resolve. The next gate to refuse (a shared
    // `libpython`, or the shim beside the image) is a different sentence.
    let interpreter = scratch.write("py/bin/python3.14", b"an executable regular file");
    std::fs::set_permissions(
        &interpreter,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("an executable fixture");
    let error = discover(
        &DiscoveryEnv::production(ProductionEnv {
            blastit_python: Some(interpreter.clone()),
            wheel_python: None,
            exe_dir: scratch.dir.clone(),
        }),
        "serve",
        &scratch.dir,
    )
    .expect_err("a bare prefix cannot serve a view");
    let sentence = error.to_string();
    assert!(
        !sentence.contains("BLASTIT_PYTHON (") && !sentence.contains("is not an executable file"),
        "the interpreter was accepted and discovery moved on: {sentence}"
    );
}
