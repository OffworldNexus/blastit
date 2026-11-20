//! The zygote: the only process that can create a WSGI view (§2.1, §4).
//!
//! init is the one process that holds the preloaded interpreter, so it is the
//! only one that can fork a view: the scheduler asks over `ctl.chan`, init
//! forks, drops privileges in the child, and passes the scheduler's end of the
//! new `view.chan` back with `SCM_RIGHTS`. That parent half stays in the
//! composition root; **this** module is the child half, and it lives beside the
//! worker it starts (`blastit-view-python`) because a view crate may not depend
//! on `blastit`.
//!
//! The fork runs CPython's own protocol — `PyOS_BeforeFork()`,
//! `PyOS_AfterFork_Parent()` in the parent, `PyOS_AfterFork_Child()` in the
//! child — while holding the GIL. The fork syscall itself is **injected**
//! ([`Forker`]): the composition root owns the one implementation of that
//! protocol (`blastit/src/forking.rs`), and this crate asks for the protocol its
//! image needs instead of calling `fork(2)`. The child then runs
//! [`blastit_sys::child::enter`], sets the private `HOME`/`TMPDIR` the
//! application is supposed to see, and never returns.
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
//!   with it is hand it to [`crate::serve_reporting`], which detaches for the
//!   whole of its work and lets the request threads attach. Everything between
//!   `PyOS_AfterFork_Child()` and that call runs on the interpreter's main
//!   thread with no other Python thread in existence — the fork contract §9.3
//!   asks for — and nothing else in the child touches the interpreter.
//!
//! The orchestrator records this deviation in `ARCHITECTURE.md` §9.3.
//!
//! # Step 0: nothing loads or opens before `enter`
//!
//! The protocol's child half runs privileged and unfiltered, before
//! [`blastit_sys::child::enter`]. Nothing here `dlopen`s, opens a file or
//! mutates the environment between the fork and `enter`: the socketpair and the
//! two pipes are created by the parent *before* the fork, and the child's first
//! action is [`blastit_sys::child::enter`].

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use blastit_sys::child::{ChildIo, ChildPlan, child_exit, panic_message};
use blastit_sys::{Fork, Pid};
use blastit_view::spawn::{ForkProtocol, Forker, SpawnContext};
use pyo3::prelude::*;
use pyo3::types::PyTracebackMethods;

use crate::WsgiConfig;

/// How many extra processes a view's uid may have beyond its request threads
/// (§7.2: gVisor leaves out the main thread, so one unit of margin).
///
/// The composition root adds this to its worker cap when it builds a
/// [`SpawnContext`]'s `nproc` limit, and to the fork ceiling it enforces itself.
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

/// Everything this crate needs to fork one WSGI view process (§4, `R19..R21`).
///
/// [`SpawnContext`] is the composition root's boot data — route, uid, frame
/// limits, rlimits, privileges, filter, egress, mask — and the rest is what only
/// a PEP 3333 worker has: the callable, its mount point, its thread cap, its
/// own scratch directory and (in `dev`) its loaded-file report socket.
///
/// `dir` is this crate's one directory field: the fork's `chdir` target and the
/// child's `HOME`/`TMPDIR` are the same value, so they can never disagree
/// (`view-crates-architecture` §4.4 D2/S6).
pub struct ViewSpawn<'a> {
    /// The composition root's boot data for this route.
    pub ctx: SpawnContext,
    /// Preloaded WSGI callable.
    pub app: &'a Py<PyAny>,
    /// `SCRIPT_NAME`, already normalised by the composition root
    /// (`config::mount_point`): empty at the root, otherwise `/prefix`.
    pub script_name: String,
    /// The largest number of requests this process serves at once.
    pub max_threads: u32,
    /// Private scratch directory (`HOME`, `TMPDIR`, `chdir`).
    pub dir: PathBuf,
    /// The dev-only loaded-file report: this view's own datagram socket and the
    /// source id its batches name (`ADR-0014` §2, design §4.4).
    ///
    /// `None` is every production view, `check`, a dev boot with reload off and
    /// every PHP slot. A view with no report serves exactly as it did before.
    pub loaded: Option<blastit_view::LoadedReport>,
}

/// Forks a view process for `spawn`, or returns the errno and leaves the
/// process as it was.
///
/// `forker` is the composition root's fork: it runs CPython's three hooks under
/// the GIL (`ForkProtocol::InterpreterLive` — a WSGI view only exists when an
/// interpreter is live) and refuses a protocol that disagrees with the image.
///
/// # Errors
///
/// [`std::io::Error`] when a socketpair or pipe cannot be created, or whatever
/// the forker refuses with — in particular a fork in a process with more than
/// one thread, which §2.1 forbids.
pub fn fork_view(
    py: Python<'_>,
    spawn: ViewSpawn<'_>,
    forker: Forker<'_>,
) -> std::io::Result<ForkedView> {
    let (view_end, scheduler_end) = blastit_sys::socketpair_stream()?;
    let (stdout_read, stdout_write) = blastit_sys::pipe()?;
    let (stderr_read, stderr_write) = blastit_sys::pipe()?;

    // The report socket is one extra descriptor in the child's keep list: it is
    // created per spawn, so a view's second boot can never reuse the first
    // reporter (`loaded::Reporter` refuses a second boot on one stream).
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
    // The fork runs CPython's protocol — `PyOS_BeforeFork()` inside the forker,
    // `PyOS_AfterFork_Parent()` / `PyOS_AfterFork_Child()` on each side —
    // because an interpreter is always live by the time a WSGI view is forked
    // (§9.3): its three calls are the matching halves of one protocol.
    let fork = forker(ForkProtocol::InterpreterLive)?;
    let fork_ms = started.elapsed().as_secs_f64() * 1000.0;

    match fork {
        Fork::Parent(pid) => Ok(ForkedView {
            pid,
            scheduler_end,
            stdout: stdout_read,
            stderr: stderr_read,
            fork_ms,
        }),
        Fork::Child => child_main(py, spawn, child, view_end),
    }
}

/// Everything the child does after `PyOS_AfterFork_Child`. Never returns.
///
/// The work after the child sequence is wrapped in `catch_unwind`: the
/// workspace cannot set `panic = "abort"` from one crate, so an unexpected
/// panic — a poisoned lock, an index that should not be out of range — is
/// turned into an exit code instead of an unwinding child that leaves a
/// half-initialised interpreter behind. A view that panics exits `70`.
fn child_main(py: Python<'_>, mut spawn: ViewSpawn<'_>, child: ChildIo, view_end: OwnedFd) -> ! {
    // The child sequence, in the order `blastit_sys::child` documents:
    // descriptors, limits, umask, working directory, then privileges, then
    // hardening, then the seccomp filter, then dumpability, then the mask —
    // which is the last thing to happen before the view starts a thread (§4,
    // §7.3).
    let plan = ChildPlan {
        name: format!("view {}", spawn.ctx.route),
        io: child,
        privileges: spawn.ctx.privileges,
        filter: spawn.ctx.filter,
        // A WSGI view is not a process-group leader: supervision kills it by
        // pid. Only the PHP probe asks for a group (`view-crates-architecture`
        // §4.4 D1).
        pgroup: false,
        mask: Some(spawn.ctx.mask),
    };
    // The fully qualified spelling is the census the design greps (R4): one
    // sequence, one name, and a fourth caller is visible as a new file.
    if let Err(message) = blastit_sys::child::enter(plan) {
        child_exit(&message, 1);
    }

    let name = spawn.ctx.route.clone();
    // The report socket travels into the view as a value, never through the
    // environment (`S3`): stage 0 strips every `BLASTIT_*` variable, and this
    // design introduces none.
    let loaded = spawn.loaded.take();
    let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve_forever(py, &spawn, view_end, loaded)
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
/// [`crate::serve_reporting`], which detaches. `loaded` is this view's report
/// socket: one `boot` batch before `READY`, then one delta per request.
fn serve_forever(
    py: Python<'_>,
    spawn: &ViewSpawn<'_>,
    view_end: OwnedFd,
    loaded: Option<blastit_view::LoadedReport>,
) {
    if let Err(err) = set_view_environment(py, &spawn.dir) {
        child_exit(&format!("view {}: HOME/TMPDIR: {err}", spawn.ctx.route), 1);
    }

    // The stream owns the descriptor from here, and `serve` closes it when it
    // is done. `From<OwnedFd>` is the safe ownership transfer: no raw number,
    // no second owner, and no `forget`.
    let chan = std::os::unix::net::UnixStream::from(view_end);
    let view_cfg = WsgiConfig {
        route_name: spawn.ctx.route.clone(),
        script_name: spawn.script_name.clone(),
        frame_limits: spawn.ctx.frame_limits,
        max_threads: Some(spawn.max_threads),
    };
    match crate::serve_reporting(py, chan, spawn.app.clone_ref(py), view_cfg, loaded) {
        Ok(()) => child_exit("", 0),
        Err(err) => child_exit(&format!("view {}: {err}", spawn.ctx.route), 1),
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

/// Formats a Python error with its traceback, for the log (§4).
///
/// init never writes anywhere but the event stream, so a failed import has to
/// become events, not a stack trace on stderr. Stage 1's `python_fail` is the
/// caller.
pub fn traceback_text(py: Python<'_>, err: &PyErr) -> String {
    match err.traceback(py) {
        Some(traceback) => {
            let header = traceback.format().unwrap_or_default();
            format!("{header}{err}")
        }
        None => err.to_string(),
    }
}
