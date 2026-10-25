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

pub mod cli;
pub mod host;

mod child;
mod config;
mod forking;
mod immortal;
mod logs;
mod roles;
mod stage0;
mod stage1;
mod supervise;
mod zygote;

pub use crate::host::{Host, Reexec};
