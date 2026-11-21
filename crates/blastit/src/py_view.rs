//! Forking one WSGI view from the preloaded interpreter (design §1.2.1, §4).
//!
//! init is the only process that holds the preloaded image, so it is the only
//! one that can create a view: the scheduler asks over `ctl.chan`, this module
//! forks, and the child runs the sequence the design pins —
//!
//! ```text
//! before_fork (inside `forking::fork`, with the GIL held)
//! fork(2)
//!   parent: after_fork_parent
//!   child:  after_fork_child
//!           blastit_sys::child::enter(plan)   (descriptors, limits, uid, filter, mask)
//!           vtable.serve_view(zygote, request)
//!           _exit(code)
//! ```
//!
//! Nothing here names an interpreter: `serve_view` is a `blastit_py_abi_v1`
//! entry point, and the request is the shim's own document. The GIL discipline
//! is the shim's too (`PyGILState_Ensure`/`Release` inside each entry), so this
//! crate never ensures it.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;

use blastit_sys::{Fork, Pid};
use blastit_view::spawn::SpawnContext;
use serde::Serialize;

use blastit_sys::child::child_exit;

use crate::child::{ChildIo, ChildPlan};

#[cfg(feature = "python")]
use crate::py::PyRuntime;

/// How many extra processes a view's uid may have beyond its request threads
/// (7.2: gVisor leaves out the main thread, so one unit of margin).
pub(crate) const VIEW_PROCESS_MARGIN: u32 = 16;

/// How many descriptors a view may hold open.
pub(crate) const VIEW_NOFILE: u64 = 1024;

/// A freshly forked view process, as the parent sees it.
pub(crate) struct ForkedView {
    /// The new process.
    pub(crate) pid: Pid,
    /// The scheduler's end of `view.chan`.
    pub(crate) scheduler_end: OwnedFd,
    /// The read end of the view's stdout pipe.
    pub(crate) stdout: OwnedFd,
    /// The read end of the view's stderr pipe.
    pub(crate) stderr: OwnedFd,
    /// How long the fork took, in milliseconds.
    pub(crate) fork_ms: f64,
}

/// Everything one view fork needs (§4, `R19..R21`).
pub(crate) struct ViewSpawn {
    /// The composition root's boot data for this route.
    pub(crate) ctx: SpawnContext,
    /// `SCRIPT_NAME`, already normalised by the composition root.
    pub(crate) script_name: String,
    /// The largest number of requests this process serves at once.
    pub(crate) max_threads: u32,
    /// Private scratch directory (`HOME`, `TMPDIR`, `chdir`).
    pub(crate) dir: PathBuf,
    /// The dev-only loaded-file report: this view's own datagram and source id.
    pub(crate) loaded: Option<blastit_view::LoadedReport>,
}

/// The `serve_view` request (design §1.2), as the shim reads it.
#[derive(Serialize)]
struct ServeRequest {
    view_fd: i32,
    loaded_fd: i32,
    loaded_source: u16,
    route: String,
    script_name: String,
    max_threads: Option<u32>,
    home: Option<Vec<u8>>,
    max_frame: Option<usize>,
}

/// Forks a view process for `spawn`, or returns the errno and leaves the
/// process as it was.
///
/// # Errors
///
/// [`std::io::Error`] when a socketpair or pipe cannot be created, or whatever
/// the fork protocol refuses with — in particular a fork in a process with more
/// than one thread ([`blastit_sys::Error::NotSingleThreaded`]).
#[cfg(feature = "python")]
pub(crate) fn fork_view(runtime: PyRuntime<'_>, spawn: ViewSpawn) -> std::io::Result<ForkedView> {
    let (view_end, scheduler_end) = blastit_sys::socketpair_stream()?;
    let (stdout_read, stdout_write) = blastit_sys::pipe()?;
    let (stderr_read, stderr_write) = blastit_sys::pipe()?;

    // The report socket is one extra descriptor in the child's keep list: it is
    // created per spawn, so a view's second boot can never reuse the first
    // reporter.
    let mut keep = vec![view_end.as_raw_fd()];
    if let Some(report) = &spawn.loaded {
        keep.push(report.socket.as_raw_fd());
    }
    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: spawn.ctx.rlimits,
        working_dir: Some(spawn.dir.clone()),
    };

    let started = std::time::Instant::now();
    let fork = crate::forking::fork(Some(runtime.hooks))?;
    let fork_ms = started.elapsed().as_secs_f64() * 1000.0;

    match fork {
        Fork::Parent(pid) => Ok(ForkedView {
            pid,
            scheduler_end,
            stdout: stdout_read,
            stderr: stderr_read,
            fork_ms,
        }),
        Fork::Child => child_main(runtime, spawn, child, view_end),
    }
}

/// Everything the child does after `PyOS_AfterFork_Child`. Never returns.
///
/// `child_exit` is `_exit`: the shim owns the two descriptors from the moment
/// `serve_view` adopts them, so no destructor may run here.
#[cfg(feature = "python")]
fn child_main(
    runtime: PyRuntime<'_>,
    mut spawn: ViewSpawn,
    child: ChildIo,
    view_end: OwnedFd,
) -> ! {
    let route = spawn.ctx.route.clone();
    let plan = ChildPlan {
        name: format!("view {route}"),
        io: child,
        privileges: spawn.ctx.privileges,
        filter: spawn.ctx.filter,
        // A WSGI view is not a process-group leader: supervision kills it by
        // pid.
        pgroup: false,
        mask: Some(spawn.ctx.mask),
    };
    // The child sequence runs **before** the interpreter is touched again, in
    // the order `blastit_sys::child` documents (design §1.2.1 item 5).
    if let Err(message) = blastit_sys::child::enter(plan) {
        child_exit(&message, 1);
    }

    let loaded = spawn.loaded.take();
    let request = ServeRequest {
        view_fd: view_end.as_raw_fd(),
        loaded_fd: loaded
            .as_ref()
            .map_or(-1, |report| report.socket.as_raw_fd()),
        loaded_source: loaded.as_ref().map_or(0, |report| report.source),
        route: route.clone(),
        script_name: spawn.script_name.clone(),
        max_threads: Some(spawn.max_threads),
        home: Some(spawn.dir.clone().into_os_string().into_encoded_bytes()),
        max_frame: Some(spawn.ctx.frame_limits.max_frame()),
    };
    let bytes = match serde_json::to_vec(&request) {
        Ok(bytes) => bytes,
        Err(err) => child_exit(&format!("view {route}: the serve request: {err}"), 1),
    };
    let (code, err) = runtime.serve_view(&bytes);
    if code == 0 {
        child_exit("", 0);
    }
    let message = crate::py::refusal_message(&err);
    child_exit(&format!("view {route}: {message}"), code)
}

/// A `python`-less build never forks a WSGI view: the boot gate refused every
/// route that would need one.
///
/// # Errors
///
/// Always one sentence: a `SPAWN` for a WSGI slot means the boot gate and the
/// scheduler disagreed.
#[cfg(not(feature = "python"))]
pub(crate) fn fork_view(_runtime: (), spawn: ViewSpawn) -> std::io::Result<ForkedView> {
    Err(std::io::Error::other(format!(
        "route {:?}: this blastit build has no Python worker runtime",
        spawn.ctx.route
    )))
}
