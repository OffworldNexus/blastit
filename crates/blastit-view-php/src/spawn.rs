//! Forking a PHP view process (`ADR-0010` §1, design §4.3, §4.4 D1/D2).
//!
//! A PHP route's worker is an ordinary **view** worker: one unprivileged process
//! per slot, a private runtime directory holding only the proxy sockets the
//! route was granted, `max_requests` recycling. The only thing that differs is
//! the [`Filter`](blastit_sys::child::Filter) — [`blastit_sys::SeccompRole::EmbeddedView`],
//! which also refuses `execve` and the process-creation calls `libphp` must
//! never make — and the runtime the child enters.
//!
//! # What this module is told
//!
//! [`SpawnContext`] is the composition root's boot data (route, uid, frame
//! limits, `RLIMIT_*`, the identity and filter stage 1 decided, egress, the
//! saved signal mask); the rest is what only a PHP worker has: the binding the
//! boot verified and, in `dev`, its loaded-file report socket. There is **one**
//! directory field — [`PhpBinding::view_tmp`], which is the child's `chdir`,
//! `HOME`, `TMPDIR` and PHP's `sys_temp_dir` — so they can never disagree (S6).
//!
//! The fork syscall is **injected** ([`Forker`]): the composition root owns the
//! one implementation of CPython's three-hook protocol, and this crate asks for
//! the protocol its image needs instead of calling `fork(2)` (§9.3).
//!
//! # The child sequence, in order
//!
//! [`blastit_sys::child::enter`] runs descriptors/limits/umask/cwd, then
//! privileges, then hardening, then the filter, then dumpability, then the saved
//! signal mask — one sequence, spelled once, for the WSGI fork and both PHP
//! forks. `HOME`/`TMPDIR` are set **after** it (invisible: both are after the
//! privilege drop), and only then does the child load: a load failure is
//! `exit 1` with a message naming the file, never a first-request fault. Only
//! then does the child send `READY`.
//!
//! # Step 0: nothing loads or opens before `enter`
//!
//! The protocol's child half (`PyOS_AfterFork_Child()` and any handler an
//! extension registered) runs privileged and unfiltered, before
//! [`blastit_sys::child::enter`]. Nothing here `dlopen`s, opens a file or mutates
//! the environment between the fork and `enter`: the socketpair and the two
//! pipes are created by the parent *before* the fork.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
use std::time::Instant;

use blastit_php_sapi::SharedObject;
use blastit_sys::child::{ChildIo, ChildPlan, child_exit, panic_message, set_env};
use blastit_sys::{Fork, Pid};
use blastit_view::spawn::{ForkProtocol, Forker, SpawnContext};

use crate::pinning::PhpBinding;
use blastit_view::loaded::LoadedReport;

use crate::worker::{PhpEmbedRuntime, PhpRuntimeConfig, serve_forever};

/// How many extra processes a view's uid may have beyond its workers (§7.2:
/// gVisor leaves out the main thread, so one unit of margin).
///
/// Stage 1 adds this to the worker cap when it builds a child's
/// `RLIMIT_NPROC`, exactly as it does for the WSGI view.
pub const VIEW_PROCESS_MARGIN: u32 = 16;

/// How many descriptors a PHP view may hold open (its channel, stdio, the
/// granted proxy sockets, `libphp` and its extensions).
pub const VIEW_NOFILE: u64 = 1024;

/// Everything the fork needs about the route it is forking: the composition
/// root's boot data, the binding the boot verified, and the protocol this image
/// needs.
pub struct PhpViewSpawn<'a> {
    /// The route's boot data, copied from stage 1's per-slot decision.
    pub ctx: SpawnContext,
    /// What the child loads and runs, and the directory whose INI pins
    /// `sys_temp_dir` — the same value the fork uses as `chdir`.
    pub binding: &'a PhpBinding,
    /// The fork protocol this image needs; the forker refuses a disagreement.
    pub protocol: ForkProtocol,
    /// The dev-only loaded-file report (`MB.23.4.2`, `MB.23.5.3`): the write end
    /// of this view's own report socketpair, created by the supervisor before the
    /// fork and moved into `ChildIo.keep` here. `None` is production,
    /// `--no-reload` and every host without a usable shim sink.
    ///
    /// It is the transport's [`LoadedReport`], the same value the WSGI fork
    /// takes: `MB.36.5` deleted this crate's copy of the type, so there is one
    /// report wire for every backend (design §4.2.1).
    pub loaded: Option<LoadedReport>,
}

/// A freshly forked PHP view, as the parent sees it.
pub struct ForkedPhpView {
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

/// Forks a PHP view process for `spawn`, or returns the error and leaves the
/// process as it was.
///
/// `forker` is the composition root's fork: it runs CPython's three hooks under
/// the GIL when an interpreter is live and refuses a protocol that disagrees
/// with the image, so a silent bare `fork(2)` under a live interpreter is an
/// error rather than a corrupt child.
///
/// The configuration is validated **before** the fork, so a bad entry or an
/// empty INI is an error the parent can log rather than a child that dies on
/// its own.
///
/// # Errors
///
/// [`std::io::Error`] when a socketpair or pipe cannot be created, whatever the
/// forker refuses with, or a route configuration the worker could not serve.
pub fn fork_php_view(
    spawn: PhpViewSpawn<'_>,
    forker: Forker<'_>,
) -> std::io::Result<ForkedPhpView> {
    let config = runtime_config(&spawn)?;
    let (view_end, scheduler_end) = blastit_sys::socketpair_stream()?;
    let (stdout_read, stdout_write) = blastit_sys::pipe()?;
    let (stderr_read, stderr_write) = blastit_sys::pipe()?;

    // The pinned `libphp`/shim descriptors must survive the child sequence:
    // `dlopen` names them `/proc/self/fd/<n>`, so `close_fds_except` closing
    // them would make the load fail with `ENOENT` in production.
    let mut keep = vec![view_end.as_raw_fd()];
    keep.extend(spawn.binding.pinned_fds());
    // The dev-only loaded-file report's write end (`MB.23.5.3`): an inherited
    // descriptor the worker's shim sink sends on, exactly like `view.chan` — no
    // environment variable, so nothing for the app to see or for stage 0 to
    // strip.
    if let Some(report) = &spawn.loaded {
        keep.push(report.socket.as_raw_fd());
    }
    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: spawn.ctx.rlimits,
        working_dir: Some(spawn.binding.view_tmp.clone()),
    };

    let started = Instant::now();
    let fork = forker(spawn.protocol)?;
    let fork_ms = started.elapsed().as_secs_f64() * 1000.0;

    match fork {
        Fork::Parent(pid) => Ok(ForkedPhpView {
            pid,
            scheduler_end,
            stdout: stdout_read,
            stderr: stderr_read,
            fork_ms,
        }),
        Fork::Child => child_main(spawn, config, child, view_end),
    }
}

/// The runtime configuration for `spawn`, validated in the parent.
fn runtime_config(spawn: &PhpViewSpawn<'_>) -> std::io::Result<PhpRuntimeConfig> {
    let config = PhpRuntimeConfig::new(
        spawn.binding,
        spawn.binding.script_name.clone(),
        spawn.binding.entry_pin,
    );
    config.validate().map_err(std::io::Error::other)?;
    Ok(config)
}

/// Everything the child does after the fork. Never returns.
///
/// The child sequence is [`blastit_sys::child::enter`] — one order, spelled
/// once, with [`SeccompRole::EmbeddedView`](blastit_sys::SeccompRole) in the
/// filter stage 1 decided: `execute`-family syscalls and process creation are
/// refused, because `libphp` must never make them and no script may reach them
/// even through an extension.
fn child_main(
    spawn: PhpViewSpawn<'_>,
    config: PhpRuntimeConfig,
    child: ChildIo,
    view_end: OwnedFd,
) -> ! {
    let plan = ChildPlan {
        name: format!("view {}", spawn.ctx.route),
        io: child,
        privileges: spawn.ctx.privileges,
        filter: spawn.ctx.filter,
        // A PHP worker is not a process-group leader: supervision kills it by
        // pid. Only the probe asks for a group (§4.4 D1).
        pgroup: false,
        mask: Some(spawn.ctx.mask),
    };
    // The fully qualified spelling is the census the design greps (R4).
    if let Err(message) = blastit_sys::child::enter(plan) {
        child_exit(&message, 1);
    }
    if let Err(err) = set_view_environment(&spawn.binding.view_tmp) {
        child_exit(&format!("view {}: HOME/TMPDIR: {err}", spawn.ctx.route), 1);
    }

    let name = spawn.ctx.route.clone();
    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve(spawn, config, view_end)
    }));
    match loaded {
        Ok(Ok(())) => child_exit("", 0),
        Ok(Err(message)) => child_exit(&format!("view {name}: {message}"), 1),
        Err(payload) => child_exit(
            &format!("view {name}: panicked: {}", panic_message(&payload)),
            70,
        ),
    }
}

/// Loads `libphp` and serves until `max_requests`, a fatal bailout, or the
/// channel's end. The interpreter, once started, is never shut down here: the
/// process exits, which is what a worker whose module state is suspect must do.
fn serve(
    spawn: PhpViewSpawn<'_>,
    config: PhpRuntimeConfig,
    view_end: OwnedFd,
) -> Result<(), String> {
    let runtime = PhpEmbedRuntime::load_reporting(
        SharedObject::Fd(spawn.binding.libphp.as_raw_fd()),
        SharedObject::Fd(spawn.binding.shim.as_raw_fd()),
        config,
        // `MB.23.4.2`'s dev-only loaded-file report, wired by `MB.23.5.3`:
        // `None` is production and `--no-reload` (no socket, no reader, and not
        // one byte of extra work per request). A shim without the optional sink
        // symbol keeps that `None` behaviour inside `load_reporting`.
        spawn.loaded,
    )
    .map_err(|err| format!("{err}"))?;
    // [`blastit_sys::child::enter`] has already run, and the runtime was built
    // with the entry's record (`ADR-0010` §3 / `MB.3.56`): every request
    // re-checks the resolved entry against it before `libphp` opens
    // `SCRIPT_FILENAME`.
    //
    // `From<OwnedFd>` is the safe ownership transfer: the stream owns the
    // descriptor from here, `serve_forever` closes it when it is done, and no
    // raw number can be closed twice.
    let chan = std::os::unix::net::UnixStream::from(view_end);
    serve_forever(chan, runtime, spawn.ctx.frame_limits).map_err(|err| format!("{err}"))
}

/// Pins the child's `HOME` and `TMPDIR` at its private scratch directory, the
/// way the WSGI view does: a PHP application that writes to `sys_get_temp_dir()`,
/// `TMPDIR` or `$HOME` must land inside the view's own `0700` directory and
/// nowhere else.
///
/// [`set_env`] is the safe spelling of `setenv(3)`: it refuses a process with
/// more than one thread instead of assuming the caller is the only one, and a
/// forked child before its first thread never is.
///
/// # Errors
///
/// [`std::io::Error`] when the directory is not a UTF-8 path (an environment
/// value is a C string, and this crate spells that safely), when it holds a NUL
/// byte, or when `setenv(3)` fails.
pub(crate) fn set_view_environment(dir: &Path) -> std::io::Result<()> {
    let value = dir.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a UTF-8 path", dir.display()),
        )
    })?;
    set_env("HOME", value)?;
    set_env("TMPDIR", value)
}
