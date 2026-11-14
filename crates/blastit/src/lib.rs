//! blastit: init (PID 1), its stages and roles (`ARCHITECTURE.md` §2.1, §4).
//!
//! The crate is a library and a binary.
//!
//! * The **binary** is the standalone install: `blastit`, root, PID 1, playing
//!   every role out of one image (§2).
//! * The **library** is that same code, reached through the pyo3 extension a
//!   wheel installs (`blastit._blastit`). There the console script runs the
//!   roles on the interpreter the wheel was installed into — the venv's, whose
//!   `site-packages` the application's dependencies live in — and stage 1 uses
//!   that interpreter instead of embedding a second one (§9.3, D2).
//!
//! [`cli::main`] is the whole entry point, and [`Host`] is the only thing that
//! differs between the two installations: which program the two `execve`s of §4
//! name, and whether stage 1 finds an interpreter that is already running.
//!
//! # Features
//!
//! `python` is every embedded-CPython role: the `BLASTIT_APP` import, the WSGI
//! views, the immortalisation pass, `blastit dev --learn` and the
//! `PyOS_*Fork` protocol around a fork. A build without it — the standalone
//! archive of `ADR-0010` §4 — links no `libpython`, serves no WSGI route, and
//! refuses a manifest that declares one, at boot, with the message naming the
//! package that can serve it. `php` is the embedded-PHP worker (`MB.3.12`,
//! `MB.3.13`): it forwards `blastit-view/php` and adds `blastit-php-sapi` (the
//! frozen ABI plus the `dlopen` loader), and nothing in this crate links
//! `libphp` — the host's NTS library and the per-minor shim are `dlopen`ed
//! inside a view, so the standalone binary keeps no `DT_NEEDED` on either.

// A build with no interpreter compiles the same modules; what only an
// interpreter can reach carries its own `allow(dead_code)` where it lives, so an
// orphan caused by a missed `cfg` is still a warning instead of hiding here.

pub mod audit_v1;
pub mod check;
pub mod cli;
pub mod config;
pub mod host;
pub mod infer_routes;
pub mod mem_usage;

mod child;
mod dev_learn;
mod forking;
#[cfg(feature = "python")]
mod immortal;
mod logs;
// Embedded PHP view workers (`MB.3.13`): the fork path sibling to `zygote`'s
// WSGI one. It compiles only when PHP routes can be served, and `libphp`
// itself is `dlopen`ed inside the worker, never linked here. The supervisor's
// `Ctl::Spawn` picks a route's binding (`MB.3.13.3`); until then nothing in
// this crate calls the fork, so the seam is explicit rather than a warning.
#[cfg(feature = "php")]
#[allow(dead_code)]
mod php_view;
mod py;
mod roles;
mod stage0;
mod stage1;
mod supervise;
mod visibility;
#[cfg(feature = "python")]
mod zygote;

// The worker transport (`view.chan` framing and flow control) is part of this
// build from the start: `MB.3.13` drives it from the embedded-PHP worker. No
// role calls it yet, so the reference is explicit rather than a dependency that
// reads as forgotten.
#[cfg(not(feature = "python"))]
use blastit_view as _;

pub use crate::host::{Host, Reexec};
