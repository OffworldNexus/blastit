//! The PHP half of a view process: the embedded `libphp` worker, its
//! `view.chan` loop, the boot's discovery gate, the `(st_dev, st_ino)` pins and
//! the contained probe child (ARCHITECTURE.md §2.5, §9.3, `ADR-0010`, `ADR-0016`).
//!
//! This crate is the **only** place in the tree that knows `libphp`, the C SAPI
//! shim, the per-minor ELF probes, the entry pin and the probe child. It builds
//! on [`blastit_view`]'s transport — the frames ([`blastit_view::wire`]), the
//! credit windows ([`blastit_view::flow`]), the loaded-file datagram and the
//! [`blastit_view::ViewError`] a broken channel is reported as — and never
//! invents a second framing.
//!
//! # Two halves, one crate, one ordering rule (design §4.4 D5)
//!
//! * [`discovery`] runs in **stage 0**: the ELF `.dynsym` inspection, the
//!   candidate order, the refusal sentences, `PhpSearch`, `PhpBoot` and the
//!   deployment's provenance gate. It names no SAPI type and no `dlopen`, so
//!   `init` can resolve `entrypoint php` on a host where the worker could never
//!   start, and a host with no usable `libphp` is refused before a listener
//!   binds.
//! * [`pinning`] and [`probe`] sit between: the boot re-opens the two objects
//!   stage 0 inspected and compares `(st_dev, st_ino)`, records the resolved
//!   entry and holds its inode, and a contained child proves the pair loads.
//! * [`worker`] and [`spawn`] are the only modules that name the SAPI: the
//!   worker `dlopen`s `/proc/self/fd/<n>` and serves one request at a time.
//!
//! # Layout
//!
//! * [`worker`] — [`PhpEmbedRuntime`], [`PhpRuntimeConfig`], the single-request
//!   `view.chan` loop ([`serve_forever`]) and the [`RequestIo`] the shim writes
//!   through.
//! * [`pinning`] — [`PhpBinding`], [`EntryPin`], `boot_bindings`, `pin_entry`,
//!   `verify_entry`, and the debug-only swap-point rendezvous `stage 1` pauses
//!   at so an inode leg has a deterministic window.
//! * [`probe`] — `PhpProbeBoot`, `probe_bindings` and the probe child: one
//!   contained process per file that runs `php_module_startup` before anything
//!   listens.
//! * [`spawn`] — `fork_php_view` and the worker child: the fork is injected
//!   ([`blastit_view::spawn::Forker`]), so the one CPython protocol stays in the
//!   composition root.
//! * [`discovery`] — the stage-0 gate: no `dlopen`, no worker, plain `std`.
//!
//! # Code
//!
//! The crate is `#![forbid(unsafe_code)]`: every syscall it makes is a `rustix`
//! or `blastit-sys` wrapper, and the child sequence is
//! [`blastit_sys::child::enter`]. Nothing here defines `unsafe`.
//!
//! This crate never depends on `blastit`: the composition root passes boot data
//! in values (`PhpSlot`, `PhpBootLimits`, `SpawnContext`) and receives the
//! fork's result, so the arrow stays one-way (`ADR-0016`, design §4.1).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![forbid(unsafe_code)]

pub mod discovery;
pub mod pinning;
pub mod probe;
pub mod spawn;
pub mod worker;

pub use crate::discovery::{PhpBoot, PhpRefusal, PhpRuntime, PhpSearch, PhpShimScope, PhpVersion};
pub use crate::pinning::{EntryPin, PhpBinding, PhpBootLimits, PhpSlot};
pub use crate::probe::{PhpProbeBoot, PhpProbeReport, probe_bindings};
pub use crate::spawn::{ForkedPhpView, PhpViewSpawn, fork_php_view};
// The transport's error type, re-exported so a caller of [`serve_forever`] does
// not have to name two crates for one result.
pub use crate::worker::{
    PhpEmbedRuntime, PhpError, PhpRuntimeConfig, RequestEnd, RequestIo, serve_forever,
};
pub use blastit_view::ViewError;
