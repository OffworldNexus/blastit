#![cfg_attr(not(feature = "python"), allow(unused_imports))]

#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::PyBytes;

use crate::py::Python;

#[cfg(feature = "python")]
pub(super) fn put_cwd_first(py: Python<'_>, cwd: Option<&[u8]>) -> PyResult<()> {
    let Some(cwd) = cwd else {
        return Ok(());
    };
    let decoded = py
        .import("os")?
        .getattr("fsdecode")?
        .call1((PyBytes::new(py, cwd),))?;
    let path = py.import("sys")?.getattr("path")?;
    // The directory is *moved* to the front when it is already on the path —
    // `sys.path` is searched in order, and "first" is the whole point — and
    // added when it is not. The index is found before anything is edited, so
    // the list is never mutated while it is being walked.
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

/// Puts the interpreter's import path back where stage 0 found it (§4).
///
/// A wheel's stage 1 runs with `-I` — the same interpreter, but no `PYTHONPATH`
/// and no working directory on `sys.path`. The entries the user's interpreter
/// had — bytes, as stage 0 recorded them with `os.fsencode` — are re-inserted in
/// front, in their original order, skipping the ones `-I` already provides: the
/// result is exactly the `sys.path` the console script saw, which is where the
/// application's modules are found. `os.fsdecode` is what turns those bytes back
/// into the text `sys.path` is made of, without dropping a path that is not
/// valid UTF-8.
///
/// # Errors
///
/// Whatever the interpreter raises when `sys.path` is not what CPython promises
/// it is.
#[cfg(feature = "python")]
pub(super) fn restore_sys_path(py: Python<'_>, entries: &[Vec<u8>]) -> PyResult<()> {
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
