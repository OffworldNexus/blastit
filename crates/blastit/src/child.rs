//! What every forked child does to its descriptors before it runs its role
//! (§3, §4, §7.2).
//!
//! [`blastit_sys`] documents the sequence; this is the shared implementation of
//! it, so dbproxy, http, scheduler and every view process start from the same
//! place: stdin on `/dev/null`, stdout and stderr on their own pipes, nothing
//! else open, the limits and umask of §7.2, and (for a view) the private
//! scratch directory as its working directory.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use blastit_sys::{
    Rlimits, SeccompRole, chdir, close_fds_except, dup_onto, set_cloexec, set_rlimits, set_umask,
};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::Resource;

/// The descriptors a child keeps, and where its stdio goes.
#[derive(Debug)]
pub struct ChildIo {
    /// Pipe the child's stdout is connected to. `None` is `/dev/null`.
    pub stdout: Option<OwnedFd>,
    /// Pipe the child's stderr is connected to. `None` is `/dev/null`.
    pub stderr: Option<OwnedFd>,
    /// Every descriptor the child owns after the fork, which is exactly what it
    /// keeps: its channels, its events pipe, its log pipes for an `execve`.
    pub keep: Vec<RawFd>,
    /// `RLIMIT_*` for the child (§7.2).
    pub limits: Rlimits,
    /// Working directory: a view's socket directory (§2.2); `None` keeps init's.
    pub working_dir: Option<PathBuf>,
}

impl ChildIo {
    /// Runs the descriptor half of the child sequence.
    ///
    /// `dup_onto` the stdio pipes, `close_fds_except` the owned descriptors,
    /// then rlimits, umask and the working directory. The caller drops
    /// privileges, hardens, and either `exec`s or serves.
    ///
    /// This consumes `self`, because the descriptors it moves onto 0, 1 and 2
    /// have to be *forgotten* afterwards: `close_fds_except` closes the
    /// originals, and the standard library aborts the process when a descriptor
    /// it believes it owns is closed behind its back.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] from any of those steps. There is no recovery in a
    /// child: the caller exits non-zero.
    pub fn prepare(mut self) -> io::Result<()> {
        let null = std::fs::File::open("/dev/null")?;
        dup_onto(null.as_fd(), 0)?;
        let stdout = self.stdout.take();
        match stdout.as_ref() {
            Some(fd) => dup_onto(fd.as_fd(), 1)?,
            None => dup_onto(null.as_fd(), 1)?,
        }
        let stderr = self.stderr.take();
        match stderr.as_ref() {
            Some(fd) => dup_onto(fd.as_fd(), 2)?,
            None => dup_onto(null.as_fd(), 2)?,
        }

        // 0, 1 and 2 are copies now; the originals go away with
        // `close_fds_except`, so nothing may drop them afterwards.
        std::mem::forget(null);
        if let Some(fd) = stdout {
            std::mem::forget(fd);
        }
        if let Some(fd) = stderr {
            std::mem::forget(fd);
        }

        close_fds_except(&self.keep)?;
        set_rlimits(Rlimits {
            nofile: self.limits.nofile.and_then(clamp_nofile),
            ..self.limits
        })?;
        set_umask(0o077);
        if let Some(dir) = self.working_dir.as_deref() {
            chdir(dir)?;
        }
        Ok(())
    }

    /// Clears `FD_CLOEXEC` on the descriptors that must survive an `execve`.
    ///
    /// §3 makes every descriptor close-on-exec; the exceptions are the ones
    /// named here — dbproxy's `secrets.pipe`, its events pipe, its ready pipe
    /// and the pg listeners it adopts — plus the child's own stdin, stdout and
    /// stderr, which [`ChildIo::prepare`] already handled.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when the flag cannot be read or set.
    pub fn clear_cloexec(&self, extra: &[RawFd]) -> io::Result<()> {
        for fd in self.keep.iter().chain(extra.iter()) {
            if *fd < 0 {
                continue;
            }
            // SAFETY: every descriptor in the list belongs to this process: the
            // caller built it from objects it holds open.
            let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
            set_cloexec(borrowed, false)?;
        }
        Ok(())
    }
}

/// The `RLIMIT_NOFILE` a child may actually be given.
///
/// Lowering a hard limit needs no capability; raising one does. A request above
/// the process's current hard limit would come back `EPERM` and take the whole
/// boot down, so the hard limit is read first and the request is dropped when it
/// is not lower than it — a container that already runs with fewer descriptors
/// than blastit asks for keeps its own limit.
fn clamp_nofile(requested: u64) -> Option<u64> {
    match rustix::process::getrlimit(Resource::Nofile).maximum {
        Some(hard) if hard <= requested => None,
        _ => Some(requested),
    }
}

/// Installs `role`'s seccomp filter when `BLASTIT_SECCOMP` asks for it (§7.3).
///
/// This is the step right before a child starts anything of its own: the filter
/// cannot be removed, applies to every thread that exists or is created later,
/// and has to be on while the child is still single-threaded — before a view
/// serves its first request and before http, scheduler or dbproxy build their
/// runtime (§4).
///
/// # Errors
///
/// [`std::io::Error`] when the filter cannot be installed. There is no
/// recovery in a child: the caller exits non-zero, and init treats a component
/// that did that as fatal (§8).
pub fn install_filter(enabled: bool, role: SeccompRole) -> io::Result<()> {
    if enabled {
        blastit_sys::install(role)?;
    }
    Ok(())
}

/// `poll(2)` for readability, with a timeout.
///
/// Returns `false` when the timeout expired. Used by stage 0 to wait for
/// dbproxy's ready byte; the supervise loop has its own poll.
///
/// # Errors
///
/// [`std::io::Error`] when `poll` fails.
pub fn wait_readable(fd: BorrowedFd<'_>, timeout: Duration) -> io::Result<bool> {
    wait_for(fd, PollFlags::IN, timeout)
}

/// `poll(2)` for writability, with a timeout.
///
/// Returns `false` when the timeout expired. Used before writing a `ctl.chan`
/// message on a non-blocking socket, so a scheduler that stopped reading cannot
/// wedge init.
///
/// # Errors
///
/// [`std::io::Error`] when `poll` fails.
pub fn wait_writable(fd: BorrowedFd<'_>, timeout: Duration) -> io::Result<bool> {
    wait_for(fd, PollFlags::OUT, timeout)
}

/// One `poll(2)` for a single descriptor.
fn wait_for(fd: BorrowedFd<'_>, flags: PollFlags, timeout: Duration) -> io::Result<bool> {
    let mut pollfd = [PollFd::from_borrowed_fd(fd, flags)];
    let timespec = Timespec {
        tv_sec: timeout.as_secs().min(i64::MAX as u64) as _,
        tv_nsec: timeout.subsec_nanos() as _,
    };
    let ready = rustix::event::poll(&mut pollfd, Some(&timespec))?;
    Ok(ready > 0)
}

/// A deadline that a loop can ask how much time is left.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    at: Instant,
}

impl Deadline {
    /// A deadline `timeout` from now.
    pub fn after(timeout: Duration) -> Self {
        Self {
            at: Instant::now() + timeout,
        }
    }

    /// How much of the budget is left; zero once it is spent.
    pub fn left(&self) -> Duration {
        self.at.saturating_duration_since(Instant::now())
    }

    /// Whether the deadline has passed.
    pub fn expired(&self) -> bool {
        self.left().is_zero()
    }
}

/// Reads one byte from `fd`, treating end-of-file as "the peer went away".
///
/// # Errors
///
/// [`std::io::Error`] on a read failure other than `EINTR`.
pub fn read_one(fd: BorrowedFd<'_>) -> io::Result<Option<u8>> {
    let mut byte = [0u8; 1];
    loop {
        match rustix::io::read(fd, &mut byte) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(byte[0])),
            Err(rustix::io::Errno::INTR) => continue,
            Err(err) => return Err(err.into()),
        }
    }
}

/// Ends a forked child: writes `message` to its own stderr and `_exit`s.
///
/// A child never returns into init's code, and it never runs a handler either:
/// `_exit` skips libc's `atexit` list, which matters in a child of a process
/// that has CPython loaded (§4, §9.3). The message goes to descriptor 2, which
/// is the child's stderr log pipe after [`ChildIo::prepare`], so init turns it
/// into an `app.output` event. An empty message writes nothing — a child that
/// ended cleanly has nothing to say.
pub fn child_exit(message: &str, code: i32) -> ! {
    if !message.is_empty() {
        let mut bytes = message.as_bytes().to_vec();
        bytes.push(b'\n');
        // SAFETY: descriptor 2 is the child's stderr: either the pipe
        // `prepare` put there, or the one inherited from init.
        let stderr = unsafe { BorrowedFd::borrow_raw(2) };
        let _ = rustix::io::write(stderr, &bytes);
    }
    // SAFETY: `_exit` never returns and runs no handler.
    unsafe { libc::_exit(code) }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// An `AF_INET` stream socket, created and closed again.
    ///
    /// `false` is what the §7.3 filter answers: `socket(AF_INET)` is exactly
    /// what a view without egress may not do.
    fn inet_socket() -> bool {
        // SAFETY: `socket` takes three integers and returns a descriptor.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if fd == -1 {
            return false;
        }
        // SAFETY: the descriptor was just created here and nothing else owns it.
        unsafe { libc::close(fd) };
        true
    }

    #[test]
    fn the_off_switch_installs_no_filter() {
        // `BLASTIT_SECCOMP=0` has to leave every process's system calls alone.
        // A filter can never be removed, so this runs in the harness's own
        // process — and that is the proof: if `install_filter` installed one
        // anyway, the socket below would come back `EPERM` and stay that way.
        // The enabled half is `blastit-sys`'s own tests, which fork a child per
        // role.
        install_filter(false, SeccompRole::View { egress: false }).unwrap();
        assert!(
            inet_socket(),
            "AF_INET must still work when the filters are off"
        );
    }
}
