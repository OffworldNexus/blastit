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

use std::cell::RefCell;
use std::io;
use std::os::fd::{FromRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use blastit_events::{Event, MAX_EVENT_BYTES, encode_json_into};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::RoutingTable;
use blastit_scheduler::{RouteInfo, SchedulerConfig, UnixCtl};
use tokio::runtime::{Builder, Runtime};

use crate::logs::write_bytes;

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

// One reusable line buffer per thread, for `events_sink`.
//
// The sink is called once per event and may run on any of a role's worker
// threads, so a buffer per thread takes the line allocation out of every
// request without a lock. It is borrowed for exactly one encode and write, and
// encoding never emits an event, so a thread cannot re-enter its own borrow.
// The capacity is one line plus its newline, both of which the encoder
// guarantees stay within `MAX_EVENT_BYTES`.
thread_local! {
    static EVENTS_LINE: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(MAX_EVENT_BYTES + 1));
}

/// An events sink that writes one JSON line per event to `fd` (§8.1).
///
/// Every crate's `Emit` is this same closure type, and the sink may be called
/// from several tasks at once: one `write` per line is what makes that safe,
/// because a line never exceeds `PIPE_BUF`. The line is built in a reused
/// per-thread buffer, so emitting an event allocates nothing once the buffer
/// has grown.
///
/// init is the only reader, and a closed events pipe never reopens, so once a
/// write fails there is no one left to consume an event: the sink stops
/// encoding them. Nothing an operator sees changes — a closed pipe produces no
/// line either way — and a server being torn down stops spending a request's
/// worth of work on lines nobody will read.
pub fn events_sink(fd: RawFd) -> Arc<dyn Fn(Event) + Send + Sync> {
    let alive = AtomicBool::new(true);
    Arc::new(move |event: Event| {
        if !alive.load(Ordering::Relaxed) {
            return;
        }
        EVENTS_LINE.with(|line| {
            let mut line = line.borrow_mut();
            line.clear();
            encode_json_into(&mut line, &event);
            line.push(b'\n');
            if !write_bytes(fd, &line) {
                alive.store(false, Ordering::Relaxed);
            }
        });
    })
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
/// dbproxy holds the real credentials, so it runs no interpreter — not even
/// this one (§2.2, §4). The role therefore lives in the crate that does not
/// link CPython, [`blastit_dbproxy::role`], and both this binary and the
/// Python-free `blastit-dbproxy` a wheel installs call the same function
/// (§4, [`crate::cli`]).
pub use blastit_dbproxy::role::dbproxy;

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
