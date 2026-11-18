//! The interpreter token that travels down stage 1's call chain (`MB.3.11`).
//!
//! A role's entry point knows whether CPython is in the image: a wheel install
//! calls [`crate::cli::main`] from `blastit._blastit.main()` and hands the
//! interpreter over, while the standalone binary passes `None` and stage 1
//! embeds one itself (§4, §9.3). From there the token is the same object all
//! the way down to the one fork that needs it: stage 1's `serve`, the
//! supervisor's loop, and [`crate::forking::fork`].
//!
//! A build with `python` off (`--no-default-features --features php`, the
//! standalone archive of ADR-0010 §4) has no interpreter and links no
//! `libpython`, but the call chain is the same code. It therefore carries
//! [`Python`] as a token that can never be attached to anything, instead of a
//! second copy of every signature. Every interpreter-facing function is
//! compiled out by the same feature, so a token is all that is left to carry.
//!
//! # Why this module is still here
//!
//! The *worker* code that needs a real interpreter — the WSGI request path, the
//! child half of the view fork, the immortalisation pass — lives in
//! `blastit-view-python`, which re-exports the interpreter's own `Python` type.
//! This module is the **composition root's** token: stage 1 and the supervisor
//! carry it, and a build with no interpreter carries a phantom so the same
//! signatures compile. It cannot move into a view crate: the no-interpreter
//! token belongs to the crate that decides whether an interpreter exists at all.

/// The calling thread's attachment to the interpreter, or the token of a build
/// that has none.
#[cfg(feature = "python")]
pub(crate) use pyo3::Python;

/// The token of a build that carries no interpreter.
///
/// There is nothing behind it and nothing that can be done with it: it exists so
/// that `run(py)`, `spawn(py, …)` and their callers keep one shape in both
/// builds. Every function that would *use* an interpreter is behind
/// `#[cfg(feature = "python")]`.
#[cfg(not(feature = "python"))]
pub(crate) type Python<'py> = std::marker::PhantomData<&'py ()>;

/// The token to hand a function in a build with no interpreter.
#[cfg(not(feature = "python"))]
pub(crate) fn no_interpreter<'py>() -> Python<'py> {
    std::marker::PhantomData
}
