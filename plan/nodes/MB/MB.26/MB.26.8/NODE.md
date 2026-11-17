---
id: MB.26.8
title: "A queued request must never be handed to a retired PHP view (502 view_died under 8-way concurrency)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.24.4]
paths:
  - "crates/blastit-scheduler/**"
  - "crates/blastit-view/**"
  - "crates/blastit-php-sapi/**"
  - crates/blastit/tests/critical_php_embed.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-17T06:29"
---

Moved from `MB.21.8` (`MB.21`) to `MB.26.8` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

A queued request must never be handed to a view that has already answered and retired. Measured
while doing `MB.21.7`: eight concurrent GETs to a PHP view return **1 × `200` and 7 × `502`**, each
`502` carrying `{"outcome":"view_died","queued_ms":15.3,"route":"api","status":502}` — the requests
that queued behind the in-flight one are dispatched to the view that served it and had already
retired. This is the same failure class as `MB.3.45` (there 1-in-20 and fixed); here it reproduces at
will. The dispatch path must treat "the slot's view retired between queue and dispatch" as a reason
to **respawn and serve**, never to answer `502`.

# Context

- The dispatch entry points are `Scheduler::dispatch_queued` (`crates/blastit-scheduler/src/scheduler.rs:2249`),
  `dispatch_queued_fair` (`:2306`) and `dispatch` (`:2386`); the `view_died` reporting path is around
  `:2895`/`:4096`/`:4179` and the process-retirement bookkeeping around `:3523-3542`/`:3642-3691`.
- Prior art, `docs/knowledge/LESSONS.md` — `MB.3.13.1`: a single-slot `view.chan` worker must drop
  stale bookkeeping frames for the previous id, both idle and in-flight. **Do not weaken or delete
  that suite**; it is protected and its legs must keep passing.
- `view_died` is the events table's "the view process died before it answered" (`scheduler.rs:117`);
  answering it for a request the scheduler *chose* to queue behind a now-retired view is a
  misreport, not just a bad status code. The fix belongs where the queued head is matched to a live
  process; a respawn must be charged and accounted like any other spawn (see `critical_charge.rs`
  for the existing accounting legs).
- One-slot semantics matter: with `max_inflight == 1` there is exactly one in-flight request, so
  every other queued waiter sees the retirement; a `2`-slot view hides the bug behind luck, so the
  regression test must pin a **one-slot** view.

# Acceptance

- New regression test drives **≥ 8 concurrent** requests at one PHP view on a **one-slot** view and
  asserts **every** response is `200` with the route's body; repeat the whole batch **20 ×** to prove
  it is not luck. Place it in the harness that can actually see libphp
  (`crates/blastit/tests/critical_php_embed.rs`) and/or a scripted-AF_UNIX-view leg in
  `crates/blastit-scheduler/tests/` if that can reproduce the dispatch interleaving without libphp;
  record in `RESULT.md` which shape reproduces and which merely guards.
- The same test, run at the parent commit (`git stash` / worktree at `053b56e`), **fails** with
  `view_died` — paste the failing assertion tail into `RESULT.md`.
- `make check` → passes.
- `make rs-test-php-image` → passes if the leg needs libphp (`php-sapi` image built once, reused).
- `critical_guard.py --node MB.21.8` clean for any protected file the node had to extend, and no
  pre-existing `critical_*` leg deleted, weakened or skipped.

# Notes

- Scope is the dispatch/retirement decision only. If the real fix turns out to be a broad scheduler
  rewrite, stop and report `needs_split` with the proposed children instead of sprawling.
- Report the measured before/after shape verbatim (status counts and the `outcome` objects) — the
  number is the evidence, not a sentence about concurrency.
- `crates/blastit-php` does not exist; the PHP SAPI crate is `crates/blastit-php-sapi`.
