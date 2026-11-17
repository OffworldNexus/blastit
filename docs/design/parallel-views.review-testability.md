# Testability review — final (round 4) — MB.24.2 parallel views (ADR-0015)

Scope: `docs/design/parallel-views.md` §1, §4.3–§4.8, §7, §8 (revised), the round-3 findings they
answer, and the files the plan cites for its oracles: `tests/common/mod.rs` (`Script::Hold` :64,
fake init :694-735, `ctl_send` :782, `advance` :823), `tests/isolation.rs:62`,
`tests/lifecycle.rs:242`, `scheduler.rs` (`retire` :3498 — fails the affected requests `502`,
removes the view, pushes `Ctl::Retire`; `dispatch_queued` :2249; `Scheduler::new` :834),
`frame/src/{lib.rs:218,233, decoder.rs:539}`, `Makefile:110,262-273,637`, and
`critical_php_embed.rs` (`refused_without_a_host` :3132, `host_php` :795).
This pass verifies the round-3 resolutions, plus one attempt-2 item (§4).

## 1. Round-3 findings

| # | Finding | Status | Where it is fixed |
|---|---|---|---|
| **T3-M1** | R4's duplicate-pid leg asserted a held worker "keeps serving" while §4.3 removed its entry | **resolved** | §4.3's branch is now `return self.retire(key, …"duplicate_pid")` on the held key — and `retire` (`scheduler.rs:3498-3540`) is exactly the one path that fails the affected requests `502`, drops the queue, removes the view and pushes `Ctl::Retire`; §8.2 adds `pending_ready`'s decrement for a non-`READY` removal. §7 R4 now says so verbatim ("its in-flight `Script::Hold` request is failed `502` … the newcomer is killed by the same `Ctl::Retire`, and a later request grows a fresh worker"). The leg is writable and the stranded-request path has a witness; the negative (next request grows) is present. |
| **T3-M2** | R2's cold-pool tie-break was unfalsifiable and unstageable in `pool.rs` | **resolved** | §7 R2 makes LRU the observable (`lru_picks_the_coldest_admitting_worker` — deleting the `last_used` half of the selection key, or the `admits` filter, fails it) and states in the row that `BTreeMap` order plus `min_by_key` already yields the lowest pid, so "the explicit pid in the key is defensive, not separately falsifiable". R2 no longer rests on a test that cannot fail; the defensive half is labelled. |
| **T3-M3** | the third/fourth protected legs had no satisfiable precondition | **resolved** | §7 puts "The cap holds and the pool shrinks" **inside the PHP branch** ("this leg is not attempted without a PHP route to run it on"), with the no-pair branch asserting the documented refusal once — so the file's PHP branch asserts on every run and the leg's absence is a scoped branch, not an unasserted run. "A refused spawn is not fatal" is no longer a wall-clock leg: §7 names the two seams that can induce it, `pool.rs::a_refused_spawn_is_one_backoff` (`Ctl::Exited { pid: 0 }`) and `supervise.rs::tests::a_spawn_past_the_ceiling_is_refused`, and gives the reason (no `RLIMIT_NPROC` under `blastit dev`), with the real-`ulimit` leg as the third witness in the recorded follow-up. |
| **T3-M4** | `Ctl::Reload`'s codec had no witness and its tables were outside the leaf's paths | **resolved** | §8.3 now names `crates/blastit-frame/src/{ctl.rs,frame.rs}` (the `TypeCode::Reload = 0x45` variant, `from_code`, `is_ctl`, `name`, `Frame::decode`) plus the round-trip tables `lib.rs:218,233` and `decoder.rs:539` (which enumerate the codes by hand, so the variant must be added), and §1 lines 127-133 record the three-file `paths` amendment as a plan request. Per this pass's rule, a recorded `paths` amendment is not a testability defect. |

## 2. Remaining round-2/3 status (all non-blocking)

| # | Item | Status |
|---|---|---|
| T2-B1 / T3-m2 | R13's rlimit oracle | **resolved in substance**: the in-lane bound `pool.rs::a_route_never_exceeds_its_process_limit` (`max_workers = 2`, 4 queued → 2 workers, never 3), the graceful-refusal seam, and a named follow-up. The epic line `MB.24:83-84` ("`RLIMIT_NPROC` is never hit") and MB.24.4 acceptance 4 ("the prefork count remains") are still unedited — both are the recorded plan request (§1 lines 111-114), not a design defect. |
| T2-M1 / T3-m3 | the `Some(1)`/`process_pool` wiring | **resolved as labelled**: the PHP timing leg and `PHP_SUITE_ARGS` are explicitly plan follow-ups (§7 line 1157-1161, §1 line 117). The in-lane `stage1.rs::tests` witness remains a cheap improvement, not a gap in a rule's oracle (R1's `pool_route` leg and R10's leak negative cover the scheduler side). |
| T2-M2 / m3 / m4 | timing recipe | **resolved**: one function measures control then parallel (`serialized_then_parallel_wsgi`), `started` before the `Barrier`, `t0` span ≤ 100 ms, `wall < 450` and `<= control - 400`. Over-tight thresholds remain cosmetic (below). |
| T2-M3 / T2-M4 / m1 / m6 / m9 | same-route concurrency, `park_spawned`, stale negative, misattribution, init-loop parking | **resolved** (`Script::Hold` + per-spawn markers; `park_spawned` queues and keeps receiving, `Log.spawned_sent` vs `Log.spawns`; R8 in `pool.rs` makes `stale` reachable; the "every laptop" sentence is gone). |
| T2-M8 / T3-m10 | `Dev` extraction, MB.24.4's unused protected globs | **resolved as designed**: the new file carries its own helper, extraction is a follow-up, and the path hygiene is a plan edit (this pass's rule excludes it). |
| m2 / m5 / m7 / m8 / m11 / m12 | R11's `DONE` clause, R14 "unmodified", pre-change number, getter leg, R9 recipe | **non-blocking** (below). |

## 3. Non-blocking nits (do not gate acceptance)

- R11's "no pid's log shows a second `REQ_HEAD` before its own `DONE`" is still phrased against a
  frame the receive-side log does not hold; the implementable form is
  `request_ids_of(pid) == [1]`, which is what the test will assert anyway. The rule still has a
  falsifiable negative (a threaded PHP route would put two heads on one pid).
- R13's shrink leg should assert the four spawns **before** advancing the clock, so "shrank" is
  distinguished from "never grew" (the limit leg proves growth separately, so the rule's oracle
  is not missing — just under-specified).
- R14 still says `isolation` stays "unmodified" while R10 and §8.6 widen `isolation.rs:62`; one
  clause ("except the widened leg") closes it.
- `parallel <= control - 400` with `parallel < 450` is unsatisfiable for a working host reading
  `control ∈ [800, 850)` and `parallel ∈ (400, 450)`; and `600 ms < wall` for the 8-request leg
  is redundant next to "exactly 4 distinct pids". Thresholds, not oracles.
- The recorded `pre-change` number is still taken before the cap is wired (i.e. on the new
  scheduler), not on the release branch; label it a control for the wiring.
- `supervise.rs::tests::a_spawn_past_the_ceiling_is_refused` should be stated as the predicate it
  can be (`count_for` + ceiling → refusal message), since `spawn` needs a full `Supervisor` and a
  `Python` token; the "never `fail`s" half is already evidenced in the scheduler leg.

## 4. Attempt-2 confirmation — the PHP warm-up premise (§1 :105-121, §7 :1240-1249, §8 :1388-1395, ADR-0015 :90-100)

| # | Finding | Status | Why it is now executable |
|---|---|---|---|
| **A2-M1** | The `< 450 ms` PHP leg measured three PHP startups **inside** `t0..t4`, and claimed a "measured per-worker startup cost" that no oracle produced | **resolved** | the measurement now warms the width it measures: a four-wide warm-up burst before `t0` (asserted: four `200`s, four markers, **four distinct pids**), then the measured leg asserts **the same four pids** and **`process.spawned` unchanged** across `t0..t4` |

Each of the three sub-objections now has a falsifiable assertion:

1. **`prefork` warms one worker** is stated as the reason for the design (§1 :105-107), and the
   suite answers it by warming its own width: one four-wide burst of the same four concurrent
   200 ms requests, asserting four `200`s, four markers and four distinct pids (§7 :1241-1245).
   The measured leg then asserts the same four pids and no new `process.spawned` between `t0` and
   the last body (§1 :115-116, §7 :1246-1249), so "no fork, load or module startup is hidden
   inside the measured number" is the assertion's own words — delete either half and the leg
   fails.
2. **The unmeasurable per-worker number is gone**: the phrase "measured per-worker startup cost"
   no longer appears in §1, §7, §8 or the ADR. What `MB.24.4/RESULT.md` records is the warm-up
   burst's **wall time** — which by construction contains each new worker's `dlopen` +
   `php_module_startup` — plus its pid set (§1 :111-114, §7 :1241-1244, §8 :1391-1394,
   ADR :94-99). That is measured on the host that ran it, not derived from the 200 ms sleep.
3. **The precondition cannot silently shrink**: "A warm-up that is not four wide, or that returns
   fewer than four pids, fails the test rather than shrinking the precondition" (§7 :1248-1250),
   and the pid-set equality then makes a mid-window retirement or fork fail loudly too.

What it still does not cover is stated, not hidden: a route with neither `prefork` nor warm
traffic pays its first requests' startup (§1 :119-121) — pre-existing cold-start behaviour,
explicitly outside the 450 ms claim, with `prefork` as its lever. So the `< 450 ms` leg now
asserts exactly what §1 claims (four concurrent requests to a route whose four workers are already
`READY`, measuring dispatch), and nothing §1 implies is measured is left without an oracle.

Non-blocking nit: §7's closing `RESULT.md` bullet (:1268-1270) still lists only "pre-change,
control, four pids" and omits the warm-up burst's duration that §1, §8 and the PHP bullet above
it require; §8's list (:1391-1394) is the correct one. Wording, not an oracle.

## Verdict

Every rule R1–R15 has a writable oracle that fails when the rule is deleted, and every rule
that needs one carries a negative case (R4's duplicate path, R8's stale/not-ready sweep, R9's
growth-only backoff, R10's `pool_route` leak, R13's limit, R15's `view_out` key filter). The
three protected legs have explicit host branches or are witnessed at the seams where their
condition can actually be induced. The one new wire message is covered by named round-trip tables
whose `paths` amendment is recorded as a request. The remaining round-1/2/3 items are
wording/threshold polish and plan-tree hygiene, none of which leaves a rule without an oracle.

**A2-M1 is resolved** (§4): the `< 450 ms` PHP leg now asserts a four-wide warm-up (four `200`s,
four markers, four distinct pids), then the same four pids and no new `process.spawned` inside
`t0..t4`, and records the warm-up burst's measured wall time instead of a per-worker number no
test could produce. The cold case is excluded in words; the warm case is now asserted rather than
assumed. Zero BLOCKER, zero MAJOR.

VERDICT: PASS
