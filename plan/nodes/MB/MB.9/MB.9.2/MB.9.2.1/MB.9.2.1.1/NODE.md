---
id: MB.9.2.1.1
title: "Scheduler config and head plumbing: UpstreamHead accessors, RouteInfo.executor, SchedulerConfig fields - design MB.9.2a part 1"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.1]
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-frame/src/raw.rs"
  - crates/blastit/src/stage1.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Land the scheduler-side configuration and head plumbing the contract needs: the
`blastit-backend` + `blastit-http` dependency edges and lock, `ReqHeadView::scheme()`,
`server_name`/`server_port`, `RouteInfo.executor` with `with_php` after
`with_static`, and the four new `SchedulerConfig` fields that `stage1` must populate
(design leaf `MB.9.2a`, part 1).

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract, with the shapes restated
  here (`UpstreamHead`; the handle lives in `RouteInfo.executor`, `with_php` **after**
  `with_static`, the exact `static_forward` expression); the retired design is deleted with
  `ADR-0010`.
- Files (the whole leaf, 7): `Cargo.lock`, `crates/blastit-scheduler/Cargo.toml`,
  `crates/blastit-scheduler/src/{route,config,lib}.rs`, `crates/blastit-frame/src/raw.rs`,
  `crates/blastit/src/stage1.rs`.
- What lands:
  - `crates/blastit-scheduler/Cargo.toml`: `blastit-backend` **and** `blastit-http`
    workspace deps (the latter for `BodyFraming`).
  - `crates/blastit-frame/src/raw.rs`: `ReqHeadView::scheme() -> Scheme` — the
    frame-owned `blastit_frame::Scheme` root re-export, **no new `blastit-frame` edge**;
    `parse` stores `scheme: Scheme` via `Scheme::from_code(..)?`, rejecting an unknown
    code with `FrameError::BadEnum`, and the stale `parse` doc comment claiming the
    scheduler never reads it is updated. Plus `server_name`/`server_port` accessors.
  - `crates/blastit-scheduler/src/route.rs`: `RouteInfo.executor: Option<Executor>`
    (`RouteInfo::new` defaults to `Some(Executor::View)`), `with_static` then
    `with_php` in `routes_from_manifest`, `static_policy`/`static_forward` **stay** on
    the descriptor (protected suites read them).
  - `crates/blastit-scheduler/src/config.rs`: `SchedulerConfig.{http_obs: Option<PathBuf>,
    pool_spawn, pinned_sockets, route_lines}` (`http_obs` is `None` until `MB.3.6.3.1`
    wires `Config.check_http_obs`), `pinned_socket_fds() -> Vec<RawFd>`, and
    `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PoolSpawn { Managed, External }`.
    Every new public item carries a `///` doc comment (`lib.rs` is `#![deny(missing_docs)]`).
  - `crates/blastit/src/stage1.rs`: sets `route_lines` from `Manifest::route_lines()` and
    forwards `pool_spawn`/`pinned_sockets`/`http_obs` into the `SchedulerConfig` literal —
    the literal ends in `..SchedulerConfig::default()`, so omitting them is a silent empty
    `route_lines` and **every** route becomes a `BootError`.
- **`Cargo.lock` is yours and this is the third link of the release chain**
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`): the first build runs
  offline **without** `--locked` to write the lock, then `--locked` everywhere.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler
$ cargo test -p blastit-scheduler --locked
$ cargo tree -p blastit-scheduler -e normal | grep -q blastit-http && cargo tree -p blastit-scheduler -e normal | grep -q blastit-backend && echo "inward edges only"
$ cargo test -p blastit --locked
$ git diff --stat Cargo.lock && echo "lock updated in this leaf"
$ make check
```

# Notes

- No `RouteRuntime`, no `run_with`, no `ViewHub`: those are `MB.9.2.1.2`'s. This leaf only
  prepares the types and the config the next two consume.
- Protected suites (`critical_static_assets`, `critical_static_resolve`,
  `critical_rewrite`, `critical_ingress_enforcement`) must pass **unmodified**; if one
  must change, stop and report it as its own node.
- `crates/blastit-frame/Cargo.toml` is **not** in `paths` and gains no edge.
