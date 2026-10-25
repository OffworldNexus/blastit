//! The dbproxy *process*: the role both `blastit --role=dbproxy` and the
//! Python-free `blastit-dbproxy` binary run (§2.2, §4).
//!
//! The protocol work lives in the rest of this crate; this module is the part
//! that only makes sense for a process init has just `execve`'d: take the pipes
//! it was handed, re-apply the hardening `execve` cleared, adopt the inherited
//! listeners, report ready, and serve.
//!
//! It is in this crate, and not in `blastit`, because of the one thing a wheel
//! changes about dbproxy: it must **not** run an interpreter. dbproxy holds the
//! real credentials (§2.2), so the `blastit` crate — which links CPython for
//! every other role — cannot be where its entry point lives. Both binaries call
//! this function: the standalone one through the `--role=dbproxy` argument it
//! has always taken, and the wheel through the `blastit-dbproxy` binary it
//! ships at `blastit/_bin/blastit-dbproxy` (D2).

use std::os::fd::{BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use blastit_events::{Event, encode_json};
use blastit_sys::{SeccompRole, Signal};
use tokio::runtime::{Builder, Runtime};

use crate::read_config;

/// The credentials-holding role: read its configuration, adopt its listeners
/// and serve (§2.2).
///
/// The configuration — the real upstream credentials, the listener table and
/// the policy — arrives over `secrets.pipe` and is closed as soon as it has been
/// read (§4). dbproxy is `execve`'d with an empty environment, which clears
/// `PR_SET_PDEATHSIG` and the dumpable flag, so both are re-applied here before
/// anything else. `seccomp` is §7.3's `BLASTIT_SECCOMP`, which cannot come from
/// the environment for this one role and arrives as `--seccomp=0|1` instead —
/// both binaries take the same flag. The ready byte goes out once the listeners
/// are adopted, which is before [`crate::run_with_listeners`] checks
/// `SO_PASSCRED` and switches them to non-blocking: that work cannot fail for a
/// socket [`blastit_sys::bind_unix_listener`] made, and waiting for it would
/// mean inventing a hook the crate does not have.
///
/// Returns the process exit code.
pub fn dbproxy(secrets_fd: RawFd, events_fd: RawFd, ready_fd: RawFd, seccomp: bool) -> i32 {
    // SAFETY: the descriptor was created for dbproxy by init and is owned by it
    // from here on; nothing else refers to the number.
    let secrets = unsafe { std::fs::File::from_raw_fd(secrets_fd) };
    let Ok(config) = read_config(secrets) else {
        return 1;
    };
    let _ = blastit_sys::harden_child(Signal::KILL);
    let _ = blastit_sys::set_dumpable(false);
    // §7.3, and before the ready byte: a filter that cannot be installed has to
    // fail the boot rather than leave dbproxy serving unfiltered. The message
    // goes to descriptor 2, which is the stderr pipe init collects (§8.1).
    if seccomp && let Err(err) = blastit_sys::install(SeccompRole::DbProxy) {
        eprintln!("blastit: dbproxy: {err}");
        return 1;
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
        match crate::run_with_listeners(config, listeners, emit).await {
            Ok(()) => 0,
            Err(_) => 1,
        }
    })
}

/// An events sink that writes one JSON line per event to `fd` (§8.1).
///
/// Every crate's `Emit` is this same closure type, and the sink may be called
/// from several tasks at once: one `write` per line is what makes that safe,
/// because a line never exceeds `PIPE_BUF`.
fn events_sink(fd: RawFd) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event: Event| write_line(fd, &encode_json(&event)))
}

/// Writes one line, with its newline, to `fd` in a single `write`.
///
/// A failure is swallowed: the pipe is gone, or the process is being torn down,
/// and losing a log line must never stop the server. `EINTR` is retried.
fn write_line(fd: RawFd, line: &str) {
    // SAFETY: the caller passes the descriptor it wants written to; nothing
    // here takes ownership of it, and it stays open for the caller's lifetime.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let mut written = 0;
    while written < bytes.len() {
        match rustix::io::write(borrowed, &bytes[written..]) {
            Ok(0) => return,
            Ok(count) => written += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return,
        }
    }
}

/// A multi-threaded tokio runtime for the role.
///
/// dbproxy builds its own runtime, *after* the `fork` and the `execve`: init
/// runs no runtime at all (§2.1), so a child's runtime is always its own.
///
/// # Errors
///
/// [`std::io::Error`] when the runtime cannot be built (no memory, no timer).
fn runtime() -> std::io::Result<Runtime> {
    Builder::new_multi_thread().enable_all().build()
}
