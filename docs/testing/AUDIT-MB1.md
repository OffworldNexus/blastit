# AUDIT MB.1 — the tests the MA-gate follow-up epic added

- **Date:** 2026-11-13
- **Scope:** `MB.1` (`MB.1.1`–`MB.1.10`) on `release/0.2.0`, tip `51dced9`, base `cab377e`
  (`Release v0.1.1`) — the new/changed test files only:
  `crates/blastit-manifest/tests/{mutation_gaps,edit_branches}.rs`, the inline `#[cfg(test)]`
  additions in `blastit-manifest/src/model.rs` and `blastit-sqlpolicy/src/{lru,statements,tables}.rs`,
  `crates/blastit-sqlpolicy/tests/{lexer_progress,policy_module_edges,policy_module_tables}.rs`,
  `crates/blastit/tests/audit_group_hint.rs`, `crates/blastit-scheduler/tests/head_allocs.rs`,
  the protected additions in `crates/blastit/tests/critical_config_and_presets.rs` and
  `python/tests/critical/test_check_contract.py`, and the `e2e/` additions.
- **Requirements:** `plan/nodes/MB/MB.1/NODE.md` # Acceptance; every child `NODE.md`/`RESULT.md`;
  `docs/design/ma-lean-dx.md` §7.1–§7.2 (`R1..R3`, `R7..R8`, `R12`, `R24..R25`, `R27..R38`,
  `R39..R42`); `plan/nodes/MA/RESULT.md` §2 (baseline mutation numbers); skill `testing` §1–§4,
  skill `security-invariants` §1/§3/§5.
- **Sibling report:** `docs/testing/AUDIT-MB.1.md` (untracked, written concurrently at 18:42 by the
  other curator session) covers the same diff. I re-derived every number below from the artefacts
  myself and reached the same verdict; its `F1`–`F10` map to my `G1`–`G7`. Where I differ (the
  `Protocol::ALL` "kill", the lexer survivor locations) the correction is called out.

## Verdict

**PASS on correctness and on the mutation bar; the protected tier does not yet own the three new
policy oracles.** Four of the five epic acceptance bullets are met on the evidence each leaf recorded;
the fifth ("`make check` and `make e2e` green") needs a **fresh run at the tip** (`G7`), because the
last e2e predates the sqlpolicy parser rewrite. What remains is
classification (security-shaped oracles in the unprotected standard tier) and five small
evidence/regression gaps — **no BLOCKER, no fluff that must be deleted, no broken oracle.**

- `blastit-manifest`: **550/564 viable = 97.5 %** killed, **0 timeouts** (baseline 69.4 %, 11 timeouts).
- `blastit-sqlpolicy`: every module ≥ 75 % (85.2 %–100 %), **0 timeouts**; 921 mutants listed, not the
  stale 1019. One whole-module number (`learn.rs`) is inherited from the pre-refactor tree — `G3`.
- Both protected edits are **additions only** (+40/0 and +10/0 lines) and authorized by
  `MB.1.6`/`MB.1.8` (`critical_guard` exit 0). The config suite change is a pure append; the only
  change to existing protected content is the Python race fix — one bounded wait, assertions
  byte-identical.
- Live signal at the tip: `cargo test -p blastit-sqlpolicy -p blastit-manifest --locked` → exit 0,
  sqlpolicy 224 lib + 10 + 25 + 6 + 10 + 18 tests, manifest 36 + 17 + 18 + 3 + 17 + 13 + 17 + 33 + 11
  + 6 + 29, doc tests green.

**Not re-run (deliberately):** no new `cargo-mutants` campaign and no `make e2e`; the retained
`/tmp/opencode/mb1-*` artefacts and `MB.1.8`'s log are the evidence. `make check`/`make e2e` at the
tip are the lead's gate step (`G7`).

## Method

1. Read the epic node, all ten child `NODE.md`/`RESULT.md`, `MB.1/LOG.md`, all six new integration
   test files line by line, and the inline test diffs (`git diff 2317493..HEAD -- crates/**`).
2. Recomputed every ratio from `mutants.out/{caught,missed,timeout,unviable}.txt`:
   `/tmp/opencode/mb1-manifest-edit`, `mb1-review-edit`, `mb1-sql-lexer3`, `mb1-sql-learn`,
   `mb1-sql-normalize`, `mb1-sql-statements`, `/tmp/opencode/mb12-whole.log`.
3. Re-derived the mutant sets on the shipped tree with `cargo mutants --list` (no test run):
   manifest 637, sqlpolicy 921 (`analysis` 299, `learn` 96, `lexer` 206, `lib` 21, `literals` 4,
   `lru` 10, `normalize` 110, `statements` 84, `tables` 91).
4. Ran `critical_guard.py` three ways (epic node, the two authorizing leaves, full-epic range) and
   diffed both protected files (`--numstat`, line diff, skip-construct grep).
5. Ran only the allowed cheap signal: `cargo test -p blastit-sqlpolicy -p blastit-manifest --locked`.

The working tree also held `MB.1.11`/`MB.1.12`/`MB.1.13` in flight (`Makefile`, `scripts/**`,
`critical_policy_surface.rs`, `critical_check_engine.rs`), so every diff and number above is for
commit `51dced9`; the live signal was green against the tree as it stood and is reported as a signal,
not as tip evidence.

## Inventory & classification

| Item | Tests | Class | Where it belongs |
|---|---|---|---|
| `blastit-manifest/tests/mutation_gaps.rs` (MB.1.2) | 17 | **useful / standard** | right: model accessors, derived flags, error rendering; the coverage predicate is fail-closed (INFO, accepted) |
| `blastit-manifest/tests/edit_branches.rs` (MB.1.1) | 17 | **useful / standard** | mostly right; the in-string `[[` state and a *wrong-route grant attach* case are manifest-integrity oracles → `G4` |
| `blastit-manifest/src/edit.rs` inline (MB.1.1) | +25 | **useful / standard** | right (private scanners) |
| `blastit-manifest/src/model.rs` inline (MB.1.2) | +2 | **useful / standard** | right (`version()`, `parameterised`) |
| `blastit-sqlpolicy/tests/lexer_progress.rs` (MB.1.3) | 6 | **useful / standard — mis-tiered** | lexer token/region boundaries + 200 KB hostile input are a parser-differential/DoS surface → `G3` |
| `blastit-sqlpolicy/tests/policy_module_edges.rs` (MB.1.4) | 10 | **critical content / standard — misplaced** | the only oracle for normalize's collapsed shapes → **MB.1.12** |
| `blastit-sqlpolicy/tests/policy_module_tables.rs` (MB.1.5) | 18 | **critical content / standard — misplaced** | table/column grant extents = the grant surface → **MB.1.12** |
| `blastit-sqlpolicy/src/{lru,statements,tables}.rs` inline | +3/+N/+2 | **useful / standard** | right (private `Lru`, parser internals, `Columns` serde) |
| `blastit/tests/audit_group_hint.rs` (MB.1.6) | 4 | **critical content / standard — misplaced** | the only executable `add_group` oracle → **MB.1.13** |
| `blastit/tests/critical_config_and_presets.rs` (MB.1.6) | 12 (was 11) | **critical** | right; +40/0 lines, both orders asserted |
| `blastit-scheduler/tests/head_allocs.rs` (MB.1.6) | 2 (was 1) | **useful / standard** | right: perf claim, exact `+3`-per-request delta, not a security wall |
| `python/tests/critical/test_check_contract.py` (MB.1.8) | 22 (same) | **critical** | right; ordering-only wait |
| `e2e/scenario.py` + `check_logs.py` (MB.1.8) | +2 assertions each | **e2e** | right; commit-time denial + proxy-cause, with a baseline branch |
| `e2e/rootless/run.py::check_negative_boot` (MB.1.8) | 2 | **e2e** | right (local driver; droplet leg absent — INFO) |
| `e2e/{grants.py,demo/blastit.toml}` | fixture | **e2e data** | correct shape; generated == committed is not asserted (INFO, accepted) |

No test in scope is `fluff` that must go, and none is `broken-oracle`: every new policy test asserts
an allowed **and** a denied twin; `head_allocs` asserts an exact delta (not "less than"); the startup
test asserts the inverse order as well; the e2e block has its non-blastit baseline.

## Anti-fluff findings

1. **`mutation_gaps.rs::a_protocol_has_exactly_one_spelling`** — the reviewer's "tolerable because it
   kills a mutant" defence is **valid for the seven `ProxyName` assertions** (`as_str`/`borrow`/
   `as_ref` → `""`/`"xyzzy"`, `From` → `Default`, `Display`; all seven are in
   `cargo mutants --list`) but **not for `assert_eq!(Protocol::ALL, [Protocol::Postgresql])`**:
   `ALL` is a `const`, cargo-mutants does not mutate it, and the line duplicates what
   `Protocol::parse("mysql") == None` already pins. Keep it (it pins the exported build surface) but
   do **not** record it as mutation-justified; no task.
2. **`check_logs.py`** asserts `sqlstate == 42501` and `reason == "the statement is not granted"` on
   *different* `sql.denied` events. Both are the `deny` route's (there is no other), so this is not a
   broken oracle, but one event carrying all three fields would be strictly stronger. INFO.
3. **`head_allocs.rs`**'s `assert_eq!(ingress, plain + 3 * iterations)` is an exact allocation count.
   It is a *delta between two worlds in one build*, warmed up and settled, so it is not
   toolchain-brittle in the way an absolute count is; measured 3/3 stable. Acceptable.
4. **Nothing** in scope would still pass if an allowlist returned `Allow`/`Ok` unconditionally: checked
   each new policy test for its denied half, and `is_placeholder_list -> true` (a real widening)
   **fails** `policy_module_edges.rs` (reproduced by the sibling curator; consistent with its
   allowed/denied pairs).

## Epic acceptance → FILE:TEST

| # | Acceptance claim | Status | Protecting test / evidence |
|---|---|---|---|
| C1 | `blastit-manifest` whole crate ≥ 75 % with 0 cursor-loop timeouts, `verify()` + patcher branches covered | **PASS** | `/tmp/opencode/mb12-whole.log`: 550 caught / 14 missed / **0 timeout** / 73 unviable = **97.5 %**; the run's miss list contains no `verify()` line; `edit.rs` 233/244 = 95.5 % (`mb1-review-edit`) |
| C2 | Every `sqlpolicy` module ≥ 75 %, 0 timeouts | **PASS** | `lib` 18/20 = 90.0 %, `analysis` 232/275 = 84.4 % (unchanged files, MA baseline), `learn` see C3, `lexer` 173/203 = **85.2 %** (`mb1-sql-lexer3`), `normalize` 97/98 = 99.0 %, `literals` 4/4, `lru` 7/7, `statements` 84/84, `tables` 84/84 (`mb1-sql-normalize`/`mb1-sql-statements`); all four `timeout.txt` empty |
| C3 | "All 1019 mutants measured" | **GAP → G3** | 921 is the tree's real set (reconciled: loop refactors remove step mutants). `learn.rs` lists 96 and only the 4 in `parse_form` were re-measured (`mb1-sql-learn` 4/4); the other 92 rest on MA's pre-refactor 90/98. `MB.1.5`'s "every module is now measured" is therefore overstated |
| C4 | Missing `./blastit.toml` named before `BLASTIT_APP`, pinned | **PASS** | `crates/blastit/tests/critical_config_and_presets.rs::an_empty_directory_names_the_missing_manifest_before_the_app` (dev: names `blastit.toml`, no `BLASTIT_APP`, empty stdout, exit 1; inverse: manifest without `app` names `BLASTIT_APP`, no "No such file") |
| C5 | Same ordering at both entry points | **GAP → G5** | production `stage0::run` is proven only by a manual `sudo -n` run (MB.1.6 RESULT leg 3); no automated root test |
| C6 | `build-wheel.sh` rewrites relative README links and restores the README on exit | **GAP → G6** | no permanent test. The publish workflow rewrites the README *before* the build, so `description, as PyPI renders it` (`check-long-description.py`) passes even if `build-wheel.sh`'s own rewrite/trap regresses |
| C7 | `CARGO_NET_OFFLINE=true` wherever maturin inherits it | **GAP → G6** | one-shot `grep` only; `grep -rn CARGO_NET_OFFLINE python/tests tools` → nothing |
| C8 | `blastit-events` drops unused `serde` | **GAP (low value) → G6** | no check; a re-add is silent (not detected by clippy/`--locked`) |
| C9 | `plan_request_body` returns `AllowEmptyOnly` before copying the method/headers | **PASS** | `crates/blastit-scheduler/tests/head_allocs.rs::a_policy_less_head_is_planned_without_copying_its_headers` (exact 3-per-request delta; reversed order fails) |
| C10 | e2e `queries = []` listener denies with `42501` | **PASS** | `e2e/scenario.py` (`body["sqlstate"] == "42501"` + route `deny`) and `e2e/check_logs.py` (`sql.denied` on `deny`, reason `the statement is not granted`, route set includes `deny`); root + rootless runs in MB.1.8 RESULT |
| C11 | e2e `--cap-drop ALL` exits 1 naming the missing capabilities | **PASS** | `e2e/rootless/run.py::check_negative_boot` (exit 1 + the exact uid/capability sentence; starts the shared interpreter) |
| C12 | learn-refusal sync deterministic, no assertion relaxed | **PASS** | protected file +10/0 lines, one bounded wait; 3× `make test-critical` green (50 Py + 19 Rust); no skip construct |
| C13 | `docs/brand/logo.svg` light-mode header contrast resolved | **GAP (partial) → G6** | asset-level `@media (prefers-color-scheme: light)` fixes the OS-light case (15.22:1), but the site's light **toggle** on a dark OS still renders the paper cutout (MB.1.7 follow-up); no test, and `make docs-check` cannot see a malformed SVG (MB.1.7 hit exactly that) |
| C14 | `audit_v1` suggests `add_group` for connection setup | **PASS (tier GAP → G2)** | `crates/blastit/tests/audit_group_hint.rs` (4 tests: `set_config`/`SET`/`SHOW` → group, `SELECT 1`/`DELETE` → `add_query`, `SAVEPOINT`/redacted → `observe`, exact serialised `fix`) |
| C15 | `make check` and `make e2e` pass green | **GAP → G7 (gate)** | last full `make check` was on the MB.1.6 tree; `make e2e` ran at `686b5b7`, **before** the sqlpolicy parser changes in `470d7db`/`140f1b0`/`51dced9`. Lead repeats at the gate |

Count: **8 PASS, 7 GAP** (one of the seven is partial-only).

## Mutation summary (from the RESULTs and the retained artefacts — nothing invented)

| Run | Artefact | caught | missed | timeout | unviable | killed (viable) |
|---|---|---|---|---|---|---|
| `blastit-manifest` whole crate | `mb12-whole.log` | 550 | 14 | **0** | 73 | **550/564 = 97.5 %** |
| `edit.rs` post-review | `mb1-review-edit` | 233 | 11 | 0 | 17 | 233/244 = 95.5 % |
| `edit.rs` as measured by MB.1.1 | `mb1-manifest-edit` | 223 | 21 | 0 | 17 | 223/244 = 91.4 % |
| `lexer.rs` | `mb1-sql-lexer3` | 173 | 30 | **0** | 3 | **173/203 = 85.2 %** |
| `normalize`+`literals`+`lru` | `mb1-sql-normalize` | 108 | 1 | **0** | 15 | 108/109 = 99.1 % |
| `statements`+`tables` | `mb1-sql-statements` | 168 | **0** | **0** | 7 | **168/168 = 100 %** |
| `learn.rs` loop | `mb1-sql-learn` | 4 | 0 | 0 | 0 | 4/4 |

`blastit-manifest` per module (MB.1.2 RESULT, whole-crate run): `model.rs` 119/119 (100 %),
`error.rs` 18/18 (100 %), `validate.rs` 53/53 (100 %), `routing.rs` 4/4 (100 %), `parse.rs` 113/115
(98.3 %), `edit.rs` 233/244 (95.5 %), `uid.rs` 10/11 (90.9 %, one equivalent). Baseline was
`model` 92.4 %, `error` 77.8 %, `edit` 42.9 % with 10 timeouts, `parse` 98.3 % with 1 timeout.

Survivors, adjudicated:

- `uid.rs:62` (`>` → `>=`): **equivalent** (both arms return `u32::MAX` at the boundary) — keep.
- `edit.rs` 11: the other nine (`Plan::is_noop`, `render`, the four `patch_grant` arithmetic mutants,
  the three `patch_grant` `&&`) are argued equivalent; the two `write_atomic` guards are **not**
  equivalent (a bare relative name makes the final `File::open("")` return `ENOENT` after a
  successful rename) → `G4`.
- `parse.rs:197` / `parse.rs:220`: **real, unadjudicated** missed validation mutants (error-set
  differences; not acceptance-widening because the model rejects the same input later) → `G4`.
- `normalize.rs:629` (`trailing_word &&` → `||`): **equivalent** (only a bare `in` word satisfies both
  arms; no distinguishing input) — keep.
- `lexer.rs` 30: **real**, not equivalent: 21 in `number_end`, 4 in `radix_end`, 4 in `grouped_digits`,
  1 in `lex` (numeric-literal extent: `0x1_2`, `0b_1`, `$1E`, `1..2`). MB.1.3's follow-up says the
  largest cluster is `grouped_digits`; that is wrong (4 of 30) → `G3`.

## Protected-critical mapping

- `critical_guard.py --base 9311ff3 --node MB.1` → **FAILED, 1 protected change** — the epic node's
  own `paths: ["docs/**"]` authorize nothing, which is the correct semantics: a child, not the epic,
  must claim the file.
- `--base 9311ff3 --node MB.1.8` → **OK**, `test_check_contract.py` authorized by its own path.
- `--base 2317493 --node MB.1.6 --node MB.1.8` (full epic) → **OK, both** protected changes
  authorized; repeatable `--node` is what a gate must use when several leaves touch protected files.
- `check_contract.py`: **+10/−0** — `import time` + a ≤ 10 s, 10 ms-step wait before the unchanged
  `assertEqual(server.statements, [literal])`. Assertions byte-identical. **Ordering only: confirmed.**
- `critical_config_and_presets.rs`: **+40/−0**, one appended `#[test]` (11 → 12), fixture style of its
  neighbours; no existing line touched.
- No `#[ignore]`, `pytest.mark.skip`, `xfail`, `importorskip`, `unittest.skip`, `#[mutants::skip]` in
  any protected file. **Nothing was weakened across the epic.**

## Gaps

| # | Sev | Gap | Task |
|---|---|---|---|
| G1 | MAJOR | normalize-shaping and table-grant extents are oracle'd only by standard-tier files; a widening `is_placeholder_list -> true` passes every protected sqlpolicy suite | **MB.1.12** (already filed) |
| G2 | MAJOR | `add_group` + its `add_query` twin are only in `audit_group_hint.rs`; `critical_check_engine.rs` has zero `add_group`, yet `docs/design/ma-lean-dx.md:392` credits it | **MB.1.13** (already filed) |
| G3 | MINOR | (a) `lexer_progress.rs` is standard-tier for a parser-differential/DoS surface; (b) 30 real numeric-extent survivors; (c) `learn.rs` 96 mutants only partly re-measured; (d) MB.1.3's survivor location/count is wrong | **MB.1.14** (new) |
| G4 | MINOR | `parse.rs:197/220` and `write_atomic`'s two guard mutants (bare-name directory; a non-`AlreadyExists` open error) have no task; the wrong-route grant-attach case is standard-tier only | **MB.1.15** (new) |
| G5 | MINOR | production (`stage0::run`) startup ordering is manual-only (`sudo -n`); only `dev` is pinned | **MB.1.16** (new) |
| G6 | MINOR | three release-hygiene claims have no regression test: wheel README rewrite/restore, SVG light-scheme contrast (and XML validity), `dev-version.sh` lagging-requirement guard; `CARGO_NET_OFFLINE=1` / `serde` re-adds are silent | **MB.1.17** (new) |
| G7 | GATE | `make check`/`make e2e` at the tip: e2e predates the sqlpolicy parser rewrite; no full `make check` since MB.1.6 | lead at the gate; no task |

Accepted without a task: e2e `check_negative_boot` runs only in the local driver (droplet images live
on the droplet); `e2e/demo/blastit.toml` generated-vs-committed is a manual review diff (a real
harvest before 0.2.0 is worth one `git diff`); the `check_logs.py` two-event conjunction (INFO).

## Tasks filed from this audit (children of `MB.1`)

| Task | Gap | Owner | Priority |
|---|---|---|---|
| `MB.1.12` (already active, filed by the sibling curator session) | G1: promote normalize/table oracles to the protected tier | implementer | 2 |
| `MB.1.13` (already active, filed by the sibling curator session) | G2: protect the `add_group` oracle + correct `ma-lean-dx.md` | implementer | 2 |
| `MB.1.14` | G3: lexer tier + 30 numeric survivors + `learn.rs` full re-measure | implementer | 3 |
| `MB.1.15` | G4: `parse.rs:197/220`, `write_atomic` bare name, protected wrong-route case | implementer | 3 |
| `MB.1.16` | G5: rooted production ordering in the Docker e2e | implementer | 3 |
| `MB.1.17` (depends `MB.1.11`) | G6: wheel README, SVG contrast/XML, `dev-version.sh --check`, offline/serde invariants | implementer | 3 |

G7 stays with the lead at the gate (no task). `crates/blastit-sqlpolicy/src/lru.rs`,
`crates/blastit-manifest/src/model.rs` and `head_allocs.rs` need no follow-up.

## Files

- `docs/testing/AUDIT-MB1.md` (this report). No product code or test file was edited by this audit.
- `plan/nodes/MB/MB.1/MB.1.14` … `MB.1.17` (new task nodes; `MB.1.12`/`MB.1.13` were already filed).
- Duplicate: `docs/testing/AUDIT-MB.1.md` is the sibling session's untracked report on the same diff.
  Keep one at commit time; this file is the one referenced by the new task bodies.
