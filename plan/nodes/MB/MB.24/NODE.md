---
id: MB.24
title: "Parallel PHP workers: several view processes per route under the scheduler's max_workers (matching Python's on-demand threads)"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.5.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T01:23"
updated: "2026-11-17T01:23"
---

# Goal

Four concurrent 200 ms requests to **one** `entrypoint php` route must finish in about
200 ms, not 800 ms. Today they serialize, while the same four to a Python route already run
in parallel. The gap is not in the PHP runtime: it is the scheduler holding **at most one
`View` per route** (`views: HashMap<usize, View>`,
`crates/blastit-scheduler/src/scheduler.rs:782`).

Distro `libphp` is NTS — one request per OS process, because Zend's `EG`/`CG`/`SG` are
process globals and two threads in one process would corrupt them and segfault. So a PHP
route parallelizes the only way NTS permits: `N` view processes for the same route, forked
on demand and bounded by the existing global `max_workers` budget.

This epic makes `blastit-scheduler` hold **several `(route, pid)` workers per route** when a
route's per-process concurrency is `1`, retires excess idle workers on `idle_timeout`, and
leaves Python's single-process, multi-threaded view exactly as it is. A developer sees one
number (`BLASTIT_MAX_WORKERS`) and one promise: a route serves as many requests as the
budget allows, whatever its runtime.

Mandatory for `0.2.0`; `depends_on: [MB.21.5.2]`.

# Context

Measured in the code today:

- **Python already parallel.** `crates/blastit-view/src/lib.rs:40-55`: one on-demand OS
  thread per concurrent `REQ_HEAD` inside one view process, each with its own
  `wsgi.input`; `DONE` releases the scheduler's slot. `max_inflight` is
  `view_worker_cap()` (`crates/blastit-view/src/runtime.rs:194`).
- **PHP is serialized by the scheduler, not by the runtime.** `crates/blastit-view/src/php.rs:10-17`
  says `max_inflight()` is `1`, `serve` takes the process handle behind a `Mutex`, and the
  module doc already says *"The scheduler forks `1..=workers` such processes instead of
  threading one"* — a sentence the scheduler does not implement (`scheduler.rs:782`,
  `scheduler.rs:2225-2235`, `scheduler.rs:2442-2447`).
- **The identity plumbing already fits a pool.** `ViewKey { route, pid }`
  (`scheduler.rs:240-248`) keys `dispatch` and every `Ctl::Retire`/`Ctl::Exited`;
  `Ctl::Spawn { route }` carries only the route (`crates/blastit-frame/src/ctl.rs:117-146`),
  so several pending spawns for one route become a per-route **count**, not one slot.
  `supervise.rs:237` keeps `route_pids: HashMap<usize, i32>` (one pid per route) and
  `SPAWN` is answered from `supervise.rs:539-680`.
- **The budget and the limits already allow it.** `can_dispatch` gates on
  `slots < cfg.max_workers`; `reserve` gates on `view.inflight < view_worker_cap()`;
  LRU uses `last_used`, idle retirement uses `idle_since` + `idle_timeout`; and
  `RLIMIT_NPROC = view_worker_cap() + VIEW_PROCESS_MARGIN` is already sized for several
  workers (`crates/blastit/src/php_view.rs:72,646`).
- **MB.23 lands in the same code.** Its `Ctl::Reload` retires every idle view; with a pool
  it must retire every idle worker of every route, together.
- **No new dependency.** `Cargo.toml`/`Cargo.lock` stay byte-identical for this epic.
- **PHP is one backend.** No sentence in this epic may offer a second PHP transport, and
  the retired manifest format is never named.

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ uv run --no-project tools/plan.py tree --under MB.24
$ cargo test -p blastit-scheduler
$ cargo test -p blastit --test critical_parallel_views
$ make check
```

and the measured promise, in `critical_parallel_views`:

* 4 concurrent 200 ms requests to ONE PHP view complete in **< 450 ms** wall, and the same
  4 to ONE Python view also complete in **< 450 ms** (each is `>= 800 ms` if serialized);
* 8 concurrent requests with `BLASTIT_MAX_WORKERS=4` never have more than 4 in flight, and
  the remaining 4 are served after the first wave, not dropped;
* a PHP pool that grows to 4 workers retires back to the prefork count after
  `idle_timeout`, and `RLIMIT_NPROC` is never hit.

# Notes

- This epic changes how many processes a route may own; it changes no HTTP, ingress,
  proxy or manifest semantics.
- Keep Python's `max_inflight()` at `view_worker_cap()` and PHP's at `1`: the difference is
  data (`per_process_cap`), never a runtime special case in the scheduler.
- `make e2e` is an epic-gate check, because process topology and rlimits change.
