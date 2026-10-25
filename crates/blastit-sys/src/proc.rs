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
pub fn thread_count() -> io::Result<usize> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Threads:") {
            return match rest.trim().parse::<usize>() {
                Ok(threads) => Ok(threads),
                Err(_) => Err(Error::ThreadCount {
                    detail: "the Threads: line of /proc/self/status is not a number",
                }
                .into()),
            };
        }
    }
    Err(Error::ThreadCount {
        detail: "/proc/self/status has no Threads: line",
    }
    .into())
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
    use std::sync::mpsc;

    use super::*;
    use crate::error::Error;
    use crate::pid::ExitStatus;
    use crate::testutil::{child_ok, in_child, must, spawn_exiting, wait_for};

    #[test]
    fn thread_count_reports_at_least_the_calling_thread() {
        let threads = must(thread_count());
        assert!(threads >= 1, "a process has at least one thread: {threads}");
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
        // allowed inside a forked child, which has exactly one thread.
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
        let dir = crate::testutil::TempDir::new("chdir");
        must(std::fs::create_dir_all(dir.path()));
        let target = dir.path().to_path_buf();
        let code = in_child(move || {
            if chdir(&target).is_err() {
                return 1;
            }
            match std::env::current_dir() {
                Ok(now) if now == target => {}
                Ok(_) => return 2,
                Err(_) => return 3,
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn chdir_reports_a_missing_directory() {
        let code = in_child(|| match chdir(Path::new("/nonexistent-blastit-sys")) {
            Ok(()) => 1,
            Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
            Err(_) => 2,
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
