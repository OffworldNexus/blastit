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
/// another thread held at the instant of the fork (`crate::testutil`'s
/// `child_status` has the long version).
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

#[cfg(test)]
mod tests {
    use std::os::fd::{AsFd, AsRawFd};
    use std::sync::mpsc;

    use super::*;
    use crate::error::Error;
    use crate::pid::ExitStatus;
    use crate::testutil::{
        PROBE_VAR, TempDir, child_ok, in_child, must, must_err, report_probe, run_probe,
        spawn_exiting, wait_for,
    };

    /// The test a probe process matches, so only this module's runner starts.
    const PROBE_RUNNER: &str = "proc::tests::probe_runner";

    #[test]
    fn thread_count_reports_at_least_the_calling_thread() {
        let threads = must(thread_count());
        assert!(threads >= 1, "a process has at least one thread: {threads}");
    }

    #[test]
    fn thread_count_reads_a_status_file() {
        // The real file is parsed by every test that forks; this pins the
        // scanner itself, including a field that straddles a read chunk.
        let mut status = Vec::new();
        status.extend_from_slice(b"Name:\tblastit-sys\nState:\tS (sleeping)\n");
        status.extend_from_slice(&[b'.'; 700]);
        status.extend_from_slice(b"\nThreads:\t7\n");
        assert_eq!(must(scan_pipe(&status)), Some(7));
    }

    #[test]
    fn thread_count_reports_a_missing_field() {
        assert_eq!(
            must(scan_pipe(b"Name:\tblastit-sys\nTgid:\t42\n")),
            None,
            "a status file without the field has no count"
        );
    }

    #[test]
    fn thread_count_reports_a_line_without_a_number() {
        let err = must_err(scan_pipe(b"Threads:\n"));
        assert!(
            matches!(Error::from_io(&err), Some(Error::ThreadCount { .. })),
            "got {err}"
        );
    }

    /// Run `scan_threads` over `bytes` through a pipe.
    fn scan_pipe(bytes: &[u8]) -> io::Result<Option<usize>> {
        let (read_end, write_end) = must(crate::pipe());
        crate::testutil::write_all(write_end.as_fd(), bytes);
        drop(write_end);
        scan_threads(read_end.as_raw_fd())
    }

    #[test]
    fn fork_refuses_when_a_second_thread_is_alive() {
        let (release, hold) = mpsc::channel::<()>();
        let (started, ready) = mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let _ = started.send(());
            // Park until the test is done: this thread only has to exist.
            let _ = hold.recv();
        });
        must(ready.recv());

        let outcome = fork();
        let _ = release.send(());
        let _ = handle.join();

        match outcome {
            Ok(fork) => panic!("fork() must refuse while another thread is alive, got {fork:?}"),
            Err(err) => match Error::from_io(&err) {
                Some(Error::NotSingleThreaded { threads }) => {
                    assert!(*threads >= 2, "the check saw {threads} threads");
                }
                other => panic!("expected NotSingleThreaded, got {other:?} ({err})"),
            },
        }
    }

    #[test]
    fn fork_unchecked_makes_a_child_that_is_not_the_parent() {
        let parent = Pid::current();
        // The body is async-signal-safe: `getpid` and `getppid` are syscalls.
        let code = in_child(move || {
            if Pid::current() == parent {
                return 1;
            }
            // SAFETY: `getppid` takes no arguments and cannot fail.
            if unsafe { libc::getppid() } != parent.as_raw() {
                return 2;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn fork_unchecked_reports_the_child_pid_to_the_parent() {
        let pid = spawn_exiting(0);
        assert_ne!(pid, Pid::current());
        assert_eq!(wait_for(pid), ExitStatus::Code(0));
        assert_eq!(wait_for(spawn_exiting(7)), ExitStatus::Code(7));
    }

    #[test]
    fn fork_accepts_a_single_threaded_child() {
        // The harness is multi-threaded, so the checked `fork()` is only
        // allowed inside a forked child, which has exactly one thread. The body
        // stays async-signal-safe: `thread_count` reads `/proc/self/status`
        // into a stack buffer, and `waitpid` is a syscall.
        child_ok("fork in a single-threaded child", || {
            match fork() {
                Err(_) => 1,
                Ok(crate::Fork::Child) => {
                    // SAFETY: `_exit` never returns.
                    unsafe { libc::_exit(42) }
                }
                Ok(crate::Fork::Parent(pid)) => {
                    let mut status = 0;
                    // SAFETY: `waitpid` writes one status word into `status`.
                    let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) };
                    if waited != pid.as_raw() {
                        return 2;
                    }
                    match ExitStatus::from_raw(status) {
                        Some(ExitStatus::Code(42)) => 0,
                        _ => 3,
                    }
                }
            }
        });
    }

    #[test]
    fn chdir_moves_the_working_directory() {
        // A fresh process, not a forked child: the subject (`crate::chdir`) and
        // the reader (`std::env::current_dir`) both allocate.
        assert_eq!(
            run_probe(PROBE_RUNNER, "chdir-moves"),
            0,
            "the fresh process could not move its working directory"
        );
    }

    #[test]
    fn chdir_reports_a_missing_directory() {
        assert_eq!(
            run_probe(PROBE_RUNNER, "chdir-missing"),
            0,
            "chdir did not report a missing directory"
        );
    }

    /// The one test a probe process runs; a no-op in an ordinary test run.
    #[test]
    fn probe_runner() {
        let Ok(name) = std::env::var(PROBE_VAR) else {
            return;
        };
        report_probe(&name, probe(&name).unwrap_or(1));
    }

    /// The bodies that need a private address space, where allocating is safe.
    fn probe(name: &str) -> Option<i32> {
        match name {
            "chdir-moves" => Some(probe_chdir_moves()),
            "chdir-missing" => Some(probe_chdir_missing()),
            _ => None,
        }
    }

    /// `crate::chdir` puts the process in the directory it was given.
    fn probe_chdir_moves() -> i32 {
        let dir = TempDir::new("chdir");
        if std::fs::create_dir_all(dir.path()).is_err() {
            return 1;
        }
        if chdir(dir.path()).is_err() {
            return 2;
        }
        match std::env::current_dir() {
            Ok(now) if now == dir.path() => 0,
            Ok(_) => 3,
            Err(_) => 4,
        }
    }

    /// A missing directory is `NotFound`, from `crate::chdir`.
    fn probe_chdir_missing() -> i32 {
        match chdir(Path::new("/nonexistent-blastit-sys")) {
            Ok(()) => 1,
            Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
            Err(_) => 2,
        }
    }
}
