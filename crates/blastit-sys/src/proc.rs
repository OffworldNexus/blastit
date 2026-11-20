//! Forking, thread counting and `chdir`.

use std::io;
use std::path::Path;

use crate::error::Error;
use crate::pid::{Pid, impossible};

/// How many threads the calling process has, from `/proc/self/status`.
///
/// [`fork`] uses this to enforce init's invariant: it is single-threaded at
/// every `fork()` (ARCHITECTURE.md §2.1). A thread that exists for any reason —
/// a tokio runtime, a BLAS pool, a `std::thread::spawn` — makes the check fail.
///
/// **The check is advisory.** It reads a file and then forks, so a thread born
/// in between is not caught, and `/proc` is the only place the number comes
/// from. What it catches is the mistake the invariant is usually broken by —
/// code that starts a runtime, or spawns a thread, and forks afterwards — not a
/// deliberate race. When the count cannot be read at all the answer is
/// [`Error::ThreadCount`], and the C1b binary treats that as fatal: if the
/// invariant cannot be *checked*, init does not fork.
///
/// **It allocates nothing.** The file is read with raw `open`/`read`/`close`
/// into a fixed stack buffer: [`fork`] and [`crate::install`] call this from
/// inside a child of `fork()`, where an allocation can wait forever on a lock
/// another thread held at the instant of the fork.
pub fn thread_count() -> io::Result<usize> {
    // SAFETY: the path is a NUL-terminated literal and neither flag takes an
    // argument.
    let fd = unsafe {
        libc::open(
            c"/proc/self/status".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    let found = scan_threads(fd);
    // SAFETY: `fd` was opened above and nothing else in this process owns it.
    unsafe { libc::close(fd) };
    match found? {
        Some(threads) => Ok(threads),
        None => Err(Error::ThreadCount {
            detail: "/proc/self/status has no Threads: line",
        }
        .into()),
    }
}

/// The `Threads:` value of the `/proc/self/status` behind `fd`, or `None` when
/// the file has no such line.
///
/// Reads in fixed-size chunks and carries its state between them, so the field
/// may straddle a read boundary. Every step is a syscall or arithmetic on this
/// frame — [`thread_count`] runs in a child of `fork()`.
fn scan_threads(fd: libc::c_int) -> io::Result<Option<usize>> {
    /// The field this scan looks for.
    const FIELD: &[u8] = b"Threads:";
    /// How much of the file is read at a time.
    const CHUNK: usize = 512;

    /// What the scan is waiting for next.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        /// Matching the `Threads:` prefix.
        Field,
        /// Past the colon, before the first digit (a tab, usually).
        Value,
        /// Reading the digits.
        Digits,
    }

    let mut buffer = [0_u8; CHUNK];
    let mut phase = Phase::Field;
    let mut matched = 0;
    let mut value = 0_usize;
    loop {
        // SAFETY: `buffer` is `CHUNK` bytes of writable storage, which `read`
        // fills at most completely.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast::<libc::c_void>(), CHUNK) };
        if read == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }
        if read == 0 {
            break;
        }
        for &byte in &buffer[..read as usize] {
            match phase {
                Phase::Field => {
                    if byte == FIELD[matched] {
                        matched += 1;
                        if matched == FIELD.len() {
                            phase = Phase::Value;
                        }
                    } else {
                        // Restart, keeping a first byte that could begin again.
                        matched = usize::from(byte == FIELD[0]);
                    }
                }
                Phase::Value => {
                    if byte.is_ascii_digit() {
                        value = usize::from(byte - b'0');
                        phase = Phase::Digits;
                    } else if byte == b'\n' {
                        return Err(not_a_number());
                    }
                }
                Phase::Digits => {
                    if byte.is_ascii_digit() {
                        value = value
                            .checked_mul(10)
                            .and_then(|current| current.checked_add(usize::from(byte - b'0')))
                            .ok_or_else(not_a_number)?;
                    } else {
                        return Ok(Some(value));
                    }
                }
            }
        }
    }
    match phase {
        Phase::Digits => Ok(Some(value)),
        Phase::Value => Err(not_a_number()),
        Phase::Field => Ok(None),
    }
}

/// The `Threads:` line was there but held no number.
fn not_a_number() -> io::Error {
    Error::ThreadCount {
        detail: "the Threads: line of /proc/self/status is not a number",
    }
    .into()
}

/// The result of a successful [`fork`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fork {
    /// Running in the parent: the pid of the new child.
    Parent(Pid),
    /// Running in the child: everything is a copy of the parent, except the pid.
    Child,
}

impl Fork {
    /// The child's pid, when this is the parent's side of a fork.
    pub fn pid(self) -> Option<Pid> {
        match self {
            Self::Parent(pid) => Some(pid),
            Self::Child => None,
        }
    }

    /// Whether this is the child's side of a fork.
    pub fn is_child(self) -> bool {
        matches!(self, Self::Child)
    }
}

/// `fork()`, refusing to run when more than one thread is alive.
///
/// A `fork` in a multi-threaded process copies only the calling thread, so
/// every lock another thread held at that moment stays locked forever in the
/// child; init must therefore be single-threaded whenever it forks (§2.1).
/// This function checks [`thread_count`] first and fails with
/// [`Error::NotSingleThreaded`] instead of forking.
///
/// On success the child gets `Fork::Child` and the parent gets
/// `Fork::Parent(pid)`. The child must only do async-signal-safe work until it
/// has replaced its address space or restored the state it needs; see the
/// crate-level sequence.
pub fn fork() -> io::Result<Fork> {
    let threads = thread_count()?;
    if threads != 1 {
        return Err(Error::NotSingleThreaded { threads }.into());
    }
    // SAFETY: `thread_count` just told us that this process has exactly one
    // thread, and the caller is bound by the contract of `fork` in this
    // crate: only async-signal-safe work happens in the child until it
    // execs or returns to the parent's discipline.
    unsafe { fork_unchecked() }
}

/// `fork()`, without the single-threaded check.
///
/// # Safety
///
/// The caller must be sure that forking is safe here. In a process with other
/// threads, the child inherits locked mutexes, half-updated allocator state and
/// no other thread to release them: any non-async-signal-safe call in the child
/// (including allocation in Rust's and glibc's standard library) can deadlock,
/// corrupt memory or read a half-written value. The child must therefore limit
/// itself to async-signal-safe calls until it `execve`s or `_exit`s.
///
/// This exists for the things init does before it is sure — tests that fork a
/// helper process, for instance — and is what [`fork`] uses internally once it
/// has checked the thread count.
pub unsafe fn fork_unchecked() -> io::Result<Fork> {
    // SAFETY: the caller accepts the fork contract documented above; `fork`
    // takes no arguments and touches no memory.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(Fork::Child),
        raw => match Pid::from_raw(raw) {
            Some(pid) => Ok(Fork::Parent(pid)),
            None => Err(impossible("fork", i64::from(raw))),
        },
    }
}

/// Change the working directory of the calling process.
///
/// A view's working directory is its own socket directory, which is what makes
/// `connect("/proc/self/cwd/.s.PGSQL.5432")` work (§2.2).
pub fn chdir(path: &Path) -> io::Result<()> {
    rustix::process::chdir(path).map_err(Into::into)
}
