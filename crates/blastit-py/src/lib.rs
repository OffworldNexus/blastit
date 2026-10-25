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

/// The interpreter's `sys.path`, as bytes, for stage 1 to restore (§4).
///
/// `os.fsencode` is what makes this exact: a path is bytes, and one that is not
/// valid UTF-8 — a home directory in a legacy encoding, say — is a path like any
/// other. Reading Python's text `sys.path` through Rust would drop it.
///
/// # Errors
///
/// Whatever the interpreter raises while reading `sys.path`.
fn interpreter_path(py: Python<'_>) -> PyResult<Vec<Vec<u8>>> {
    let fsencode = py.import("os")?.getattr("fsencode")?;
    let mut entries = Vec::new();
    for entry in py.import("sys")?.getattr("path")?.try_iter()?.flatten() {
        // One entry that `fsencode` refuses — an `int`, an object that is not a
        // path — is not worth failing a boot over.
        if let Ok(bytes) = fsencode
            .call1((entry,))
            .and_then(|bytes| bytes.extract::<Vec<u8>>())
        {
            entries.push(bytes);
        }
    }
    Ok(entries)
}

/// The `blastit._blastit` module: one function, [`main`].
///
/// # Errors
///
/// [`PyErr`] when the module cannot be built, which is a pyo3 failure rather
/// than anything blastit can act on.
#[pymodule]
fn _blastit(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(main, module)?)?;
    Ok(())
}
