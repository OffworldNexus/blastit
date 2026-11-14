---
id: MB.9.3
title: "Generalize the WSGI port into ViewUpstream serving Executor::Wsgi and Executor::Php over an injected WorkerRuntime"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.9.2]
paths:
  - "crates/blastit-scheduler/src/upstream/wsgi.rs"
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/conformance_wsgi.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T14:39"
---

# Goal

Port the **WSGI forwarding path** onto the same `Upstream` trait, so the Python view is an
injected implementation too: `WsgiUpstream` in `upstream/wsgi.rs`, `"wsgi"` registered in
`builtin_registry()`, and the transitional `stages.is_empty()` fallback deleted (design
leaf `MB.9.3`).

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  `WorkerRuntime`, with the WSGI port, `SessionPort`/`ExchangeChannel` and the transitional
  rule restated here; the retired design is deleted with `ADR-0010`. Scope: narrow `paths`
  to the five files, register `"wsgi"`, delete the fallback.
- Files (the whole leaf, 5): `crates/blastit-scheduler/src/{upstream/wsgi,upstream/mod,scheduler,lib}.rs`
  and `crates/blastit-scheduler/tests/conformance_wsgi.rs`.
- What lands:
  - `WsgiUpstream`:
    - `capacity` reads the shared `RouteState` (`view.ready && inflight < view_worker_cap`,
      0.2.0's `can_dispatch`); `ensure_ready` calls `SessionPort::spawn(route)` (the
      existing `Ctl::Spawn`) with `tokio::time` backoff (`pause()` in tests).
    - `plan(&UpstreamHead)` maps the head onto the route's `IngressPolicy` (body codec plus
      the existing request credit) and returns `UpstreamError::Refused` for a method or
      body the view cannot take — the same answers the view path gives today.
    - `exchange` opens the channel with `SessionPort::open(route) -> (request_id, channel)`,
      writes the encoded `REQ_HEAD`, relays `REQ_BODY`/`REQ_END`/`CANCEL`/`WINDOW`, maps
      `RESP_HEAD`/`RESP_BODY`/`RESP_END`/`RESP_END_COST` to the sink and `APP_OUTPUT` lines
      to `sink.output`, preserving status, hop-by-hop stripping, body credit, `app.output`
      events and cancellation. It returns the `WireRequest`/`Output` observations the learn
      path already records — no new field, no PHP/Python-specific variant.
  - `upstream/mod.rs`: `pub mod wsgi;`. `scheduler.rs`: `"wsgi"` appended to
    `builtin_registry()`'s list, and the transitional `stages.is_empty()` fallback deleted
    (a missing builder becomes `RunError::Boot`; `stages` is never empty afterwards).
  - `conformance_wsgi.rs`: iterates `blastit_backend::conformance::cases()` against the WSGI
    implementation using the existing in-process view harness (no Docker, no real Python
    worker where the harness already fakes it), keeps the fake `Upstream` double for the
    dispatch assertions, and asserts the learn-record `Observation` list is identical
    before/after the port. No `case.id` literals.
- `ObservationKind::Query` is **reserved**: WSGI does not emit it and no implementation may
  emit it in `0.2.0`.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit-scheduler --test conformance_wsgi --test critical_ingress_enforcement --locked
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --locked
$ ! grep -rn 'backend_kind\|is_static' crates/blastit-scheduler/src/scheduler.rs && echo "one dispatch path for every backend"
$ ! grep -rn 'stages.is_empty()' crates/blastit-scheduler/src && echo "transitional fallback deleted"
$ make check
```

# Notes

- `--learn`/`check` must still record exactly what they record today: this leaf moves the
  code path, it does not change the wire record.
- Protected suites (`critical_rewrite.rs`, `critical_ingress_enforcement.rs`,
  `critical_static_*`) pass **unmodified**; a protected contract change is its own node.
- Do not touch `python/**`. If the adapter needs an additive signature change in
  `blastit-view`'s public request entry, record it in `RESULT.md` — but `blastit-view` is
  **not** in `paths`, so prefer the additive seam inside the scheduler.
- Fallback split if it overruns 550 lines: `.1` (`WsgiUpstream`) + `.2`
  (`ExchangeChannel` + `"wsgi"` registration) — mechanical, no path change.
