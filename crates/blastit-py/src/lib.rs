//! `blastit._blastit`: the pyo3 extension a wheel installs (D2, §2.1, §4).
//!
//! The module is deliberately tiny and one function wide. Everything blastit
//! does lives in the `blastit` library; the only thing a wheel changes about it
//! is where the program image comes from:
//!
//! * the console script runs on the interpreter the wheel was installed into —
//!   the project's virtualenv — so the application's dependencies are importable
//!   for the same reason they are for `python app.py`;
//! * stage 0 re-executes that interpreter for stage 1 (`python -I -m blastit`,
//!   the arguments the standalone binary passes to `/proc/self/exe`) and carries
//!   the interpreter's `sys.path` across the barrier with it;
//! * dbproxy is `execve`'d as the Python-free binary the wheel ships, because
//!   the process holding the credentials must run no interpreter at all.
//!
//! Importing the module starts nothing: no thread, no interpreter, no file is
//! touched until the `main` function is called. Tools that never serve anything
//! (`pip`, `uv`, `python -c 'import blastit'`) run on such a wheel, and stage 1
//! must find the process exactly as its parent left it.
//!
//! Two more functions serve the pytest plugin (D3b): `normalize`, blastit's own
//! SQL normalisation, and `audit`, which diffs what a test suite observed
//! against what a manifest grants. Both are pure: they parse, they compare, they
//! return — no file, no connection, nothing to clean up.

mod audit;

use std::path::PathBuf;

use blastit::{Host, cli};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Runs blastit on this interpreter: the whole console script (§4).
///
/// `argv` is `sys.argv[1:]`, the `--key=value` arguments [`cli::main`] parses;
/// `executable` is `sys.executable`, the interpreter stage 0 re-executes for
/// stage 1; `dbproxy` is the path of the Python-free `blastit-dbproxy` binary
/// the wheel ships next to this module.
///
/// # Errors
///
/// [`PyValueError`] when the installation is incoherent — an empty
/// `sys.executable`, or a `dbproxy` binary that is not there. That is the one
/// failure the wheel itself can cause, and Python names it better than a
/// half-started sandbox can.
///
/// # Returns
///
/// The process exit status. Stage 0 and stage 1 never return at all: the server
/// becomes this process, or it fails and exits itself.
#[pyfunction]
fn main(py: Python<'_>, argv: Vec<String>, executable: String, dbproxy: String) -> PyResult<u8> {
    let executable = PathBuf::from(executable);
    if executable.as_os_str().is_empty() {
        return Err(PyValueError::new_err(
            "sys.executable is empty: blastit has no interpreter to re-execute for stage 1",
        ));
    }
    let dbproxy = PathBuf::from(dbproxy);
    if !dbproxy.is_file() {
        return Err(PyValueError::new_err(format!(
            "the dbproxy binary this installation ships is missing: {}",
            dbproxy.display()
        )));
    }
    Ok(cli::main(
        Host::wheel(executable, dbproxy, interpreter_path(py)?),
        argv,
        Some(py),
    ))
}

/// Normalises one SQL text the way blastit compares it to a grant (§6).
///
/// The rules are [`blastit_sqlpolicy::normalize`]'s: trim, drop trailing `;`,
/// replace every comment by a single space, collapse whitespace outside quoted
/// regions, and keep quoted strings, identifiers and dollar-quoted text exactly
/// as written. The manifest's text and the text on the wire go through the same
/// function, so this is what a test-suite report means when it says a statement
/// is not granted.
///
/// # Returns
///
/// The normalised text, which is what an allowlist stores and compares.
#[pyfunction]
fn normalize(sql: &str) -> String {
    audit::normalize_sql(sql)
}

/// Diffs what a test suite observed against what a manifest grants (D3b, §6).
///
/// `manifest_toml` is the whole `blastit.toml`; `observations_json` is the
/// document the pytest plugin writes (the [`audit`] module documents its shape
/// and the shape of the report); `script_name` is a `BLASTIT_SCRIPT_NAME` mount
/// prefix to strip before routing, or `None` when the observations already carry
/// the `PATH_INFO` the app saw.
///
/// Routing is blastit's own: [`blastit_manifest::RoutingTable`] on the
/// method and path, [`blastit_manifest::Manifest::effective_grants`] per
/// `(route, proxy)`, and [`blastit_sqlpolicy::Allowlist::check`] on the
/// normalised SQL. Nothing about the manifest is re-implemented here.
///
/// # Errors
///
/// [`PyValueError`] when the manifest is invalid, when the observations do not
/// have the documented shape, or when a statement cannot be attributed to a
/// proxy the manifest declares. A failure is a problem with what was handed
/// over, never a partially written report.
///
/// # Returns
///
/// The report, pretty-printed JSON.
#[pyfunction]
#[pyo3(name = "audit")]
#[pyo3(signature = (manifest_toml, observations_json, script_name=None))]
fn audit_report(
    manifest_toml: &str,
    observations_json: &str,
    script_name: Option<&str>,
) -> PyResult<String> {
    audit::audit(manifest_toml, observations_json, script_name)
        .map_err(|error| PyValueError::new_err(error.to_string()))
}

/// The interpreter's `sys.path`, for stage 1 to restore (§4).
///
/// `sys.path` is a list of strings by construction, so anything else in it is
/// not worth failing a boot over: entries that are not text are skipped, and the
/// ones that are travel with the configuration document into stage 1, which puts
/// them back in front of the path `-I` computes.
///
/// # Errors
///
/// Whatever the interpreter raises while reading `sys.path`.
fn interpreter_path(py: Python<'_>) -> PyResult<Vec<String>> {
    let mut entries = Vec::new();
    for entry in py.import("sys")?.getattr("path")?.try_iter()?.flatten() {
        if let Ok(text) = entry.extract::<String>() {
            entries.push(text);
        }
    }
    Ok(entries)
}

/// The `blastit._blastit` module: the console script's `main`, and the two
/// functions the pytest plugin calls (D3b).
///
/// # Errors
///
/// [`PyErr`] when the module cannot be built, which is a pyo3 failure rather
/// than anything blastit can act on.
#[pymodule]
fn _blastit(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(main, module)?)?;
    module.add_function(wrap_pyfunction!(normalize, module)?)?;
    module.add_function(wrap_pyfunction!(audit_report, module)?)?;
    Ok(())
}
