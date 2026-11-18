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
//! # This crate is the composition root
//!
//! The per-language worker code is not here any more: a WSGI view's child half
//! is `blastit-view-python` (`zygote`, `immortal`), and an embedded-PHP worker
//! is on its way to `blastit-view-php`. What stays is what only the root can
//! know or do: the CLI, stage 0 and stage 1, the supervisor, the role wiring,
//! `forking` — the one CPython fork protocol, handed to a view crate as a
//! `Forker` — and the mapping from this crate's `Config` into the view crates'
//! own boot values (`SpawnContext`). No view crate depends on this one
//! (`ADR-0016`, design `view-crates-architecture.md` §4.4).
//!
//! `py` is part of that root role too: it is the token stage 1 and the
//! supervisor carry down the call chain. In a `python` build it re-exports the
//! interpreter's own type (which `blastit-view-python` also re-exports, as the
//! crate that owns the interpreter); in a build without the feature it is a
//! phantom token, so the same signatures compile with no interpreter anywhere.
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
// The `dev` live-reload policy (`MB.23.5.2`, `ADR-0014`). `pub` for now because
// the supervisor that consumes it is the next leaf: a private module with no
// caller would be a wall of `dead_code` for code that is already tested.
pub mod dev_reload;
pub mod host;
pub mod infer_routes;
// `blastit lsp` is one delegation: the server is `crates/blastit-lsp`, which
// builds on macOS and Windows where this crate cannot serve (`MB.35.4`,
// `ADR-0013`). The re-export keeps `crate::lsp::run()` — and the capability
// drop `cli.rs` performs before it — compiling untouched.
pub use blastit_lsp as lsp;
pub mod mem_usage;

mod child;
mod dev_learn;
mod forking;
mod logs;
// Embedded PHP lives in `blastit-view-php` (`MB.36.5`): the worker, the boot's
// discovery gate, the `(st_dev, st_ino)` pins and the probe child. Nothing about
// PHP is left in this crate but the wiring — `stage 0` resolves the host's
// library through the gate, `stage 1` builds the `PhpSlot`s and the bindings,
// and `init`'s `Ctl::Spawn` reads the binding (`ADR-0016`).
mod py;
mod roles;
mod stage0;
mod stage1;
mod supervise;
mod visibility;

// The worker transport (`view.chan` framing and flow control) is part of this
// build from the start: `MB.3.13` drives it from the embedded-PHP worker. No
// role calls it yet, so the reference is explicit rather than a dependency that
// reads as forgotten.
#[cfg(not(feature = "python"))]
use blastit_view as _;

pub use crate::host::{Host, Reexec};
