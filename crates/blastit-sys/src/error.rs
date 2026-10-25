//! Typed detail behind the `io::Error`s this crate returns.

use std::io;
use std::path::PathBuf;

/// What went wrong, when an [`io::Error`] is not enough.
///
/// Every fallible function in this crate returns `io::Result`, so a failure
/// that carries more than an `errno` is stored inside the `io::Error` as its
/// source. [`Error::from_io`] gets it back.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// [`crate::fork`] was called while the process had more than one thread.
    #[error("refusing to fork: {threads} threads are alive, init must be single-threaded")]
    NotSingleThreaded {
        /// How many threads the process had, as reported by `/proc/self/status`.
        threads: usize,
    },

    /// A step of [`crate::drop_privileges`] failed.
    #[error("dropping privileges failed at `{stage}`: {source}")]
    PrivilegeDrop {
        /// The call that failed.
        stage: &'static str,
        /// The `errno` it reported.
        #[source]
        source: io::Error,
    },

    /// A step of [`crate::drop_privileges`] succeeded, but the process did not
    /// end up in the state it must be in.
    #[error("the check after `{stage}` failed: {detail}")]
    PrivilegeCheck {
        /// The call whose effect was checked.
        stage: &'static str,
        /// What was wrong.
        detail: &'static str,
    },

    /// A `recvmsg` carried more than the one descriptor the caller asked for.
    ///
    /// The extra descriptors are closed before this is returned.
    #[error("received {count} file descriptors over SCM_RIGHTS, expected at most one")]
    TooManyFds {
        /// How many descriptors arrived.
        count: usize,
    },

    /// [`crate::send_with_fd`] was called with nothing to send.
    #[error(
        "send_with_fd needs at least one byte: a control message attached to a zero-length write may be dropped"
    )]
    EmptyPayload,

    /// A path that must hold a socket holds something else.
    #[error("{}: exists, but is not a socket", path.display())]
    NotASocket {
        /// The offending path.
        path: PathBuf,
    },

    /// The control message did not fit in the receive buffer, so the set of
    /// received descriptors is unknown; whatever the kernel did copy is closed.
    #[error("control message was truncated; the received file descriptor set is unknown")]
    ControlMessageTruncated,

    /// A path that must be a real directory is something else — most likely a
    /// symlink, which is how a stale path could point somewhere else entirely.
    #[error("{}: exists, but is not a directory this process created", path.display())]
    NotADirectory {
        /// The offending path.
        path: PathBuf,
    },

    /// An entry of an environment block cannot be handed to `execve`.
    #[error("environment entry {index} is invalid: {reason}")]
    InvalidEnv {
        /// Position in the slice that was passed to [`crate::exec`].
        index: usize,
        /// What is wrong with it.
        reason: &'static str,
    },

    /// A syscall returned a value the kernel does not produce.
    #[error("{call} returned an impossible value: {value}")]
    Impossible {
        /// The call that surprised us.
        call: &'static str,
        /// The value it returned.
        value: i64,
    },

    /// The thread count could not be read, so the single-threaded invariant
    /// cannot be checked before a fork.
    #[error("cannot count the threads of this process: {detail}")]
    ThreadCount {
        /// What `/proc/self/status` looked like instead.
        detail: &'static str,
    },

    /// A step of [`crate::install`] failed.
    #[error("the seccomp filter failed at `{stage}`: {source}")]
    Seccomp {
        /// The call that failed.
        stage: &'static str,
        /// The `errno` it reported.
        #[source]
        source: io::Error,
    },

    /// [`crate::install`] was called from a process that had more than one
    /// thread (ARCHITECTURE.md §7.3).
    #[error(
        "refusing to install a seccomp filter: {threads} threads are alive, and a filter applies to every thread that already exists"
    )]
    NotSingleThreadedSeccomp {
        /// How many threads the process had, as reported by `/proc/self/status`.
        threads: usize,
    },

    /// This platform has no `AUDIT_ARCH_*` value to check a filter's
    /// `seccomp_data.arch` against (ARCHITECTURE.md §7.3).
    #[error("{arch} has no AUDIT_ARCH value, so no seccomp filter can be installed")]
    UnsupportedArchitecture {
        /// The architecture, from `std::env::consts::ARCH`.
        arch: &'static str,
    },
}

impl Error {
    /// Recover the typed error from an [`io::Error`], if there is one.
    pub fn from_io(err: &io::Error) -> Option<&Self> {
        err.get_ref()?.downcast_ref::<Self>()
    }
}

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        io::Error::other(err)
    }
}
