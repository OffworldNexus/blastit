# RESULT — MB.24.3

Status: **done** (slice complete, targeted acceptance green). Worktree
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, branch `release/0.2.0`, base
`605d785`, attempt 1/3. Nothing committed; `make check`/`make e2e` left to `lead`. No
`Cargo.toml`/`Cargo.lock` change, no new dependency, no `tests/critical_*` file touched.

## Summary

A route now holds a **pool** of view processes instead of one, exactly as
`docs/design/parallel-views.md` §4.1–§4.7 and `ADR-0015` (decisions 1–9) specify.

* `RouteInfo` carries the two boot-time concurrency facts (`per_process_cap: Option<u32>`,
  `process_pool: bool`) with `with_per_process_cap`/`with_process_pool`; `new` keeps
  `None`/`false`, so every existing route stays the threaded shape.
* `Scheduler.views` became `pools: HashMap<usize, ViewSet>` (`views: BTreeMap<u32, View>` +
  `spawning: Option<(epoch, deadline)>` + `pending_ready`), plus `reload_epoch`. `View`
  lost `spawning` and gained `stale`/`spawned_epoch`/`admits(cap)`; `View::spawning` is
  gone, so a `ViewKey` can never name a process that does not exist.
* Selection is LRU among the workers that admit a request (`(last_used, pid)`);
  `can_dispatch` = `slots < max_workers && has_dispatchable(route)`. Growth is `grow(route,
  wanted)` with one `SPAWN` in flight per route, a `pending_ready == 0` gate until the first
  memory measurement (unless `BLASTIT_MAX_VIEW_PROCESSES` pins the cap), the route's
  process limit (`max_workers` pooled, `1` threaded), the need bound
  (`needed_processes` = ceil((inflight + queued) / per_process_cap)), the per-route
  backoff, the "first process first" rule, `make_room`, and the memory-derived cap over
  **live processes** (`live_processes` counts workers plus the promised fork).
* The three call sites that used to mean "this route has no view" now mean "no worker of
  this route admits a request" (`dispatch_queued`, `dispatch_queued_fair`, and the `--learn`
  fallback with `wanted = 1`), which is the invariant that fixes `MB.26.8`: a route with a
  queued request and no admitting worker **grows** instead of failing.
* `RETIRE`/`EXITED`/`READY`/`Closed` are per `(route, pid)`: `retire` releases
  `pending_ready` for a worker that never became `READY`, `view_ready` retires a
  stale-at-birth worker and measures memory, `view_exited` answers `pid: 0` with
  `spawn_failed` (slot freed, one backoff, one `manifest.warning`, never a `fail`), and
  `view_gone` only `502`s a route's queue when the route has no worker and no `SPAWN` left.
* `Ctl::Reload` (`0x45`, empty payload) is added end to end: variant + payload table +
  encode/decode in `blastit-frame/src/ctl.rs`, `TypeCode::Reload` + `from_code`/`is_ctl`/
  `name`/decode arm in `frame.rs`, both hand-written round-trip tables (`lib.rs`,
  `decoder.rs`, `testutil::all_codes`), zero descriptors on the scheduler's `ctl.rs`, the
  `Ctl::Reload => self.reload()` arm, and init's ignore arm. Retire reason is
  `RetireReason::Other("reload")` (`const RELOAD`, `MB.23.1`'s vocabulary) because
  `crates/blastit-events` is outside this node's paths.
* `supervise.rs`: `ChildTable` (pid → `ViewChild { route, uid }`) replaces `views` +
  `route_pids`; `pending_spawns`/`service_pending_spawns` are deleted; `Exited` is answered
  from the child's own route; `RETIRE` is refused for a pid that is not a live child of that
  route; `spawn` refuses past `view_worker_cap() + VIEW_PROCESS_MARGIN` (`may_fork`) with one
  `manifest.warning` and `Ctl::Exited { pid: 0, Code(1) }`; a fork error warns and refuses
  instead of `fail`ing. What the in-module tests witness is the `ChildTable` contract and the
  `may_fork` predicate, not the refusal's own emitted warning and `pid: 0` answer — see
  `## Review polish`, finding 3.
* Harness (`tests/common/mod.rs`) gained exactly the five additive items of design §7
  (`cap_route`/`pool_route`, `Log::{by_process, closed_by_process, retired_pids,
  spawned_sent}` + `received_by_pid`/`request_ids_of`/`retired_pids_of`,
  `script_for_spawn`, `park_spawned`, `Script::ReadyAfter`, and a `Ctl::Reload` arm in the
  fake init). `received`/`closed`/`retires` keep their exact shapes; `isolation.rs` changed
  only in the one widened R10 leg.

## Evidence

Each `# Acceptance` command, with the tail actually seen on the final tree.

**1. `cargo test -p blastit-scheduler` — PASS** (20 test binaries, 280 tests, 0 failed):

```console
$ timeout 400 cargo test -p blastit-scheduler 2>&1 | grep -E "^test result: ok" | wc -l
20
$ … | grep -E "^test result: ok" | awk -F'[ ;]' '{s+=$4} END {print "total passed:", s}'
total passed: 280
$ … | grep "^test result"
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (lib)
test result: ok. 11 passed; 0 failed …   (admission)
test result: ok. 13 passed; 0 failed …   (isolation)
test result: ok. 18 passed; 0 failed …   (lifecycle)
test result: ok. 14 passed; 0 failed …   (memory)
test result: ok. 19 passed; 0 failed …   (pool)
test result: ok. 32 passed; 0 failed …   (critical_rewrite)
test result: ok. 18 passed; 0 failed …   (critical_static_assets)
… (20 binaries, every one `ok`)
```

**2. `cargo test -p blastit-scheduler --test lifecycle --test admission --test memory` — PASS:**

```console
$ timeout 400 cargo test -p blastit-scheduler --test lifecycle --test admission --test memory
     Running tests/admission.rs (…/admission-14f23e3ac83b96f1)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
     Running tests/lifecycle.rs (…/lifecycle-31c6c76d01c45b1d)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests/memory.rs (…/memory-12bb84d609903ade)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

The new pool suite (also part of acceptance 1) with its test names:

```console
$ timeout 400 cargo test -p blastit-scheduler --test pool 2>&1 | grep "^test "
test a_colliding_spawned_retires_the_held_pid
test a_failed_spawn_backs_off_growth_only
test a_php_route_never_threads
test a_queued_request_survives_the_worker_it_queued_behind
test a_refused_spawn_is_one_backoff
test a_route_never_exceeds_its_process_limit
test a_second_fork_waits_for_the_first_ready
test a_spawn_sent_before_a_reload_is_retired_on_arrival
test a_threaded_route_serves_both_requests_from_one_process
test burst_pool_shrinks_on_idle_timeout
test growth_counts_a_promise_not_a_process
test growth_yields_to_a_starved_route
test lru_picks_the_coldest_admitting_worker
test one_fork_at_a_time
test per_process_cap_one_forks_per_request
test reload_retires_every_idle_worker_of_every_route
test siblings_never_answer_each_others_requests
test spawned_consumes_the_slot
test two_cold_routes_each_get_their_first_process
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

**3. `cargo test -p blastit-frame` — PASS:**

```console
$ timeout 400 cargo test -p blastit-frame 2>&1 | grep "test result"
test result: ok. 71 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.18s; merged doctests compilation took 0.17s
```

**4. `cargo test -p blastit --test dev_signal_reap` — PASS.** Run in the crate's
interpreter configuration (the form `make rs-test`/`make rs-test-critical` use for this
crate: `--features python`, with the `PYO3_*` environment the Makefile derives, because the
built `blastit` binary links `libpython`):

```console
$ env PYTHONHOME=$PYO3_PREFIX PYO3_PYTHON=$PYO3_BIN LD_LIBRARY_PATH=$PYO3_PREFIX/lib \
    cargo test -p blastit --features python --test dev_signal_reap
     Running tests/dev_signal_reap.rs (…/dev_signal_reap-4088cced2091b497)
running 1 test
test sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

**5. `make check` — NOT RUN**, by instruction (`lead` runs it). Everything that gates this
node's crates was run directly on the final tree:

```console
$ cargo fmt --all -- --check
FMT CLEAN
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler -p blastit-frame --no-deps
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.56s
$ cargo clippy -p blastit --all-targets --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.79s
$ cargo clippy -p blastit-scheduler -p blastit-frame --all-targets -- -D warnings
(no output: clean)
$ cargo test -p blastit --lib --features python supervise
running 6 tests
test supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort ... ok
test supervise::tests::a_scheduler_that_closes_with_data_queued_resets_the_channel ... ok
test supervise::tests::a_route_cannot_be_forked_past_its_ceiling ... ok
test supervise::tests::retire_refuses_a_sibling_routes_pid ... ok
test supervise::tests::every_other_ctl_failure_stays_fatal ... ok
test supervise::tests::exited_names_the_childs_own_route ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 248 filtered out; finished in 0.00s
```

### Mutation spot-checks (each temporarily applied, then reverted; the tree is green as it stands)

| Mutation | Witness that went red |
|---|---|
| `reserve`'s `min_by_key((last_used, pid))` → `max_by_key` | `pool::lru_picks_the_coldest_admitting_worker` (`condition never became true`) |
| `ViewSet::len` drops the promised fork (`u32::from(spawning.is_some())` → `0`) | `pool::growth_counts_a_promise_not_a_process` (boot burst forks all four routes) |
| drop `View::admits`' `inflight < per_process_cap` | `pool::a_php_route_never_threads` |
| drop `grow`'s `pending_ready == 0` gate | `pool::a_second_fork_waits_for_the_first_ready` |
| drop the `other_route_starved` call in `grow` | `pool::growth_yields_to_a_starved_route` |
| `other < route` → `other != route` | `pool::two_cold_routes_each_get_their_first_process` |
| `view_gone`'s `empty` → `true` (fail the queue although a sibling serves) | `pool::a_failed_spawn_backs_off_growth_only` |
| drop `view_exited`'s `pid == 0` arm | `pool::a_refused_spawn_is_one_backoff` |

Two mutations survive and are **equivalent, not gaps**: `reload`'s immediate `retire` for an
idle worker (the `stale` arm of `retire_finished_views` retires it one pump later with the
same reason) and `view_spawned`'s `spawned_epoch < reload_epoch` check (`view_ready` applies
the same rule to the same worker). Both behaviours are asserted.

## Review polish (five MINOR findings; 1–4 addressed here)

| # | Finding | What changed |
|---|---|---|
| 1 | `pool::growth_counts_a_promise_not_a_process` could not fail for the promise term: a request-driven burst is bounded by its route's one `SPAWN` slot, so only the slot was under test | **Reshaped**, name kept and now honest: the shape is `start()`'s **eager boot burst** (`eager_spawn`, four routes, `max_view_processes = 2`, `park_spawned`), where every route has its own slot in flight at once and only the live-process cap can bound the fleet. It asserts `spawns == [0, 1]`, and that it stays two once the promises are delivered. The `ViewSet::len` mutation above now kills it. **R5's other real witness** is the pre-existing `memory.rs::the_derived_cap_bounds_the_initial_prefork_burst`, which bounds a three-route prefork burst by the *derived* cap from a memory probe; this leg covers the operator-pinned cap with no probe attached. |
| 2 | `scheduler.rs` module doc still said a view retired for the cap before serving cools its route down | Clause **deleted**; the bullet now says only `MemoryPressure` books `spawn_backoff`, which is what `retire` does after §4.7 dropped the `LruCap` cool-down. |
| 3 | RESULT.md claimed three R7 tests witness `spawn`'s `count_for` refusal + `send_spawn_refused`; the shipped tests only cover the pure `may_fork` helper | **Claim corrected, not the code.** Driving `spawn` itself needs a constructed `Supervisor` (a `Parts` with `SignalFd`/`SignalMask`/`ctl` and, under `--features python`, an imported `Py<PyAny>`), i.e. a test-only constructor — a refactor this pass must not make. `may_fork` (the predicate `spawn` branches on), `ChildTable::count_for`, `retire`'s refusal and the child's-own-route `EXITED` stay tested; the refusal's emitted `manifest.warning` and its `Ctl::Exited { pid: 0 }` answer are **unwitnessed by a Rust test here** and lean on `MB.24.4`'s real-`ulimit` e2e leg (follow-up 6), which sees the graceful refusal end to end. |
| 4 | `reap_children(&mut self, _py: Python<'_>)` carried a token with no user | **Deleted**, together with `reap_until`'s and `progress_shutdown`'s `py` (their only use was forwarding it); `run` calls both without it. `poll_once`/`read_ctl`/`on_ctl`/`spawn` keep theirs: they fork. |
| 5 | Not fixed here | Out of this pass's scope, as instructed. |

Verification of this pass (final tree):

```console
$ cargo fmt --all -- --check
FMT CLEAN
$ timeout 600 cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.2.0 (…)
    Checking blastit v0.2.0 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.17s
$ timeout 300 cargo test -p blastit-scheduler --test pool
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
$ timeout 300 cargo test -p blastit --lib --features python supervise
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 248 filtered out; finished in 0.00s
```

The `MB.26.8` regression leg (`pool::a_queued_request_survives_the_worker_it_queued_behind`:
`cap_route(0,1)` + `pool_route(0,true)`, `max_requests = 1`, eight concurrent requests, every
one `200 ok`, at least two pids, every retire reason `max_requests`) is the end-to-end barrier
for that report; it exercises the pump's grow path plus the `retire`→`view_gone` ordering
rather than one branch, and the branch-level guarantee ("a route's queue is failed only when
it has no worker and no `SPAWN` left") is the `empty` mutant above.

## Files

| File | What |
|---|---|
| `crates/blastit-scheduler/src/route.rs` | `per_process_cap`, `process_pool`, two builders, doc; `new` keeps `None`/`false`. |
| `crates/blastit-scheduler/src/scheduler.rs` | `ViewSet`, `View` (`stale`/`spawned_epoch`/`admits`), `pools`/`reload_epoch`, the new geometry/selection/growth/retire/reload methods, the rewritten dispatch/retire/ready/exited/gone/release paths, `Ctl::Reload` arm, `const RELOAD`, module-doc bullets. |
| `crates/blastit-scheduler/src/task.rs` | `#[derive(Debug)]` on `Outbound`, so `ViewSet` can derive `Debug` (no behaviour change). |
| `crates/blastit-scheduler/tests/common/mod.rs` | The five additive harness items of §7 + `Script::ReadyAfter` + the `Ctl::Reload` arm (additive only; `received`/`closed`/`retires` unchanged). |
| `crates/blastit-scheduler/tests/pool.rs` | **New.** R1–R9, R11, R13, R15 (19 tests) + the `MB.26.8` leg. |
| `crates/blastit-scheduler/tests/isolation.rs` | Only the widened R10 leg: 4 concurrent requests, `view_max_workers = 1 < max_workers = 8`, `spawns == [0]`, `pids.len() == 1`. |
| `crates/blastit-frame/src/ctl.rs` | `Ctl::Reload` variant + payload table row + `type_code`/`write_payload`/`decode`; `Exited`'s `pid: 0` sentence. |
| `crates/blastit-frame/src/frame.rs` | `TypeCode::Reload = 0x45`, `from_code`, `is_ctl`, `name`, decode arm, doc range `0x40–0x45`. |
| `crates/blastit-frame/src/{lib.rs,decoder.rs,testutil.rs}` | The two round-trip tables and `all_codes()` gain `0x45`. |
| `crates/blastit/src/supervise.rs` | `ChildTable`, `VIEW_PROCESS_MARGIN`, `may_fork`, `spawn`'s refusal + `send_spawn_refused`, non-fatal fork error, `record_view`/`reap_children`/`retire`/`signal_all`/`progress_shutdown`/`reap_until` over the table, `pending_spawns` deleted, `Ctl::Reload` ignore arm, three in-module R7 legs (`ChildTable` routing, `RETIRE` refusal, `may_fork`'s ceiling). |

## Follow-ups

Carried forward, unchanged:

1. `pool.rs::lru_picks_the_coldest_admitting_worker` is mid-rewrite: its current 3-worker
   shape cannot distinguish LRU from MRU (only one worker admits after the first permit).
   Replace it with the 2-worker/2-idle shape (`r1`→pid A, `r2`→pid B, allow both, `r3` must
   land on A) so the `min_by_key`→`max_by_key` mutant dies; then re-run clippy/tests.
   — **DONE in the completion pass** (2-worker/2-idle shape; the `max_by_key` mutant is red,
   the reverted tree is green).
2. Run and record `cargo test -p blastit --test dev_signal_reap` (NODE acceptance #4).
   — **DONE** (PASS with `--features python`, tail above).
3. Run `RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler -p blastit-frame --no-deps`
   and `cargo clippy -p blastit --all-targets --features python -- -D warnings` for the
   touched supervisor. — **DONE** (both clean).
4. Write `RESULT.md` (Summary/Evidence/Files/Follow-ups) with the tails above plus the
   finished R2 evidence. — **DONE** (this file).
5. Docs/`ARCHITECTURE.md` §2.4/§2.6/§8/§8.2, `docs/reference/settings.md:68-69`,
   `blastit-view/src/php.rs` comment, `PHP_SUITE_ARGS` + real-`ulimit` e2e leg, and the ADR/
   design index rows are all outside this node's paths → plan requests.

New requests from this node:

6. **Plan request — `make e2e` real-`ulimit` leg.** Set `ulimit -u` to
   `MAX_WORKERS + VIEW_PROCESS_MARGIN - 1`, send four concurrent requests to a
   `per_process_cap = 1` route, assert four `process.spawned` with distinct pids **or** a
   graceful refusal with no shutdown (`e2e/` is outside every leaf's paths, so it is not
   this node's to write). Together with adding `--test critical_parallel_views` to
   `PHP_SUITE_ARGS` (`Makefile:637`), both belong to `MB.24.4`.
7. **Plan request — documentation deltas** from design §4.10, verbatim and unapplied here:
   `ARCHITECTURE.md` §2.4 (Admission: processes per route, one fork at a time up to
   `BLASTIT_MAX_WORKERS`; Lifecycle: a `SPAWN` again for each request the live workers cannot
   take), §2.6 (a PHP route's concurrency is its process count under the budget, and
   `BLASTIT_VIEW_MAX_WORKERS` caps one process), §8 (the cap counts workers plus the fork in
   flight, charged one fork at a time), §8.2 (both rows: "live request **slots**", "requests
   per view process; also sets its `RLIMIT_NPROC`; it does not bound a route"),
   `docs/reference/settings.md:68-69`, `crates/blastit-view/src/php.rs:9-14`, and the
   `docs/decisions/README.md`/`docs/design/README.md` index rows for `ADR-0015` and
   `parallel-views.md`.
8. **Plan request — `MB.23.5` order.** `Ctl::Reload` `0x45` exists exactly once, added here
   (the leaf that also widens `reload()`); `MB.23.5` must land **after** this node and only
   send the message (it may substitute a typed `RetireReason::Reload` once
   `crates/blastit-events` is in its paths).
9. **Observation for `MB.24.4`.** `supervise.rs`'s local `VIEW_PROCESS_MARGIN = 16` is a twin
   of `php_view`'s private, `php`-gated constant; unifying them needs a shared home outside
   both leaves' paths.
