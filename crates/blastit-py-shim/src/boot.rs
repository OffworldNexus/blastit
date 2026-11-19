#![allow(unsafe_code)]

//! `preload`: the immortal parent every view is forked from (design §1.2, §9.2).
//!
//! The order is the whole value of the slice, and it is the composition root's
//! order, moved behind the ABI:
//!
//! 1. the dev-only loaded-file tracker, **before** the first import, so the
//!    `preload` phase's modules and file opens are seen (`ADR-0014` §2);
//! 2. the application callable, the `preload` modules, and the warm-up call;
//! 3. the boot batch through the report socket, with `boot` set, so the
//!    supervisor replaces that source's set before a view exists;
//! 4. `CLOSE_AND_COLLECT` — database handles are not inherited by every view,
//!    and the heap the next step walks has no garbage in it;
//! 5. [`blastit_view_python::immortal::apply`], between the collection and the
//!    freeze, on the interpreter whose marker this build knows;
//! 6. `gc.freeze()`, whether or not the pass ran.
//!
//! The application callable lives in a [`Zygote`] and travels out as an opaque
//! pointer; nothing here exposes `Py<…>` beyond this crate.

use std::ffi::CStr;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;

use blastit_view::LoadedReport;
use blastit_view::loaded::PRELOAD_SOURCE;
use blastit_view_python::immortal;
use blastit_view_python::{LoadedReporter, LoadedTracker};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::request::Preload;

/// The preloaded parent: the one callable a forked view serves requests with.
///
/// Opaque across the ABI (`PyZygoteOpaque`): the pointer a caller stores is this
/// box, and only this crate ever reads it back.
pub(crate) struct Zygote {
    /// The WSGI application, imported before the fork.
    pub(crate) app: Py<PyAny>,
}

/// What `preload` reports about §9.2 #7's pass.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Answer {
    /// `"on"` or `"off"`.
    pub(crate) decision: &'static str,
    /// How many objects carry the marker afterwards.
    pub(crate) objects: u64,
    /// Why the pass did not run, when it did not.
    pub(crate) reason: Option<String>,
    /// How long the pass took, in nanoseconds (0 when it was skipped).
    pub(crate) elapsed_ns: u64,
}

/// Imports and prepares the application (§4 of the design).
///
/// # Errors
///
/// A sentence naming the step that failed — `BLASTIT_APP`, `BLASTIT_PRELOAD`,
/// `BLASTIT_WARMUP`, `gc.collect`, the immortalisation pass or `gc.freeze` —
/// with the interpreter's own message after it.
pub(crate) fn preload(req: &Preload) -> Result<(Box<Zygote>, Answer), String> {
    Python::attach(|py| boot(py, req))
}

/// The body of [`preload`], with the GIL held.
fn boot(py: Python<'_>, req: &Preload) -> Result<(Box<Zygote>, Answer), String> {
    let (tracker, socket) = install_tracker(py, req)?;
    let app = match req.app.as_deref() {
        Some(spec) => {
            import_callable(py, spec).map_err(|err| format!("BLASTIT_APP ({spec}): {err}"))?
        }
        // An all-static site has no app to import: every answer comes from the
        // scheduler, so there is nothing to preload and no heap to inherit.
        None => py.None(),
    };
    for module in &req.preload {
        if let Err(err) = py.import(module.as_str()) {
            return Err(format!("BLASTIT_PRELOAD ({module}): {err}"));
        }
    }
    if let Some(warmup) = req.warmup.as_deref() {
        let callable = import_callable(py, warmup)
            .map_err(|err| format!("BLASTIT_WARMUP ({warmup}): {err}"))?;
        callable
            .call0(py)
            .map_err(|err| format!("BLASTIT_WARMUP ({warmup}): {err}"))?;
    }
    report_boot(py, tracker.as_ref(), socket);
    py.run(CLOSE_AND_COLLECT, None, None).map_err(|err| {
        format!("closing the preloaded database handles and collecting garbage failed: {err}")
    })?;
    let outcome = immortal::apply(py, req.immortalize.setting())
        .map_err(|err| format!("heap immortalisation: {err}"))?;
    py.run(FREEZE, None, None)
        .map_err(|err| format!("freezing the heap failed: {err}"))?;
    let answer = Answer {
        decision: if outcome.decision.is_on() {
            "on"
        } else {
            "off"
        },
        objects: outcome.objects(),
        reason: outcome.decision.reason().map(str::to_owned),
        elapsed_ns: outcome
            .millis()
            .map_or(0, |millis| (millis * 1_000_000.0) as u64),
    };
    Ok((Box::new(Zygote { app }), answer))
}

/// Arms the dev-only loaded-file tracker and takes ownership of the report
/// socket, when one was handed in.
///
/// The descriptor is adopted **first**, so every path below — the missing root,
/// a tracker the interpreter refuses, a successful boot — drops it exactly once.
/// Leaving it to `report_boot` would leak it past a failed `install`, and the
/// preload parent is the process every view is forked from: an inherited write
/// end is one every view can report on.
fn install_tracker(
    py: Python<'_>,
    req: &Preload,
) -> Result<(Option<LoadedTracker>, Option<UnixDatagram>), String> {
    let socket = (req.report_fd >= 0).then(|| {
        // SAFETY: `report_fd` is this process's write end of the sink
        // socketpair, created by stage 1 and owned here from now on.
        unsafe { UnixDatagram::from_raw_fd(req.report_fd) }
    });
    let Some(socket) = socket else {
        return Ok((None, None));
    };
    let root = req
        .root
        .clone()
        .ok_or_else(|| "preload: a report descriptor needs a project root".to_string())?;
    let root = PathBuf::from(std::ffi::OsString::from_vec(root));
    // A tracker the interpreter refuses is no tracker: instrumentation must
    // never fail a boot, and a view then reports nothing — but the socket is
    // still owned by this frame, so it is closed when we return.
    Ok((LoadedTracker::install(py, &root), Some(socket)))
}

/// Sends the `preload` phase's own batch, with `boot` set.
///
/// `socket` is stage 1's write end, adopted by [`install_tracker`]; the cold
/// `sys.modules` scan is paid once here, in this parent, so every view's first
/// report is warm (`MB.23.4`). A boot with no tracker consumes the socket
/// without sending anything.
fn report_boot(py: Python<'_>, tracker: Option<&LoadedTracker>, socket: Option<UnixDatagram>) {
    let (Some(tracker), Some(socket)) = (tracker, socket) else {
        return;
    };
    let _ = tracker.harvest(py);
    if let Some(reporter) = LoadedReporter::adopt(
        py,
        LoadedReport {
            socket,
            source: PRELOAD_SOURCE,
        },
    ) {
        reporter.boot(py);
    }
}

/// `module:attribute` -> the callable, without executing anything else.
fn import_callable(py: Python<'_>, spec: &str) -> PyResult<Py<PyAny>> {
    let Some((module, attribute)) = spec.split_once(':') else {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "`{spec}` is not `module:attribute`"
        )));
    };
    let module: Bound<'_, PyModule> = py.import(module)?;
    Ok(module.getattr(attribute)?.unbind())
}

/// §4: close database handles before the fork, then collect.
///
/// A connection opened at import time would be inherited — uselessly and
/// dangerously — by every view.
const CLOSE_AND_COLLECT: &CStr = c"
import gc, sys
if 'django' in sys.modules:
    from django.db import connections
    connections.close_all()
gc.collect()
";

/// §9.2 #7: freeze the heap, whether or not it could be immortalised.
const FREEZE: &CStr = c"
import gc
gc.freeze()
";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::request::Immortalize;

    #[test]
    fn the_freeze_follows_the_immortalisation_pass() {
        // The order is the value of the slice: an immortal object is never
        // freed, so its address cannot be reused by a later allocation.
        let collect = super::CLOSE_AND_COLLECT.to_str().expect("utf-8");
        let freeze = super::FREEZE.to_str().expect("utf-8");
        assert!(collect.contains("gc.collect()"));
        assert!(freeze.contains("gc.freeze()"));
        assert!(!collect.contains("gc.freeze()"), "freeze is its own step");
    }

    #[test]
    fn an_operator_who_turned_the_pass_off_is_reported_as_off() {
        // `off` is a decision, not a failure: the shim still freezes the heap.
        assert_eq!(Immortalize::Off.setting(), Settings::Off);
    }

    use blastit_view_python::immortal::Setting as Settings;
}
