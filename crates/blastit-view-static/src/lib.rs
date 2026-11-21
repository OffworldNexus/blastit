//! The static view kind: the one responder the scheduler calls for a `static`
//! mount, and no process of its own (ADR-0016, design §4.2.4).
//!
//! A static route owns no uid, no `view.chan` and no seccomp role: the answer is
//! one `openat2` beneath a dirfd the scheduler opened at boot, and one `pread`
//! per chunk that the scheduler's own credit window already bounds
//! (`ARCHITECTURE.md` §9.2). So this crate is a **library** the scheduler links,
//! not a view process: `static_asset` resolves a decoded request path,
//! `rewrite` decides which of the five shapes answers it, and `static_response`
//! builds the head (MIME, `ETag`, `Last-Modified`, the cache rule, `304`).
//!
//! It is compiled only when `blastit-scheduler`'s `static` feature is on, and
//! that feature is a **default** one: a manifest that declares a mount is
//! refused at boot in a build without it (`SchedulerError::StaticNotCompiled`),
//! never answered with a file this binary cannot prove it opened.
//!
//! Nothing here links an interpreter: the crate is reachable from the
//! standalone binary, so it depends on `blastit-view` with
//! `default-features = false` and on no PHP surface at all.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod rewrite;
pub mod static_asset;
pub mod static_response;
