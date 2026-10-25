//! `fork()` under CPython's fork protocol, when an interpreter is live (§9.3).
//!
//! §9.3 #10: `PyOS_AfterFork_Child()` must run while holding the GIL, and its
//! two siblings are the matching halves of the same protocol —
//! `PyOS_BeforeFork()` stops the world, and each side of the fork restarts it
//! with `PyOS_AfterFork_Parent()` or `PyOS_AfterFork_Child()`. Skipping either
//! half leaves CPython's locks and its thread bookkeeping describing a process
//! that no longer exists.
//!
//! Which callers need this depends on the installation (D2):
//!
//! * the **standalone binary** forks *before* CPython exists — stage 0 has no
//!   interpreter at all, and stage 1 forks http and the scheduler before
//!   initialising one (§4) — so there is nothing to stop or restart;
//! * a **wheel install** runs inside the interpreter the console script started,
//!   and stage 0 (dbproxy) and stage 1 (http, scheduler) fork *while it is
//!   live*. Those forks go through [`fork`] with `interpreter = true`.
//!
//! The fork itself is still [`blastit_sys::fork`], which refuses to run in an
//! image with more than one thread: the protocol makes a *Python* process safe
//! to fork, but it says nothing about threads CPython did not start, and §2.1's
//! invariant is what keeps every child's address space sane.

use std::io;

use blastit_sys::Fork;

/// Forks, holding the GIL and stopping the world when a live interpreter is in
/// this image (§9.3).
///
/// `interpreter` is `true` when CPython is initialised and the caller holds the
/// GIL — a wheel install, where the console script's interpreter is the one
/// every stage runs in.
///
/// # Errors
///
/// Whatever [`blastit_sys::fork`] refuses with, in particular
/// [`blastit_sys::Error::NotSingleThreaded`]: the protocol is what makes the
/// fork consistent for the interpreter, not a licence to fork a process with
/// threads blastit knows nothing about.
pub fn fork(interpreter: bool) -> io::Result<Fork> {
    if interpreter {
        // SAFETY: the caller holds the GIL, which is what `PyOS_BeforeFork`
        // requires: it stops the world from this thread.
        unsafe { pyo3::ffi::PyOS_BeforeFork() };
    }
    let outcome = blastit_sys::fork();
    // Exactly one of the two `AfterFork` calls matches the `BeforeFork` above,
    // whichever side of the fork we are on — including the side that has no
    // child to talk about, which is the parent's.
    match outcome {
        Ok(Fork::Child) => {
            if interpreter {
                // SAFETY: we are in the child of the fork above and the GIL is
                // held; this call runs before anything else in this process
                // touches the interpreter.
                unsafe { pyo3::ffi::PyOS_AfterFork_Child() };
            }
        }
        _ => {
            if interpreter {
                // SAFETY: we are in the parent of the fork above (or it did not
                // happen) and the GIL is held.
                unsafe { pyo3::ffi::PyOS_AfterFork_Parent() };
            }
        }
    }
    outcome
}
