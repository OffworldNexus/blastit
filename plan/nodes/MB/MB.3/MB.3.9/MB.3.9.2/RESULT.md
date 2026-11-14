# RESULT MB.3.9.2 — ADR-0008 + design §4.1 amendment: no `listen`, derived pool socket, optional budgeted `workers`

## Summary

`[route.fastcgi]` is now `entry` plus an **optional** `workers`. `listen` is gone: `blastit dev`
spawns the pool, so blastit owns the socket, and the socket is a pure function of the route's
manifest index and blastit's run directory (production `<runtime>/fastcgi/<route_index>/fpm.sock`,
`dev`/`check` `<run_root>/fastcgi/<route_index>/fpm.sock`). `workers` is optional; absent means
the route's **per-view cap** (`Config::view_worker_cap()`, `config.rs:1407-1411`), and both the
explicit and the resolved value are bounded by `1..=64` (`MAX_POOL_WORKERS`) because `pm = static`
pre-forks every worker. Over budget or over the ceiling is a **boot error naming both numbers**,
never a clamp. `ADR-0008` decisions 1, 2 and 6 carry dated `Amends` bullets (with an inline
pointer at decision 2 and two at decision 6), so this is a documented amendment, not a silent edit.

Three things a reviewer should look at first:

1. **The error variant is `FastCgiListenDerived`, not `UnknownKey`** — the brief said "unknown
   key"; the implementation that landed first (`parse.rs:958`, `error.rs:511`, already tested at
   `parse.rs:1249`) returns a dedicated variant carrying **no value** whose message is
   `route "app" (index 0): fastcgi listen: unknown key; the pool socket is created by blastit`.
   That satisfies "refused with the key named" in substance and is strictly better DX, so the
   design adopts the shipped shape and says so (§4.1 key/error tables, §7.2
   `listen_is_refused_by_name`, §8 `MB.3.5.1`, ADR `Amends`). Reversing this is one line in
   three places if the lead prefers the literal `UnknownKey`.
2. **`workers` was kept**, justified by one real constraint: PHP worker RSS is tens of MB, so a
   memory-heavy route may need four workers under a sixteen-worker deployment without shrinking
   every WSGI view (the only other knob, `BLASTIT_VIEW_MAX_WORKERS`, is deployment-global). It can
   only *lower* a route below its budget, is documented "usually omit", and `dev` prints the
   resolved number once on the existing stderr channel.
3. **The socket directory story is three modes, not one**: `<runtime>` keeps the shipped
   root-owned `0755` (php-fpm's absolute `bind` needs `x`), the new `<runtime>/fastcgi` container
   is `init 0711` (traverse-only, init the only writer, not listable), and `<i>` is
   `0700` → `0755 route_uid` only in the `S2`–`S6` bind window → `0511 init` steady. `dev`/`check`
   run the single-uid variant (`S0`,`S1`,`S3`,`S4`,`S7`,`U1`) and never enter the window.

## Evidence

### (a) `make check` (full fast unprivileged gate)

```console
$ make check > /tmp/opencode/make-check.log 2>&1; echo "EXIT=$?"
...
56 fences: 29 executed, 27 accounted, 0 failed · 20.8s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

Rust and Python suites in that run all report `test result: ok` (e.g. `32 passed`, `25 passed`,
`22 passed`, `19 passed`); no failures and no ignored critical tests. The **final tree** (after the
last round-3 edit) was re-run: `make check` again ended

```console
56 fences: 29 executed, 27 accounted, 0 failed · 25.5s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

with `grep -c 'test result: FAILED'` = `0`. Attempt 2's findings-fix pass re-ran the gate on its
final tree:

```console
56 fences: 29 executed, 27 accounted, 0 failed · 21.4s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c 'test result: FAILED'` = `0`; `zensical build --strict` → `No issues found`.

### (b) `make docs-check` (Zensical `--strict`)

```console
$ uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
```

### (c) `plan.py validate`

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

### (d) `listen` is no longer a manifest key anywhere in the design

```console
$ grep -c 'listen' docs/design/php-fastcgi.md
54
$ grep -n 'listen' docs/design/php-fastcgi.md | grep -vi 'derived\|SYS_listen\|listen(2)\|listen=\|listen.mode'
24:> `listen` key**. `blastit dev` spawns and supervises the pool, so blastit owns the
29:> §4.1 `R3`, §4.8.1), and a manifest that still writes `listen = "…"` is an unknown
50:| the manifest with `[route.static]` + `[route.fastcgi]` (`entry`, optional `workers`; **no `listen`**) | §4.1 |
195:user map (§6). **No user-authored socket path, socket name or socket mode:** `listen` is
205:  (optional integer). `listen` is **not** a key: a manifest that writes it gets
651:and a third is a page change with its own DX node. `env`, `listen` and any other key are
```

Every hit is the removal itself, the generated pool file's INI `listen=`/`listen.mode`, the
`listen(2)`/`SYS_listen` syscall, or the "why not" alternative (§6 item 13). No table row,
requirement or error variant treats `listen` as a key a developer writes, and `UnknownKey` now
appears only in the historical round table.

### (e) `ADR-0008` carries the dated amendment

```console
$ grep -n 'MB.3.9.2' docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md
90:   `MB.3.9.2`: `listen` is removed and `workers` is optional - see *Amends* below.)**
275:- **Decision 2 of this ADR (amended 2026-11-14 by `MB.3.9.2`, owner decision):** the
343:- **Decisions 1 and 6 of this ADR (amended 2026-11-14 by `MB.3.9.2`):** the two sentences that
```

The `Amends` bullets name the removed key, the budget mapping with the config.rs line numbers
(`1407-1411`, `674,677`, `841-842`, `1014-1023`, `1301-1321`), the `zygote.rs:52`/`:115-121`
comparison, the two-route interaction, the refusal-not-clamp argument, and the three retracted
"`dev` never spawns" sentences.

### (f) Fresh goldfish critics on disk

```
docs/design/php-fastcgi.review-implementability-amendment.md      round 1, FAIL
docs/design/php-fastcgi.review-security-amendment.md              round 1, FAIL
docs/design/php-fastcgi.review-testability-amendment.md           round 1, FAIL
docs/design/php-fastcgi.review-consistency-amendment.md           round 1, FAIL
docs/design/php-fastcgi.review-implementability-amendment-r2.md   round 2, FAIL
docs/design/php-fastcgi.review-security-amendment-r2.md           round 2, FAIL
docs/design/php-fastcgi.review-testability-amendment-r2.md        round 2, FAIL
docs/design/php-fastcgi.review-consistency-amendment-r2.md        round 2, FAIL
docs/design/php-fastcgi.review-implementability-amendment-r3.md   round 3, FAIL (4 MAJOR, all fixed)
docs/design/php-fastcgi.review-security-amendment-r3.md           round 3, FAIL (2 MAJOR, all fixed)
docs/design/php-fastcgi.review-testability-amendment-r3.md        round 3, FAIL (2 MAJOR, both fixed; report cites a superseded snapshot)
docs/design/php-fastcgi.review-consistency-amendment-r3.md        round 3, PASS (3 MINOR, all fixed)
docs/design/php-fastcgi.review-consistency-amendment-r3b.md       round 3 re-verify, FAIL (1 MAJOR, fixed; 3 MINOR, fixed)
```

Round 3 (verification pass) files and their dispositions are recorded below.

## Round-3 dispositions

Round 3 was the **last** review round (per the lead's instruction): BLOCKER/MAJOR were fixed,
MINORs recorded here as accepted-with-reason, and mechanism-dependent findings deferred to
`MB.12` instead of resolved.

| Pass | Lens | Verdict | BLOCKER / MAJOR opened | Disposition |
|---|---|---|---|---|
| 3 | `implementability` | FAIL | R3-1 decision 6's `0555` + a self-contradicting `U1` sentence; R3-2 the retired "unreachable" e2e leg name and a duplicated `verify_private_dir`; R3-4 `check_fastcgi_workers`' own doc comment inverted the arms ("absent is always fine") | R3-1: decision 6 opens with an `MB.3.9.2` note declaring every `0555` pre-amendment and naming the amended modes, and `U1` reads "freeze … **amended: 0511**, `dev`/`check` run the single-uid variant"; R3-2: leg renamed `…_is_traversable_but_not_writable_…`, and `verify_private_dir` is **moved** to `probe.rs` and called by `Dev::adopt` (one home); R3-4: the doc comment now states the three arms in order and says omission is **not** automatically fine. Its MINORs R3-5/R3-6/R3-7 were taken too (formatter and 512 unit added to `Lands`; the §9 external-mode annotation corrected) |
| 3 | `security` | FAIL | SEC-R3-1 the rename-aside moved the wedge (a predictable destination, and 5 planted entries tripping ">4 = boot error"); SEC-R3-2 the "single-uid variant" skipped only `S2`/`S5`/`S6` but kept `U1`'s freeze and `S0`/`S3`'s `drop_privileges` (which fails unprivileged at `setgroups`) | SEC-R3-1: eight rename slots, empty `stale.*` removed opportunistically, and **a leaf that cannot be freed never fails the boot** — it fails that one route's pool (`U1` → `Busy` → `503`) with a warning, so one compromised worker cannot brick the process; SEC-R3-2: the variant is now a delta **table** (skip `drop_privileges`, skip `S2`/`S5`/`S6`, `U1` without the freeze; `S1`/`S4`/`S7` as written). SEC-R3-3 (MINOR) taken: ADR mode note added, "one derivation site"→"one builder per consumer, no caller-supplied path", and `write_ini_value` covers **keys** as well as values |
| 3 | `testability` | FAIL (report written against a superseded snapshot; both MAJORs addressed) | R3-1 the ADR still pinned `0555` five times and the mode trio had **no named oracle**; R3-2 `check_leaf`'s two exit codes had no named test and no owner that can run one | R3-1: `pool_parent_modes_are_0600_0511_0711` (in `MB.3.6.3.3`, R11 and §7.2), the §9 `MINOR-T12` row annotated "Superseded by `MB.3.9.2`", and the ADR decision-6 annotation; R3-2: `leaf_missing_after_spawn_exits_1_in_dev` (`MB.3.9.5`) and `check_child_leaf_failure_exits_2_naming_the_derived_socket` (`MB.3.6.4.2`), both named in R20 and in the two `Lands` cells (the critic's own note confirms the dev leg landed while it ran; the check leg is in §8.1 item 4 and `MB.3.6.4.2`). Its MINORs R3-3/R3-4/R3-5 taken: every `Lands` cell synced to §7.2/§8.1; the capacity twin moved to the `R9`–`R20` row and owned by `MB.3.6.2.1`; the harness seam now states the fake child's `ChildIo.limits = pool_rlimits(workers)` and its `getrlimit` report pipe |
| 3 | `consistency` | **PASS** | none | MINORs M1–M3 taken (the ADR decision-6 note, §8.2 no longer says "drop the arm", `Dev::adopt` throughout). Because the documents changed after this PASS, one re-verify of this lens was run (`…review-consistency-amendment-r3b.md`) |

**Consistency re-verify (after the final edits).** `…review-consistency-amendment-r3b.md` returned
FAIL with **one MAJOR**, introduced by my own `B3` rewrite: `B3`'s failure cell still said a
non-`ENOENT` removal is a boot error, contradicting the new "a leaf that cannot be freed never
fails the boot" rule. Fixed: `B3`'s action cell now defers to `remove_entry` and its failure cell
splits the two outcomes (capability `EPERM` → `exit 1`; unfreeable leaf → warning + that route's
pool `Busy`/`503`, other routes unaffected). Its three MINORs were taken as well (`check`'s phase 2
now states its observable — the child `dev` exiting `1` mapped to `exit 2`; the last `<DevRoot>`
became `<run_root>`; and the dev socket's `0600` is attributed to the rendered `listen.mode`, not
to php-fpm's umask). M1–M3 from the passed pass were verified closed by the same report. Per the
lead's round cap, the fix was not followed by another lens pass.

**MINORs accepted with reason (not edited, or deferred to a named owner).**

1. §9's **historical** rows keep their original `0555` and `external_pool_never_spawns_php_fpm`
   text, annotated "superseded by `MB.3.9.2`"; rewriting a past disposition would falsify the
   record.
2. `MB.3.9.3/NODE.md:76` and `MB.3.9.4/NODE.md:38-39` still say "`PoolSpawn::External` survives
   for `check`". They are outside this node's `paths`; §8.2 marks this document normative there
   and the conflict is flagged for the lead. With no external pool any more that sentence makes
   `blastit check -- <suite>` answer `503` on every request.
3. The real 5 s probe-timeout path (`kill(SIGKILL)` + `waitpid`) is a `requires-root` leg: no
   unprivileged test can hold a silent child for five seconds and observe the kill.
4. `MB.3.7` runs before `MB.3.9.4`, so it cannot assert "the pool `dev` started exists"; that
   assertion lives in `MB.3.9.5` and §7.3 says so.
5. `pool_rlimits(300)` is a formula-only unit: at `resolved ≤ 64` the `max(1024, …)` branch is
   unreachable, so `RLIMIT_NOFILE` is effectively the constant `1024` (kept so a future ceiling
   change cannot silently lower it).
6. `MB.3.9.5`'s brief asks for the resolved absolute socket path in the `--json` boot document;
   §1.2/R3 keep the path out of `--json`/`http_obs.jsonl`/`--learn` (boot diagnostics only). The
   security rule wins and §8.2 says so.
7. `MB.3.5.2` exceeds the 7-file cap once `dev_learn.rs` joins its paths (for the single
   predicate); §8.1's `MB.3.5.2.1`/`.2` fallback split is the planned relief.
8. `ADR-0008`'s status stays `Proposed` (the node text called it accepted); the amendment says it
   takes effect with the same acceptance.

## Decisions taken that the brief left open

| Question | Answer | Where |
|---|---|---|
| over-budget: refuse or clamp? | **refuse**, naming both numbers. A clamp would deliver a concurrency the manifest does not declare (the defect class that removed `listen`) and make `pm.max_children`, `RLIMIT_NPROC` and `RouteState.worker_cap` disagree | design §3 `R4`, §4.1; ADR `Amends` |
| keep `workers` or delete it? | **keep**, as a downward per-route override for PHP's per-worker RSS; "usually omit", resolved value printed by `dev` | design §1.2, §4.1; ADR `Amends` |
| two FastCGI routes, one budget | **not divided**: each resolves to the full per-view cap; the global `slots < max_workers` gate plus each route's permit is the admission truth, so a pool is never saturated by admitted requests | design §3 `R4`, §4.2.1; ADR `Amends` |
| where is the derived value printed? | `dev` prints one stderr line per route through the existing `eprintln!("blastit dev: …")` channel, naming the variable that supplied the cap; production prints nothing and records `pm.max_children` in the pool file; the socket **path** is only ever in a boot diagnostic | design §1.2 |
| `<runtime>` mode | **unchanged** root-owned `0755`: php-fpm's absolute `bind` needs traversal, and `blastit-sys::sock::create_parents` already creates it that way (`sock.rs:452-454,497-519`). Only the new `fastcgi` container (`0711`) and `<i>` (`0700`/`0755`/`0511`) are new | design §4.8.3 |
| `listen` error shape | `FastCgiListenDerived` (no value, names the key and the owner), adopting the shipped parser; see Summary item 1 | design §4.1; ADR `Amends` |

## Deferred to `MB.12` (embed-vs-pool re-decision) and to sibling nodes

- **Deferred to `MB.12`'s embed-vs-pool decision** (a finding here depends on whether a pool
  process exists at all):
  - the pool file's ownership hardening — `php-fpm.conf` is `0600` route-uid and protected only by
    `open_basedir` today (security round-1 S-4 / round-6 MINOR-39, still open);
  - `dev`'s `enforce_socket_inode` value — `false` today because a human could restart the pool;
    blastit-owned `dev` should probably pin `true` (security S-10);
  - deleting `PoolSpawn::External` and every `php-fpm.conf` directive/path decision;
  - **the `workers` resolution itself** — the `resolved(cap)` helper, `MAX_POOL_WORKERS`, `check_fastcgi_workers`
    and `FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded`. Their owner is the **pool leaf,
    gated on `MB.12`**, not `MB.3.5.1`; the manifest key and its `1..=64` grammar bound are this node's
    and survive either mechanism (attempt-2 fix (e)).
- **`MB.3.9.3`**: who spawns the pool in `dev`/`check`, discovery order, cleanup, the phase-2 call
  site, and the `dev` run root (`Dev::adopt`'s today vs a project-local `.blastit/run`).
- **`MB.3.9.4`**: flips `dev`/`check` to `Managed`.
- **`MB.3.9.5`**: owns `dev_prints_resolved_pool_workers` and `dev_starts_one_pool_per_route`.
- **Sibling briefs are stale and outside my paths** (design is normative, recorded in §8.2):
  `MB.3.9.3/NODE.md:76` and `MB.3.9.4/NODE.md:38-39` still say "`PoolSpawn::External` survives
  for `check`"; with no external pool any more that sentence makes `blastit check -- <suite>`
  answer `503` on every request.
- **`MB.3.7` runs before `MB.3.9.4`** (`depends_on: [MB.3.6]`), so its e2e cannot assert "the pool
  `dev` started exists"; that assertion moved to `MB.3.9.5` and §7.3 says so.
- **`MB.3.8`** must document the new `<runtime>/fastcgi/<route_index>/` layout and its modes in
  `ARCHITECTURE.md` §2.2 (added to its `Lands` cell).
- **`MB.3.5.2` file budget**: `dev_learn.rs` joins its paths (so `verify_private_dir` has one
  home), which pushes it over 7 files; the §8.1 item 3 fallback split (`MB.3.5.2.1`/`.2`) is the
  planned relief.

## Attempt 2 (findings-fix pass, 2026-11-14)

Review of attempt 1 raised **one MAJOR and five MINORs**. All six are closed; nothing else was
restructured and every earlier resolution stands. At attempt 2 the design was **2682 lines**; `ADR-0008`
is **409** (attempt 3 ends at 2686 / 409).

| Finding | Fix | Where |
|---|---|---|
| **MAJOR** — "a leaf that cannot be freed never fails the boot" had no named test or owner | named `eight_planted_stale_entries_leave_only_that_route_503` (temp `parentfd` + eight `stale.*` entries: a warning, that route's `PoolExited`/`Busy`/`503` + `Retry-After: 1`, other routes boot and serve, never `exit 1`; no root). It is named in the rule itself, in §7.2 and in `MB.3.6.3.1`'s `Lands` next to `planted_entry_at_the_leaf_is_removed` | §4.8.3, §7.2 (`R2`/`R3`/`R20`), §8 `MB.3.6.3.1` |
| (a) §9 said "three review passes" but listed two | the heading is now "two review passes and the round-3 verification", and pass-3 rows were added for all four lenses plus the `3b` re-verify (with its one MAJOR) | §9 |
| (b) §7.3 implied `dev` starts a pool before `MB.3.9.4` | scoped: the replay asserts `dev` answers and the **generated** pool files are correct; "the pool `dev` started exists" is `MB.3.9.5`'s `dev_starts_one_pool_per_route` and only holds after `MB.3.9.4` (before it, `stage1` sets `External` for `dev`/`check`) | §7.3 |
| (c) all four production legs were routed through `FastCgiPoolSocketInvalid` | the **three `dev` legs** use the variant; the **production** legs (`check_fastcgi_second_boot_normalises_a_frozen_parent`, `check_fastcgi_b2_foreign_owner_segment_refused`) are stage-0 boot diagnostics through `stage0::fail_pool_parent`, explicitly **not** a `ManifestError` | §7.2 |
| (d) `U1` claimed the owner can unlink in `0511`, and that no non-socket entry can survive | corrected: at `0511` only `CAP_DAC_OVERRIDE` unlinks; an inert `stale.<pid>` may remain and that is fine | §4.8.3 `U1` |
| (e) `resolved_workers`/`MAX_POOL_WORKERS`/worker variants were assigned to `MB.3.5.1`, contradicting the frozen pool path and `model.rs`'s comment | **re-scoped per the lead's decision**: `MB.3.5.1` keeps only `workers: Option<u32>` (and the grammar/`FastCgiPoolSocketInvalid`); the resolution helper, `MAX_POOL_WORKERS`, `check_fastcgi_workers` and `FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded` belong to the **pool leaf, gated on `MB.12`**, which also fixes `model.rs`'s comment. §3 `R4` says the key and its `1..=64` grammar bound are this node's while the resolution/ceiling are the pool mechanism's and may be retired by an embedded SAPI; §4.1, §7.2, §8 (Lands + type matrix) and §8.2 all say so | §3 `R4`, §4.1, §4.8.1, §7.2, §8, §8.1 item 4, §8.2 |

The earlier RESULT wording that said "§7.3 defers this" was true only after fix (b); it is now
accurate as stated, and the §7.3 sentence is the evidence.

## Attempt 3 (consistency sweep, 2026-11-14)

The review's remaining finding was one assignment drifting into three places: every oracle of
the **deferred budget resolution** belongs to the **pool leaf, gated on `MB.12`** —
`MB.3.5.1` keeps the grammar plus `FastCgiPoolSocketInvalid`, and `MB.3.5.2` keeps the
grammar/parse-bound rows only, so its `critical_fastcgi_manifest` suite compiles before the
pool leaf lands. Mechanical sweep, no prose rewritten:

1. §7.2 (`R1`–`R6`): `absent_workers_under_a_512_budget_is_boot_fatal` is no longer "in
   `MB.3.5.1`'s model tests" — it is an oracle of the deferred resolution, re-homed to the
   **pool leaf, gated on `MB.12`**.
2. §8.2 (`MB.3.5` cell): the "add `Manifest::check_fastcgi_workers(budget)` +
   `FastCgiWorkersOverBudget`" clause is **deleted**; the cell keeps only
   `FastCgiPoolSocketInvalid` for this epic and states that the pool leaf adds the function and
   both variants.
3. §8 (`MB.3.5.2` Lands) and §7.2: `workers_at_budget_is_accepted` and
   `workers_over_budget_is_boot_fatal` moved to the **pool leaf's** lands, together with
   `resolved_workers_table` and the 512 oracle; `MB.3.5.2` keeps only
   `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`,
   `listen_is_refused_by_name`, `planted_symlink_in_the_run_dir_is_refused`,
   `group_or_world_writable_segment_is_refused`, `regular_file_at_a_segment_is_refused`,
   `probe_timeout_is_boot_fatal`.

**Proof of completeness.** The reviewer's grep, every hit of which now names the pool leaf /
`MB.12` (or is design-rule/call-site prose with no implementer named; the only two `MB.3.5.x`
mentions say "**not** this leaf's" and "does not exist in this crate until the pool leaf lands
it"), truncated to 200 columns per line:

```console
$ grep -nE 'check_fastcgi_workers|MAX_POOL_WORKERS|resolved_workers|FastCgiWorkersOverBudget|FastCgiWorkersCapExceeded|absent_workers_under_a_512_budget|workers_over_budget_is_boot_fatal|workers_at_budget_is_accepted' docs/design/php-fastcgi.md | cut -c1-200
111:and `manifest.check_fastcgi_workers(config.view_worker_cap())` (the **pool leaf's** function, gated
254:  (`pub const MAX_POOL_WORKERS: u32 = 64` — pool leaf, gated on `MB.12`). The ceiling is not
257:  size `RLIMIT_NPROC`/`RLIMIT_NOFILE` from the same number. `Manifest::check_fastcgi_workers`
258:  therefore refuses `policy.resolved_workers(cap) > MAX_POOL_WORKERS` with
259:  `FastCgiWorkersCapExceeded` (naming the resolved value, the ceiling, and the budget
390:| `workers` | integer | **derived** = `Config::view_worker_cap()` (`BLASTIT_VIEW_MAX_WORKERS`, else `BLASTIT_MAX_WORKERS`, default `16`) | the route's per-view cap (**not** a divided share): `pm.m
411:| `FastCgiWorkersCapExceeded { location, workers, limit }` *(pool leaf, gated on `MB.12`)* | `route "app" (index 0): fastcgi pool size 512 exceeds the FastCGI pool ceiling 64 (pm = static pre-fork
412:| `FastCgiWorkersOverBudget { location, workers, budget }` *(pool leaf, gated on `MB.12`)* | `route "app" (index 0): fastcgi workers 32 exceeds this deployment's per-view worker budget 16 (BLASTIT
421:cannot carry a user value. `Manifest::check_fastcgi_workers` (the **pool leaf's**, gated on `MB.12`) evaluates its three arms in one
423:(`FastCgiWorkersOverBudget`), derived ceiling (`FastCgiWorkersCapExceeded`) — so
426:its own). `FastCgiWorkersOverBudget` is **not** a `Manifest::validate` error: `validate`
429:`Manifest::check_fastcgi_workers(&self, budget: u32) -> Result<(), ManifestErrors>`, called
519:    /// `FastCgiWorkersOverBudget`; and `resolved_workers(budget) > MAX_POOL_WORKERS` is
520:    /// `FastCgiWorkersCapExceeded` — so **omitting** `workers` is not automatically fine (it is
523:    pub fn check_fastcgi_workers(&self, budget: u32) -> Result<(), ManifestErrors>;
726:| `RouteState` | `pub struct RouteState { ready: AtomicBool, inflight: AtomicU32, worker_cap: u32 }` (`new(ready, worker_cap)`, `capacity(&self) -> Capacity`, `set_ready(bool)`, **`try_acquire(sel
1221:`workers` may lose its meaning entirely. `Manifest::check_fastcgi_workers(cap)` runs at boot
1330:`RouteState::new(false, policy.resolved_workers(cfg.view_worker_cap()))` after `MB.3.9.4`
1343:| `workers` vs `pm.max_children` | equal, both the **resolved** value (`policy.resolved_workers(view_worker_cap())`, resolved by the **pool leaf**, gated on `MB.12`), never `Option`-dependent; `R
1948:| `R1`–`R6` | `blastit-manifest` | `critical_fastcgi_manifest.rs` (`connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `listen_is_refused_by_name`, `planted_symlin
1953:| `R11` pool file | `supervise::render_pool_conf` (pure) + `critical_fastcgi_pool.rs` | `critical_fastcgi_pool.rs` | *(+)* every directive, `pool_conf_is_fully_rendered` (every directive), `pm.ma
1957:| `R2`, `R3`, `R20` production | manifest probe + boot + `supervise::spawn_pool` + `e2e/rootless/run.py` | `critical_fastcgi_pool.rs`, `critical_fastcgi_parent_fd.rs` + `e2e/rootless/run.py` (sin
2046:| `MB.3.5.1` | `crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs`, `crates/blastit-manifest/tests/docs_fences.rs` (6) | `FastCgiPolicy` (`#[derive(Debug, Clone, PartialEq, Eq)]`) a
2047:| `MB.3.5.2` | `crates/blastit-manifest/src/{probe,lib}.rs` (`.1` first appends the two `pub use model::{…}` names; `.2` then appends `pub mod probe;`), `crates/blastit/src/{probe,lib,supervise
2058:| `MB.3.6.2.1` | `crates/blastit-scheduler/src/{upstream/fastcgi,upstream/fastcgi_builder,upstream/mod,scheduler}.rs` (4) | `FastCgiUpstream` session pump: builds `FastCgiPaths`, maps `LockdownEr
2060:| `MB.3.6.3.1` | `crates/blastit/src/{stage0,stage1,config,cli,dev_learn}.rs`, `crates/blastit/tests/critical_fastcgi_parent_fd.rs` (6) | `B1`–`B3` (stage 0; the derived-parent boot diagnostic 
2087:| `resolved(cap)`, `MAX_POOL_WORKERS`, `Manifest::check_fastcgi_workers` | `blastit-manifest` (added by the pool leaf) | **pool leaf, gated on `MB.12`** | `MB.3.6.3.1`, `MB.3.6.3.2`, `MB.3.6.4.2`
2195:     this leaf's:** `resolved_workers`, `MAX_POOL_WORKERS`, `check_fastcgi_workers`,
2196:     `FastCgiWorkersOverBudget`/`FastCgiWorkersCapExceeded` — they belong to the pool leaf
2202:     `MAX_POOL_WORKERS`, `Manifest::check_fastcgi_workers` and its three ordered arms, the
2203:     two worker-budget variants, and their oracles `workers_at_budget_is_accepted`,
2204:     `workers_over_budget_is_boot_fatal` and `absent_workers_under_a_512_budget_is_boot_fatal`
2205:     (plus `resolved_workers_table`). `workers` keeps its grammar meaning regardless; under
2213:   - `MB.3.6.3.1`: `stage0` calls `check_fastcgi_workers(config.view_worker_cap())`
2223:     `check_fastcgi_workers`, then phase 1
2256:| `MB.3.5` (`.1`/`.2`) — **amended 2026-11-14, re-scoped 2026-11-14 (attempt 2)** | a `listen` field on `FastCgiPolicy`, a `listen` arm plus `!has_listen` `MissingKey` in `parse::fastcgi_policy
2260:| `MB.3.6.3`/`MB.3.6.4` — **amended 2026-11-14** | `B1`/`B2` resolve a manifest parent from `manifest_dirfd`; `render_pool_conf` takes a manifest `listen`; `RouteState::new(…, workers)`; `che
2579:| `testability` MINOR-T8 | MINOR | **Accepted → §7.2 (`R1`–`R6`).** Named fns: `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `probe_timeout_is_boot_fatal`
2657:| 1 | `security` | FAIL | S-1 omission bypasses the `1..=64` ceiling (`pm = static` pre-forks), S-2 pin/freeze not re-entrant over a preserved `<runtime>`, S-3 a planted non-empty directory wedge
2664:| 3 | `implementability` | FAIL | R3-1 decision 6's `0555` + a self-contradicting `U1` sentence; R3-2 the retired "unreachable" e2e leg name and a duplicated `verify_private_dir`; R3-4 `check_fas
$ echo "hits=$(... | wc -l) attributed-to-MB.3.5.1-or-.2=0"
hits=40 attributed-to-MB.3.5.1-or-.2=0
```

`grep -c` over the same pattern with a `MB.3.5.1|MB.3.5.2` filter returns only the two negative
statements, so no oracle names `MB.3.5.1` or `MB.3.5.2` as its implementer. The design is
**2686 lines**; `ADR-0008` is **409**.

Gates on the settled tree: `zensical build --strict` → `No issues found`; `plan.py validate` →
`plan OK`. One `make check` run tripped the runnable-docs **install** fence in
`docs/frameworks/static-and-spa.md` with `a wheel cached by an earlier run was installed, not the
one this run built` — a harness cache artefact from concurrent runs, in a page this node does not
touch; `make docs-test` was re-run alone and passed (below), and `make check` was re-run on the same
tree (result below).

```console
$ make docs-test
...
56 fences: 29 executed, 27 accounted, 0 failed · 45.4s
EXIT=0

$ make check                     # full gate on the settled tree
56 fences: 29 executed, 27 accounted, 0 failed · 21.9s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c 'test result: FAILED'` = `0`; `grep -c '^FAIL '` over the fence report = `0`.

## Attempt 4 (owner correction and two wording fixes, 2026-11-14)

The reviewer's attempt-3 FAIL was one real plan error plus two wording items. Three edits, no
restructure:

1. **[MAJOR] `FastCgiPoolSocketInvalid`'s owner is `MB.3.5.2`.** It was attributed to
   `MB.3.5.1`, which is done and does not carry it (`crates/blastit-manifest/src/error.rs`
   has only the four grammar variants), while §7.2's three `dev` refusal legs need it inside
   `MB.3.5.2`'s protected suite. Moved in all three places (§8 Lands, §8.1 item 4, §8.2), and
   `crates/blastit-manifest/src/error.rs` is now in `MB.3.5.2`'s Files cell (7 → 8; the lead
   added the path to the node itself). `MB.3.5.1` is recorded as having landed **grammar
   only**: `entry`, `workers: Option<u32>`, `FastCgiListenDerived`, `FastCgiDeadDeclaration`
   (and the `1..=64` validation for an explicit value).
2. **[MINOR] the wording that said `check_fastcgi_workers` "does not exist".** The **budget-aware**
   `Manifest::check_fastcgi_workers(budget)` is what is missing; the private grammar-only
   `validate::check_fastcgi_workers(&location, policy, errors)` exists at `validate.rs:597`.
   §8's `MB.3.5.2` cell says so.
3. **[MINOR] the critic cap, recorded in §9** ("the critic cap, stated so the record cannot be
   misread"): round 3 ended FAIL on three lenses and PASS on one, the `3b` re-verify found one
   MAJOR, and every fix — through attempts 3 and 4 — was verified by the reviewer's next read,
   **not** by a fourth critic round. The artifacts on disk are therefore not a clean
   critics-PASS, and the record says so.

**Proof.**

```console
$ grep -n 'FastCgiPoolSocketInvalid' docs/design/php-fastcgi.md | cut -c1-180
413:| `FastCgiPoolSocketInvalid { location, path, rule }` *(implemented by `MB.3.5.2`)* | `route "app" (index 0): pool socket "<run>/fastcgi/0/fpm.sock": is not a socket` (the path
432:budget's source. `FastCgiPoolSocketInvalid` (`MB.3.5.2`'s) names the **derived** path, never a manifest
530:    /// `FastCgiPoolSocketInvalid { location, path: <run_root>/fastcgi/<i>/fpm.sock, rule }`
617:| `SocketProbeError` (derived-parent path) | `FastCgiPoolSocketInvalid.rule` |
1201:     (`FastCgiPoolSocketInvalid`) — and returning the pinned `parentfd`;
1957:| `R2`, `R3`, `R20` production | manifest probe + boot + `supervise::spawn_pool` + `e2e/rootless/run.py` | `critical_fastcgi_pool.rs`, `critical_fastcgi_parent_fd.rs` + `e2e/r
2046:| `MB.3.5.1` | `crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs`, `crates/blastit-manifest/tests/docs_fences.rs` (6) | `FastCgiPolicy` (`#[derive(Debug, Clone,
2047:| `MB.3.5.2` | `crates/blastit-manifest/src/{probe,lib}.rs` (`.1` first appends the two `pub use model::{…}` names; `.2` then appends `pub mod probe;`), `crates/blastit/src/
2209:   - `MB.3.5.2`: `ManifestError::FastCgiPoolSocketInvalid { location, path, rule }` in
2258:| `MB.3.5` (`.1`/`.2`) — **amended 2026-11-14, re-scoped 2026-11-14 (attempt 2)** | a `listen` field on `FastCgiPolicy`, a `listen` arm plus `!has_listen` `MissingKey` in `p
2679:`FastCgiPoolSocketInvalid` owner, the `validate::check_fastcgi_workers` wording, this note) are

$ grep -n 'error.rs' docs/design/php-fastcgi.md | cut -c1-180
898:`crates/blastit-scheduler/src/error.rs` (both files owned by `MB.9.2.1.2`). `SchedulerError::Boot`
2047:| `MB.3.5.2` | `crates/blastit-manifest/src/{probe,lib}.rs` (`.1` first appends the two `pub use model::{…}` names; `.2` then appends `pub mod probe;`), `crates/blastit/src/
2051:| `MB.9.2.1.2` | `crates/blastit-scheduler/{src/{upstream/mod,scheduler,error,lib}.rs,tests/head_allocs.rs}` (5) | `UpstreamHead` (`from_utf8_lossy`), `RouteRuntime.stages`, `
2210:     `crates/blastit-manifest/src/error.rs` (added to its files; the three `dev` refusal legs
2251:| `MB.9.2.1` (`.1`/`.2`/`.3`) | 11 files; first cmd `--locked`; `stage1.rs` vs `roles.rs` root; no `SessionDoneGuard` or proxy-control strip; no `mod upstream;` site | split `
2258:| `MB.3.5` (`.1`/`.2`) — **amended 2026-11-14, re-scoped 2026-11-14 (attempt 2)** | a `listen` field on `FastCgiPolicy`, a `listen` arm plus `!has_listen` `MissingKey` in `p
2372:| `implementability` M2 (`run_with` vs `run`) | MAJOR | **Accepted → §4.2 (new `run_with` paragraph), §8 (`MB.9.2.1.2`).** `run_with(...) -> Result<(), RunError>` with `Ru
```

Every `FastCgiPoolSocketInvalid` hit that has an owner names `MB.3.5.2` (the definition cells
and §7.2's behaviour cell name no leaf, correctly); **no hit names `MB.3.5.1`**. Of the
`error.rs` hits, `crates/blastit-manifest/src/error.rs` is `MB.3.5.2`'s (two places), while
`crates/blastit-scheduler/src/error.rs` belongs to `MB.9.2.1.2` and one §9 history row names the
attempt-4 edit itself — neither is the variant's owner.

Gates on the attempt-4 tree:

```console
$ uvx --offline zensical build --strict
No issues found

$ uv run --no-project tools/plan.py validate
plan OK

$ make check
56 fences: 29 executed, 27 accounted, 0 failed · 109.9s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c 'test result: FAILED'` = `0`; `grep -c '^FAIL '` over the fence report = `0`. The design
is **2698 lines**; `ADR-0008` is **409**.

## Out-of-scope observations

- `ADR-0008`'s frontmatter is `Status: Proposed`, while the node text calls it accepted. I did not
  change the status line; the ADR now says "this amendment takes effect with the same acceptance".
  If the lead wants the ADR flipped to `Accepted`, that is a one-line edit outside this node's
  intent.
- The in-flight `blastit-manifest` (`MB.3.5.1`) model doc comment still says "the derivation is the
  scheduler's"; `resolved_workers` belongs in `blastit-manifest` because both init and the
  scheduler consume it. §8.2 tells that leaf; the comment itself is the leaf's to fix.
