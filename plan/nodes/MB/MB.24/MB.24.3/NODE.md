---
id: MB.24.3
title: "Multi-instance per-route view pool in blastit-scheduler and (route, pid) supervision, with scheduler concurrency tests"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.24.2]
paths: ["crates/blastit-scheduler/**", "crates/blastit-frame/**", crates/blastit/src/supervise.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T01:26"
updated: "2026-11-17T08:16"
---

# Goal

Make `blastit-scheduler` hold a **pool of view processes per route** instead of one, exactly
as `docs/design/parallel-views.md` and `ADR-0015` specify. After this task a route with
`per_process_cap == 1` (NTS PHP) accepts a second concurrent request by forking a second
worker, third by a third, bounded by the global `max_workers` budget and the derived
view-process cap; idle extras retire on `idle_timeout`; a route with
`per_process_cap == view_worker_cap()` (threaded Python) still runs **one** process and
behaves byte-identically.

Scheduler + supervisor only. Setting `per_process_cap = 1` for `entrypoint php` routes and
the wall-time proof are `MB.24.4`.

# Context

- Read the design first; it names every function to change. Starting points:
  `crates/blastit-scheduler/src/scheduler.rs` — `views: HashMap<usize, View>` (`:782`),
  `View::spawning` (`:297`), `can_dispatch` (`:2212`), `reserve` (`:2442`),
  `view_spawned` (`:3550`), `try_spawn` (`:3420`), the idle/LRU/`max_requests` retirement
  path (`:3480-3545`), `dispatch_queued` (`:2249`), and the drain path.
- `ViewKey { route, pid }` already keys `dispatch`, `Ctl::Retire` and `Ctl::Exited`, so a
  pool is mostly `HashMap<usize, RoutePool>` plus the spawn bookkeeping the design chose
  for `Ctl::Spawn { route }` / `Spawned { route, pid }`
  (`crates/blastit-frame/src/ctl.rs:117-146`).
- `crates/blastit/src/supervise.rs`: `views: HashMap<i32, ViewChild>` (`:236`) and
  `route_pids: HashMap<usize, i32>` (`:237`) both assume one pid per route; `spawn`
  (`:539`) and `record_view` (`:652`) must key by `(route, pid)`, and `Ctl::Exited`/
  `Ctl::Retire` must find the process by pid, not by route.
- `crates/blastit-scheduler/src/route.rs:15-28` gains the per-route cap (default: the
  config cap, so nothing changes for a route that does not set it).
- `Ctl::Reload` (from `MB.23`) retires **every idle worker of every route**; in-flight
  requests finish and their process retires after `DONE`.
- Invariants to keep green: `crates/blastit-scheduler/tests/{admission,fairness,flow,lifecycle,memory,isolation,ctl_channel}.rs`
  and the protected `critical_charge.rs`, `critical_rewrite.rs`, `critical_static_*.rs`.
- No new dependency; `Cargo.toml`/`Cargo.lock` unchanged. If the design adds a spawn id to
  `Ctl`, that is a wire change in `crates/blastit-frame` and the design owns the
  compatibility argument (both ends ship together).

# Acceptance

```console
$ cargo test -p blastit-scheduler
$ cargo test -p blastit-scheduler --test lifecycle --test admission --test memory
$ cargo test -p blastit-frame
$ cargo test -p blastit --test dev_signal_reap        # supervisor still reaps one tree
$ make check
```

New tests in `crates/blastit-scheduler/tests/` must prove, with fake views over the real
ctl/view channels:

1. two concurrent requests for a `per_process_cap = 1` route produce **two** `SPAWN`s and
   two pids, and each request is answered by the process it was dispatched to;
2. a second request is queued (not failed) while `slots == max_workers`, and dispatched when
   a slot frees;
3. a `per_process_cap = view_worker_cap()` route spawns exactly **one** process for the
   same load;
4. `max_requests` recycling and `idle_timeout` retirement still fire, per process;
5. `Ctl::Reload` retires every idle worker of two routes and lets an in-flight one finish,
   then retires it after `DONE`;
6. a frame from a retired `(route, pid)` can never answer a live request of the same route
   (the existing per-`ViewKey` guarantee, now with siblings).

# Notes

- `RLIMIT_NPROC` is already `view_worker_cap + VIEW_PROCESS_MARGIN`; do not raise it.
- Keep the LRU and memory caps counting **live processes**, per the design; a route must not
  be able to grow past the derived cap by asking for more.
- Never thread one NTS `libphp` process and never add a second PHP transport.
