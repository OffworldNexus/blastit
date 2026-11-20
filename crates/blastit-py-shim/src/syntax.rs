//! `check_syntax`: compile one changed file without executing it (design §1.2).
//!
//! `blastit dev`'s reload preflight compiles every changed Python file before it
//! disturbs a view, so a half-saved module is refused with the file's own
//! sentence. `PyCode::compile(…, PyCodeInput::File)` is that compile: it builds
//! the code object and stops. Nothing here imports, executes or imports anything
//! on the application's behalf.
//!
//! The sentence the caller prints is byte-identical to the composition root's:
//! `<path>: <interpreter message>`.

use std::ffi::CString;

use pyo3::prelude::*;
use pyo3::types::code::{PyCode, PyCodeInput};

/// Compiles `code` as the file at `path`, and executes none of it.
///
/// # Errors
///
/// `<path>: <interpreter message>`, or a refusal naming the NUL byte when the
/// bytes cannot be a C string (an interpreter cannot compile such a file
/// either).
pub(crate) fn check_syntax(code: &[u8], path: &[u8]) -> Result<(), String> {
    let name = String::from_utf8_lossy(path).into_owned();
    let source = CString::new(code).map_err(|_| format!("{name}: the file contains a NUL byte"))?;
    let file = CString::new(path).map_err(|_| format!("{name}: the path contains a NUL byte"))?;
    Python::attach(|py| {
        PyCode::compile(py, &source, &file, PyCodeInput::File)
            .map(|_| ())
            .map_err(|err| format!("{name}: {err}"))
    })
}
