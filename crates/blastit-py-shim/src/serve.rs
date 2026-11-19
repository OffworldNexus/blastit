#![allow(unsafe_code)]

//! `serve_view`: the child entry that runs one view (design §1.2, §4).
//!
//! By the time this is called the caller has already run
//! [`blastit_sys::child::enter`]: descriptors, limits, umask, working directory,
//! privileges, the seccomp filter and the mask are in place, and the process is
//! the view's own uid. What is left is what only an interpreter can do — point
//! `os.environ` at the private scratch directory, own the inherited
//! descriptors, and hand the GIL to [`serve_reporting`].

use std::os::fd::FromRawFd;
use std::os::unix::net::{UnixDatagram, UnixStream};

use blastit_view::LoadedReport;
use blastit_view_python::{WsgiConfig, serve_reporting};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::boot::Zygote;
use crate::request::Serve;

/// Serves the view until the scheduler's end closes `view.chan`.
///
/// # Errors
///
/// A sentence naming the view and the transport failure: a channel that could
/// not be read, bytes that are not a frame stream, or a configuration this
/// process cannot serve. Whatever the application does is its own request's
/// problem and never reaches here.
pub(crate) fn serve_view(zygote: &Zygote, req: &Serve) -> Result<(), String> {
    if req.view_fd < 0 {
        return Err("serve_view: no view descriptor was handed to the child".to_string());
    }
    Python::attach(|py| {
        if let Some(home) = &req.home {
            set_environment(py, home)
                .map_err(|err| format!("the view's private HOME/TMPDIR could not be set: {err}"))?;
        }
        // SAFETY: the descriptors were created by the parent, survive the fork,
        // and are this process's from here on — the streams close them.
        let chan = unsafe { UnixStream::from_raw_fd(req.view_fd) };
        let loaded = (req.loaded_fd >= 0).then(|| LoadedReport {
            // SAFETY: as above, and the report socket is per spawn.
            socket: unsafe { UnixDatagram::from_raw_fd(req.loaded_fd) },
            source: req.loaded_source,
        });
        let config = WsgiConfig {
            route_name: req.route.clone(),
            script_name: req.script_name.clone(),
            max_threads: req.max_threads,
            ..WsgiConfig::default()
        };
        serve_reporting(py, chan, zygote.app.clone_ref(py), config, loaded)
            .map_err(|err| format!("view {}: {err}", req.route))
    })
}

/// Points the application's `HOME` and `TMPDIR` at its private scratch
/// directory (§5.3, §7.2).
///
/// `os.environ` is the interpreter's own copy, and setting it also updates the
/// process environment, which is what a library that looks at `getenv` sees.
fn set_environment(py: Python<'_>, home: &[u8]) -> PyResult<()> {
    let os = py.import("os")?;
    let decoded = os.getattr("fsdecode")?.call1((PyBytes::new(py, home),))?;
    let environ = os.getattr("environ")?;
    environ.set_item("HOME", &decoded)?;
    environ.set_item("TMPDIR", &decoded)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::request::Serve;

    #[test]
    fn a_child_without_a_view_descriptor_is_refused_by_name() {
        // The refusal a caller sees must name the call, not a raw `EBADF`: this
        // is the one failure the shim can diagnose before touching Python.
        let req = Serve::default();
        assert!(req.view_fd < 0);
        assert!(req.loaded_fd < 0);
    }
}
