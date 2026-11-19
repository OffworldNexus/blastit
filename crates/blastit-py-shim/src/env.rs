#![allow(unsafe_code)]

//! `init_paths`: the shim's half of stage 0's discovery (design §1.2).
//!
//! Stage 0 chose the interpreter and the paths without executing Python (§2 of
//! the design); this module is the first thing that runs inside it, and it is
//! deliberately the **only** place the interpreter is started:
//!
//! 1. `Py_InitializeEx(0)` — no signal handlers, because the host's are the
//!    ones a server must keep;
//! 2. the distribution's `site-packages` for an embedded interpreter;
//! 3. the `sys.path` entries stage 0 recorded, in front, then the working
//!    directory;
//! 4. the running minor, checked against the one the boot discovered;
//! 5. `SYS_PATH` and `VIEW_PATHS` evaluated in this interpreter, so what the
//!    view may import and what it must be able to read are read from CPython
//!    itself rather than guessed from a version number.

use std::ffi::CStr;

use blastit_py_sapi::dotted_py_minor;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use crate::request::InitPaths;

/// What `init_paths` answers: enough for the boot to log the interpreter and
/// for the visibility check to ask about the right trees.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Answer {
    /// `sys.version`, the whole banner.
    pub(crate) version: String,
    /// `sys.prefix`.
    pub(crate) prefix: String,
    /// Everything a view must be able to read (§7.2), resolved.
    pub(crate) view_paths: Vec<String>,
    /// The interpreter's own `sys.path`, absolute and de-duplicated (§5.3).
    pub(crate) sys_path: Vec<String>,
}

/// Starts the interpreter and prepares its import path.
///
/// # Errors
///
/// A sentence naming the step that failed. A minor mismatch names both CPythons
/// in dotted form, never the `313` wire encoding.
pub(crate) fn init_paths(req: &InitPaths) -> Result<Answer, String> {
    initialize();
    Python::attach(|py| run(py, req))
}

/// The one `Py_InitializeEx` call, on the thread that will hold the GIL.
fn initialize() {
    // SAFETY: the shim is `dlopen`ed into stage 1 before any other call, and
    // this is the embedding contract's own first step: one initialisation, on
    // the main thread, before any Python API use. `initsigs = 0` keeps the
    // host's signal handlers.
    unsafe { pyo3::ffi::Py_InitializeEx(0) };
}

/// The body of [`init_paths`], with the GIL held.
fn run(py: Python<'_>, req: &InitPaths) -> Result<Answer, String> {
    let info = py.version_info();
    let running = u32::from(info.major) * 100 + u32::from(info.minor);
    if req.expected_minor != 0 && req.expected_minor != running {
        let running = dotted_py_minor(running);
        let expected = dotted_py_minor(req.expected_minor);
        return Err(format!(
            "this shim is CPython {running}, but blastit was started for CPython {expected}; \
             install the shim built for CPython {expected}"
        ));
    }
    if req.site_packages {
        py.run(SITE_PACKAGES, Some(&PyDict::new(py)), None)
            .map_err(|err| format!("the interpreter's site-packages could not be added: {err}"))?;
    }
    restore_sys_path(py, &req.sys_path)
        .map_err(|err| format!("the interpreter's sys.path could not be restored: {err}"))?;
    put_cwd_first(py, req.cwd.as_deref())
        .map_err(|err| format!("the working directory on sys.path: {err}"))?;

    let scope = PyDict::new(py);
    scope
        .set_item("blastit_app_module", req.app_module.clone())
        .map_err(|err| format!("the application module: {err}"))?;
    py.run(SYS_PATH, Some(&scope), None)
        .map_err(|err| format!("the interpreter's sys.path could not be read: {err}"))?;
    py.run(VIEW_PATHS, Some(&scope), None)
        .map_err(|err| format!("the view's readable paths: {err}"))?;

    Ok(Answer {
        version: Python::version_str().to_owned(),
        prefix: py
            .import("sys")
            .and_then(|sys| sys.getattr("prefix"))
            .and_then(|prefix| prefix.extract::<String>())
            .map_err(|err| format!("the interpreter's prefix could not be read: {err}"))?,
        view_paths: take_list(&scope, "blastit_view_paths")?,
        sys_path: take_list(&scope, "blastit_sys_path")?,
    })
}

/// One list `SYS_PATH`/`VIEW_PATHS` put in the scope.
fn take_list(scope: &Bound<'_, PyDict>, key: &str) -> Result<Vec<String>, String> {
    let item = scope
        .get_item(key)
        .map_err(|err| format!("the view's {key} could not be read: {err}"))?
        .ok_or_else(|| format!("the shim's `{key}` did not run"))?;
    item.extract::<Vec<String>>()
        .map_err(|err| format!("the view's {key} could not be read: {err}"))
}

/// Puts the interpreter's import path back where stage 0 found it (§4).
///
/// A wheel's stage 1 runs with `-I` — the same interpreter, but no `PYTHONPATH`
/// and no working directory — so an embedded boot's project `.venv` has to win
/// over the base interpreter's `purelib`. Appending `SITE_PACKAGES` first and
/// inserting these at the front is exactly that order.
fn restore_sys_path(py: Python<'_>, entries: &[Vec<u8>]) -> PyResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let fsdecode = py.import("os")?.getattr("fsdecode")?;
    let path = py.import("sys")?.getattr("path")?;
    for entry in entries.iter().rev() {
        let decoded = fsdecode.call1((PyBytes::new(py, entry),))?;
        if !path.contains(decoded.clone())? {
            path.call_method1("insert", (0, decoded))?;
        }
    }
    Ok(())
}

/// Moves the working directory to the front of `sys.path` (§5.3).
///
/// "First" is the whole point — `sys.path` is searched in order — so an entry
/// that is already present is moved rather than added.
fn put_cwd_first(py: Python<'_>, cwd: Option<&[u8]>) -> PyResult<()> {
    let Some(cwd) = cwd else {
        return Ok(());
    };
    let decoded = py
        .import("os")?
        .getattr("fsdecode")?
        .call1((PyBytes::new(py, cwd),))?;
    let path = py.import("sys")?.getattr("path")?;
    // The index is found before anything is edited, so the list is never
    // mutated while it is being walked.
    let length = path.len()?;
    let mut found = None;
    for index in 0..length {
        if path.get_item(index)?.eq(&decoded)? {
            found = Some(index);
            break;
        }
    }
    if let Some(at) = found {
        path.call_method1("pop", (at,))?;
    }
    path.call_method1("insert", (0, decoded))?;
    Ok(())
}

/// §9.3: an embedded interpreter does not find the distribution's
/// `site-packages` on its own. Best effort — a system without `sysconfig` is not
/// a reason to refuse to start.
const SITE_PACKAGES: &CStr = c"
try:
    import os, sys, sysconfig
    for _key in ('purelib', 'platlib'):
        _path = sysconfig.get_paths().get(_key)
        if _path and _path not in sys.path and os.path.isdir(_path):
            sys.path.append(_path)
except Exception:
    pass
";

/// The whole `sys.path` a view will import through (§5.3), absolute.
///
/// Not only what blastit put there: the interpreter's own entries, the working
/// directory, `PYTHONPATH`, whatever a `.pth` file added, and anything an import
/// has appended. A writable entry is a cross-route code plant whoever added it,
/// so the boot asks about all of them.
const SYS_PATH: &CStr = c"
import os, sys
blastit_sys_path = []
for _entry in sys.path:
    _path = os.path.abspath(_entry)
    if _path not in blastit_sys_path:
        blastit_sys_path.append(_path)
";

/// Everything a view has to be able to read, as Python paths (§7.2).
///
/// A view imports *after* the fork, under its own uid, so the interpreter, its
/// standard library, `site-packages` and the application's own directory all
/// have to be readable as "other". Both a path and what it resolves to are
/// collected: a symlinked interpreter is reached through the link *and* read at
/// the target.
const VIEW_PATHS: &CStr = c"
import importlib.util, os, sys, sysconfig
blastit_view_paths = []
def _add_blastit_path(path):
    if not path:
        return
    for _candidate in (path, os.path.realpath(path)):
        if _candidate not in blastit_view_paths:
            blastit_view_paths.append(_candidate)
for _prefix in (getattr(sys, 'base_prefix', ''), sys.prefix):
    _add_blastit_path(_prefix)
for _key, _path in sysconfig.get_paths().items():
    if _key in ('stdlib', 'platstdlib', 'purelib', 'platlib'):
        _add_blastit_path(_path)
if blastit_app_module:
    try:
        _spec = importlib.util.find_spec(blastit_app_module)
    except Exception:
        _spec = None
    if _spec is not None:
        if getattr(_spec, 'origin', None):
            _add_blastit_path(os.path.dirname(_spec.origin))
        for _location in (getattr(_spec, 'submodule_search_locations', None) or ()):
            _add_blastit_path(_location)
";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_different_minor_is_refused_in_dotted_form() {
        let err = dotted_py_minor(313);
        assert_eq!(err, "3.13");
        assert!(!err.contains("313"));
    }
}
