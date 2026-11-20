//! Signals: `signalfd`, reaping and killing.

use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use crate::Signal;
use crate::pid::{ExitStatus, Pid, impossible};

/// A saved signal mask.
///
/// [`SignalFd::new`] changes the calling thread's mask; this is the value it
/// remembers, handed out so a freshly forked child can put it back with
/// [`restore_signal_mask_in_child`] without needing the `SignalFd` itself.
#[derive(Clone, Copy)]
pub struct SignalMask(libc::sigset_t);

impl SignalMask {
    /// Whether `signal` is blocked in this mask.
    pub fn blocks(&self, signal: Signal) -> bool {
        // SAFETY: `sigismember` only reads bits of an initialised set.
        unsafe { libc::sigismember(&self.0, signal.as_raw()) == 1 }
    }
}

impl std::fmt::Debug for SignalMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut blocked = Vec::new();
        for signal in [
            Signal::HUP,
            Signal::INT,
            Signal::QUIT,
            Signal::TERM,
            Signal::CHILD,
            Signal::USR1,
            Signal::USR2,
            Signal::PIPE,
        ] {
            if self.blocks(signal) {
                blocked.push(format!("{signal:?}"));
            }
        }
        f.debug_tuple("SignalMask").field(&blocked).finish()
    }
}

/// The calling thread's current signal mask.
pub fn current_signal_mask() -> io::Result<SignalMask> {
    let mut mask = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: a null set means "change nothing, only read the current mask into
    // the valid storage we pass".
    if unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), mask.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call above filled the set in.
    Ok(SignalMask(unsafe { mask.assume_init() }))
}

/// Restore a mask saved by [`SignalFd::new`], in a freshly forked child.
///
/// `fork` copies the signal mask into the child, so a child of init starts with
/// `SIGCHLD`, `SIGTERM`, `SIGINT` and `SIGHUP` blocked — the mask init set up
/// for its own `signalfd`, which the child neither has (the descriptor is
/// close-on-exec) nor wants. `execve` does **not** clear the mask either: it
/// resets handlers to their defaults but leaves blocked signals blocked. So the
/// child has to put the mask back itself, and this is how:
///
/// ```text
/// // in init, once:
/// let events = SignalFd::new(&[Signal::CHILD, Signal::TERM, Signal::INT, Signal::HUP])?;
/// let mask = events.previous_mask();
/// // in the child, after the fork:
/// restore_signal_mask_in_child(mask)?;      // or events.unblock_in_child()
/// ```
///
/// The mask is per-thread, so this only restores the calling thread — which is
/// the whole process in a child that has just been forked.
pub fn restore_signal_mask_in_child(mask: SignalMask) -> io::Result<()> {
    // SAFETY: `SIG_SETMASK` replaces the calling thread's mask with the
    // initialised set we hold; no pointer is written to.
    if unsafe { libc::sigprocmask(libc::SIG_SETMASK, &mask.0, std::ptr::null_mut()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A `signalfd` for the signals a supervisor cares about.
///
/// init wants to be woken by `SIGCHLD` (a child exited), `SIGTERM` and `SIGINT`
/// (the platform asked it to stop) and `SIGHUP`, and it wants that to be a
/// readable descriptor it can put in the same event loop as `ctl.chan` and the
/// log pipes — not a signal handler (§4). So the signals are blocked for the
/// process with `sigprocmask`, which means the kernel queues them for the
/// `signalfd` instead of running a disposition, and the descriptor is opened
/// non-blocking and close-on-exec.
///
/// Blocking is per-thread. init blocks before it forks its children, so the
/// mask is inherited by every child — and `execve` does not clear it either —
/// which is why [`SignalFd::previous_mask`] hands out the mask that was in
/// effect before, and why [`restore_signal_mask_in_child`] is in the child
/// sequence: a fresh image should start with the mask it would have had, since
/// it does not own the `signalfd` (the descriptor is close-on-exec and the
/// child closes it anyway).
pub struct SignalFd {
    fd: OwnedFd,
    previous_mask: libc::sigset_t,
}

impl SignalFd {
    /// Block `signals` and return a non-blocking, close-on-exec `signalfd` for them.
    ///
    /// The mask in effect before this call is remembered, so a child can put it
    /// back with [`SignalFd::unblock_in_child`] or, without the descriptor,
    /// [`restore_signal_mask_in_child`] — see [`SignalFd::previous_mask`]. A
    /// signal that is already pending when it is blocked becomes readable
    /// immediately.
    pub fn new(signals: &[Signal]) -> io::Result<Self> {
        let mut mask = MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: `sigemptyset` initialises the set it is given, which is
        // uninitialised, correctly aligned storage of the right size.
        if unsafe { libc::sigemptyset(mask.as_mut_ptr()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the set is now initialised.
        let mut mask = unsafe { mask.assume_init() };
        for signal in signals {
            // SAFETY: `sigaddset` only sets a bit in an initialised set.
            if unsafe { libc::sigaddset(&mut mask, signal.as_raw()) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }

        let mut previous_mask = MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: the three arguments are valid, aligned `sigset_t` storage;
        // `SIG_BLOCK` adds `mask` to the calling thread's mask and writes the
        // old mask to `previous_mask`.
        if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &mask, previous_mask.as_mut_ptr()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call above succeeded and filled the set in.
        let previous_mask = unsafe { previous_mask.assume_init() };

        // SAFETY: `signalfd` takes a valid set and two flag words. `-1` means
        // "create a new descriptor" rather than "replace this one".
        let fd = unsafe { libc::signalfd(-1, &mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
        if fd == -1 {
            let err = io::Error::last_os_error();
            // SAFETY: restore the mask we changed; the set is initialised.
            unsafe { libc::sigprocmask(libc::SIG_SETMASK, &previous_mask, std::ptr::null_mut()) };
            return Err(err);
        }
        // SAFETY: `signalfd` returned an open descriptor that nothing else owns.
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            previous_mask,
        })
    }

    /// Read one pending signal, or `None` when none is pending.
    ///
    /// A `SIGCHLD` delivered this way says that *something* happened to a child;
    /// [`crate::reap`] says what.
    pub fn read(&self) -> io::Result<Option<Signal>> {
        let mut info = MaybeUninit::<libc::signalfd_siginfo>::uninit();
        // SAFETY: `read` writes at most `size_of::<signalfd_siginfo>()` bytes
        // into a buffer of exactly that size and alignment.
        let read = unsafe {
            libc::read(
                self.fd.as_raw_fd(),
                info.as_mut_ptr().cast::<libc::c_void>(),
                std::mem::size_of::<libc::signalfd_siginfo>(),
            )
        };
        if read == -1 {
            let err = io::Error::last_os_error();
            return match err.kind() {
                // A non-blocking signalfd with nothing queued, or a call that
                // was interrupted before it read anything.
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Ok(None),
                _ => Err(err),
            };
        }
        if read as usize != std::mem::size_of::<libc::signalfd_siginfo>() {
            return Err(io::Error::other("signalfd returned a short read"));
        }
        // SAFETY: the read filled the whole structure in.
        let info = unsafe { info.assume_init() };
        let signo = info.ssi_signo as i32;
        match Signal::from_named_raw(signo) {
            Some(signal) => Ok(Some(signal)),
            None => Err(io::Error::other(
                "signalfd delivered a signal that is not a named signal",
            )),
        }
    }

    /// Put the signal mask that was in effect before [`SignalFd::new`] back.
    ///
    /// This is what a freshly forked child calls: it inherited the blocked mask
    /// — and, if it was not closed, the `signalfd` itself — but it is a new
    /// program, and the descriptors it owns are the ones its parent chose for
    /// it. The name says where it is meant to be called, not what it does:
    /// the call itself is per-thread and does not care who the caller is.
    ///
    /// [`SignalFd::previous_mask`] plus [`restore_signal_mask_in_child`] is the
    /// same thing when the child does not have — or should not have — the
    /// `SignalFd` value in scope.
    pub fn unblock_in_child(&self) -> io::Result<()> {
        restore_signal_mask_in_child(self.previous_mask())
    }

    /// The mask that was in effect before this call blocked `signals`.
    ///
    /// This is the value a forked child restores, and the one the documented
    /// child sequence hands to [`restore_signal_mask_in_child`]: keep it
    /// somewhere the fork path can reach, because nothing else can recreate it.
    pub fn previous_mask(&self) -> SignalMask {
        SignalMask(self.previous_mask)
    }

    /// The underlying descriptor, for a poller.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

/// The children one non-blocking `waitpid` sweep collected.
#[derive(Debug)]
pub struct Reaped {
    /// Every child that ended, in the order the kernel reported them.
    pub ended: Vec<(Pid, ExitStatus)>,
    /// Whether the sweep reached `ECHILD`: this process has no child left, so a
    /// shutdown drain is done even when its bookkeeping tables still list one.
    pub exhausted: bool,
}

/// Collect every child that has exited, without blocking.
///
/// `waitpid(-1, …, WNOHANG)` in a loop, which is what PID 1 does: it inherits
/// orphans from the whole container, and gVisor gives it no init of its own
/// (§2.1). Only ended children are reported; a child that was merely stopped is
/// skipped. `ECHILD` ends the loop normally and sets [`Reaped::exhausted`] —
/// that is the only way a caller can tell "no children left" from "every child
/// is still running", which `pid == 0` reports.
///
/// An [`Err`] is returned only for a `waitpid` failure that is neither `EINTR`
/// nor `ECHILD`; children reaped before it are lost, which is why the caller
/// should treat it as fatal.
pub fn reap() -> io::Result<Reaped> {
    let mut ended = Vec::new();
    loop {
        let mut status = 0;
        // SAFETY: `waitpid` writes one status word to `status`, which is valid
        // storage; `-1` means "any child".
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid == 0 {
            return Ok(Reaped {
                ended,
                exhausted: false,
            });
        }
        if pid < 0 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                // A signal arrived while we were waiting; try again.
                Some(libc::EINTR) => continue,
                // No children left at all: the drain is over.
                Some(libc::ECHILD) => {
                    return Ok(Reaped {
                        ended,
                        exhausted: true,
                    });
                }
                _ => return Err(err),
            }
        }
        let Some(pid) = Pid::from_raw(pid) else {
            return Err(impossible("waitpid", i64::from(pid)));
        };
        match ExitStatus::from_raw(status) {
            Some(status) => ended.push((pid, status)),
            // Stopped or continued: nothing in this crate asks for those, so
            // the child is simply not reported as ended.
            None => continue,
        }
    }
}

/// Send `sig` to `pid`.
///
/// Distinct uids cannot signal each other, which is one of the things keeping
/// one view out of another's business; init is root and can signal anything it
/// started (§7.2).
pub fn kill(pid: Pid, sig: Signal) -> io::Result<()> {
    // SAFETY: `kill` takes two integers.
    if unsafe { libc::kill(pid.as_raw(), sig.as_raw()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
