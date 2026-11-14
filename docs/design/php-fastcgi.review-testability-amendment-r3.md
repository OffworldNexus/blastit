# `php-fastcgi` amendment review (round 3) — `testability` lens

- **Lens:** `testability` (verification and oracles).
- **Scope:** a focused re-check of the six clusters in the 2026-11-14 `MB.3.9.2` amendment,
  as requested: (1) `T17`/`T18` listen variant, (2) `T19` mode story, (3) `T20` capacity
  twin, (4) `T21`/`T26` `check_fastcgi_workers` + `pool_rlimits`, (5) `T22`–`T25`
  per-rule oracles, (6) `T27` protected-file inventory. Round-2 IDs are from
  `php-fastcgi.review-testability-amendment-r2.md`.
- **Snapshot:** `docs/design/php-fastcgi.md` md5 `6a03ad4befab0f2eaefe797044a74297`,
  **2627 lines**; `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md` md5
  `326839cdf606f58d12f33f261c0cf2fc`, 402 lines. Every line number below is that snapshot.
  The design was **still being edited during this pass** (an earlier read was md5
  `7817e25a…`, 2584 lines), so re-resolve numbers if the hash moved.
- **Round-2/3 line of record:** the document's own decision log claims
  `T19–T27 fixed (… remove_entry/B2/check_leaf oracles, one inventory)` (`:2608`) and that
  `I-8 … the e2e oracle named the pre-fix invariant` was fixed (`:2606`). Two of those
  claims are not true on disk (R3-1, R3-2); one is only partially true (R3-3).

## VERDICT: **FAIL**

0 `BLOCKER`, 2 `MAJOR`, 3 `MINOR`.

The listen half of the amendment is really closed now, and the capacity, 512-budget,
`remove_entry`, `B2` and inventory fixes landed. But the mode story still has two numeric
specs in the normative ADR and no named oracle, and `check_leaf`'s two exit codes still
have **no** named test and **no** owner that can run one. A testability PASS is not
possible while a protected mode assertion and a user-visible exit code are unowned.

## Findings

| ID | Grade | Where (file:line) | What | Fix |
|---|---|---|---|---|
| **R3-1** | **MAJOR** | `ADR-0008:192,196,220,225,226,228` vs `:223,285-294`; `php-fastcgi.md:1406-1419`; `:1916`; `:2529`; `:2612-2615` | **`T19` not closed — two mode stories.** The ADR's normative decision 6 still says `creating only an absent final leaf 0555` (`:192`), `freezes parentfd init_uid:init_gid 0555` (`:196`), `restore owner and mode init_uid:init_gid 0555` (`:220`), and the U1 freeze/unlink prose repeats `0555` (`:225,226,228`). Only one parenthetical on the U1 line says `(**amended**: 0511)` (`:223`). The amendment block (`:285-294`) and §4.8.3 (`:1406-1410`) pin the real trio: `<runtime>` `0755`, `<runtime>/fastcgi` `0711`, `<i>` `0511`, dev/check `0700`. An implementer of decision 6 freezes `0555` — listable by every route uid, so "route count/layout is not public" is lost, and R11's protected assertion fails. The §9 row `testability MINOR-T12` also still says `fchownat`/`0555`/`0600` (`:2529`) and is **not** annotated "superseded" although the amendment note promises that annotation (`:2612-2615`). **And the trio has no named oracle:** R11 says only "mode constants `0600`/`0511`/`0711`" (`:1916`, an unnambed clause); grep finds no `pool_parent_modes_*` test anywhere. | Rewrite the five remaining `0555` occurrences to `init_uid:init_gid 0511` (and the create step to the `0711` container + `0700 <i>`), annotate the §9 row, and name one protected test — `pool_parent_modes_are_0755_0711_0511_and_dev_0700` (or the suggested `pool_parent_modes_are_0600_0511_0711`) — in R11 and `MB.3.6.3.3`'s `Lands`: unprivileged dev `0700`, `#[ignore] requires-root` production `0755` non-route-writable / `0711` / `0511`, plus the socket `0600`. One test, one owner. |
| **R3-2** | **MAJOR** | `php-fastcgi.md:339-342` (R20), `:1192-1197` (phase 2), `:1919` (R17/R18), `:1920` (R2/R3/R20), `:2024` (`MB.3.6.4.2`), `plan/nodes/MB/MB.3/MB.3.9/MB.3.9.3/NODE.md` (paths: docs only), `MB.3.9.5/NODE.md` (five modes), `:2608` | **`T23` not closed — `check_leaf`'s per-mode exit codes still have no oracle and no runnable owner.** R20 now states both codes: dev `exit 1` and `check`'s "early child exit → `exit 2`" (`:340-342`). §7.2 R2/R3/R20 says only `dev` with missing/foreign-uid socket exits `1` — unnamed — and has **no** `check`-side leaf assertion (`:1920`). R17/R18 named the other `check` codes (`2` for empty `--`, `3` for a non-zero suite) but not the leaf failure (`:1919`). `MB.3.6.4.2`'s `Lands` names `critical_fastcgi_learn.rs` but no such test (`:2024`); `MB.3.9.5`'s five modes are missing binary, immediate exit, two pools, SIGTERM, grammar — none is this; `MB.3.9.3` is a **docs-only** leaf (`paths: docs/...`), so it cannot host a runnable test despite owning the ordering. A regression that maps a leaf failure to `1` in `check` (or to `3`) passes every named test and breaks the documented exit contract. The decision log's "check_leaf oracles" claim (`:2608`) is false. | Name two tests and give them a leaf that can build them: `leaf_missing_after_spawn_exits_1_in_dev` and `check_child_leaf_failure_exits_2_naming_the_derived_socket`. Owner **`MB.3.9.5`** (`critical_fastcgi_dev_pool.rs`, `depends_on: [MB.3.9.4]`), not `MB.3.6.4.2` — before `MB.3.9.4` `dev` does not start a pool, so neither leg can run earlier. Add both to that brief's mode list and to R2/R3/R20's cell with the two exact codes; drive `check` with a fake `BLASTIT_PHP_FPM` that exits before binding and assert the message names `<CheckRoot>/dev/fastcgi/<i>/fpm.sock`. If only the `check` leg is wanted in `MB.3.6.4.2`, say so and keep the dev leg in `MB.3.9.5`. |
| **R3-3** | MINOR | `php-fastcgi.md:2025` vs `:1898,1920,2175`; also `:2024`, `:2021`, `:2020` | **Dispatch cells still lag the oracle map.** `MB.3.7`'s `Lands` names `e2e/rootless/run.py::check_fastcgi_pool_dir_is_unreachable_for_the_route_uid` (`:2025`) — a test name that exists nowhere else. §7.1, §7.2 and §8.1 all name `check_fastcgi_pool_dir_is_traversable_but_not_writable_for_the_route_uid` with the exact three-way predicate (`stat`/`O_PATH` succeeds; `readdir` `EACCES`; creates/renames fail), which is what the `0511` mode story requires. The same cell also omits `check_fastcgi_production_workers_over_budget_exits_1` (T21), `check_fastcgi_second_boot_normalises_a_frozen_parent` (T25) and `check_fastcgi_b2_foreign_owner_segment_refused` (T22). `MB.3.6.4.2` omits the three worker/512 tests; `MB.3.6.3.2` omits `pool_rlimits` and `fake_pool_child_has_the_resolved_rlimits`; `MB.3.6.3.1` omits `planted_entry_at_the_leaf_is_removed`. | Sync each `Lands` to the names §7.2/§8.1 already assign: fix the traversable name and add the three e2e names to `MB.3.7`; add the three worker/512 names to `MB.3.6.4.2`; add `pool_rlimits` + the getrlimit test to `MB.3.6.3.2`; add `planted_entry_at_the_leaf_is_removed` to `MB.3.6.3.1`. §8.1 is otherwise the correcting authority, which is why this is MINOR rather than MAJOR. |
| **R3-4** | MINOR | `php-fastcgi.md:1911` vs `:2007`, `:2159-2160`; R9–R20 row `:1915` | **The capacity twin is expressible, but it is filed in a suite that cannot host it.** R1–R6's row (Suite = `critical_fastcgi_manifest.rs`, i.e. `blastit-manifest`) now says "`RouteState::try_acquire` yields exactly … permits and then `None` … (`capacity_is_the_resolved_budget_when_workers_are_omitted`)" (`:1911`). `RouteState`/`try_acquire` are `blastit-backend` items; `blastit-manifest` has no `blastit-backend` dev-dependency, and `MB.3.5.2`'s own `Lands` explicitly disclaims this test as "`MB.3.6.2.1` units, never rows here" (`:2007`). §8.1 agrees with `MB.3.5.2` (`:2159-2160`). Same class as round-2 T34. | Move that sentence/name out of R1–R6 into the R9–R20 row (`:1915`), which already repeats the `try_acquire` assertion, and leave `MB.3.6.2.1` as the owner. Do not list it under `critical_fastcgi_manifest.rs`. |
| **R3-5** | MINOR | `php-fastcgi.md:1454-1475` (seam) vs `:1916` (R11) | **The `getrlimit` oracle is pinned to a harness behaviour the harness contract does not declare.** R11 says `fake_pool_child_has_the_resolved_rlimits` uses "the **unprivileged `PoolHarness`'s forked fake pool child**, which the harness forks through `ChildIo { limits: pool_rlimits(resolved), .. }`, reads `getrlimit(RLIMIT_NPROC)…` and reports on a pipe" (`:1916`). The authoritative seam only says `spawn()` runs "S1, S2, S4–S7" against "a forked fake child in a new pgid" (`:1464,1470-1472`) — it never says `ChildIo.limits: pool_rlimits(workers)` is applied or that a report pipe exists. Since `pool_rlimits` is also `MB.3.6.3.2`'s, this is one sentence, not a redesign — but a test writer following §4.8.3 alone forks a child with no limits and the test cannot pass. | Add to the seam: the fake pool child is forked through the S3 `ChildIo` literal with `limits: pool_rlimits(workers)`, and reports `getrlimit(RLIMIT_NPROC).rlim_cur` / `RLIMIT_NOFILE` on a pipe. Then `S3`'s "`requires-root`" note is only about the real `php-fpm` exec, not the limits observation. |

## Verified (requested items 1–6)

| # | Item | Result | Evidence on disk |
|---|---|---|---|
| 1 | `T17`/`T18` — `FastCgiListenDerived`, no site says `UnknownKey` for `listen` | **PASS** | `MB.3.5.1`: "refusing `listen` with the landed `FastCgiListenDerived` (no value; exact message as §4.1)" (`:2006`); §8.1 item 4: "keeps the landed `FastCgiListenDerived` arm (it carries no value) and loses `!has_listen` `MissingKey`" (`:2156`); §4.1 key row + error row with the verbatim message (`:385,409`); ADR `FastCgiListenDerived` + same message + hint rationale (`ADR:298-302`). `grep -i unknownkey` matches **nothing** in either file (the only "unknown key" strings are the message text itself). |
| 2 | `T19` — one mode story + one named oracle | **FAIL** | Design/§4.8.3 and the ADR amendment agree on `0755`/`0711`/`0511`, dev `0700` (`:1406-1419`, `ADR:285-294`). But ADR decision 6 still pins `0555` five times (`ADR:192,196,220,225-228`), the §9 row still pins `0555` (`:2529`), and no test is named for the trio (`:1916`). → **R3-1**. |
| 3 | `T20` — capacity twin expressible via `try_acquire` | **PASS** | "`RouteState::try_acquire` yields exactly `policy.resolved_workers(view_worker_cap())` permits and then `None`, for `None` **and** `Some(2)` (`capacity_is_the_resolved_budget_when_workers_are_omitted`; `Capacity` is `Ready`/`Busy` and carries no number, so the oracle is the permit count, not `capacity()`)" (`:1911`). No numeric `capacity()` assertion remains. Placement defect only → **R3-4**. |
| 4 | `T21`/`T26` — `check_fastcgi_workers` at all three sites + `pool_rlimits`, each named with a runnable owner | **PASS** | Sites: production + `dev` in `MB.3.6.3.1` (`:2020`, `:2163`), `check::execute` in `MB.3.6.4.2` (`:2170`). Oracles named: `critical_fastcgi_learn::dev_exits_1_when_workers_exceed_budget`, `check_exits_2_when_workers_exceed_budget` (`:1911`, `MB.3.6.4.2`), and `e2e/rootless/run.py::check_fastcgi_production_workers_over_budget_exits_1` (`requires-root`, `MB.3.7` paths). The 512 case is **split** correctly: pure `absent_workers_under_a_512_budget_is_boot_fatal` in the `blastit-manifest` suite (`MB.3.5.1`) plus `dev_exits_1_under_a_512_budget` in `blastit` (`MB.3.6.4.2`) — no `blastit` spawn inside a manifest test. `pool_rlimits` has `pool_rlimits_table` (`2`/`8`/`64` + formula-only `300`) and `fake_pool_child_has_the_resolved_rlimits`, owned by `critical_fastcgi_pool.rs` (`MB.3.6.3.3`) over the `MB.3.6.3.2` harness (`:1916`). Residual: harness `ChildIo.limits` unstated → **R3-5**; `Lands` names missing → **R3-3**. |
| 5 | `T22`–`T25` — `InsecureMode`, `regular_file_at_a_segment_is_refused`, `check_leaf` per-mode codes, `remove_entry`, `B2` re-entrancy | **FAIL** | `InsecureMode { mode }` has one rule (`is not 0700`, `:613`) and one named oracle `group_or_world_writable_segment_is_refused` (`0755`/`0777` → exactly that variant, `:1911,1920`). `regular_file_at_a_segment_is_refused` has one name (`NotDirectory`, `:1911,1920,2007`). `remove_entry` has exactly one named oracle, `planted_entry_at_the_leaf_is_removed`, now covering socket / symlink / regular file / empty dir / **non-empty dir (rename-aside)** / `ENOENT` (`:1421-1432`). `B2` re-entrancy has one named oracle, `e2e/rootless/run.py::check_fastcgi_second_boot_normalises_a_frozen_parent` (`:1920`). **But `check_leaf`'s per-mode exit codes have none** (unnamed dev leg at `:1920`, no `check` leg at all) → **R3-2**. |
| 6 | `T27` — one inventory for `critical_fastcgi_manifest.rs` | **PASS** | The same **nine** names appear in §7.2 (`:1911`), `MB.3.5.2`'s `Lands` (`:2007`) and §8.2's amended row ("exactly §7.2's … inventory (9 names, quoted there)", `:2203`): `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`, `listen_is_refused_by_name`, `workers_at_budget_is_accepted`, `workers_over_budget_is_boot_fatal`, `planted_symlink_in_the_run_dir_is_refused`, `group_or_world_writable_segment_is_refused`, `regular_file_at_a_segment_is_refused`, `probe_timeout_is_boot_fatal`. `resolved_workers_table`, the 512 unit and the capacity unit are explicitly disclaimed as units of `MB.3.5.1`/`MB.3.6.2.1` in the same cell; no count disagrees. |

## Round-2 ID disposition (T17–T27 only)

| Round-2 ID | Status | Residual |
|---|---|---|
| T17, T18 | **CLOSED** | variant + verbatim message in `MB.3.5.1`, §8.1 and the ADR; no `UnknownKey` for `listen` |
| T19 | **NOT CLOSED** | → **R3-1** (ADR body `0555`, no named mode oracle) |
| T20 | **CLOSED** (placement residual) | `try_acquire` permit count; → **R3-4** |
| T21 | **CLOSED** (Lands residual) | three sites named; 512 split; → **R3-3** |
| T22 | **CLOSED** | one `InsecureMode` rule + one name; regular-file named; B2 foreign-owner named in e2e |
| T23 | **NOT CLOSED** | → **R3-2** |
| T24 | **CLOSED** | non-empty-dir rename-aside + one named unit |
| T25 | **CLOSED** | `check_fastcgi_second_boot_normalises_a_frozen_parent` |
| T26 | **CLOSED** (harness residual) | `pool_rlimits_table` + fake-child getrlimit; → **R3-5** |
| T27 | **CLOSED** | one 9-name inventory in §7.2 / `MB.3.5.2` / §8.2 |

## What holds

- **The listen half is genuinely finished**: one variant, one verbatim string, "carries no
  value", and the hint rationale, in the design, the checklist and the ADR. No site tells a
  cheap implementer to restore `UnknownKey`.
- **The capacity twin now fails when it should**: counting permits through `try_acquire`
  distinguishes `None -> cap`, `Some(1) -> 1`, `Some(cap) -> cap`, and a `Busy` route.
- **The 512 case no longer asks a `blastit-manifest` test to spawn `blastit`**: the pure
  `check_fastcgi_workers(512)` unit and the `dev` boot leg are separate and correctly homed.
- **`remove_entry`'s hostile-leftover matrix is complete** (socket, symlink, file, empty
  dir, non-empty dir, `ENOENT`), with the rename-aside bounded at four stale entries, and
  one unprivileged named unit.
- **The new protected file has one inventory** and no leaf is told to write a test another
  leaf owns.
- Nothing new needs root or a real `php-fpm` in the fast tier; the root/capability legs stay
  in `#[ignore]`/`e2e/rootless`.

## Re-run protocol

Re-run this lens after **R3-1** and **R3-2** land (they add a protected mode assertion and
a protected exit-code test). `consistency` should re-run with R3-1, because the `0511`/`0555`
choice is operator-visible in `ARCHITECTURE.md` §2.2 and `docs/security.md`; `implementability`
should re-run with R3-3/R3-4/R3-5 (dispatch cells and harness seam). If R3-2 assigns the
dev leg to `MB.3.9.5`, `conductor`/`planner` must add the two names to that node's brief
before dispatch — the leaf already exists (`status: ready`), so this is an `edit`, not a `new`.
