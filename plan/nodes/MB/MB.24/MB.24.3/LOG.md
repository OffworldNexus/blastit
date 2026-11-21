# Log MB.24.3

## 2026-11-17T01:26 [planner]

created (task) Multi-instance per-route view pool in blastit-scheduler and (route, pid) supervision, with scheduler concurrency tests

## 2026-11-17T06:31 [planner]

edited fields

## 2026-11-17T07:22 [lead]

edited fields

## 2026-11-17T07:34 [lead]

status ready -> active

## 2026-11-17T07:34 [lead]

attempt 1/3 started

## 2026-11-17T07:34 [lead]

base 605d78583dabc420d121fce88ab13861963c76ae

## 2026-11-17T08:01 [lead]

status active -> review

## 2026-11-17T08:01 [lead]

implementer reports done; reviewer dispatched against base 605d785 (HEAD moved to 3e70af3 by two MB.22 plan-only gate commits, not part of this node).

## 2026-11-17T08:06 [lead]

reviewer VERDICT: PASS (matched mutants: reserve max_by_key, grow's other_route_starved, ViewSet::len's spawning term via memory.rs). Five MINOR findings, 1-4 fixed in a polish pass, 5 (design §4.6/§4.7 wording owes 'fail and release whatever is still keyed') is a docs follow-up.

## 2026-11-17T08:20 [implementer]

polish pass on the reviewer's four in-scope MINORs, no refactors:
(1) `pool::growth_counts_a_promise_not_a_process` reshaped to the eager boot burst (`eager_spawn`,
4 routes, `max_view_processes = 2`, `park_spawned`), so the live-process cap is the only bound and
dropping `ViewSet::len`'s promised-fork term turns it red; R5's other, pre-existing witness is
`memory.rs::the_derived_cap_bounds_the_initial_prefork_burst`. (2) deleted `scheduler.rs`'s module
doc clause about an `LruCap` cool-down for a worker that never served — `retire` backs off on
`MemoryPressure` only. (3) corrected, not coded: the shipped supervise tests witness `ChildTable`
and `may_fork`, not `spawn`'s emitted warning and `Ctl::Exited{pid:0}` (driving `spawn` needs a
constructed `Supervisor`, a refactor); that refusal contract leans on MB.24.4's real-`ulimit` e2e
leg. (4) `reap_children`/`reap_until`/`progress_shutdown` lost the unused `py` token.
Verified: fmt clean, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean,
`--test pool` 19 passed, `--lib --features python supervise` 6 passed. No `make check`, no commit.

## 2026-11-17T08:16 [lead]

status review -> done

## 2026-11-17T08:16 [lead]

committed 9feb291 (make check green except MB.23.5's docs/reference/cli.md fence 1; every other target incl. rs-no-python/zed-check green).
