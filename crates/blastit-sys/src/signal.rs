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

/// Collect every child that has exited, without blocking.
///
/// `waitpid(-1, …, WNOHANG)` in a loop, which is what PID 1 does: it inherits
/// orphans from the whole container, and gVisor gives it no init of its own
/// (§2.1). Only ended children are reported; a child that was merely stopped is
/// skipped. `ECHILD` ends the loop normally — nothing left to reap.
///
/// An [`Err`] is returned only for a `waitpid` failure that is neither `EINTR`
/// nor `ECHILD`; children reaped before it are lost, which is why the caller
/// should treat it as fatal.
pub fn reap() -> io::Result<Vec<(Pid, ExitStatus)>> {
    let mut reaped = Vec::new();
    loop {
        let mut status = 0;
        // SAFETY: `waitpid` writes one status word to `status`, which is valid
        // storage; `-1` means "any child".
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid == 0 {
            return Ok(reaped);
        }
        if pid < 0 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                // A signal arrived while we were waiting; try again.
                Some(libc::EINTR) => continue,
                // No children left at all.
                Some(libc::ECHILD) => return Ok(reaped),
                _ => return Err(err),
            }
        }
        let Some(pid) = Pid::from_raw(pid) else {
            return Err(impossible("waitpid", i64::from(pid)));
        };
        match ExitStatus::from_raw(status) {
            Some(status) => reaped.push((pid, status)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fd::fd_is_cloexec;
    use crate::pid::ExitStatus;
    use crate::testutil::{
        child_ok, in_child, must, must_err, reap_until, spawn_dying, spawn_exiting, spawn_waiting,
        wait_for,
    };

    /// Whether the calling thread currently blocks `signal`.
    fn mask_blocks(signal: Signal) -> bool {
        must(current_signal_mask()).blocks(signal)
    }

    #[test]
    fn previous_mask_is_the_mask_before_the_signalfd() {
        let before = must(current_signal_mask());
        assert!(!before.blocks(Signal::TERM), "the harness blocks nothing");
        let events = must(SignalFd::new(&[Signal::CHILD, Signal::TERM]));
        assert!(must(current_signal_mask()).blocks(Signal::TERM));

        let saved = events.previous_mask();
        assert!(!saved.blocks(Signal::TERM));
        assert!(!saved.blocks(Signal::CHILD));
        // The free function and the method both put the saved mask back.
        must(restore_signal_mask_in_child(saved));
        assert!(!mask_blocks(Signal::TERM));

        let events = must(SignalFd::new(&[Signal::TERM]));
        must(events.unblock_in_child());
        assert!(!mask_blocks(Signal::TERM));
        // Leave the thread as we found it: other tests run in this process.
        must(restore_signal_mask_in_child(before));
    }

    #[test]
    fn a_forked_child_inherits_the_blocked_mask_and_can_put_it_back() {
        let events = must(SignalFd::new(&[Signal::TERM]));
        let saved = events.previous_mask();
        let code = in_child(move || {
            // `fork` copies the mask, and `execve` does not clear it either...
            match current_signal_mask() {
                Ok(mask) if mask.blocks(Signal::TERM) => {}
                _ => return 1,
            }
            // ... which is why a child restores it itself.
            if restore_signal_mask_in_child(saved).is_err() {
                return 2;
            }
            match current_signal_mask() {
                Ok(mask) if !mask.blocks(Signal::TERM) => 0,
                _ => 3,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
        // This thread blocked SIGTERM for the test; give it back.
        must(events.unblock_in_child());
    }

    #[test]
    fn signalfd_blocks_its_signals_and_is_cloexec_and_nonblocking() {
        let events = must(SignalFd::new(&[
            Signal::CHILD,
            Signal::TERM,
            Signal::INT,
            Signal::HUP,
        ]));
        assert!(must(fd_is_cloexec(events.as_fd())));
        for signal in [Signal::CHILD, Signal::TERM, Signal::INT, Signal::HUP] {
            assert!(mask_blocks(signal), "{signal:?} must be blocked");
        }

        // Nothing to read and no handler in the way: a non-blocking signalfd
        // returns `None` rather than waiting.
        must(events.read());

        // The helper a fresh child calls: the mask goes back to what it was.
        must(events.unblock_in_child());
        for signal in [Signal::CHILD, Signal::TERM, Signal::INT, Signal::HUP] {
            assert!(!mask_blocks(signal), "{signal:?} must be unblocked again");
        }
    }

    #[test]
    fn signalfd_reports_sigchld_and_reap_returns_the_exit_code() {
        child_ok("signalfd + reap", || {
            let events = match SignalFd::new(&[Signal::CHILD]) {
                Ok(events) => events,
                Err(_) => return 1,
            };
            // This child has no children yet, so nothing is pending and nothing
            // is to be reaped.
            if let Ok(Some(signal)) = events.read() {
                let _ = signal;
                return 2;
            }
            match crate::reap() {
                Ok(reaped) if reaped.is_empty() => {}
                _ => return 3,
            }

            let child = spawn_exiting(3);
            match reap_until(child) {
                ExitStatus::Code(3) => {}
                _ => return 4,
            }
            // The exit queued SIGCHLD, and the signalfd still holds it.
            match events.read() {
                Ok(Some(Signal::CHILD)) => {}
                _ => return 5,
            }
            0
        });
    }

    #[test]
    fn reap_reports_a_child_that_was_killed_by_a_signal() {
        child_ok("reap a signal death", || {
            let child = spawn_dying(Signal::KILL);
            match reap_until(child) {
                ExitStatus::Signal(signo) if signo == Signal::KILL.as_raw() => {}
                _ => return 1,
            }
            match crate::reap() {
                Ok(reaped) if reaped.is_empty() => {}
                _ => return 2,
            }
            0
        });
    }

    #[test]
    fn reap_collects_every_child_that_exited() {
        child_ok("reap several", || {
            let first = spawn_exiting(1);
            let second = spawn_exiting(2);
            let mut seen = Vec::new();
            for _ in 0..200_000 {
                match crate::reap() {
                    Ok(reaped) => seen.extend(reaped),
                    Err(_) => return 1,
                }
                if seen.len() >= 2 {
                    break;
                }
                std::thread::yield_now();
            }
            if seen.len() != 2 {
                return 2;
            }
            let mut codes: Vec<i32> = seen
                .iter()
                .filter_map(|(_, status)| status.code())
                .collect();
            codes.sort_unstable();
            if codes != [1, 2] {
                return 3;
            }
            let pids: Vec<i32> = seen.iter().map(|(pid, _)| pid.as_raw()).collect();
            if !pids.contains(&first.as_raw()) || !pids.contains(&second.as_raw()) {
                return 4;
            }
            0
        });
    }

    #[test]
    fn kill_terminates_a_waiting_child() {
        child_ok("kill", || {
            let child = spawn_waiting();
            if kill(child, Signal::TERM).is_err() {
                return 1;
            }
            match reap_until(child) {
                ExitStatus::Signal(signo) if signo == Signal::TERM.as_raw() => {}
                _ => return 2,
            }
            0
        });
    }

    #[test]
    fn kill_reports_a_process_that_is_already_gone() {
        let pid = spawn_exiting(0);
        assert_eq!(wait_for(pid), ExitStatus::Code(0));
        let err = must_err(kill(pid, Signal::TERM));
        assert_eq!(err.raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn reap_without_children_returns_nothing() {
        // `ECHILD` is the normal end of the loop, not an error.
        child_ok("reap nothing", || {
            let pid = spawn_exiting(0);
            match wait_for(pid) {
                ExitStatus::Code(0) => {}
                _ => return 1,
            }
            match crate::reap() {
                Ok(reaped) if reaped.is_empty() => 0,
                Ok(_) => 2,
                Err(_) => 3,
            }
        });
    }

    #[test]
    fn signalfd_read_reports_a_signal_sent_to_this_process() {
        child_ok("signalfd read", || {
            let events = match SignalFd::new(&[Signal::TERM]) {
                Ok(events) => events,
                Err(_) => return 1,
            };
            if kill(Pid::current(), Signal::TERM).is_err() {
                return 2;
            }
            // The signal is blocked, so the process survives and the descriptor
            // is readable: SIGTERM is queued, not acted on.
            match events.read() {
                Ok(Some(Signal::TERM)) => 0,
                _ => 3,
            }
        });
    }

    #[test]
    fn reap_reports_only_the_child_that_exited() {
        child_ok("reap selectively", || {
            let waiting = spawn_waiting();
            let exiting = spawn_exiting(9);
            let status = reap_until(exiting);
            match status {
                ExitStatus::Code(9) => {}
                _ => return 1,
            }
            match crate::reap() {
                Ok(reaped) if reaped.is_empty() => {}
                _ => return 2,
            }
            if kill(waiting, Signal::KILL).is_err() {
                return 3;
            }
            match reap_until(waiting) {
                ExitStatus::Signal(signo) if signo == Signal::KILL.as_raw() => 0,
                _ => 4,
            }
        });
    }

    #[test]
    fn reap_and_signalfd_agree_that_nothing_is_pending() {
        let code = in_child(|| {
            // A fresh child of the harness has no children of its own.
            match crate::reap() {
                Ok(reaped) if reaped.is_empty() => 0,
                _ => 1,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
