//! LLVM coverage profiles for an image that ends with `_exit` (§8, `MB.40.2`).
//!
//! Every role of a `blastit` boot ends with [`libc::_exit`](libc::_exit): init
//! cannot unwind, because it may be running with the GIL held or inside a
//! signal's handler, and a `fork`ed view must not run its parent's destructors.
//! That is the right behaviour for a server and the wrong one for a coverage
//! build: `_exit` skips the atexit list the coverage runtime registers, so an
//! instrumented run would stop at the first `_exit` — which is every role's
//! exit — and report nothing an end-to-end run ever did.
//!
//! # Which gate
//!
//! **Not `cfg(coverage)`.** `rustc -C instrument-coverage` emits no such cfg:
//! `rustc --print cfg -C instrument-coverage | grep coverage` prints nothing, so
//! a function gated on it would be the empty function in exactly the build that
//! needs it — silently, with green tests. The gate is this crate's build script
//! (`build.rs`), which turns `BLASTIT_COVERAGE=1` or the `coverage` cargo feature
//! into `cargo:rustc-cfg=coverage` and exports its decision as
//! `BLASTIT_COVERAGE_GATE`; `the_coverage_gate_is_the_build_scripts_decision`
//! fails if the cfg and that decision ever disagree.
//!
//! [`flush`] therefore calls the runtime's writer in an instrumented build and
//! compiles to an empty function otherwise, so a release binary carries no
//! symbol, no call and no branch. [`inherit`] is the other half: a child spawned
//! with `env_clear` (dbproxy, the suite) keeps `LLVM_PROFILE_FILE` so its profile
//! lands beside this process's instead of in the working directory. Both are
//! inert unless the build and the environment asked for them.

use std::process::Command;

/// The environment variable rustc's coverage runtime reads (`%p`, `%m`).
pub const PROFILE_FILE: &str = "LLVM_PROFILE_FILE";

// The coverage runtime's profile writer, linked into an instrumented image.
//
// A `//` comment: rustdoc generates nothing for an `extern` block, so a doc
// comment on it is an `unused_doc_comments` warning in exactly the coverage
// build that compiles it. `flush` below carries the contract.
#[cfg(coverage)]
unsafe extern "C" {
    // Writes this process's counters to `LLVM_PROFILE_FILE` and answers `0` on
    // success. Returns without writing when the variable names no file.
    fn __llvm_profile_write_file() -> std::os::raw::c_int;
}

/// Writes this process's coverage profile, before an `_exit` cannot.
///
/// A no-op in every build that is not instrumented: `flush` is then one empty
/// inline function, and `__llvm_profile_write_file` is not named at all.
pub fn flush() {
    #[cfg(coverage)]
    {
        // SAFETY: `__llvm_profile_write_file` is the coverage runtime rustc links
        // into an instrumented image; it reads this process's counters and writes
        // the file `LLVM_PROFILE_FILE` names. It is called once, from the exiting
        // thread, before `_exit`, and its return value carries nothing to act on.
        unsafe {
            __llvm_profile_write_file();
        }
    }
}

/// Hands `command` the profile file this process was given, when it has one.
///
/// Callers are the non-production spawns — `check`'s dbproxy and pytest — whose
/// `Command::env_clear` would otherwise drop the variable and send every child's
/// profile to a different file. When the variable is unset (every ordinary run)
/// nothing is added, so no child's environment changes.
pub fn inherit(command: &mut Command) {
    if let Some(value) = std::env::var_os(PROFILE_FILE) {
        command.env(PROFILE_FILE, value);
    }
}
