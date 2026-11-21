# Result MB.3.38

**Status: done.** A contained bailout is now charged at least the wall time its
client waited and is no longer reported `outcome: "ok"`. The epic's sentence is
the bar; nothing was weakened to fit the old behaviour.

**Review fixes (attempt 2, all inside `crates/blastit-scheduler/src/scheduler.rs`):**

- MINOR 1 (fixed): the failure gate is now
  `view_answered_a_failure(head_sent, status)` — a `RESP_HEAD` from the view plus
  `status >= 500` — not `dispatched_at.is_some() && status >= 500`, so the
  scheduler's own `502`/`504`/`503` keep the documented dispatch→completion
  charge instead of being billed their queue wait. Flag used: `head_sent`
  (`Request::head_sent`, set only in `view_head`). New unit test
  `a_view_answered_500_pays_its_wall_and_a_timeout_does_not` puts the same
  numbers through both branches (18 ms vs 3.3 ms) and
  `only_a_view_answered_5xx_is_a_view_failure` pins the gate.
- MINOR 2 and MINOR 3 are the lead's follow-up node; nothing was changed for
  them here.

## Who owns the charge

`blastit-scheduler` owns it — `crates/blastit-scheduler/src/scheduler.rs`, the
uniform `R19` charge path (`Scheduler::charge`) plus the two places that finish a
response (`Scheduler::view_end`, `Scheduler::view_done`).

Neither `crates/blastit-view/src/php.rs` nor `crates/blastit/src/php_view.rs`
needed a change: the view already reports what the epic is about. `php.rs` maps
`OUTCOME_BAILOUT` (the shim's `contained = 1`) to `DoneOutcome::Errored` with the
shim's `cpu_ns`, and `serve_forever` already sends `RESP_END_COST`, and the shim
already answers the packaged `500` (`SLEEP_PHP`/`EXIT_PHP` both see it). The bug
was entirely downstream: the scheduler treated a completed `5xx` as a success and
clamped the view's own report. That report — and the clamp's upper bound,
`observed` — measure from `REQ_HEAD` (dispatch) to the last byte, while the
event's `duration_ms` measures **arrival** to last byte: the gap is exactly the
request's `queued_ms`, which on this cold-start `workers = 1` route was ~14.7 ms
of an 18 ms request (~15.8 ms of 76 ms for the `usleep` leg).

### What changed

1. `Scheduler::charge`: a request a view **answered** with a `5xx` is charged
   `R19`'s crash/`504` ceiling and its report is ignored. "Answered" is the flag
   `head_sent` (set only when a `RESP_HEAD` came from the view), taken through
   `view_answered_a_failure(head_sent, status)`; the charge decision itself is
   the pure `charge_ns(Charge { … })`. The charged span is the one the event
   reports as `duration_ms` (`finished_at - arrived`), so `cost_ns >= wall_ns`
   holds by construction. `R19`'s `ceiling = observed_wall_ns × max_threads` is
   that wall at the one live thread the NTS PHP worker `ADR-0010` forks has, and
   no kernel floor can exceed one process's own wall time, so
   `ceiling.max(kernel_floor_ns)` is that wall — no `/proc` read is needed to
   satisfy `R19` here. A `5xx` the scheduler wrote itself — the `502` of a view
   that never answered, a `504` timeout, a `503` queue refusal — is **not** this
   case and keeps the old dispatch→completion clamp: no view answered it, so
   there is no view report to distrust and no worker time to bill.
   *(Reviewer MINOR 1: the first cut keyed on `dispatched_at.is_some()`, which
   also caught scheduler-written `502`/`504` and charged a timed-out request its
   queue wait. Fixed in the same file, before any commit; `head_sent` is the
   flag that distinguishes them.)*
2. `completed_outcome(status)`: a response a view completed is `ok` unless its
   status is `5xx`, which is `view_died` — §8.1's outcome list is closed and has
   no "the app failed", exactly as `view_abort` already reports a `RESP_ABORT`.
   The label and the bill are one decision, so both finish paths
   (`view_end`, and the missing-`RESP_END` branch of `view_done`) take it from
   the same helper. Both paths already require `head_sent`, so the label only
   ever applies to a view-answered response. No new wire value and no new
   `Outcome` variant was introduced, so `docs/reference/log-events.md`'s
   enumerated list stays true (`view_died` is already one of the seven).
3. `crates/blastit/tests/critical_php_embed.rs` (`containment(...)`):
   `cost_ns >= cpu_ns` → `cost_ns >= wall_ns` (the event's own
   `duration_ms × 1e6`), `outcome != "ok"` added, `cpu_ns <= wall_ns`,
   `cpu_ns > 0 || wall_ns < TICK_FLOOR_NS`, respawn and one-live-worker
   assertions untouched, plus the new `usleep` discriminator leg.
4. `scheduler.rs`'s unit module (the only test home this node's `paths` allow;
   `crates/blastit-scheduler/tests/**` is not in them): three tests — the label
   table, `only_a_view_answered_5xx_is_a_view_failure` (`head_sent` × status,
   including `false`/`502`, `false`/`503`, `false`/`504`), and
   `a_view_answered_500_pays_its_wall_and_a_timeout_does_not`, which pins the
   nanosecond charge for both: identical numbers, a view-answered `500` pays
   `18 ms` (the wall) and a `504` pays `3.3 ms` (dispatch→completion).

## The `exit()` triple, before and after

Before (base `7d4df45`, `-- --nocapture --test-threads=1`, host PHP 8.4.27 +
`make php-sapi`):

```console
test an_exit_is_contained_and_the_next_request_is_served ... exit(): status 500  cpu_ns=228653  wall_ns=17991297  cost_ns=3390879  retired pid 2916070  respawned pid 2916071 (ppid 2916044) uid 1000  live=[2916071]
test a_fatal_is_contained_and_the_next_request_is_served ... fatal: status 500  cpu_ns=1560570  wall_ns=17905814  cost_ns=3279372  retired pid 2915840  respawned pid 2915841 (ppid 2915814) uid 1000  live=[2915841]
```

`MB.3.13.4` recorded `cpu_ns=1543666 wall_ns=18336040 cost_ns=3221782`: the same
shape — **3.4 ms charged for an 18 ms request**, `outcome: "ok"`.

After (final tree, after the MINOR 1 fix):

```console
test an_exit_is_contained_and_the_next_request_is_served ... exit(): status 500  outcome=view_died  cpu_ns=258287  wall_ns=18013008  cost_ns=18013008  retired pid 3082914  respawned pid 3082915 (ppid 3082888) uid 1000  live=[3082915]
test a_fatal_is_contained_and_the_next_request_is_served ... fatal: status 500  outcome=view_died  cpu_ns=1777934  wall_ns=18174586  cost_ns=18174586  retired pid 3082655  respawned pid 3082656 (ppid 3082629) uid 1000  live=[3082656]
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... usleep(): status 500  outcome=view_died  cpu_ns=96459  wall_ns=75470582  cost_ns=75470583  retired pid 3082808  respawned pid 3082809 (ppid 3082782) uid 1000  live=[3082809]
```

`cost_ns == wall_ns` on all three (the `usleep` leg is one nanosecond over, from
the f64 round-trip in `duration_ms`); the worker still retires cleanly
(`status 0`), the slot still respawns into one live worker, and the next request
is still served `200`.

## The discriminator leg (`a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time`)

`SLEEP_PHP` is `usleep(60000); exit(0);` — 60 ms of wall and no CPU to speak of.
Measured: `wall_ns=75470582`, `cpu_ns=96459` (1/782 of the wall). `containment`
asserts, for that leg only, `wall_ns >= 60 ms` and
`cpu_ns * CPU_SHARE (20) <= wall_ns`, so a charge taken from `cpu_ns` cannot
satisfy `cost_ns >= wall_ns` by orders of magnitude instead of by a rounding
error. (This is the clock distinction `LESSONS.md` (`MB.3.12`) records from the
other side: a *fixture* must never use `usleep` when it needs CPU.)

## The timeout/`500` split (reviewer MINOR 1)

Same numbers, two different bills, pinned as a unit test
(`scheduler::tests::a_view_answered_500_pays_its_wall_and_a_timeout_does_not`):

```console
$ cargo test -p blastit-scheduler --locked --lib scheduler::tests   # EXIT=0
running 4 tests
test scheduler::tests::a_completed_response_is_ok_unless_the_view_failed_it ... ok
test scheduler::tests::a_static_slot_is_releasable_only_when_its_read_is_not_parked ... ok
test scheduler::tests::only_a_view_answered_5xx_is_a_view_failure ... ok
test scheduler::tests::a_view_answered_500_pays_its_wall_and_a_timeout_does_not ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 46 filtered out; finished in 0.00s
```

| shape | `head_sent` | status | charged |
|---|---|---|---|
| contained bailout (view answered) | `true` | `500` | `wall` = 18.0 ms |
| request timeout (scheduler-written) | `false` | `504` | `observed` = 3.3 ms |
| dead view (scheduler-written) | `false` | `502` | `observed`, as before |
| an under-reporting `200` | `true` | `200` | `floor` clamp, unchanged |

## Acceptance

### `cargo test -p blastit --locked --test critical_php_embed -- --nocapture --test-threads=1`

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --nocapture --test-threads=1   # EXIT=0
running 9 tests
test a_fatal_is_contained_and_the_next_request_is_served ... fatal: status 500  outcome=view_died  cpu_ns=1777934  wall_ns=18174586  cost_ns=18174586  …
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... usleep(): status 500  outcome=view_died  cpu_ns=96459  wall_ns=75470582  cost_ns=75470583  …
test an_exit_is_contained_and_the_next_request_is_served ... exit(): status 500  outcome=view_died  cpu_ns=258287  wall_ns=18013008  cost_ns=18013008  …
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.21s
```

(9, not the 8 in the node's sample block: the node asked for a CPU-vs-wall
discriminator fixture, which is the ninth test. `make check` runs the same suite
with `--features python` and also reports `9 passed`.)

Same suite in the default parallel mode, and the refusal branch on a host without
PHP, are unchanged: every leg branches on `host_php()` alone.

### `cargo test -p blastit-scheduler --locked` (the non-crash clamp is unchanged)

```console
$ cargo test -p blastit-scheduler --locked   # EXIT=0, 16 "test result: ok" lines, 0 failed
...
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.21s; merged doctests compilation took 0.20s
```

`fairness.rs`'s `a_report_larger_than_the_observed_time_is_clamped` (200-status,
`cost_ns <= bound`) and `an_under_reporting_view_is_charged_at_least_until_its_first_frame`
pass untouched — only the `5xx` path changed.

### `cargo test -p blastit --locked`

```console
$ cargo test -p blastit --locked   # EXIT=0, 13 × "test result: ok", 0 failed
test result: ok. 184 passed; 0 failed; 1 ignored; …   unittests src/lib.rs
test result: ok. 19 passed; …                         critical_check_engine
test result: ok. 11 passed; …                         critical_config_and_presets
test result: ok. 5 passed; …                          critical_php_backend
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.88s   critical_php_embed
test result: ok. 11 passed; …                         critical_static_cli
test result: ok. 31 passed; …                         smoke
test result: ok. 0 passed; 0 failed; 0 ignored; …     Doc-tests blastit
```

(`critical_dev_and_fail_closed` reports `0 passed` on this host because every leg
it has is gated — pre-existing, and `0 failed` either way.)

### `make check`

```console
$ make check   # EXIT=0
guard: OK (12 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 6 changed file(s)
  M  crates/blastit/tests/critical_php_embed.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
… RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
… 123 × "test result: ok"
python -m unittest discover -s python/tests/critical
Ran 50 tests in 3.117s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline zensical build --strict
… uv run --no-project python python/tests/runnable_docs/run.py
clippy      -p blastit{,-view} (standalone)
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

The `[VIOLATION: not covered by any node path]` line is `make check`'s own
**report-only** call (`critical_guard.py --base HEAD`, no `--node` — it consults
no node's `paths` at all). The node-scoped invocation the acceptance names is
green:

```console
$ uv run --no-project tools/critical_guard.py --base 7d4df458cef3483fa0b3f8d8f4702fda14bca9ab --node MB.3.38   # EXIT=0
critical-guard: 7d4df458cef3483fa0b3f8d8f4702fda14bca9ab..working tree, 8 changed file(s)
node MB.3.38: paths ['crates/blastit/tests/critical_php_embed.rs', 'crates/blastit/src/php_view.rs', 'crates/blastit-view/src/php.rs', 'crates/blastit-scheduler/src/**', 'docs/knowledge/LESSONS.md']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.38
```

(The other changed files in that range are `plan/` edits from other live nodes,
plus this node's own `RESULT.md`; `LESSONS.md` is authorized by the node's own
`paths`, which `lead` extended.)

### `uv run --no-project tools/plan.py validate`

```console
$ uv run --no-project tools/plan.py validate   # EXIT=0
plan OK
```

## Mutation note (revert the charge, watch the new assertion fail)

The charge was reverted locally (the `R19` branch of `charge_ns` disabled:
`if charge.failed && false { … }`) while the label change stayed, so the run
isolates the **charge** as the thing the new assertion catches:

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --nocapture --test-threads=1
test a_fatal_is_contained_and_the_next_request_is_served ...
thread 'a_fatal_is_contained_and_the_next_request_is_served' panicked at crates/blastit/tests/critical_php_embed.rs:1873:5:
fatal: a crashed request must be charged at least its wall time (cost 3218461, wall 18003078, cpu 10164): {…,"cost_ns":3218461,"cpu_ns":10164,"duration_ms":18.003078,…,"outcome":"view_died","queued_ms":14.756012,…,"status":500,…}
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ...
thread 'a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time' panicked at crates/blastit/tests/critical_php_embed.rs:1873:5:
usleep(): a crashed request must be charged at least its wall time (cost 60511709, wall 74853921, cpu 89246): {…,"cost_ns":60511709,"cpu_ns":89246,"duration_ms":74.853921,…,"outcome":"view_died","queued_ms":14.311198,…,"status":500,…}
test an_exit_is_contained_and_the_next_request_is_served ...
thread 'an_exit_is_contained_and_the_next_request_is_served' panicked at crates/blastit/tests/critical_php_embed.rs:1873:5:
exit(): a crashed request must be charged at least its wall time (cost 4360086, wall 19057485, cpu 2206425): {…,"cost_ns":4360086,"cpu_ns":2206425,"duration_ms":19.057485999999997,…,"outcome":"view_died","queued_ms":14.673307,…,"status":500,…}
failures:
test result: FAILED. 6 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.06s
```

All three containment legs, and only those, go red. Restored immediately; the
suite is green again (`9 passed` above) and `git diff` shows only the real fix.

(The unit tests do not cover this mutation — they pass a `failed` flag in rather
than the flag's inputs — so the PHP leg is still the charge's witness, which is
the CI gap follow-up 3 records.)

## Files touched

`git status --short -- crates/`:

```
 M crates/blastit-scheduler/src/scheduler.rs
 M crates/blastit/tests/critical_php_embed.rs
```

## Follow-ups

1. **`ARCHITECTURE.md` now under-describes `cost_ns`** (out of this node's
   `paths`). Two places need the `5xx` rule: §2.4's "The `request` event keeps
   both numbers" paragraph (`ARCHITECTURE.md` ~323–326) and §8's `cost_ns`
   paragraph (~1497–1501: "the view process's own measurement, clamped between
   … or that observed time when no report arrived"). Both now need "…or, when
   the view **answered** the request with a `5xx`, the request's own wall time —
   a `5xx` the scheduler wrote (`502`/`504`/`503`) keeps the clamp".
   `docs/reference/log-events.md` needs no edit: the value set is unchanged and
   `cost_ns`/`cpu_ns` are still described as what was charged / what was
   reported.
2. **`blastit-events`' `Outcome::ViewDied` gloss** says "The view process died
   mid-request, `502`"; it now also covers a contained bailout that answers
   `500` and keeps its worker (the same extension `view_abort` already relied
   on). Needs a node that owns `crates/blastit-events/src/lib.rs`.
3. **The charge has no `make check` witness on a PHP-less host** (this node's
   `paths` cover `crates/blastit-scheduler/src/**`, not its `tests/`). The
   *decision* is now pinned in CI by
   `only_a_view_answered_5xx_is_a_view_failure` +
   `a_view_answered_500_pays_its_wall_and_a_timeout_does_not`; the
   end-to-end wire path (a view that really sends `RESP_HEAD 500` +
   `RESP_END_COST`) is only witnessed where a NTS `libphp` exists. A
   `blastit-scheduler/tests/*.rs` case — a view script that sends
   `RESP_HEAD 500`, `RESP_END_COST` for a fraction of the request's wall, then
   `DONE`, asserting `cost_ns >= duration_ms × 1e6` and `outcome != ok` — closes
   it without PHP. Separately, `critical_php_embed.rs`'s own header already
   records that `.github/workflows/php-sapi.yml` never runs this suite; adding
   `cargo test -p blastit --locked --test critical_php_embed` there (after the
   shim build) is still the CI owner's follow-up from `MB.3.13.4`.
4. **A bailout after the script produced output cannot be a `5xx`**: the shim
   only forces the `500` when nothing was promised and PHP's own output buffer is
   empty (`bailed && !ctx.headers_sent && ctx.body_bytes == 0 && bt_output_empty()
   && http_response_code == 0`, `blastit_php_sapi.c`), so `echo "hi"; exit(0);`
   completes as a `200` + body — reported `ok` and charged the clamp, not the
   wall. The signal that would fix it is already on the wire: `DoneOutcome::Errored`
   follows the response, and `view_done` drops it once the request is `complete`.
   Acting on it means labelling and charging at `DONE` instead of at `RESP_END`
   for that shape — a scheduler policy decision wider than this node.
