//! The role entry points: what each forked child runs (§2.2, §2.3, §2.4).
//!
//! A role is a function that takes the descriptors init left open in the child
//! and never returns: it builds whatever runtime it needs *after* the fork
//! (§2.1 — init runs no runtime, so a child's runtime is always its own), then
//! serves until the channel closes or the process is killed.
//!
//! Returning from a role means the child failed. init treats the exit of http,
//! scheduler or dbproxy as fatal (§8), so the exit code matters only to whoever
//! reads the log: it is `0` when the role returned cleanly.

use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use blastit_events::{Event, encode_json};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::RoutingTable;
use blastit_scheduler::{RouteInfo, SchedulerConfig, UnixCtl};
use blastit_sys::{SeccompRole, Signal};
use tokio::runtime::{Builder, Runtime};

use crate::child::{child_exit, install_filter};
use crate::logs::write_line;

/// The descriptors the http role owns.
#[derive(Debug, Clone, Copy)]
pub struct HttpFds {
    /// The listening TCP socket init bound.
    pub listener: RawFd,
    /// http's end of `http.chan`.
    pub chan: RawFd,
    /// The write end of http's events pipe.
    pub events: RawFd,
}

/// The descriptors the scheduler role owns.
#[derive(Debug, Clone, Copy)]
pub struct SchedulerFds {
    /// The scheduler's end of `http.chan`.
    pub chan: RawFd,
    /// The scheduler's end of `ctl.chan`.
    pub ctl: RawFd,
    /// The write end of the scheduler's events pipe.
    pub events: RawFd,
}

/// An events sink that writes one JSON line per event to `fd` (§8.1).
///
/// Every crate's `Emit` is this same closure type, and the sink may be called
/// from several tasks at once: one `write` per line is what makes that safe,
/// because a line never exceeds `PIPE_BUF`.
pub fn events_sink(fd: RawFd) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event: Event| write_line(fd, &encode_json(&event)))
}

/// A multi-threaded tokio runtime for a role that needs one.
///
/// # Errors
///
/// [`std::io::Error`] when the runtime cannot be built (no memory, no timer).
pub fn runtime() -> io::Result<Runtime> {
    Builder::new_multi_thread().enable_all().build()
}

/// The http role: serve the inherited listener until `http.chan` fails (§2.3).
///
/// Returns the process exit code: `0` when `serve` returned cleanly (which only
/// happens when the scheduler's end closed, so init is going away), `1`
/// otherwise.
pub fn http(fds: HttpFds, cfg: HttpConfig) -> i32 {
    let emit = events_sink(fds.events);
    let Ok(runtime) = runtime() else {
        return 1;
    };
    runtime.block_on(async move {
        let Ok(listener) = tcp_listener(fds.listener) else {
            return 1;
        };
        let Ok(chan) = unix_stream(fds.chan) else {
            return 1;
        };
        match blastit_http::serve(listener, chan, cfg, emit).await {
            Ok(()) => 0,
            Err(_) => 1,
        }
    })
}

/// The scheduler role: route and admit requests until `ctl.chan` or `http.chan`
/// fails (§2.4).
pub fn scheduler(
    fds: SchedulerFds,
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    limits: Limits,
) -> i32 {
    let emit = events_sink(fds.events);
    let Ok(runtime) = runtime() else {
        return 1;
    };
    runtime.block_on(async move {
        let Ok(http) = unix_stream(fds.chan) else {
            return 1;
        };
        let Ok(ctl) = unix_stream(fds.ctl) else {
            return 1;
        };
        let ctl = UnixCtl::with_limits(ctl, limits);
        match blastit_scheduler::run(cfg, routes, table, http, ctl, emit).await {
            Ok(()) => 0,
            Err(_) => 1,
        }
    })
}

/// The dbproxy role: read its configuration, adopt its listeners and serve
/// (§2.2).
///
/// The configuration — the real upstream credentials, the listener table and
/// the policy — arrives over `secrets.pipe` and is closed as soon as it has been
/// read (§4). dbproxy is `execve`'d with an empty environment, which clears
/// `PR_SET_PDEATHSIG` and the dumpable flag, so both are re-applied here before
/// anything else. `seccomp` is §7.3's `BLASTIT_SECCOMP`, which cannot come from
/// the environment for this one role and arrives as `--seccomp=0|1` instead.
/// The ready byte goes out once the listeners are adopted, which is before
/// `run_with_listeners` checks `SO_PASSCRED` and switches them to non-blocking:
/// that work cannot fail for a socket [`blastit_sys::bind_unix_listener`] made,
/// and waiting for it would mean inventing a hook the crate does not have.
///
/// Returns the process exit code.
pub fn dbproxy(secrets_fd: RawFd, events_fd: RawFd, ready_fd: RawFd, seccomp: bool) -> i32 {
    // SAFETY: the descriptor was created for dbproxy by init and is owned by it
    // from here on; nothing else refers to the number.
    let secrets = unsafe { std::fs::File::from_raw_fd(secrets_fd) };
    let Ok(config) = blastit_dbproxy::read_config(secrets) else {
        return 1;
    };
    let _ = blastit_sys::harden_child(Signal::KILL);
    let _ = blastit_sys::set_dumpable(false);
    // §7.3, and before the ready byte: a filter that cannot be installed has to
    // fail the boot rather than leave dbproxy serving unfiltered.
    if let Err(err) = install_filter(seccomp, SeccompRole::DbProxy) {
        child_exit(&format!("dbproxy: {err}"), 1);
    }

    let mut listeners = Vec::with_capacity(config.listeners.len());
    for spec in &config.listeners {
        // SAFETY: init bound each of these sockets and passed the descriptor
        // across dbproxy's `execve`; dbproxy takes ownership of it.
        let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(spec.fd) };
        listeners.push((listener, spec.clone()));
    }

    // SAFETY: `ready_fd` is dbproxy's own end of the ready pipe.
    let ready = unsafe { OwnedFd::from_raw_fd(ready_fd) };
    let _ = rustix::io::write(&ready, b"1");
    drop(ready);

    let emit = events_sink(events_fd);
    let Ok(runtime) = runtime() else {
        return 1;
    };
    runtime.block_on(async move {
        match blastit_dbproxy::run_with_listeners(config, listeners, emit).await {
            Ok(()) => 0,
            Err(_) => 1,
        }
    })
}

/// Adopts an inherited TCP listener, in non-blocking mode (§2.3).
fn tcp_listener(fd: RawFd) -> io::Result<tokio::net::TcpListener> {
    // SAFETY: the descriptor was inherited across the fork for exactly this
    // purpose; this process is the only owner.
    let listener = unsafe { std::net::TcpListener::from_raw_fd(fd) };
    listener.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(listener)
}

/// Adopts an inherited `AF_UNIX` stream, in non-blocking mode.
fn unix_stream(fd: RawFd) -> io::Result<tokio::net::UnixStream> {
    // SAFETY: the descriptor was inherited across the fork for exactly this
    // purpose; this process is the only owner.
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(stream)
}
