//! Privileged process primitives for `init` (ARCHITECTURE.md §2.1, §3, §4, §7.2, §9.1, §10).
//!
//! `init` is the only process in blastit that has privileges, and the only one
//! that forks. This crate holds the small, Linux-specific primitives that job
//! needs — forking, file-descriptor hygiene, `SCM_RIGHTS` file-descriptor
//! passing, listener binding, privilege dropping, hardening, `execve`,
//! signals and reaping — each wrapped in one safe, documented and tested
//! function, so the `C1b` binary is mostly wiring.
//!
//! # Rules this crate implements
//!
//! * **Linux only.** Every function is a thin, documented wrapper around a
//!   Linux syscall; there is no fallback for other platforms.
//! * **Every descriptor this crate creates is `O_CLOEXEC`**, or gets
//!   `FD_CLOEXEC` immediately after creation. [`set_cloexec`] exists for the
//!   deliberate exceptions — `secrets.pipe`, for instance, has to survive
//!   dbproxy's `execve` (§4).
//! * **`fork` refuses to run while more than one thread is alive.** init is
//!   single-threaded at every `fork()` (§2.1); [`fork`] checks that and
//!   [`fork_unchecked`] is the documented escape hatch for the rare caller
//!   that knows better.
//! * **`unsafe` is confined to this crate.** Every `unsafe` block carries a
//!   `// SAFETY:` comment; the API on top of it is safe.
//! * **No secrets in `environ` or `argv`** (§4). [`exec`] builds `envp` from
//!   exactly the environment it is given, so an empty slice means an empty
//!   environment for the new image.
//!
//! # Errors
//!
//! Everything here returns [`std::io::Result`], because that is what the
//! callers and the kernel speak. Where an `io::Error` is too blunt, the detail
//! is a typed [`Error`] carried inside it: recover it with
//! [`Error::from_io`] (or `io::Error::get_ref().downcast_ref::<Error>()`).
//!
//! # A child's recommended sequence
//!
//! After [`fork_unchecked`], inside the child, and before anything that is not
//! async-signal-safe:
//!
//! ```text
//! SignalFd::unblock_in_child()   // restore the inherited signal mask
//! close_fds_except(&[keep])      // fd hygiene, every other fd is gone
//! harden_child(Signal::KILL)     // PR_SET_PDEATHSIG + PR_SET_NO_NEW_PRIVS
//! set_rlimits(Rlimits { .. })    // NPROC, NOFILE, CORE=0
//! set_umask(0o077)
//! set_dumpable(false)
//! drop_privileges(uid)           // setgroups, setresgid, setresuid
//! exec(exe_path(), argv, env)    // or a view's own post-fork work (§9.3)
//! ```
//!
//! [`kill`] and [`reap`] drive supervision, and [`SignalFd`] turns `SIGCHLD`,
//! `SIGTERM`, `SIGINT` and `SIGHUP` into readable events.
//!
//! # Example
//!
//! Passing the scheduler's end of a view channel back over `ctl.chan` (§3):
//!
//! ```
//! use std::os::fd::AsFd;
//! use blastit_sys::{recv_with_fd, send_with_fd, socketpair_stream};
//!
//! # fn main() -> std::io::Result<()> {
//! let (ctl_init, ctl_sched) = socketpair_stream()?;
//! let (view_sched, _view) = socketpair_stream()?;
//!
//! send_with_fd(ctl_init.as_fd(), b"SPAWNED", view_sched.as_fd())?;
//! let mut buf = [0_u8; 16];
//! let (len, fd) = recv_with_fd(ctl_sched.as_fd(), &mut buf)?;
//! assert_eq!(&buf[..len], b"SPAWNED");
//! assert!(fd.is_some());
//! # Ok(())
//! # }
//! ```

mod error;
mod exec;
mod fd;
mod pid;
mod privs;
mod proc;
mod signal;
mod sock;

#[cfg(test)]
mod testutil;

pub use crate::error::Error;
pub use crate::exec::{exe_path, exec};
pub use crate::fd::{close_fds_except, close_fds_except_scan, fd_is_cloexec, set_cloexec};
pub use crate::pid::{ExitStatus, Pid};
pub use crate::privs::{
    Rlimits, drop_privileges, dumpable, harden_child, set_dumpable, set_rlimits, set_umask,
};
pub use crate::proc::{Fork, chdir, fork, fork_unchecked, thread_count};
pub use crate::signal::{SignalFd, kill, reap};
pub use crate::sock::{
    UNIX_BACKLOG, bind_tcp, bind_unix_listener, make_private_dir, pipe, recv_with_fd, send_with_fd,
    socketpair_stream,
};

/// Signal numbers, re-exported from `rustix`.
///
/// `Signal::CHILD` is `SIGCHLD`, `Signal::TERM` is `SIGTERM`, and so on; the
/// constants are spelled without the `SIG` prefix.
pub use rustix::process::Signal;

/// The `Result` used across this crate.
pub type Result<T> = std::io::Result<T>;
