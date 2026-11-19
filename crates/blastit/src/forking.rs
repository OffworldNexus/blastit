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
//!   live*. Those forks arrive here with `Some(hooks)`;
//! * a build with `python` off (`MB.3.11`, ADR-0010 §4) has no interpreter in
//!   any role: the three hooks are a phantom and [`fork`] is `fork(2)`.
//!
//! This module names no interpreter API: the three entry points are function
//! pointers the composition root injects ([`crate::py::runtime_hooks`] today,
//! a shim's vtable once one is loaded), so a fork is the same call whether the
//! interpreter is embedded or hosted.
//!
//! The fork itself is still [`blastit_sys::fork`], which refuses to run in an
//! image with more than one thread: the protocol makes a *Python* process safe
//! to fork, but it says nothing about threads CPython did not start, and §2.1's
//! invariant is what keeps every child's address space sane.

use std::io;

use blastit_sys::Fork;
use blastit_view::spawn::ForkProtocol;

use crate::py::PyForkHooks;

/// Forks for a view crate, refusing a protocol that disagrees with this image
/// (`view-crates-architecture` §4.4 D1, S4).
///
/// A view crate cannot call [`fork`] itself — it does not own the protocol, and
/// it cannot name an interpreter — so the composition root hands it a `Forker`
/// that ends here. `protocol` is what the caller's image needs and the hooks
/// are what *this* image actually has; a pair that disagrees is an error,
/// because the alternative is a silent bare `fork(2)` under a live interpreter
/// (CPython's locks and thread bookkeeping would then describe a process that no
/// longer exists) or a `PyOS_BeforeFork` with no interpreter behind it.
///
/// # Errors
///
/// [`io::ErrorKind::Other`] for a mismatched protocol, and whatever [`fork`]
/// refuses with — in particular [`blastit_sys::Error::NotSingleThreaded`].
///
/// A build without `python` forks no view at all, so its one caller (the WSGI
/// arm of the supervisor) is compiled out and the function has no user there.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub fn fork_for(protocol: ForkProtocol, hooks: Option<PyForkHooks<'_>>) -> io::Result<Fork> {
    agrees(protocol, hooks.is_some())?;
    fork(hooks)
}

/// Whether a caller's protocol is the one this image can honour.
///
/// The pure half of [`fork_for`], so both refusals are unit-tested without a
/// fork (a case that forks would leave a child that never returns).
fn agrees(protocol: ForkProtocol, interpreter_live: bool) -> io::Result<()> {
    match protocol {
        ForkProtocol::InterpreterLive => {
            // A build without `python` links no interpreter, so no caller can
            // be in this case: naming it is a bug, and a bare fork is exactly
            // what the protocol exists to prevent.
            if !cfg!(feature = "python") {
                return Err(io::Error::other(
                    "fork protocol mismatch: the caller asked for a live interpreter and this \
                     blastit build has none (it was built without the `python` feature)",
                ));
            }
            if !interpreter_live {
                return Err(io::Error::other(
                    "fork protocol mismatch: the caller asked for a live interpreter and this \
                     image reports none, so the fork would skip `PyOS_BeforeFork`",
                ));
            }
        }
        ForkProtocol::NoInterpreter => {
            if interpreter_live {
                return Err(io::Error::other(
                    "fork protocol mismatch: an interpreter is live in this image, so this fork \
                     must run `PyOS_BeforeFork` and its two matching halves",
                ));
            }
        }
    }
    Ok(())
}

/// Forks, stopping the world when this image has a live interpreter (§9.3).
///
/// `hooks` is `Some` exactly when CPython is initialised in this image and the
/// caller holds the GIL — a wheel install, or a stage 1 that has loaded the
/// project's runtime. `None` is a PHP/static image (and every standalone stage 0
/// fork), which is a bare `fork(2)`.
///
/// # Errors
///
/// Whatever [`blastit_sys::fork`] refuses with, in particular
/// [`blastit_sys::Error::NotSingleThreaded`]: the protocol is what makes the
/// fork consistent for the interpreter, not a licence to fork a process with
/// threads blastit knows nothing about.
pub fn fork(hooks: Option<PyForkHooks<'_>>) -> io::Result<Fork> {
    // One call site for the whole GIL protocol, and the only one in the crate.
    // A build with `python` off has a phantom hooks type and every caller passes
    // `None`, so the three calls are compiled out with it.
    #[cfg(feature = "python")]
    if let Some(hooks) = &hooks {
        (hooks.before_fork)();
    }
    #[cfg(not(feature = "python"))]
    let _ = &hooks;
    let outcome = blastit_sys::fork();
    // Exactly one of the two `AfterFork` calls matches the `BeforeFork` above,
    // whichever side of the fork we are on — including the side that has no
    // child to talk about, which is the parent's.
    #[cfg(feature = "python")]
    match outcome {
        Ok(Fork::Child) => {
            if let Some(hooks) = &hooks {
                (hooks.after_fork_child)();
            }
        }
        _ => {
            if let Some(hooks) = &hooks {
                (hooks.after_fork_parent)();
            }
        }
    }
    outcome
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// The mismatch refusal, which is the mechanism `ForkProtocol` has instead
    /// of a type that could prove the GIL is held: the protocol cannot be
    /// checked at compile time, so the one implementation refuses the wrong
    /// answer. No case here forks — a fork would leave a child that never
    /// returns.
    #[test]
    fn a_fork_whose_protocol_disagrees_with_the_image_is_refused() {
        // An interpreter-live fork where the image says there is none.
        let err = agrees(ForkProtocol::InterpreterLive, false).unwrap_err();
        assert!(err.to_string().contains("fork protocol mismatch"), "{err}");

        // And the other way round: a bare fork while an interpreter is live.
        let err = agrees(ForkProtocol::NoInterpreter, true).unwrap_err();
        assert!(err.to_string().contains("fork protocol mismatch"), "{err}");
        assert!(err.to_string().contains("PyOS_BeforeFork"), "{err}");
    }

    /// A `python`-less build has no interpreter behind the protocol at all, so
    /// even a caller that claims one is live is refused: a bare `fork(2)` under
    /// that spelling is the silent case the enum exists to make impossible.
    #[cfg(not(feature = "python"))]
    #[test]
    fn a_build_without_the_python_feature_refuses_an_interpreter_live_fork() {
        let err = agrees(ForkProtocol::InterpreterLive, true).unwrap_err();
        assert!(
            err.to_string().contains("without the `python` feature"),
            "{err}"
        );
    }
}
