# Result MB.24

## Summary

Four concurrent 200 ms requests to one `entrypoint php` route now finish in **206.7 ms**, not the
serialized **810.8 ms**; the same four to a Python route finish in **209.0 ms**, which Python
already did. The scheduler holds a **per-`(route, pid)` pool** of views instead of one view per
route: each route states its own ceiling in boot data (`per_process_cap`), workers spawn on
demand one fork at a time, selection is LRU among the workers that admit a request, excess idle
workers retire on `idle_timeout`, and `BLASTIT_MAX_WORKERS` stays the single budget. PHP pools
because NTS `libphp` is one request per process; Python keeps one process and its threads.
Deliberately rejected: a per-route concurrency map (a route owns only the processes its queued
requests need, under the shared budget) and any second PHP transport.

## Delivered, per child

**MB.24.1 — docs.** `docs/how-it-works.md` (pool row, Mermaid label, spawn condition "no idle
view process for this route", two sentences on the shared budget) and
`docs/frameworks/php/index.md` (`## Concurrency` replacing the serial heading, plus the
`BLASTIT_VIEW_MAX_WORKERS` wrong-knob sentence). Guard: `crates/blastit-manifest/tests/docs_fences.rs`
(`11 passed`, including `test_the_real_tree_carries_no_retired_php_vocabulary`) and
`make docs-check`, both green; `dx` and `ai-ergonomics` both `VERDICT: PASS`.

**MB.24.2 — ADR-0015.** `docs/design/parallel-views.md` (15 numbered requirements, function
shapes, the wall-clock recipe) and `docs/decisions/ADR-0015-multi-worker-views.md` (nine
decisions, rejected alternatives). No protected test; guarded by `make docs-check` and four lens
reviews (`security`, `implementability`, `testability`, `consistency`) ending `PASS` with zero
BLOCKER/MAJOR.

**MB.24.3 — the pool in `blastit-scheduler`.** `route.rs` (`per_process_cap`, `process_pool`),
`scheduler.rs` (`ViewSet`, `stale`/`admits`, LRU reserve, `grow` with one `SPAWN` in flight,
per-`(route, pid)` retire/ready/exited, `Ctl::Reload` arm), `blastit/src/supervise.rs`
(`ChildTable` pid → `ViewChild { route, uid }`, `may_fork`, non-fatal refusal), and
`blastit-frame` (`Reload` `0x45`, the one wire addition). No `tests/critical_*` file touched;
witnesses are the new `crates/blastit-scheduler/tests/pool.rs` (19 tests) and the widened R10 leg
in `isolation.rs`, with eight mutants killed.

**MB.24.4 — the wiring and the protected proof.** `crates/blastit/src/stage1.rs` writes
`per_process_cap = Some(1)` + `process_pool = true` on every bound PHP slot (after
`boot_bindings`, before `probe_bindings`, so dev, dev `--learn` and production are covered);
`crates/blastit/tests/critical_parallel_views.rs` is the new protected suite
(`serialized_then_parallel_wsgi`, `serialized_then_parallel_php`,
`the_cap_holds_and_the_pool_shrinks`, plus the no-pair refusal branch); `Makefile`'s
`PHP_SUITE_ARGS` gains the suite; ADR-0010 §1 item 2 reconciled. Deleting the two assignments
turns two legs red.

## Measured promise

| Leg (4 × 200 ms unless noted) | Serialized control | Parallel / measured | Processes |
|---|---|---|---|
| PHP, one route (pre-change, before the cap was wired) | 810.8 ms (warm 809.5) | 206.7 ms | 1 pid → 4 pids |
| PHP, one route (`serialized_then_parallel_php`) | 824.2 ms (`MAX_WORKERS=1`) | 206.7 ms | 4 pids, 4 `process.spawned` |
| WSGI, one route (`serialized_then_parallel_wsgi`) | 821.7 ms (`MAX_WORKERS=1`) | 209.0 ms | 1 process; four `t0` span 0.6 ms |
| 8 concurrent, `BLASTIT_MAX_WORKERS=4` | — | 472.3 ms | exactly 4 pids, `process.spawned` == those pids |
| Idle retirement | — | every burst pid retired with reason `idle` | follow-up got fresh pid 3592062 |

The design's §7 cap-leg floor `600 ms < wall < 1000 ms` is **unreachable on a fast host**: a fork
+ `dlopen` + `php_module_startup` measures ~30 ms (the four-wide warm-up burst is 251.9 ms of
which 200 ms is the sleep), so six runs land at 444–482 ms. `the_cap_holds_and_the_pool_shrinks`
therefore asserts MB.24.4's **causal** two-wave floor (≥ `2 × 200 ms` and < 1000 ms) plus two
host-independent oracles: exactly four pids for eight requests, and `process.spawned` equal to
that pid set. Correcting §7's constant is MB.30.

## Gate evidence

| Check | Command | Result |
|---|---|---|
| Plan validity | `uv run --no-project tools/plan.py validate` | `plan OK` (exit 0) |
| Epic tree | `uv run --no-project tools/plan.py tree --under MB.24` | four children `done`; epic at gate |
| Scheduler | `cargo test -p blastit-scheduler --locked` | all targets + doctests `ok` |
| Protected promise | `cargo test -p blastit --test critical_parallel_views` | `6 passed; 0 failed` (no-pair refusal shape on this shell; the real pair gives `7 passed` in MB.24.4) |
| Workspace | `make check` | exit 2 with exactly one failure: `docs/reference/cli.md` fence 1 (`blastit dev: unknown argument --no-reload`), MB.23.5's deliverable; 169 test targets `ok` |
| End-to-end | `make e2e` | exit 0 — 263 `ok`/`PASS` lines, no failure; the parallel-views, php-sapi and reload-adjacent harnesses all green |

`make e2e` was run because the epic changes process topology and rlimits, exactly as the epic's
Notes require. The epic's mutation evidence is MB.24.3's reviewer pass over `blastit-scheduler`
(matched mutants: `reserve`'s `max_by_key`, `grow`'s `other_route_starved`, `ViewSet::len`'s spawning term), so no separate epic audit was opened.

## Open follow-ups

- **MB.27** — compile and run `critical_parallel_views` inside the php-sapi image, so CI proves the measured promise, not just a local `make rs-test-php`.
- **MB.28** — apply the accepted doc deltas: `ARCHITECTURE.md` §2.4/§2.6/§8/§8.2, `docs/reference/settings.md:68-69`, and `crates/blastit-view/src/php.rs:9-14`.
- **MB.29** — real-`ulimit` e2e: a refused view fork costs one backoff and a warning, never a request or the tree.
- **MB.30** — one shared `VIEW_PROCESS_MARGIN` constant (today a twin in `supervise.rs` and `php_view.rs`) and a truthful `view_gone` paragraph.
- **MB.26.8** — `Errored` and client-cancelled (`Aborted`) retires still lean on the bounded `expire_exited_views` window; the real fix is an explicit signal (its follow-up 1).

## Notes

No new dependency: `Cargo.toml`/`Cargo.lock` unchanged across the epic. Python's `max_inflight`
stayed `view_worker_cap()` and PHP's stayed `1`; the difference is boot data, never a scheduler
branch.
