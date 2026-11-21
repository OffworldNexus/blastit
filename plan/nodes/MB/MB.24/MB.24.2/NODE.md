---
id: MB.24.2
title: "Design the per-route view pool: per_process_cap, spawn-on-demand, LRU and reload (ADR-0015)"
kind: task
status: done
owner: designer
priority: 1
depends_on: [MB.24.1]
paths:
  - "docs/design/parallel-views.md"
  - "docs/design/parallel-views.review-security.md"
  - "docs/design/parallel-views.review-implementability.md"
  - "docs/design/parallel-views.review-testability.md"
  - "docs/design/parallel-views.review-consistency.md"
  - "docs/decisions/ADR-0015-multi-worker-views.md"
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-17T01:26"
updated: "2026-11-17T06:31"
---

# Goal

One design document plus one ADR that turns the accepted DX sentences into a scheduler data
model: a **per-route worker pool** where each process has its own `per_process_cap`
(`1` for NTS PHP, `view_worker_cap()` for threaded Python), extra workers are forked **on
demand** when every ready instance for the route is at its cap and `slots < max_workers`,
excess idle workers retire on `idle_timeout`, and every other existing rule (LRU cap,
memory-derived view cap, `max_requests` recycling, `Ctl::Reload`, drain) still holds.

No Rust in this task. The output must be implementable by an agent that never re-reads this
brief.

# Context

- Accepted DX: `docs/design/parallel-views.review-dx.md` and
  `docs/design/parallel-views.review-ai-ergonomics.md` (`MB.24.1`), and the two page
  sentences it landed.
- Current state to design against: `views: HashMap<usize, View>` and `can_dispatch`/
  `reserve` in `crates/blastit-scheduler/src/scheduler.rs` (`:782`, `:2212`, `:2442`);
  `ViewKey { route, pid }` (`:240`); `Ctl::Spawn { route }` / `Spawned { route, pid }`
  (`crates/blastit-frame/src/ctl.rs:117-146`); supervisor `route_pids: HashMap<usize, i32>`
  (`crates/blastit/src/supervise.rs:237`); `spawn` at `supervise.rs:539`;
  `Ctl::Reload` from `MB.23`; `RLIMIT_NPROC = view_worker_cap + VIEW_PROCESS_MARGIN`
  (`crates/blastit/src/php_view.rs:72,646`).
- Mechanisms that already exist and must be reused, not re-invented: global `slots`
  versus `max_workers`, `view.inflight` versus a per-process cap, LRU `last_used`, idle
  retirement `idle_since`, spawn backoff (`backoff`), the derived view cap
  (`derive_view_cap`), and the `SPAWN → SPAWNED(fd)` handshake.
- Decisions the design must settle with a written reason, not a shrug:
  1. Which instance a request goes to (LRU ready-and-below-cap, deterministic).
  2. How many `SPAWN`s may be in flight per route, and how `Spawned { route, pid }` is
     matched to one of them (the wire carries no spawn id — say explicitly whether the
     design adds one or counts).
  3. What the derived view-process cap now counts (processes, not routes) and how one
     route cannot starve the others.
  4. How the supervisor keys children by `(route, pid)` and answers `Exited`/`Retire`.
  5. `Ctl::Reload` with a pool: all idle workers of all routes retire together, in-flight
     ones finish.
  6. Failure policy: a spawn that fails repeatedly must back off without wedging the other
     instances.
- PHP facts are settled: NTS `libphp` is one request per process (Zend `EG`/`CG`/`SG` are
  process globals). Python keeps one process per route.
- No new dependency; no wire-format change unless the design names it and `MB.24.3` owns it.

# Acceptance

```console
$ test -f docs/decisions/ADR-0015-multi-worker-views.md && echo ok
$ grep -n "per_process_cap" docs/design/parallel-views.md | head
$ make docs-check
$ uv run --no-project tools/plan.py validate
```

Then four fresh-context `critic` passes, recorded next to the design, each ending
`VERDICT: PASS`:

* `docs/design/parallel-views.review-security.md` — no cross-route identity leak (a frame
  from a retired `(route, pid)` can never answer another route's request), rlimit and
  memory accounting per live process, no new capability.
* `docs/design/parallel-views.review-implementability.md` — file-by-file change list with
  the exact functions and their new shapes.
* `docs/design/parallel-views.review-testability.md` — how each rule is witnessed
  (scheduler unit tests with fake views, then the wall-time suite), including a negative
  case per rule.
* `docs/design/parallel-views.review-consistency.md` — no contradiction with ADR-0009
  (runtime providers), ADR-0010 (embedded PHP), `MB.23`'s reload ADR, or
  `ARCHITECTURE.md` §2.4/§8.

# Notes

- `docs/design/parallel-views.md` must state the **measured** before/after numbers the epic
  promises (4 × 200 ms: ~800 ms serialized, < 450 ms parallel) and how the test measures
  them (wall clock around 4 concurrent requests, same manifest, same cap).
- Keep the Python path explicitly untouched: `per_process_cap = view_worker_cap()` and a
  pool that never exceeds one process.
- Never name the retired manifest format, and never offer a second PHP transport.
