//! What every forked child does to its descriptors and its identity before it
//! runs its role (§3, §4, §7.2).
//!
//! [`crate`] documents the sequence; this is the shared implementation of it,
//! so dbproxy, http, scheduler and every view process start from the same
//! place: stdin on `/dev/null`, stdout and stderr on their own pipes, nothing
//! else open, the limits and umask of §7.2, and (for a view) the private
//! scratch directory as its working directory.
//!
//! # Where the sequence lives, and where it does not
//!
//! [`enter`] is the **view** child sequence: steps 1–7 and nothing after them.
//! Its callers are the WSGI fork (`blastit-view-python`'s `zygote`) and the PHP
//! probe (`blastit-view-php`'s `probe`); the sequence is written down here so
//! neither caller can arrange the steps differently. It runs
//! `prepare` → `setpgid(0, 0)` (when the plan asks) → `drop_privileges` (when
//! the plan says `DropTo`) → `harden_child` → `install_filter` (when the plan
//! says `Seccomp`) → `set_dumpable(false)` → the saved signal mask (when the
//! plan carries one), and returns.
//!
//! Init's **own** role children (`http`, `scheduler`, `dbproxy`) are not view
//! children: stage 1 builds their `ChildIo` and calls the steps itself
//! (`stage1::prepare_child`), and that sequence is unchanged.
//!
//! # The order is the security property, not a style
//!
//! * `setpgid(0, 0)` runs **before** the filter. `SeccompRole::EmbeddedView`
//!   answers `setpgid` with `EPERM` (its filter denies the call), so a plan that
//!   installed the filter first could never make its child a group leader —
//!   and the PHP probe's containment (`kill(-pgid, SIGKILL)` on a hung
//!   `worker_init`) depends on that leader existing.
//! * `drop_privileges` runs before `harden_child`: `setresgid`/`setresuid`
//!   clear `PR_SET_PDEATHSIG` ([`crate::harden_child`]).
//! * `install_filter` runs after the drop and after `setpgid`, and before the
//!   first thread: the filter cannot be removed and applies to every thread
//!   created later (§7.3).
//! * `set_dumpable(false)` is best effort and its error is discarded on
//!   purpose ([`crate::set_dumpable`]): gVisor ignores it, and the `execve`
//!   secret barrier is the mechanism.
//!
//! # Step 0: nothing loads or opens before [`enter`]
//!
//! The fork protocol's child half (`PyOS_AfterFork_Child()` and any handler a
//! Python extension registered) runs privileged and unfiltered, before
//! [`enter`]. Nothing may `dlopen`, open a file or mutate the environment
//! before [`enter`] runs, and no step here does: [`ChildIo::prepare`] opens
//! `/dev/null` and the working directory, which is the descriptor half of the
//! sequence itself.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, RawFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{
    Rlimits, SeccompRole, close_fds_except, drop_privileges, dup_onto, harden_child, install,
    restore_signal_mask_in_child, set_cloexec, set_dumpable, set_rlimits, set_umask,
};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::Resource;

use crate::SignalMask;

/// The descriptors a child keeps, and where its stdio goes.
#[derive(Debug)]
pub struct ChildIo {
    /// Pipe the child's stdout is connected to. `None` is `/dev/null`.
    pub stdout: Option<std::os::fd::OwnedFd>,
    /// Pipe the child's stderr is connected to. `None` is `/dev/null`.
    pub stderr: Option<std::os::fd::OwnedFd>,
    /// Every descriptor the child owns after the fork, which is exactly what it
    /// keeps: its channels, its events pipe, its log pipes for an `execve`.
    pub keep: Vec<RawFd>,
    /// `RLIMIT_*` for the child (§7.2).
    pub limits: Rlimits,
    /// Working directory: a view's socket directory (§2.2); `None` keeps init's.
    ///
    /// A path that is a symlink is refused ([`ChildIo::prepare`] opens it
    /// `O_NOFOLLOW`): a scratch directory another uid could repoint is not a
    /// scratch directory.
    pub working_dir: Option<PathBuf>,
}

impl ChildIo {
    /// Runs the descriptor half of the child sequence.
    ///
    /// `dup_onto` the stdio pipes, `close_fds_except` the owned descriptors,
    /// then rlimits, umask and the working directory. The caller drops
    /// privileges, hardens, and either `exec`s or serves.
    ///
    /// The working directory is opened `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` and
    /// entered with `fchdir`, never `chdir(path)`: the path a caller named and
    /// the directory the child then runs in are the same object, so a scratch
    /// directory swapped for a symlink between the two is refused instead of
    /// followed. Linux answers that refusal with `ENOTDIR` — the directory
    /// check fires before the no-follow check — and with `ELOOP` only when
    /// `O_DIRECTORY` is absent; both mean "not the directory that was named".
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
            enter_dir(dir)?;
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

/// Enters `dir` through a descriptor, so a symlink where a directory was
/// promised is refused instead of followed.
fn enter_dir(dir: &std::path::Path) -> io::Result<()> {
    use rustix::fs::{Mode, OFlags};
    let opened = rustix::fs::open(
        dir,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    rustix::process::fchdir(&opened)?;
    Ok(())
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
/// A view child goes through [`enter`] instead, which takes the same decision
/// as a [`Filter`]; this is the spelling init's own role children use.
///
/// # Errors
///
/// [`std::io::Error`] when the filter cannot be installed. There is no
/// recovery in a child: the caller exits non-zero, and init treats a component
/// that did that as fatal (§8).
pub fn install_filter(enabled: bool, role: SeccompRole) -> io::Result<()> {
    if enabled {
        install(role)?;
    }
    Ok(())
}

/// What identity the child keeps.
///
/// Built by the composition root, which is the only place that knows the boot
/// mode: [`Privileges::DropTo`] in production, [`Privileges::Shared`] for
/// `dev`/`check`. A view crate copies the value into its [`ChildPlan`] and
/// never constructs it — the child cannot see the mode after the `execve`
/// barrier, so a child-side choice would be forgeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privileges {
    /// Drop to this uid: `setgroups`, `setresgid`, `setresuid`, and every
    /// capability set cleared ([`drop_privileges`]).
    DropTo(u32),
    /// Keep init's identity (`dev`/`check` only: one uid for the whole boot).
    Shared,
}

/// Which seccomp filter the child installs.
///
/// The mode exists only in the composition root (see [`Privileges`]):
/// [`Filter::Seccomp`] in every mode, [`Filter::Off`] only for an explicit
/// `BLASTIT_SECCOMP=0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Install `role`'s filter ([`install`]).
    Seccomp(SeccompRole),
    /// Install nothing.
    Off,
}

/// One view child, as the parent describes it before the fork.
#[derive(Debug)]
pub struct ChildPlan {
    /// What a failure is prefixed with, so the child's one log line names the
    /// process: `view app` for a WSGI view, for instance.
    pub name: String,
    /// Descriptors, limits and working directory.
    pub io: ChildIo,
    /// The identity the child keeps.
    pub privileges: Privileges,
    /// The filter the child installs.
    pub filter: Filter,
    /// Whether the child becomes its own process-group leader.
    ///
    /// Set by the PHP probe alone: the two worker forks are not group leaders,
    /// and supervision kills them by pid.
    pub pgroup: bool,
    /// The mask stage 1 saved before the fork, to restore in the child.
    /// `None` keeps the inherited mask (the probe).
    pub mask: Option<SignalMask>,
}

impl ChildPlan {
    /// A plan with nothing privileged in it, for a test that exercises the
    /// sequence itself.
    ///
    /// `Shared` + `Off`, so a unit test needs no privilege and installs no
    /// filter it cannot remove. It lives here — the crate that owns [`enter`],
    /// not a view crate — so it is outside the censuses that count
    /// [`Privileges::Shared`] and [`Filter::Off`] in the composition root.
    #[doc(hidden)]
    pub fn for_test(io: ChildIo) -> Self {
        Self {
            name: "test child".to_string(),
            io,
            privileges: Privileges::Shared,
            filter: Filter::Off,
            pgroup: false,
            mask: None,
        }
    }
}

/// The view child sequence: steps 1–7, in order, and nothing after them.
///
/// This is what a forked view runs before it loads anything: see the module
/// documentation for the order and why each pair of steps cannot swap. On
/// success it returns `Ok(())` — the caller's next step is its own (for the
/// WSGI fork, pointing `HOME`/`TMPDIR` at the scratch directory and serving) —
/// and `Err` carries one sentence, already prefixed with [`ChildPlan::name`],
/// for the caller's `child_exit(&message, 1)`.
///
/// **Step 0 is the caller's:** the fork protocol's child half
/// (`PyOS_AfterFork_Child()` and any handler an extension registered) runs
/// privileged and unfiltered before this, and nothing between the fork and this
/// call may `dlopen`, open a file or mutate the environment.
///
/// Nothing here is async-signal-safe in the narrow sense: it allocates
/// (formatting the error, `/proc/self/fd` in the descriptor scan) and reads
/// files. That is the contract of a blastit child — it is forked from a
/// deliberately single-threaded process (§2.1) — and it is not a general
/// purpose post-`fork` sequence.
///
/// # Errors
///
/// One sentence naming what failed, already prefixed with [`ChildPlan::name`],
/// for the child's stderr log line.
pub fn enter(plan: ChildPlan) -> Result<(), String> {
    let ChildPlan {
        name,
        io,
        privileges,
        filter,
        pgroup,
        mask,
    } = plan;

    io.prepare().map_err(|err| format!("{name}: {err}"))?;
    // Before the filter: `EmbeddedView` answers `setpgid` with `EPERM`.
    if pgroup {
        set_process_group().map_err(|err| format!("{name}: {err}"))?;
    }
    if let Privileges::DropTo(uid) = privileges {
        drop_privileges(uid).map_err(|err| format!("{name}: {err}"))?;
    }
    harden_child(crate::Signal::KILL).map_err(|err| format!("{name}: {err}"))?;
    if let Filter::Seccomp(role) = filter {
        install(role).map_err(|err| format!("{name}: {err}"))?;
    }
    // Best effort on purpose: gVisor ignores it, and `execve` is the barrier.
    let _ = set_dumpable(false);
    if let Some(mask) = mask {
        restore_signal_mask_in_child(mask).map_err(|err| format!("{name}: {err}"))?;
    }
    Ok(())
}

/// `setpgid(0, 0)`: the calling process into its own process group.
///
/// Only meaningful **before** a seccomp filter is installed: every role that
/// filters process creation answers this call with `EPERM`.
///
/// The denying role is `EmbeddedView`, whose filter refuses `setpgid` and
/// `setsid`; the crate-level sequence records why that makes this call come
/// first.
///
/// [`enter`] is the one caller, and it runs this on
/// [`ChildPlan::pgroup`].
///
/// # Errors
///
/// [`std::io::Error`] from `setpgid(2)`.
pub fn set_process_group() -> io::Result<()> {
    rustix::process::setpgid(None, None).map_err(Into::into)
}

/// Sets `name=value` in the process environment.
///
/// This is the safe spelling of `std::env::set_var` (which is `unsafe` in
/// edition 2024), for a view child that points `HOME` and `TMPDIR` at its
/// private scratch directory.
///
/// **`setenv`/`getenv` are not thread-safe:** another thread's `getenv` while
/// this writes the environment is undefined behaviour, which is why the
/// standard library makes `set_var` `unsafe` and why a safe wrapper has to
/// *check* rather than assume. It therefore refuses a process with more than
/// one thread, through the same `/proc/self/status` reading [`crate::fork`]
/// uses — advisory in exactly the same way (a thread born between the read and
/// the write is not caught), and refusing when the count cannot be read at all.
///
/// Every caller is a forked child before its first thread (§2.1), so the check
/// costs one `/proc` read per child boot and can never refuse a supported path.
/// The value is never logged: it is a scratch directory, but the rule is the
/// same one every other message here keeps.
///
/// # Errors
///
/// [`std::io::Error`] when the calling process has more than one thread (or the
/// count cannot be read), when the name or value holds a NUL, or when
/// `setenv(3)` fails (in practice only `ENOMEM`).
pub fn set_env(name: &str, value: &str) -> io::Result<()> {
    let threads = crate::thread_count()?;
    if threads != 1 {
        return Err(io::Error::other(format!(
            "refusing to set an environment variable: {threads} threads are alive, and \
             setenv is not thread-safe"
        )));
    }
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the name holds a NUL byte"))?;
    let value = CString::new(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the value holds a NUL byte"))?;
    // SAFETY: both pointers are valid NUL-terminated strings that live for the
    // call, `overwrite` is 1, and the check above established that this process
    // has one thread — the precondition `setenv` needs to be free of a racing
    // `getenv`.
    if unsafe { libc::setenv(name.as_ptr(), value.as_ptr(), 1) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The text of a panic payload, for the log a forked child writes.
///
/// A view child wraps its work in `catch_unwind` (the workspace cannot set
/// `panic = "abort"` from one crate), and the exit message has to say what
/// happened rather than "a panic".
pub fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_string()
    }
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
