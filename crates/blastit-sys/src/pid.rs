//! Process ids and exit statuses.

use std::fmt;

use crate::error::Error;

/// A process id.
///
/// The kernel never reports zero or a negative pid for a live process, so
/// [`Pid`] can only be built from a positive value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(i32);

impl Pid {
    /// Wrap a raw `pid_t`, refusing the values the kernel never produces.
    pub const fn from_raw(raw: i32) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    /// The raw `pid_t`.
    pub const fn as_raw(self) -> i32 {
        self.0
    }

    /// The pid of the calling process.
    pub fn current() -> Self {
        // SAFETY: `getpid` takes no arguments, cannot fail and always returns
        // the positive pid of the calling process.
        Self(unsafe { libc::getpid() })
    }
}

impl fmt::Debug for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pid({})", self.0)
    }
}

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A raw `waitpid` status the kernel never produces.
pub(crate) fn impossible(call: &'static str, value: i64) -> std::io::Error {
    Error::Impossible { call, value }.into()
}

/// How a child process ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExitStatus {
    /// The child exited normally, with this exit code.
    Code(i32),
    /// The child was killed by this signal number.
    Signal(i32),
}

impl ExitStatus {
    /// Decode a raw `waitpid` status word.
    ///
    /// Returns `None` for a child that was stopped or continued rather than
    /// ended; nothing in this crate asks the kernel for those, so a caller
    /// seeing `None` should ignore the entry.
    pub fn from_raw(raw: i32) -> Option<Self> {
        if libc::WIFEXITED(raw) {
            Some(Self::Code(libc::WEXITSTATUS(raw)))
        } else if libc::WIFSIGNALED(raw) {
            Some(Self::Signal(libc::WTERMSIG(raw)))
        } else {
            None
        }
    }

    /// The exit code, when the child exited normally.
    pub fn code(self) -> Option<i32> {
        match self {
            Self::Code(code) => Some(code),
            Self::Signal(_) => None,
        }
    }

    /// The signal, when the child was killed by one and it has a name.
    pub fn signal(self) -> Option<crate::Signal> {
        match self {
            Self::Code(_) => None,
            Self::Signal(signo) => crate::Signal::from_named_raw(signo),
        }
    }

    /// The raw signal number, when the child was killed by a signal.
    pub fn raw_signal(self) -> Option<i32> {
        match self {
            Self::Code(_) => None,
            Self::Signal(signo) => Some(signo),
        }
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Code(code) => write!(f, "exit code {code}"),
            Self::Signal(signo) => write!(f, "killed by signal {signo}"),
        }
    }
}
