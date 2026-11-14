---
id: MB.9.2.1.2
title: "RouteRuntime.stages, ViewHub/SessionPort, session task, run_with and builtin_registry - design MB.9.2a part 2"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.2.1.1]
paths:
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/error.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/head_allocs.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Land the scheduler's contract seam: build `UpstreamHead` once per request, hold the
per-route stage chain beside the descriptor (`RouteRuntime { info, stages }`), give the
view side a `SessionPort`, run one `tokio::spawn`ed session task per request guarded by
`SessionDoneGuard`, add `run_with`/`RunError`/`builtin_registry()`, and make `Done` the
only event that releases a slot (design leaf `MB.9.2a`, part 2).

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  `WorkerRuntime`, with the shapes restated here (the dispatch call site, the
  `Session`/`SessionMsg` seam, `run_with`/`RunError`/`builtin_registry`, the transitional
  `stages.is_empty()` rule, `ResponseSink` proxy-control denial, `ViewHub` + the session
  task); the retired design is deleted with `ADR-0010`.
- Files (the whole leaf, 5): `crates/blastit-scheduler/src/{upstream/mod,scheduler,error,lib}.rs`
  and `crates/blastit-scheduler/tests/head_allocs.rs`.
- What lands:
  - `UpstreamHead` construction in `begin_raw` from `ReqHeadView` (`String::from_utf8_lossy`
    everywhere, never a `from_utf8` unwrap), deriving `body: BodyFraming` /
    `expect_continue: bool` from `Content-Length`/`Transfer-Encoding`/`Expect:
    100-continue` **before** `Expect` is stripped; `tests/head_allocs.rs` documents the
    bounded copy (not a protected suite).
  - `struct RouteRuntime { info: RouteInfo, stages: Vec<Arc<dyn Upstream>> }` (1 or 2),
    `Scheduler.routes: Vec<RouteRuntime>`, `Request.stage: u8`; no
    `Option<Box<dyn …>>`, no lookup by name, no rebuild.
  - `ViewHub` (`SessionPort`) wrapping the existing per-view `ready`/`inflight` state, the
    request-id counter and the `dispatch` map — **moved, not duplicated**; `Scheduler`
    holds `Arc<ViewHub>`. The one session task (cloned `http_out`, local sink,
    `Box<dyn BodySource + Send>`, `Arc<dyn Upstream>`, `RoutePermit`) sends
    `SessionMsg::Done`; `SessionDoneGuard` holds `Option<mpsc::Sender<SessionMsg>>`
    (capacity `1`) and its `Drop` sends `Done { id, result: Err(Fault { detail: "session
    task panicked" }) }` only if the sender is still there — exactly one `Done`, and a
    panic never unwinds `run_with`.
  - `ResponseSink` proxy-control denial: refuse exactly `X-Accel-Redirect`, `X-Sendfile`,
    `X-Lighttpd-Send-File`, `X-Rewrite-URL` (`502` before any head), drop every other
    `X-Accel-*` name (`X-Accel-Buffering` stays `200`, header absent).
  - `http_obs.jsonl` writer for `SchedulerConfig.http_obs`.
  - `crates/blastit-scheduler/src/error.rs`: `pub enum RunError { Boot(BootError),
    Scheduler(SchedulerError) }` + `SchedulerError::Boot(BootError)` declared
    `#[error("{0}")]` with **no** `#[source]`.
  - `scheduler.rs`: `run_with(registry, cfg, routes, table, http, ctl, emit)` and
    **`builtin_registry() -> Result<Registry, BootError>` defined here** (fallible
    `Registry::new` over a fixed list, no `expect`/`unwrap`), with `run` keeping its 0.2.0
    signature and delegating through `RunError::into_scheduler`.
  - `lib.rs`: `mod upstream;` declared at the **crate root** so `crate::upstream` resolves
    to `src/upstream/mod.rs`; `.3` only appends `pub use` lines.
  - `BuildCtx.external_executor` set from `cfg.pool_spawn == PoolSpawn::External`, and the
    transitional `stages.is_empty()` path (a route with no registered builder keeps the
    0.2.0 in-place path; it is a shape check, **not** a `kind` match).
- No `Cargo.toml`, no `Cargo.lock`: the edges landed in `MB.9.2.1.1`. Keep `--locked`.

# Acceptance

```console
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit-scheduler --test critical_static_assets --test critical_static_resolve --test critical_rewrite --test critical_ingress_enforcement --locked
$ grep -q 'struct RouteRuntime' crates/blastit-scheduler/src/scheduler.rs && grep -q 'stages' crates/blastit-scheduler/src/scheduler.rs && echo "handle lives beside the descriptor"
$ grep -q 'pub fn run_with' crates/blastit-scheduler/src/scheduler.rs && grep -q 'pub fn builtin_registry' crates/blastit-scheduler/src/scheduler.rs && echo "registry-driven boot, defined in scheduler.rs"
$ grep -q 'SessionMsg::Done' crates/blastit-scheduler/src/scheduler.rs && echo "Done is the only slot-release event"
$ make check
```

The first command is the crate root, **not** a `--lib` subset.

# Notes

- Protected suites pass unmodified. The static-specific slot plumbing
  (`Request.pumping`, `Scheduler.static_in`, `release_static_slot`) can only be deleted
  with the last static branch, so it stays until `MB.9.2.2` — removing it here breaks
  static serving.
- `StaticUpstream`, `pub mod static;` and the `"static"` registration are `MB.9.2.2`'s;
  `"wsgi"` is `MB.9.3`'s. This leaf registers nothing.
- Determinism: no Docker, no pool, no sleeping; `head_allocs` is a documented number, not
  a benchmark.
