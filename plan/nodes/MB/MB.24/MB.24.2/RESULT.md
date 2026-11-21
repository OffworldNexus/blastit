# RESULT — MB.24.2

Status: **docs complete, design `In review`**. No Rust, Python, manifest, plan or Cargo change;
`Cargo.toml`/`Cargo.lock` untouched. Worktree
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, branch `release/0.2.0`, attempt
1/3. Nothing committed.

## What landed

| File | What |
|---|---|
| `docs/design/parallel-views.md` | The design (nine sections, frontmatter `status: In review`, owner `designer`, node `MB.24.2`), 15 numbered requirements, the six settled decisions with their written reasons, exact function shapes for `MB.24.3`, the harness additions, the wall-clock test recipe, and `## 9. Review Resolution` covering every finding of all three review rounds. |
| `docs/decisions/ADR-0015-multi-worker-views.md` | Nine numbered decisions, rejected alternatives, consequences. `Status: Proposed` (acceptance is `lead`'s). |
| `docs/design/parallel-views.review-{security,implementability,testability,consistency}.md` | The four fresh-context lens reviews, each ending `VERDICT: PASS` with zero open BLOCKER/MAJOR. |

## The six decisions `NODE.md` required, as settled

1. **Instance selection** — least-recently-used among the workers that admit a request
   (`ready`, below `per_process_cap`, not stopping/stale/exited, channel open), ties by
   ascending pid. §4.2.
2. **Spawns in flight / `Spawned` matching** — exactly **one** `SPAWN` in flight per route
   (`ViewSet::spawning: Option<(epoch, deadline)>`); `Spawned { route, pid }` answers it, and
   the pid is the identity from then on. **No spawn id is added**; `MB.24.3` owns any wire
   change and the only one it makes is `Ctl::Reload` `0x45`, which `ADR-0014` already
   reserves. §4.3.
3. **What the derived cap counts** — live **processes** (workers plus the in-flight fork),
   across all routes. No route starves another: it owns no worker its requests do not need,
   a route with requests and no process is served before any route grows, and eviction only
   takes another route's idle worker. §4.4.
4. **Supervisor keys** — `ChildTable` maps pid → `ViewChild { route, uid }`; `Exited` carries
   the child's own route, `Retire` is refused for a foreign pid, and `spawn` keeps init's own
   `view_worker_cap() + 16` bound. `route_pids`/`pending_spawns` are deleted. §4.5.
5. **Reload** — one transaction over every pool: retire every idle or not-yet-`READY` worker,
   mark busy ones stale, retire them at `DONE`, retire on arrival a worker whose `SPAWN`
   predates the reload. `MB.24.3` owns `Ctl::Reload` + its arm; `MB.23.5` consumes it. §4.6.
6. **Spawn failure** — a refused or failed fork is answered with `Ctl::Exited { pid: 0 }`,
   which frees the slot and books **one** backoff for **that route only**; a view fork is
   never `fail()`, so one route's exhaustion cannot take another route's availability. §4.7.

Hard rules held: Python keeps `per_process_cap = view_worker_cap()` and one process
(`process_pool = false`, so `view_max_workers = 1 < max_workers` still yields one process,
pinned by R10); PHP keeps `per_process_cap = 1` and `max_inflight() == 1`; the difference is
boot data (`RouteInfo`), never a scheduler branch; no dependency; no second PHP transport; the
retired manifest format is not named; the phrase "one per route" does not occur in either new
file.

## Acceptance

```console
$ test -f docs/decisions/ADR-0015-multi-worker-views.md && echo ok
ok
$ grep -n "per_process_cap" docs/design/parallel-views.md | head -3
7:# Parallel views: a per-route view-process pool with `per_process_cap` (ADR-0015)
17:each with its own concurrency ceiling (`per_process_cap`), forked on demand **one at a
144:2. The ceiling is **data**: `per_process_cap` (and whether the route may pool at all),
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.34s
$ uv run --no-project tools/plan.py validate
plan OK
```

(Line numbers after `:17` drift as the document is edited; the attempt-2 block re-runs the
same commands against the final text.)

Guard greps (no public page, no forbidden wording):

```console
$ grep -rn "one per route" docs/design/parallel-views.md docs/decisions/ADR-0015-multi-worker-views.md
(no output)
$ grep -niE "blastit[.]toml" docs/design/parallel-views.md docs/decisions/ADR-0015-multi-worker-views.md
(no output)
```

## Critic passes

Three rounds: round 1 (four fresh lenses), one re-run per lens, then a closeout verification
per lens. Every finding is recorded in the design's `## 9. Review Resolution` with
`accepted -> §X`; none was rejected.

| Lens | File | Round 1 | Final |
|---|---|---|---|
| `security` | `parallel-views.review-security.md` | FAIL — 1 BLOCKER / 5 MAJOR | **PASS — 0 BLOCKER / 0 MAJOR** (2 non-graded notes) |
| `implementability` | `...review-implementability.md` | FAIL — 6 BLOCKER / 9 MAJOR | **PASS — 0 BLOCKER / 0 MAJOR** |
| `testability` | `...review-testability.md` | FAIL — 3 BLOCKER / 8 MAJOR | **PASS — 0 BLOCKER / 0 MAJOR** (6 MINOR nits) |
| `consistency` | `...review-consistency.md` | FAIL — 2 BLOCKER / 5 MAJOR | **PASS — 0 BLOCKER / 0 MAJOR** |

The findings that changed the design, not just the prose: pre-measurement fork amplification
(one fork at a time until the memory cap has been measured); a refused `fork(2)` was
tree-fatal; the growth predicate counted only queued requests (the second concurrent request
never forked); `process_ceiling` inferred the provider from a value, which made
`BLASTIT_VIEW_MAX_WORKERS` a route lever; the supervisor lost its own fork bound; the reload
missed the `SPAWNED`→`READY` window; `pending_ready` was never released; two cold routes
deferred to each other for ever.

## Plan requests (this node may not edit `plan/` or other nodes' files)

1. **`MB.24.3` `paths` amendment, and `MB.24.3` lands before `MB.23.5`** — add
   `crates/blastit-frame/src/frame.rs`, `crates/blastit-frame/src/lib.rs`,
   `crates/blastit-frame/src/decoder.rs` to **`MB.24.3`'s** paths only. `Ctl::Reload` `0x45`
   needs `TypeCode::Reload` (`frame.rs:99-170`) and the two hand-written round-trip tables
   (`lib.rs:218,233`, `decoder.rs:539`). **MB.23.5 lacks those three too** (its eleven paths
   are `frame/src/ctl.rs`, `scheduler/src/ctl.rs`, `scheduler.rs`, `supervise.rs`,
   `dev_reload.rs`, `cli.rs`, `child.rs`, `blastit/src/lib.rs`, `events/src/lib.rs`,
   `tests/critical_dev_reload.rs`, `tests/common/`), and it already overlaps `scheduler.rs` and
   `supervise.rs`, so the variant is added **exactly once, by `MB.24.3`**, and `MB.23.5` is
   ordered **after** it and only sends the message. If the amendment is not applied, the
   implementer must stop and open a human request rather than edit outside its lane.
2. **`MB.24.3` brief** — `:53` says the pending spawns become "a per-route **count**, not one
   slot". The settled answer is **one slot**; `init` forks sequentially (so a count buys no
   wall clock) and a count is what allowed a cold burst to fork unmeasured heap copies.
3. **`MB.24.4` brief** — Notes should say "asserts the documented refusal without the pair",
   not "fails without `libphp`"; acceptance 4 should read "retires to **at most** the prefork
   count" (`idle_timeout` retires a preforked worker too, `scheduler.rs:3605-3617`).
4. **Epic `MB.24` third bullet** — same "at most the prefork count" wording.
5. **New nodes**: `--test critical_parallel_views` in `PHP_SUITE_ARGS` (`Makefile:637`) and the
   real-`ulimit` leg under `e2e/`. Both files are outside every leaf's paths, so the epic's
   "`RLIMIT_NPROC` is never hit" is currently witnessed in-lane only as
   `pool.rs::a_route_never_exceeds_its_process_limit` (the property that makes the rlimit
   sufficient in the shipped default).
6. **Index rows** — `docs/decisions/README.md` (`ADR-0015` → `Proposed`, node `MB.24.2`) and
   `docs/design/README.md` (`parallel-views.md` → `In review`, node `MB.24.2`). Both tables are
   owned by `architect`/`doc-writer` and are outside this node's paths, so the rows are
   requested here and not written.
7. **`MB.24.4`'s PHP fixture** — `prefork` plus a **four-wide warm-up burst** before `t0`
   (asserting four distinct pids), and the warm-up burst's duration recorded in
   `MB.24.4/RESULT.md` (design §1/§7): a PHP worker `dlopen`s `libphp` and runs
   `php_module_startup` after the fork, so the `< 450 ms` leg only means what it says when the
   whole measured width is warm, not just the one preforked worker.

## Follow-ups (documentation, recorded for `architect`/`doc-writer`)

`ARCHITECTURE.md` §2.4 (Admission + Lifecycle), §2.6 (`:471-473`, the "caps per view" clause),
§8 Sizing, §8.2 (both `MAX_WORKERS`/`VIEW_MAX_WORKERS` rows); `docs/reference/settings.md:68-69`
(F1's "slots" wording plus the "does not bound a route" claim the accepted PHP page already
makes); `crates/blastit-view/src/php.rs:9-14` ("forks `1..=workers`"). All exact replacement
sentences are in design §4.10 (items 1-7) and §1's delta list.

## Attempt 2 (reviewer FAIL at attempt 1 — all five fixed)

The attempt-1 evidence above is kept. Findings from `LOG.md` ("REVIEW FAIL (attempt 1)"):

| Finding | Grade | Fix |
|---|---|---|
| "on a preloaded image each fork is milliseconds" is false for PHP: the child `dlopen`s `libphp` and runs `php_module_startup` after the fork (`php_view.rs:20-29`, ADR-0010 dec. 2), so §4.3's chain puts PHP startups inside the 450 ms window | MAJOR | §1's sentence is **deleted**. Because `prefork` warms only **one** worker (not the four-wide burst), the measurement now warms its own width: the fixture declares `prefork`; the suite sends a **four-wide warm-up burst** before `t0`, asserting four `200`s / four markers / four distinct pids and recording that burst's duration in `MB.24.4/RESULT.md`; the measured leg then asserts the same four pids and **no new `process.spawned`** inside `t0..t4`. The cold case is stated as pre-existing and excluded. §7, §8 and the ADR say the same; the libphp-less refusal branch and the real-`ulimit` leg remain recorded follow-ups. |
| `review-implementability.md` still said M3 "NOT RESOLVED" and quoted `other != route` while §4.4 carries `other < route` | MINOR | the row is rewritten as **RESOLVED** at §4.4:695-707 (deterministic `other < route`; acyclic; lower index forks first), and its verdict paragraph no longer calls the item returned. |
| the `frame` paths amendment did not say how MB.23.5 fits | MINOR | §1 and `RESULT.md` request **1** now commit to an order: the amendment belongs to **MB.24.3 only** (MB.23.5 lacks `frame.rs`, `frame/src/lib.rs` and `frame/src/decoder.rs`), the variant is added exactly once by MB.24.3, and MB.23.5 lands after it. |
| no index rows for ADR-0015 / `parallel-views.md` | MINOR | requested in `RESULT.md` request **6** and in §1's delta list; `docs/decisions/README.md` and `docs/design/README.md` are not this node's files. |
| R14 called `isolation` "unmodified" while R10 widens it; §4.7 used `.map()` as a statement; §4.3 cited `:1832` for the "none keyed" branch; `queued_for`/`queued_for_streams` were used but unspecified | MINOR | R14 now says "unmodified except the one widened leg R10 owns"; §4.7's decrement is `if … && let Some(pool) = …`; §4.3 cites `:1815`/`:2285`/`:2325`'s `!self.views.contains_key(&route)` guard and `:1801-1812`'s learn branch; both helpers are printed in §4.1 (`queued_for` = `queued_for_streams(route).count()`, and the iterator over `queue.ids()` filtered by `request.route`). |

§9 gains a round-4 row for these five; the **testability** lens was re-run once (fresh session)
because the MAJOR changes §7, and it ends `VERDICT: PASS`. The other three lens files were not
re-run; all four were re-read on disk and end `VERDICT: PASS` with zero BLOCKER/MAJOR.

Attempt-2 acceptance (the attempt-1 block above is unchanged):

```console
$ test -f docs/decisions/ADR-0015-multi-worker-views.md && echo ok
ok
$ grep -n "per_process_cap" docs/design/parallel-views.md | head -3
7:# Parallel views: a per-route view-process pool with `per_process_cap` (ADR-0015)
17:each with its own concurrency ceiling (`per_process_cap`), forked on demand **one at a
170:2. The ceiling is **data**: `per_process_cap` (and whether the route may pool at all),
$ make docs-check
Build started
No issues found
Build finished in 0.29s
$ uv run --no-project tools/plan.py validate
plan OK
$ grep -rn "one per route" docs/design/parallel-views.md docs/decisions/ADR-0015-multi-worker-views.md
(no output, exit 1)
```

All four verdict files on disk: `review-security.md` **PASS** (0 BLOCKER/0 MAJOR),
`review-implementability.md` **PASS** (0/0, its returned M3 rewritten as resolved),
`review-testability.md` **PASS** (fresh round 4, 0/0, including the four-wide warm-up oracle
that replaces the false "milliseconds" premise), `review-consistency.md` **PASS** (0/0).
§9 of the design carries a round-4 row for every attempt-1 finding, in the order the reviewer
listed them.

## Attempt 3 (reviewer FAIL at attempt 2 — all five fixed)

Attempts 1 and 2 above are kept. Findings from `LOG.md` ("REVIEW FAIL (attempt 2)"):

| Finding | Grade | Fix |
|---|---|---|
| `view_gone` did not compile (E0502): the printed body iterated `self.queued_for_streams(key.route)` (`&self`) while calling `self.queue.remove` / `fail_request` / `release` (`&mut self`) | MAJOR | §4.7 collects first, exactly like today's `scheduler.rs:3733-3744`: `let waiting: Vec<u32> = self.queued_for_streams(key.route).collect();` then `for stream_id in waiting { … }`. Every other printed body was re-audited: `grow` (the `Option<&ViewSet>` dies at `let owned`), `reload()` (keys collected), `retire_unheld`/`spawn_failed` (owned value/bool), `reserve` (self.lru after the last view use), and `view_spawned`'s duplicate branch is now a `bool` computed before the pool borrow. |
| §4.6 still offered "MB.23.5 lands first … either order compiles" | MINOR | the paragraph now states one order: MB.24.3 ships the message, MB.23.5 sends it. The "either order" sentence is deleted. |
| "MB.23.5's paths list only `frame/src/ctl.rs`" was false (eleven paths) | MINOR | §1 and request 1 now state the truth: MB.23.5 lacks `frame.rs`, `frame/src/lib.rs` and `frame/src/decoder.rs` and already overlaps `scheduler.rs`/`supervise.rs`, so it cannot add `TypeCode::Reload` and lands after MB.24.3. |
| `review-implementability.md`'s annex listed stale nits m5/m6/m7/m10 and missed the E0502 | MINOR | a labelled `## Designer resolution (round 5)` block is appended to that file; its verdict line is untouched. m5/m6/m7/m10 are marked already fixed at §4.7/§4.3/§7/§4.1, and the E0502 is acknowledged real and fixed at §4.7. |
| the PHP leg's `<= control - 400 ms` named no control | MINOR | §7's PHP leg is now `serialized_then_parallel_php`, which runs its **own** PHP `BLASTIT_MAX_WORKERS=1` control (`>= 800 ms`) and then, on a fresh boot, the four-wide warm-up burst and the measured four, comparing in the same function; §1's table and the ADR say the same. |

§9 gains a round-5 row for each of the five. No critic was re-run (the cap is reached); all
four review files were re-read on disk and end `VERDICT: PASS` with zero open BLOCKER/MAJOR.

Attempt-3 acceptance (attempts 1 and 2 are above; line numbers in the `grep` drift as the
document is edited):

```console
$ test -f docs/decisions/ADR-0015-multi-worker-views.md && echo ok
ok
$ grep -n "per_process_cap" docs/design/parallel-views.md | head -3
7:# Parallel views: a per-route view-process pool with `per_process_cap` (ADR-0015)
17:each with its own concurrency ceiling (`per_process_cap`), forked on demand **one at a
171:2. The ceiling is **data**: `per_process_cap` (and whether the route may pool at all),
$ make docs-check
Build started
No issues found
Build finished in 0.20s
$ uv run --no-project tools/plan.py validate
plan OK
$ grep -rn "one per route" docs/design/parallel-views.md docs/decisions/ADR-0015-multi-worker-views.md
(no output, exit 1)
```

Verdict files on disk after this round: `review-security.md` **PASS**, `review-implementability.md`
**PASS** (+ the appended designer-resolution block), `review-testability.md` **PASS**,
`review-consistency.md` **PASS** — zero BLOCKER/MAJOR in all four.

## Files touched

`docs/design/parallel-views.md`, `docs/design/parallel-views.review-{security,implementability,testability,consistency}.md`,
`docs/decisions/ADR-0015-multi-worker-views.md`, `plan/nodes/MB/MB.24/MB.24.2/RESULT.md` (this
file). Nothing else; no `git add`, no commit.
