---
id: MB.29
title: "Real-ulimit e2e: a refused view fork costs one backoff, never a request or the tree"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T08:41"
updated: "2026-11-18T20:36"
---

# Goal

Prove, on a real host limit, that the pool's third bound is harmless: when a deployment's
`RLIMIT_NPROC` refuses the fourth worker, the refusal costs one route one `spawn_backoff` and never
a request (beyond that route's own queue), a sibling, or the process tree. `MB.24.3` witnessed the
`EAGAIN` path only through a fake init (`Ctl::Exited { pid: 0 }`); `RLIMIT_NPROC` is absent under
`blastit dev`, so the real `fork(2)` refusal needs `e2e/`.

# Context

- Design `docs/design/parallel-views.md` §4.7 and §7's «real-`ulimit` leg»: set `ulimit -u` to
  `MAX_WORKERS + VIEW_PROCESS_MARGIN - 1` for the boot's uid, send four concurrent 200 ms requests
  to one `entrypoint php` route, and assert either four `process.spawned` with distinct pids or a
  graceful refusal: one `manifest.warning` naming the route, no `manifest.fatal`, the process tree
  alive, the un-served request answered `503`/`502` per the documented queue contract — never
  `exit 1`.
- `ADR-0015` decision 8: a refused fork is never `fail()`. `RLIMIT_NPROC` is never raised.
- `MB.24.3`'s `supervise.rs::ChildTable` bound (`view_worker_cap() + 16`) is the other half; the
  real limit is the third.

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ E2E_ULIMIT=1 uv run --no-project e2e/php_ulimit.py   # new harness, exits 0
$ make e2e
```

- the harness prints the refused fork's errno and the surviving tree, and asserts the four
  assertions above; it is wired into `e2e/run.py` so `make e2e` covers it;
- a run with the limit generous enough for four workers asserts four distinct pids (the control);
- `make check` unchanged.

# Notes

- Docker/rootless only; no `make check` dependency, no new knob, no raised limit.
