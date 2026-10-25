//! The zygote: the only process that can create a view (§2.1, §4).
//!
//! init is root and holds the preloaded interpreter, so it is the only one that
//! can fork a view: the scheduler asks over `ctl.chan`, init forks, drops
//! privileges in the child, and passes the scheduler's end of the new
//! `view.chan` back with `SCM_RIGHTS`.
//!
//! The fork runs CPython's own protocol — `PyOS_BeforeFork()`,
//! `PyOS_AfterFork_Parent()` in the parent, `PyOS_AfterFork_Child()` in the
//! child — while holding the GIL. The child then runs the crate-level child
//! sequence, sets the private `HOME`/`TMPDIR` the application is supposed to
//! see, and never returns.
//!
//! # Forks happen with the GIL held
//!
//! §9.3 describes the GIL as released around the fork, with each child
//! re-attaching. This implementation holds it instead, on both sides:
//!
//! * `PyOS_BeforeFork()` stops the world, which CPython only allows with the
//!   GIL held, and `PyOS_AfterFork_Parent()` / `PyOS_AfterFork_Child()` restart
//!   it — they are the matching half of the call, not an optional extra;
//! * init holds the GIL for its whole life (there is no other thread in it that
//!   could want it), so "release around the fork" would mean detaching and
//!   re-attaching around every view, for no benefit;
//! * the child therefore **inherits the GIL**, and the only thing it may do
//!   with it is hand it to [`blastit_view::serve`], which detaches for the
//!   whole of its work and lets the request threads attach. Everything between
//!   `PyOS_AfterFork_Child()` and that call runs on the interpreter's main
//!   thread with no other Python thread in existence — the fork contract §9.3
//!   asks for — and nothing else in the child touches the interpreter.
//!
//! The orchestrator records this deviation in `ARCHITECTURE.md` §9.3.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use blastit_frame::Limits;
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, drop_privileges, fork, harden_child, pipe,
    restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};
use blastit_view::ViewConfig;
use pyo3::prelude::*;
use pyo3::types::PyTracebackMethods;

use crate::child::{ChildIo, child_exit};
use crate::config::{Config, view_dir};

/// How many extra processes a view's uid may have beyond its request threads
/// (§7.2: gVisor leaves out the main thread, so one unit of margin).
pub const VIEW_PROCESS_MARGIN: u32 = 16;

/// How many descriptors a view may hold open.
///
/// A request thread needs its channel, its stdout and stderr, one database
/// socket and whatever the application opens; 1024 is far above that and far
/// below what a runaway could exhaust.
pub const VIEW_NOFILE: u64 = 1024;

/// A freshly forked view process, as the parent sees it.
pub struct ForkedView {
    /// The new process.
    pub pid: Pid,
    /// The scheduler's end of `view.chan`.
    pub scheduler_end: OwnedFd,
    /// The read end of the view's stdout pipe.
    pub stdout: OwnedFd,
    /// The read end of the view's stderr pipe.
    pub stderr: OwnedFd,
    /// How long the fork took, in milliseconds.
    pub fork_ms: f64,
}

/// Forks a view process for `info`, or returns the errno and leaves the process
/// as it was.
///
/// # Errors
///
/// [`std::io::Error`] when a socketpair or pipe cannot be created, or when
/// `fork` refuses to run because init is not single-threaded.
pub fn fork_view(
    py: Python<'_>,
    cfg: &Config,
    limits: Limits,
    info: &RouteInfo,
    app: &Py<PyAny>,
    mask: blastit_sys::SignalMask,
) -> std::io::Result<ForkedView> {
    let (view_end, scheduler_end) = socketpair_stream()?;
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let dir = view_dir(info.uid);

    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep: vec![view_end.as_raw_fd()],
        limits: Rlimits {
            nproc: Some(cfg.view_worker_cap().saturating_add(VIEW_PROCESS_MARGIN) as u64),
            nofile: Some(VIEW_NOFILE),
            core_zero: true,
        },
        working_dir: Some(dir.clone()),
    };

    let started = std::time::Instant::now();
    // SAFETY: init is single-threaded and holds the GIL, which is exactly what
    // CPython's fork protocol needs (§9.3).
    unsafe { pyo3::ffi::PyOS_BeforeFork() };
    let outcome = fork();
    // CPython stops the world in `PyOS_BeforeFork` and restarts it in either
    // of the two `AfterFork` calls, whichever side of the fork we are on.
    let fork = match outcome {
        Ok(fork) => fork,
        Err(err) => {
            // SAFETY: `PyOS_BeforeFork` was called just above; this is its
            // matching call when no child was created.
            unsafe { pyo3::ffi::PyOS_AfterFork_Parent() };
            return Err(err);
        }
    };
    let fork_ms = started.elapsed().as_secs_f64() * 1000.0;

    match fork {
        Fork::Parent(pid) => {
            // SAFETY: we are in the parent of the fork above.
            unsafe { pyo3::ffi::PyOS_AfterFork_Parent() };
            Ok(ForkedView {
                pid,
                scheduler_end,
                stdout: stdout_read,
                stderr: stderr_read,
                fork_ms,
            })
        }
        Fork::Child => {
            // SAFETY: we are in the child of the fork above, and the GIL is
            // held. Nothing before this call has touched an allocator lock.
            unsafe { pyo3::ffi::PyOS_AfterFork_Child() };
            child_main(py, cfg, limits, info, app, mask, child, view_end, dir)
        }
    }
}

/// Everything the child does after `PyOS_AfterFork_Child`. Never returns.
///
/// The work after the child sequence is wrapped in `catch_unwind`: the
/// workspace cannot set `panic = "abort"` from one crate, so an unexpected
/// panic — a poisoned lock, an index that should not be out of range — is
/// turned into an exit code instead of an unwinding child that leaves a
/// half-initialised interpreter behind. A view that panics exits `70`.
#[allow(clippy::too_many_arguments)]
fn child_main(
    py: Python<'_>,
    cfg: &Config,
    limits: Limits,
    info: &RouteInfo,
    app: &Py<PyAny>,
    mask: blastit_sys::SignalMask,
    child: ChildIo,
    view_end: OwnedFd,
    dir: PathBuf,
) -> ! {
    // The child sequence, in the order `blastit_sys` documents: descriptors,
    // limits, umask, working directory, then privileges, then hardening.
    let outcome = child
        .prepare()
        .and_then(|()| drop_privileges(info.uid))
        .and_then(|()| harden_child(Signal::KILL));
    if let Err(err) = outcome {
        child_exit(&format!("view {}: {err}", info.name), 1);
    }
    let _ = set_dumpable(false);
    if let Err(err) = restore_signal_mask_in_child(mask) {
        child_exit(&format!("view {}: {err}", info.name), 1);
    }

    let name = info.name.clone();
    let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve_forever(py, cfg, limits, info, app, view_end, dir)
    }));
    match started {
        // `serve_forever` never returns normally; this is unreachable in
        // practice, and exiting non-zero is the honest answer if it happens.
        Ok(()) => child_exit(
            &format!("view {name}: the view stopped without a reason"),
            1,
        ),
        Err(payload) => child_exit(
            &format!("view {name}: panicked: {}", panic_message(&payload)),
            70,
        ),
    }
}

/// The rest of a view's life: the interpreter's `HOME`/`TMPDIR`, then serving.
///
/// This is the part `child_main` guards with `catch_unwind`; it is with the GIL
/// held throughout, and the only call that touches the interpreter is
/// [`blastit_view::serve`], which detaches.
fn serve_forever(
    py: Python<'_>,
    cfg: &Config,
    limits: Limits,
    info: &RouteInfo,
    app: &Py<PyAny>,
    view_end: OwnedFd,
    dir: PathBuf,
) {
    if let Err(err) = set_view_environment(py, &dir) {
        child_exit(&format!("view {}: HOME/TMPDIR: {err}", info.name), 1);
    }

    // SAFETY: `view_end` is this process's end of `view.chan`, created for it
    // before the fork; nothing else in this process owns the number.
    let raw = view_end.as_raw_fd();
    let chan = unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw) };
    // The stream owns the descriptor from here, and `serve` closes it when it
    // is done. `view_end` must not close it a second time — a descriptor that
    // has already gone back to the kernel can have been handed out again.
    std::mem::forget(view_end);
    let view_cfg = ViewConfig {
        route_name: info.name.clone(),
        script_name: mount_point(cfg.script_name.as_deref()),
        frame_limits: limits,
        max_threads: Some(cfg.view_worker_cap()),
    };
    match blastit_view::serve(py, chan, app.clone_ref(py), view_cfg) {
        Ok(()) => child_exit("", 0),
        Err(err) => child_exit(&format!("view {}: {err}", info.name), 1),
    }
}

/// The text of a panic payload, for the log.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_string()
    }
}

/// Points the application's `HOME` and `TMPDIR` at its private scratch
/// directory (§5.3, §7.2).
///
/// `os.environ` is the interpreter's own copy, and setting it also updates the
/// process environment, which is what a library that looks at `getenv` sees.
fn set_view_environment(py: Python<'_>, dir: &Path) -> PyResult<()> {
    let os = py.import("os")?;
    let environ = os.getattr("environ")?;
    let path = dir.to_string_lossy().into_owned();
    environ.set_item("HOME", &path)?;
    environ.set_item("TMPDIR", &path)?;
    Ok(())
}

/// The mount point a view reports as `SCRIPT_NAME`.
///
/// The same normalisation [`blastit_scheduler`] applies to
/// `BLASTIT_SCRIPT_NAME`: trailing slashes are dropped, a missing leading slash
/// is added, and an empty result means "mounted at the root".
pub fn mount_point(script_name: Option<&str>) -> String {
    let Some(value) = script_name else {
        return String::new();
    };
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Formats a Python error with its traceback, for the log (§4).
///
/// init never writes anywhere but the event stream, so a failed import has to
/// become events, not a stack trace on stderr.
pub fn traceback_text(py: Python<'_>, err: &PyErr) -> String {
    match err.traceback(py) {
        Some(traceback) => {
            let header = traceback.format().unwrap_or_default();
            format!("{header}{err}")
        }
        None => err.to_string(),
    }
}
