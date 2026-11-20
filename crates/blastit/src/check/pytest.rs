//! The suite: the interpreter it runs on, the preflights, and the spawn.
//!
//! Nothing here hands the child a credential: the environment is
//! [`blastit_env::Prepared`]'s secret-free rewrite plus this command's three
//! descriptor/path variables, and a probe child is hermetic for the same reason
//! the suite is.

use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::CheckOptions;

use super::session::stdio_stderr;
use super::{CTL_FD, DJANGO_SETUP, HTTP_OBS};

/// Refuses to start a suite whose interpreter cannot import pytest (`R4`).
///
/// A missing pytest is exit `2` ("Blastit could not start"), not `3`: nothing
/// ran, so there is no suite failure to report.
pub fn require_pytest(python: &Path, prepared: &blastit_env::Prepared) -> Result<(), String> {
    if imports(python, prepared, "pytest")? {
        return Ok(());
    }
    Err(format!(
        "{}: pytest is not installed in the interpreter blastit runs on; \
         install pytest: uv add --dev pytest",
        python.display()
    ))
}

/// Whether `module` imports in the suite's own secret-free environment.
///
/// The child is hermetic (`env_clear` plus [`Prepared::app_env`]) for the same
/// reason the suite is: a probe must not see a credential either. ImportError
/// is exit `1`, which is "absent"; anything else means the interpreter itself
/// could not run and is an error of its own.
pub fn imports(
    python: &Path,
    prepared: &blastit_env::Prepared,
    module: &str,
) -> Result<bool, String> {
    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg(format!("import {module}"))
        .env_clear()
        .envs(prepared.app_env())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command
        .status()
        .map_err(|error| format!("{}: {error}", python.display()))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(code) => Err(format!(
            "{}: the check for {module} exited {code}, so the interpreter cannot run",
            python.display()
        )),
        None => Err(format!(
            "{}: the check for {module} was killed by a signal",
            python.display()
        )),
    }
}

/// Everything one `pytest` spawn reads (`MB.40.8`).
///
/// A struct rather than eight positional arguments: the output-stream flag, the
/// optional control descriptor and the optional paths read as one document, and
/// a caller cannot swap two of them by accident.
pub struct PytestSpawn<'a> {
    pub options: &'a CheckOptions,
    pub prepared: &'a blastit_env::Prepared,
    pub python: &'a Path,
    pub http_path: &'a Path,
    pub ctl_fd: Option<RawFd>,
    pub json: bool,
    pub django: Option<&'a Path>,
}

/// Runs the suite, mapping a spawn failure to exit `2` (`R4`).
pub fn run_pytest(spawn: PytestSpawn<'_>) -> Result<i32, String> {
    let PytestSpawn {
        options,
        prepared,
        python,
        http_path,
        ctl_fd,
        json,
        django,
    } = spawn;
    let mut command = Command::new(python);
    command.arg("-m").arg("pytest").arg("-q");
    // R30: `-p no:blastit` disables the installed `0.1.0` entry point, and the
    // gate is loaded by module name instead.
    command.arg("-p").arg("no:blastit");
    command.arg("-p").arg("blastit._check_pytest");
    command.args(&options.pytest_args);
    command.env_clear();
    for (name, value) in prepared.app_env() {
        command.env(name, value);
    }
    // `MB.40.8`: the bundle `make install` stages carries the gate itself at
    // `<exe_dir>/../python`, so `-p blastit._check_pytest` imports even when
    // `blastit` was never installed into the application's own venv. The caller's
    // explicit `PYTHONPATH` stays first and the bundle's directory follows it
    // (`MB.53`): Python keeps `PYTHONPATH` in order and searches it before the
    // interpreter's own `site-packages`, so the caller's entry wins a name
    // collision — `blastit check`'s contract hands the child a project
    // `stub/pytest` that must shadow the real one — while the bundle still
    // precedes the child interpreter's stdlib and `site-packages`, which is what
    // makes the gate importable under a wheel/`.deb`/`uv tool` install that has
    // no venv of its own.
    if let Some(directory) = bundled_python() {
        let existing = prepared
            .app_env()
            .get("PYTHONPATH")
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        command.env("PYTHONPATH", pythonpath_with_bundle(existing, &directory));
    }
    command.env(HTTP_OBS, http_path);
    // A store-less app runs no dbproxy, so there is no channel to push a target
    // into and no `BLASTIT_CHECK_CTL_FD` to name: the gate records the request
    // and pushes nothing (`_check_pytest._send`).
    if let Some(fd) = ctl_fd {
        command.env(CTL_FD, fd.to_string());
    }
    if let Some(path) = django {
        // R30/ADR-0004: the only place the real proxy inputs exist in this
        // process tree after the rewrite, `0600`, and gone as soon as the gate
        // has read it.
        command.env(DJANGO_SETUP, path);
    }
    crate::coverage::inherit(&mut command);
    command.stdin(Stdio::null());
    if json {
        // R5: the child's fd 1 *is* our stderr, so no `print()` can reach the
        // document's stream.
        command.stdout(stdio_stderr()?);
    } else {
        command.stdout(Stdio::inherit());
    }
    command.stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "{}: {error} (install pytest in the environment blastit runs on: \
             `uv add --dev pytest`)",
            python.display()
        )
    })?;
    let status = child
        .wait()
        .map_err(|error| format!("waiting for pytest: {error}"))?;
    Ok(status.code().unwrap_or(-1))
}

/// The interpreter `pytest` runs on (`MB.40.8`, `MB.42.2.2.2`).
///
/// A wheel's console script hands the command line to the universal binary, so
/// there is no interpreter in this process to inherit: it records the one it ran
/// on in `BLASTIT_WHEEL_PYTHON` (dropped from the application's environment by
/// `blastit-env` like every other `BLASTIT_*` variable) and that is the project's
/// own environment. Otherwise the search is what a developer's shell answers:
/// `BLASTIT_PYTHON`, the activated `$VIRTUAL_ENV`, the project's `.venv`,
/// `BLASTIT_WHEEL_PYTHON`, `PATH` — so an activated venv always wins.
pub fn python_program(manifest_dir: &Path) -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("BLASTIT_PYTHON") {
        return Ok(PathBuf::from(explicit));
    }
    if let Some(found) = std::env::var_os("VIRTUAL_ENV")
        .map(PathBuf::from)
        .filter(|directory| !directory.as_os_str().is_empty())
        .and_then(|directory| venv_python(&directory))
    {
        return Ok(found);
    }
    if let Some(found) = venv_python(&manifest_dir.join(".venv")) {
        return Ok(found);
    }
    if let Some(found) = std::env::var_os("BLASTIT_WHEEL_PYTHON")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
    {
        return Ok(found);
    }
    if let Some(found) = path_python() {
        return Ok(found);
    }
    Err(
        "no interpreter to run pytest on: set BLASTIT_PYTHON to the `python3` \
         that has pytest installed"
            .to_string(),
    )
}

/// `<venv>/bin/python3` (or `bin/python`), when it exists.
pub(super) fn venv_python(venv: &Path) -> Option<PathBuf> {
    for name in ["python3", "python"] {
        let candidate = venv.join("bin").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The first `python3` (or `python`) on `PATH`.
fn path_python() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in path.to_string_lossy().split(':') {
        if directory.is_empty() {
            continue;
        }
        for name in ["python3", "python"] {
            let candidate = Path::new(directory).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `<exe_dir>/../python`, or the wheel root two levels up, when it holds the
/// staged `blastit` gate (`MB.40.8`, `MB.42.2.2.2`, `MB.42.11`, `MB.42.3.3`).
///
/// Two layouts, one marker pair. `make install` puts the binary at
/// `<prefix>/share/blastit/lib/blastit` and the package at
/// `<prefix>/share/blastit/python/blastit`, so `<exe_dir>/../python` is the
/// package root. The published `py3-none-*` wheel (`MB.42.3.3`) puts the binary
/// at `<site-packages>/blastit/_bin/blastit` with the package's own modules
/// beside it, so `<exe_dir>/../..` is `<site-packages>` — the root that holds
/// `blastit/`. (`MB.42.11`'s `../../..` resolved one level *above*
/// `site-packages`, which can never hold the gate: it was a dead fallback, and
/// `MB.42.3.3` replaced it with the root the repacked wheel actually has.)
///
/// Both markers have to be there — `_check_pytest.py` is the gate `-p` imports
/// and `__main__.py` is the package's entry point, and every bundle ships both —
/// so a `cargo` build tree whose `target/<profile>` sits beside the repository's
/// own `python/` can never be mistaken for one. `_bin` is deliberately not a
/// marker: it is a *directory name*, and the wheel's root is two levels above it.
fn bundled_python() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    bundle_root(exe.parent()?)
}

/// The `PYTHONPATH` a `pytest` child receives: the caller's entries first, the
/// bundle's package root last (`MB.53`).
///
/// Python searches `PYTHONPATH` in order and ahead of the interpreter's own
/// stdlib and `site-packages`, so the caller's explicit directory keeps the
/// precedence it was given — a project `stub/pytest` shadows the real package —
/// and [`bundled_python`]'s directory only ever shadows what the child
/// interpreter found by itself. Pure and argument-driven, so the order is
/// testable without reading this process's environment.
pub fn pythonpath_with_bundle(existing: &str, directory: &str) -> String {
    if existing.is_empty() {
        directory.to_string()
    } else {
        format!("{existing}:{directory}")
    }
}

/// The `blastit` package root beside `exe_dir` that carries the staged gate.
pub(super) fn bundle_root(exe_dir: &Path) -> Option<String> {
    let roots = [exe_dir.join("../python"), exe_dir.join("../..")];
    roots
        .into_iter()
        .find(|root| {
            let package = root.join("blastit");
            package.join("_check_pytest.py").is_file() && package.join("__main__.py").is_file()
        })
        .map(|root| root.to_string_lossy().into_owned())
}
