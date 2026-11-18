//! The CPython half of a view process: PEP 3333, `wsgi.input` and the worker
//! pool that serves one route's WSGI application over `view.chan`
//! (ARCHITECTURE.md §2.5, §3.1, §3.2, §9.3).
//!
//! This crate is the **interpreter**, not the wire. `blastit-view` owns the
//! transport every view speaks — the frames ([`blastit_view::wire`]), the
//! credit windows ([`blastit_view::flow`]), the loaded-file datagram, and the
//! [`ViewError`] a broken channel is reported as — and this crate builds the
//! PEP 3333 `environ` on top of it, calls the application, streams the response
//! back through those credit windows, and runs the worker stack that does it
//! (§2.5). A view kind that embeds another interpreter (or a C SAPI) is a
//! sibling crate over the same transport, never a second framing.
//!
//! A view process is one route of the manifest, running under that route's uid.
//! By the time [`serve`] is called, `init` has already initialised CPython,
//! imported the application, taken the GIL, forked and called
//! `PyOS_AfterFork_Child()` while holding it (§4, §9.3); [`serve`] is what the
//! forked child runs — on that thread, with the GIL still held. It therefore
//! **never initialises or finalises the interpreter**: the interpreter and the
//! application are inherited, and touching either here would break the fork
//! contract.
//!
//! # Layout
//!
//! * [`serve`] — the I/O thread, the lazily grown worker stack,
//!   `READY`/`DONE` and the request lifecycle (its module documentation is the
//!   sequence, in full).
//! * `environ`, `input`, `request`, `response` — the PEP 3333 bridge: the
//!   `environ` dict, `wsgi.input`, the per-request state and
//!   `start_response`/the response body.
//! * [`WsgiConfig`] — what the process is told about its route.
//! * `loaded` — the dev-only loaded-file tracker: the module closure and the
//!   `open` audit hook a view installs (and inherits across the fork). The
//!   datagram it reports through is `blastit-view`'s codec.
//! * [`zygote`] — the **child half** of the view fork: `enter`, the private
//!   `HOME`/`TMPDIR`, and the hand-off of the inherited GIL to [`serve`]. The
//!   fork syscall itself is injected (`blastit_view::spawn::Forker`), so the
//!   one CPython fork protocol stays in the composition root.
//! * [`immortal`] — §9.2 #7's heap immortalisation pass, its version policy and
//!   its decision. It is the crate's only `unsafe` code (see below).
//!
//! # Code
//!
//! The crate is `#![deny(unsafe_code)]`, not `forbid`: everything that touches
//! the C API goes through pyo3's safe wrappers and nothing here starts or stops
//! the interpreter, with **one** exception — [`immortal`] writes CPython's
//! reference-count and `ob_flags` words directly (that is the whole point of
//! the pass), and its module carries the crate's only `#[allow(unsafe_code)]`.
//!
//! This crate never depends on `blastit`: the composition root passes the boot
//! data in (`SpawnContext`) and receives the fork's result, so the arrow stays
//! one-way (`ADR-0016`, design §4.4 D1/D2).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(unsafe_code)]

mod config;
mod environ;
pub mod immortal;
mod input;
mod loaded;
mod request;
mod response;
mod serve;
pub mod zygote;

pub use crate::config::WsgiConfig;
// The boot batch and its reporter (`MB.23.4`, `ADR-0014`): the Python half of
// the loaded-file report, which installs the audit hook and inherits it across
// the fork.
pub use crate::loaded::{LoadedReporter, LoadedTracker};
// The error a broken channel is reported as. It belongs to the transport
// (`blastit-view`), and re-exporting it here is what keeps a caller of `serve`
// from having to name two crates for one result.
pub use crate::serve::{serve, serve_reporting};
pub use blastit_view::ViewError;
// The interpreter token the composition root carries down its call chain. It is
// re-exported here, from the crate that owns the interpreter, so a caller can
// name it without a second `pyo3` path — `blastit/src/py.rs` stays behind only
// as the *no-interpreter* token of a build without this crate.
pub use pyo3::Python;
