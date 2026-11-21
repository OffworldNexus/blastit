---
id: MB.9.2.1
title: "UpstreamHead, RouteRuntime.stages, run_with and the Done slot rule - design MB.9.2a"
kind: epic
status: superseded
priority: 2
depends_on: [MB.9.1]
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/error.rs"
  - "crates/blastit-scheduler/tests/head_allocs.rs"
  - "crates/blastit-frame/src/raw.rs"
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/roles.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-17T14:07"
---

# Goal

Land the scheduler side of the contract: build `UpstreamHead` once per request, hold the
per-route stage chain beside the descriptor (`RouteRuntime { info, stages }`), construct it
at boot in `run_with` from a `Registry`, give the view side a `SessionPort`, make `Done` the
only event that releases a slot, and expose the boot surface to the composition root.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.9.2.1.1` | `blastit-backend` + `blastit-http` edges + `Cargo.lock`, `ReqHeadView::scheme()`, `server_name`/`server_port`, `RouteInfo.executor`, `SchedulerConfig.{http_obs, pool_spawn, pinned_sockets, route_lines}` + `pinned_socket_fds()`, `stage1.rs` literal | `MB.9.1` |
| `MB.9.2.1.2` | `UpstreamHead`, `RouteRuntime.stages`, `ViewHub`/`SessionPort`, the session task + `SessionDoneGuard`, `ResponseSink` proxy-control denial, `http_obs` writer, `run_with`/`RunError`/`SchedulerError::Boot`, `builtin_registry()` + `run`, `mod upstream;`, `BuildCtx.external_executor`, the transitional `stages.is_empty()` path | `MB.9.2.1.1` |
| `MB.9.2.1.3` | `lib.rs` re-exports only, `roles::scheduler` `run_with` + `RunError` print | `MB.9.2.1.2` |

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  `WorkerRuntime`, with the shapes restated here (injection, the dispatch call site, the
  `Session`/`SessionMsg` seam, `run_with`/`RunError`/`builtin_registry`, the transitional
  rule, `UpstreamHead`, `SessionPort`, `ViewHub` + the session task); the retired design is
  deleted with `ADR-0010`.
- **`.1` runs its first cargo command offline without `--locked`** (it owns the third lock
  link) then `--locked`; it adds `SchedulerConfig.route_lines` and owns
  `crates/blastit/src/stage1.rs` (the `SchedulerConfig` literal ends in
  `..SchedulerConfig::default()`, so an omitted field is a silent empty `route_lines` and
  every route is a `BootError`), with `cargo test -p blastit --locked` in its acceptance.
- **`.2` owns `lib.rs`** and declares `mod upstream;` at the crate root (so `crate::upstream`
  resolves to `src/upstream/mod.rs`); it defines `RunError`/`SchedulerError::Boot` in
  `src/error.rs`, `run_with`, **`builtin_registry()` and `run` delegating to `run_with`**,
  `BuildCtx.external_executor` from `pool_spawn`, the transitional `stages.is_empty()` path,
  and `SessionDoneGuard` (`Drop` → `Done(Fault)`) + the `ResponseSink` proxy-control denial.
- **`.3` only re-exports** `builtin_registry`/`run_with` and `RunError`/`SchedulerError` from
  `lib.rs` and adds the `roles::scheduler` `RunError` print — no definitions, no second
  `mod upstream;`.
- No child edits a `Cargo.toml` except `.1`; only `.1` lists `Cargo.lock`.

# Acceptance

- [ ] All three children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit-scheduler --test critical_static_assets --test critical_static_resolve --test critical_rewrite --test critical_ingress_enforcement --locked
$ cargo test -p blastit --locked
$ grep -q 'struct RouteRuntime' crates/blastit-scheduler/src/scheduler.rs && echo "handle lives beside the descriptor"
$ grep -q 'pub fn builtin_registry' crates/blastit-scheduler/src/scheduler.rs && grep -q 'pub use crate::scheduler::{builtin_registry, run_with};' crates/blastit-scheduler/src/lib.rs && echo "defined in scheduler.rs, re-exported from lib.rs"
$ grep -q 'SessionMsg::Done' crates/blastit-scheduler/src/scheduler.rs && echo "Done is the only slot-release event"
$ make check
```
- [ ] The protected suites pass **unmodified** at the end of every leaf.

# Notes

- The static-specific slot plumbing (`Request.pumping`, `Scheduler.static_in`,
  `release_static_slot`) can only be deleted with the last static branch, so it survives
  until `MB.9.2.2`.
- `StaticUpstream` is `MB.9.2.2`'s; `"wsgi"` is `MB.9.3`'s; `"php"` is `MB.3.6`'s
  (`PhpEmbedRuntime`, `MB.3.13`). This epic registers nothing.
- Determinism: no Docker, no PHP runtime, no sleeping.
