//! Privileged process primitives for `init` (ARCHITECTURE.md §2.1, §3, §4, §7.2, §9.1, §10).
//!
//! `init` is the only process in blastit that has privileges, and the only one
//! that forks. This crate holds the small, Linux-specific primitives that job
//! needs — forking, file-descriptor hygiene, `SCM_RIGHTS` file-descriptor
//! passing, listener binding, privilege dropping, hardening, seccomp filters,
//! `execve`, signals and reaping — each wrapped in one safe, documented and
//! tested function, so the `C1b` binary is mostly wiring.
//!
//! # Rules this crate implements
//!
//! * **Linux only.** Every function is a thin, documented wrapper around a
//!   Linux syscall; there is no fallback for other platforms.
//! * **Every descriptor this crate creates is `O_CLOEXEC`**, or gets
//!   `FD_CLOEXEC` immediately after creation. [`set_cloexec`] exists for the
//!   deliberate exceptions — `secrets.pipe`, for instance, has to survive
//!   dbproxy's `execve` (§4) — and [`dup_onto`] for descriptors 0, 1 and 2,
//!   which a child makes the log pipes and which have to survive its own
//!   `execve`. Those are the only descriptors in blastit without `FD_CLOEXEC`.
//! * **`fork` refuses to run while more than one thread is alive.** init is
//!   single-threaded at every `fork()` (§2.1); [`fork`] checks that and
//!   [`fork_unchecked`] is the documented escape hatch for the rare caller
//!   that knows better. The check is advisory — see [`thread_count`] — and a
//!   caller that cannot make it gets an error rather than a fork.
//! * **`unsafe` is confined to this crate.** Every `unsafe` block carries a
//!   `// SAFETY:` comment; the API on top of it is safe.
//! * **No secrets in `environ` or `argv`** (§4). [`exec`] builds `envp` from
//!   exactly the environment it is given, so an empty slice means an empty
//!   environment for the new image.
//! * **Nothing in a child needs a thread.** Every function here is safe to call
//!   in a freshly forked child of a single-threaded process; the few that
//!   allocate say so, because allocation in a child of a *multi-threaded*
//!   process can deadlock ([`exec`], for one).
//!
//! # Errors
//!
//! One rule, everywhere: a fallible function returns [`std::io::Result`] — the
//! callers and the kernel both speak `errno` — never panics, and never
//! `unwrap`s anything that came from outside this crate. When an `errno` is not
//! enough to act on, the detail is a typed [`Error`] carried *inside* the
//! `io::Error`, so there is only ever one failure shape to handle:
//!
//! ```
//! use std::os::fd::AsFd;
//!
//! use blastit_sys::{Error, send_with_fd};
//!
//! # fn main() -> std::io::Result<()> {
//! # let (sock, _peer) = blastit_sys::socketpair_stream()?;
//! # let (_read, write) = blastit_sys::pipe()?;
//! match send_with_fd(sock.as_fd(), b"", write.as_fd()) {
//!     Ok(_) => unreachable!(),
//!     Err(err) => assert!(matches!(
//!         Error::from_io(&err),
//!         Some(Error::EmptyPayload)
//!     )),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! `Error` is `#[non_exhaustive]`: callers match the cases they can act on and
//! treat the rest as the `io::Error` they already are.
//!
//! # A child's recommended sequence
//!
//! After [`fork_unchecked`], inside the child, before anything that is not
//! async-signal-safe. The order matters in two places, and both are marked:
//!
//! ```text
//! dup_onto(log_pipe?, 0)          // stdin: the log pipes, no FD_CLOEXEC
//! dup_onto(log_pipe?, 1)          // stdout
//! dup_onto(log_pipe?, 2)          // stderr
//! close_fds_except(&[keep])       // fd hygiene: every other fd is gone
//! set_rlimits(Rlimits { .. })     // NPROC, NOFILE, CORE=0
//! set_umask(0o077)
//! chdir(&scratch)                 // a view: /run/blastit/views/<uid>
//! drop_privileges(uid)            // setgroups, setresgid, setresuid (+ checks)
//! harden_child(Signal::KILL)      // ORDER: setresgid/setresuid clear PDEATHSIG
//! set_dumpable(false)
//! install(SeccompRole::Http)      // §7.3: on while still single-threaded
//! restore_signal_mask_in_child(mask)  // ORDER: exec keeps the mask, so drop it
//! exec(exe_path(), argv, env)     // or a view's own post-fork work (§9.3)
//! ```
//!
//! * `harden_child` comes **after** `drop_privileges`: `setresgid` and
//!   `setresuid` clear `PR_SET_PDEATHSIG`, so hardening first loses the death
//!   signal ([`harden_child`] has the details).
//! * [`install`] comes **before** anything that can create a thread — a view's
//!   first request, or a role's runtime: the filter cannot be removed and
//!   applies to every thread that already exists as well as every one created
//!   later (§7.3).
//! * [`restore_signal_mask_in_child`] comes **after** everything that must not
//!   be interrupted by a signal and **before** the exec: `execve` resets signal
//!   handlers but not the mask, so a child that does not restore it starts with
//!   `SIGCHLD`, `SIGTERM`, `SIGINT` and `SIGHUP` blocked and no handler.
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
mod seccomp;
mod signal;
mod sock;

#[cfg(test)]
mod testutil;

pub use crate::error::Error;
pub use crate::exec::{exe_path, exec};
pub use crate::fd::{
    close_fds_except, close_fds_except_scan, dup_onto, fd_is_cloexec, set_cloexec,
};
pub use crate::pid::{ExitStatus, Pid};
pub use crate::privs::{
    Rlimits, drop_privileges, dumpable, harden_child, set_dumpable, set_rlimits, set_umask,
};
pub use crate::proc::{Fork, chdir, fork, fork_unchecked, thread_count};
pub use crate::seccomp::{SeccompRole, install};
pub use crate::signal::{
    SignalFd, SignalMask, current_signal_mask, kill, reap, restore_signal_mask_in_child,
};
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
