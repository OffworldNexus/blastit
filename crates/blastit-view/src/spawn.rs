//! The seam between the composition root and a view crate: what a forked view
//! child is, and how the fork itself happens (`view-crates-architecture` §4.4
//! D1/D2).
//!
//! A view crate cannot name `blastit` — the arrow is one-way — so the boot data
//! a view child needs travels here instead, in values that are only ever
//! *values*: no `Config`, no `RouteInfo`, no path to the manifest.
//!
//! * [`SpawnContext`] is the boot data: the route's name and uid, the channel's
//!   frame limits, the child's `RLIMIT_*`, its identity ([`Privileges`]), its
//!   filter ([`Filter`]), whether it may reach the network, and the signal mask
//!   stage 1 saved before the fork. What is deliberately **not** in it: a
//!   directory (each crate's own spawn type adds one field — one value, never
//!   two that could disagree) and a report socket (it is an entry of
//!   `ChildIo::keep`, built per boot).
//! * [`ForkProtocol`] and [`Forker`] are the fork syscall as a parameter: the
//!   composition root hands a view crate `fork_for`, which runs CPython's three
//!   `PyOS_*Fork` hooks under the GIL when an interpreter is live (§9.3). A
//!   view crate asks for the protocol its image needs and never calls `fork(2)`
//!   itself, so there is exactly one GIL protocol in the tree.
//!
//! # Why the protocol is a value, not a proof
//!
//! The type system cannot prove "the GIL is held": a token proving it can only
//! be built inside the crate that owns `pyo3`, and putting `pyo3` in this crate
//! is what `ADR-0016` forbids. So the one implementation
//! (`blastit::forking::fork_for`) refuses a protocol that disagrees with the
//! image instead, and a unit test pins that refusal — a silent bare `fork(2)`
//! under a live interpreter is the failure mode this type names.
//!
//! # Step 0
//!
//! The protocol's child half (`PyOS_AfterFork_Child()` and any handler a Python
//! extension registered) runs privileged and unfiltered, before
//! `blastit_sys::child::enter`. Nothing may `dlopen`, open a file or mutate the
//! environment between the fork and `enter`.

use std::io;

use blastit_frame::Limits;
use blastit_sys::child::{Filter, Privileges};
use blastit_sys::{Fork, Rlimits, SignalMask};

/// Which fork protocol an image needs for one fork.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkProtocol {
    /// CPython is live in this image and the caller holds the GIL: the fork
    /// runs `PyOS_BeforeFork` / `PyOS_AfterFork_Parent` /
    /// `PyOS_AfterFork_Child` around it (§9.3).
    InterpreterLive,
    /// No interpreter is in this image: the fork is the bare `fork(2)`.
    NoInterpreter,
}

impl ForkProtocol {
    /// The protocol's name, for an error sentence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InterpreterLive => "interpreter-live",
            Self::NoInterpreter => "no-interpreter",
        }
    }
}

/// The fork syscall, injected: the caller's one implementation of the protocol
/// above.
///
/// `Err` is whatever the composition root's fork refused with, in particular
/// `blastit_sys::Error::NotSingleThreaded`.
pub type Forker<'a> = &'a dyn Fn(ForkProtocol) -> io::Result<Fork>;

/// The boot data every view child is forked from (`view-crates-architecture`
/// §4.4 D2).
///
/// Built by stage 1 and copied into the view crate's own spawn type; a view
/// crate reads it and never constructs a [`Privileges`] or a [`Filter`].
#[derive(Debug, Clone)]
pub struct SpawnContext {
    /// The route's name: the child's log prefix, its `READY` report and the
    /// `blastit.route` the application sees.
    pub route: String,
    /// The uid the route runs under.
    pub uid: u32,
    /// The frame size limit of `view.chan` (`BLASTIT_FRAME_MAX`, §3.1).
    pub frame_limits: Limits,
    /// `RLIMIT_*` for the child (§7.2): `nproc` only when the child drops
    /// privileges, `nofile` from the view budget, and no core file.
    pub rlimits: Rlimits,
    /// The identity the child keeps: `DropTo(uid)` in production,
    /// `Shared` in `dev`/`check`.
    pub privileges: Privileges,
    /// The filter the child installs: `Seccomp(View { egress })` in every mode,
    /// `Off` only for an explicit `BLASTIT_SECCOMP=0`.
    pub filter: Filter,
    /// Whether the route's filter keeps `AF_INET`/`AF_INET6` (§7.3).
    pub egress: bool,
    /// The signal mask stage 1 saved before the fork, to restore in the child.
    pub mask: SignalMask,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use blastit_sys::child::{Filter, Privileges};
    use blastit_sys::{Rlimits, SeccompRole, current_signal_mask};

    use super::*;

    /// The protocol a forker is asked for is the one it is handed, and
    /// [`SpawnContext`] carries the boot data through unmapped: a view crate
    /// reads the composition root's values, it does not re-derive them.
    #[test]
    fn a_forker_is_asked_for_the_protocol_and_the_context_is_verbatim() {
        let asked = Cell::new(None);
        let forker: Forker<'_> = &|protocol| {
            asked.set(Some(protocol));
            // A real fork would leave a child that never returns; the value
            // this test cares about is the argument, so the answer is a refusal.
            Err(io::Error::other("not forking in a unit test"))
        };
        assert!(forker(ForkProtocol::NoInterpreter).is_err());
        assert_eq!(asked.get(), Some(ForkProtocol::NoInterpreter));

        let ctx = SpawnContext {
            route: "web".to_string(),
            uid: 1000,
            frame_limits: Limits::default(),
            rlimits: Rlimits {
                nproc: None,
                nofile: Some(1024),
                core_zero: true,
            },
            privileges: Privileges::DropTo(1000),
            filter: Filter::Seccomp(SeccompRole::View { egress: true }),
            egress: true,
            mask: current_signal_mask().unwrap_or_else(|err| panic!("mask: {err}")),
        };
        assert_eq!(ctx.route, "web");
        assert_eq!(ctx.uid, 1000);
        assert_eq!(ctx.privileges, Privileges::DropTo(1000));
        assert_eq!(
            ctx.filter,
            Filter::Seccomp(SeccompRole::View { egress: true })
        );
        assert!(ctx.egress);
    }
}
