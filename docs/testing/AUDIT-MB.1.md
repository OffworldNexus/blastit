# AUDIT MB.1 — mutation coverage, startup diagnostics, harness hygiene

- **Date:** 2026-11-13
- **Scope:** the ten leaves `MB.1.1`–`MB.1.10` on `release/0.2.0`, diff `cab377e..51dced9` limited to
  `crates/blastit-manifest/**`, `crates/blastit-sqlpolicy/**`,
  `crates/blastit/src/{stage0,audit_v1}.rs`, `crates/blastit-scheduler/src/scheduler.rs`,
  `scripts/{dev-version,build-wheel}.sh`, `Makefile`, `Cargo.toml`/`Cargo.lock`/`crates/*/Cargo.toml`,
  `e2e/**`, `python/tests/critical/test_check_contract.py`, `docs/**`.
- **Base:** `cab377e` (`Release v0.1.1`). **Tip audited:** `51dced9`.
- **Requirements:** `plan/nodes/MB/MB.1/NODE.md` (the five acceptance bullets), each leaf's
  `NODE.md`/`RESULT.md`; `docs/design/ma-lean-dx.md` rows `R1..R3`, `R7..R8`, `R11..R12`, `R24..R25`,
  `R27..R38`, `R39..R42`, §1.3/§6/§7.2 (`MA.8`); `ADR-0003`, `ADR-0004`; skill `testing` §1–§4;
  skill `security-invariants` §1, §3, §5.
- **Not re-audited:** ingress/WASM/CVE harnesses (untouched by this epic), `AUDIT-MA.2.*`,
  `AUDIT-MA.3.md` (its `F1..F8` are the prior state this epic inherits).

## Verdict

**PASS at the epic gate; the protected tier does not yet own the three new policy oracles.**

All five `MB.1` acceptance bullets are met with artefacts on disk: `blastit-manifest` whole-crate
**550/564 viable = 97.5 %** with **0 timeouts**, the five modules this epic newly measured at
**85–100 %** (lexer 85.2 %, normalize 99.0 %, literals/lru/statements/tables 100 %) with the two
already-measured modules unchanged (analysis 84.4 %, lib 90.0 %), the missing-`./blastit.toml` ordering
pinned in the protected `critical_config_and_presets.rs` (**+1 test, 0 lines removed**), both e2e
negatives permanent, and the one protected Python change is **+10 lines of ordering, 0 assertions
changed**.

What holds it below a clean pass is classification, not correctness: the three largest new test files
that pin *security* behaviour — `policy_module_edges.rs` (normalize shaping), `policy_module_tables.rs`
(table/column grant extents) and `audit_group_hint.rs` (`add_group`) — sit in the unprotected standard
tier, so the checks they pin can still be deleted together with their oracle and
`tools/critical_guard.py` stays green. **F1 is reproduced, not argued:** a widening mutant
(`is_placeholder_list -> true`, which makes a grant for `IN ($?)` allow `IN ('x')`) passes both
protected sqlpolicy suites and is caught only by the non-critical file.

No test in scope is fluff, no assertion was weakened or deleted, no skip construct was added, and no
mutation ratio in the records is overstated in a way that changes a verdict (two labels are inaccurate
— `F6`). **No BLOCKER.**

## Method

1. Read every leaf `NODE.md`/`RESULT.md`, the epic node, `plan/nodes/MB/MB.1/LOG.md`,
   `docs/knowledge/LESSONS.md`, the twelve protected Rust suites and the three Python critical suites;
   read all six new integration test files line by line.
2. Recomputed every mutation ratio from the artefacts the leaves left on disk
   (`/tmp/opencode/mb1-*/mutants.out/`, `./mutants.out/`) and checked that each measured *mutant set*
   is still the set the shipped source produces (`cargo mutants --list`, no test run):
   `blastit-manifest` 637, `sqlpolicy` 921, `lexer.rs` 206, `{normalize,literals,lru}` 124,
   `{statements,tables}` 175 — all match the RESULTs.
3. Spot-checked two claims against the source (§"Mutation evidence"): `uid.rs:62` equivalence, and the
   30 `lexer.rs` survivors.
4. Probed the protected tier with one anchored mutant in a `/tmp` copy of the tree
   (`/tmp/opencode/mb1-curate-probe`, `CARGO_TARGET_DIR=/tmp/opencode/mb1-probe-target`): the
   `is_placeholder_list -> true` substitution, then both protected sqlpolicy suites and the
   non-critical twin. No file in the repository was edited.
5. Re-ran the changed suites at the tip (`cargo test -p blastit-manifest -p blastit-sqlpolicy`,
   `-p blastit --test audit_group_hint --test critical_check_engine --test critical_config_and_presets`,
   `-p blastit-scheduler --test head_allocs`): all green, counts as quoted below.
6. `cargo-mutants` was **not** re-run (expensive; the artefacts are complete and the mutant sets
   verified). Docker/`make e2e` was **not** re-run; `MB.1.8`'s log is the evidence.

## Inventory & classification

| Path | Tests | Class | Verdict |
|---|---|---|---|
| `crates/blastit-manifest/tests/edit_branches.rs` (new, MB.1.1) | 17 | useful (standard) | right tier, two cases belong in the protected edit suite (`F5`) |
| `crates/blastit-manifest/tests/mutation_gaps.rs` (new, MB.1.2) | 17 | useful (standard) | right tier: accessors, derived flags, error rendering; the coverage predicate is check-side only (`F9`) |
| `crates/blastit-manifest/src/edit.rs` inline `mod tests` | +25 | useful (standard) | the scanner's escape/quote state lives **only** here and in `edit_branches.rs` (`F5`) |
| `crates/blastit-manifest/src/model.rs` inline | +2 | useful (standard) | `version()` + `parameterised`; kills the two model mutants |
| `crates/blastit-sqlpolicy/tests/lexer_progress.rs` (new, MB.1.3) | 6 (`fn` count 8) | useful (standard) — **should grow into critical** | lexer boundaries + 200 KB hostile inputs = parser-differential/DoS surface (`F4`) |
| `crates/blastit-sqlpolicy/tests/policy_module_edges.rs` (new, MB.1.4) | 10 | **misplaced — should be critical** | the only oracle for normalize's collapsed shapes; widening mutant escapes every protected suite (`F1`) |
| `crates/blastit-sqlpolicy/tests/policy_module_tables.rs` (new, MB.1.5) | 18 | **misplaced — should be critical (subset)** | the table/column grant extents are the grant surface (`F2`) |
| `crates/blastit-sqlpolicy/src/{normalize,tables,statements,lru}.rs` inline | +45/+2/+N/+3 | useful (standard) | oracle in the same file as the code it judges — deletion-prone (`F1`, `F2`) |
| `crates/blastit/tests/audit_group_hint.rs` (new, MB.1.6) | 4 | **misplaced — should be critical** | the only executable statement of `add_group` and of the `SELECT 1` twin (`F3`) |
| `crates/blastit/tests/critical_config_and_presets.rs` (MB.1.6) | 12 (was 11) | **critical (protected)** | correct placement; +1 test, 0 removed lines |
| `crates/blastit-scheduler/tests/head_allocs.rs` (MB.1.6) | 2 (was 1) | useful (standard) | right tier: allocation budget, not a security wall; exact `+3 per request` oracle |
| `python/tests/critical/test_check_contract.py` (MB.1.8) | 22 (unchanged) | **critical (protected)** | correct; +10 lines of bounded wait, no assertion touched |
| `e2e/scenario.py`, `check_logs.py`, `rootless/run.py` (MB.1.8) | +6 checks | e2e | right tier: Docker/rootless, not `make check` |
| `e2e/{grants.py,demo/**}` | — | fixtures | generator learned the empty grant; shape not asserted (`F10`) |
| `docs/**`, `scripts/**`, `Cargo.*` | — | docs/build | no test-tier question; three of them have no regression test (`F8`) |

Nothing in scope is **fluff**: the accessor-shaped tests in `mutation_gaps.rs`
(`a_proxy_name_borrows_as_its_own_string`, `a_protocol_has_exactly_one_spelling`) pin wire/serde
contracts a caller reads, and they are the only kill for those mutants — keep them. No
**broken-oracle** test was found among the new files: every policy test pairs an allowed case with a
denied twin, the e2e additions carry a baseline branch, and the allocation test asserts an exact
delta rather than "less than".

## Epic acceptance, bullet by bullet

| Bullet | Status | Evidence |
|---|---|---|
| `manifest` ≥ 75 % whole crate, `verify()` + patcher branches, 0 timeouts | **met** | `./mutants.out/`: 550 caught / 14 missed / 0 timeout / 73 unviable = **97.52 %**; `grep -i verify missed.txt` empty; `edit.rs` 233/244 = 95.5 % in `/tmp/opencode/mb1-review-edit` |
| All `sqlpolicy` mutants measured, ≥ 75 % per module, timeouts bounded | **met, with `learn.rs` only partly re-measured** (`F6`) | lexer 173/203 = 85.2 %, normalize 97/98, literals 4/4, lru 7/7, statements 84/84, tables 84/84; `--list` = 921 = the module sum |
| Missing `./blastit.toml` named first, pinned in the protected suite, docs reconciled | **met** | `critical_config_and_presets` 12/12 (one new test, dev leg pinned); `docs/troubleshooting.md:12-13,33` |
| Build/scheduler hygiene | **met** | `grep CARGO_NET_OFFLINE=1` → none; no `serde` in `blastit-events`; `trap restore_readme EXIT` at `build-wheel.sh:76`; `scheduler.rs:1236-1238` early return, pinned by `head_allocs` (66.02 vs 69.02) |
| e2e `queries = []` (42501) + `--cap-drop ALL` negative boot, deterministic learn refusal, logo, `add_group` docs, `make check`/`make e2e` green | **met** | `MB.1.8` log (both positives + negatives, `make e2e` exit 0, 3× `make test-critical`); `MB.1.6` `make check` exit 0; `docs/brand/logo.svg:10` `prefers-color-scheme`; `docs/ci.md:74-76` |

## Requirement → protected-test map

| Requirement | Behaviour | Protected today? |
|---|---|---|
| `R24..R25` (sql literals), `R31` | every literal form redacts to one `…`; learn refuses inlines | **yes** — `critical_literals.rs` (25), `critical_learn_policy.rs` (10) |
| `R7..R8`, `R11`, `R24` (manifest) | schema, reserved keys, catch-all, redacted/literal grants | **yes** — `critical_manifest_011.rs` |
| `R28..R29` (edit) | surgical edit, insertion above the catch-all, atomic write, symlink | **yes** — `critical_manifest_edit.rs` (18) |
| `R33` `add_query` | every grantable statement's fix re-parses | **yes** — `critical_check_engine.rs:416` |
| `R33` **`add_group`** (`MA.8`) | connection setup names `[group.connection.<proxy>].queries`; `SELECT 1` keeps `add_query`; tx/redaction stay `observe` | **NO** — only `audit_group_hint.rs` (`F3`); `docs/design/ma-lean-dx.md:392` names `critical_check_engine.rs`, which contains **zero** `add_group` |
| `R38` closed `fix.action` set | nine values, `add_group` included | **NO** — no schema enum anywhere; docs only (`docs/ci.md:74`) |
| §6 allowlist (normalize shaping) | reformatting keeps the verdict; comment splice, casing, literal-for-placeholder, NBSP are denied; `IN`/`VALUES` shapes collapse only when identical | **NO** — `policy_module_edges.rs` only (`F1`) |
| §6 allowlist (table grants) | columns/star, read vs write, join alias, subquery, `INSERT…SELECT` read side, lock needs one column | **partly** — a table-name case, a star grant and one column denial in `critical_learn_policy.rs`; every extent case is `policy_module_tables.rs` only (`F2`) |
| `R12` no body read without a policy | `AllowEmptyOnly` from the policy alone | **yes** — `head_allocs.rs` exact-delta oracle |
| `R1..R3`/`R6` startup ordering | missing manifest before `BLASTIT_APP` | **half** — `dev` pinned; production `run` manual only (`F7`) |
| `R39..R42` check contract | learn refusal writes nothing, `add_query` for `SELECT 1` | **yes** — `python/tests/critical/test_check_contract.py` |
| `security-invariants` §5 (never hang on hostile input) | every scanner advances structurally | **partly** — no timeouts in any run; the lexer's numeric scans still have 30 real survivors (`F4`) |
| CVE bar (`php2shell`, `node2shell`, polyglots) | — | unchanged by this epic; no new CVE test required |

## Mutation evidence

### Counts recomputed from the artefacts (measured tree == shipped tree)

| Run | Dir | caught | missed | timeout | unviable | Killed (viable) |
|---|---|---|---|---|---|---|
| `manifest` whole crate | `./mutants.out/` | 550 | 14 | **0** | 73 | **550/564 = 97.5 %** |
| `edit.rs` (post-review) | `/tmp/opencode/mb1-review-edit` | 233 | 11 | 0 | 17 | 233/244 = 95.5 % |
| `edit.rs` (as measured by MB.1.1) | `/tmp/opencode/mb1-manifest-edit` | 223 | 21 | 0 | 17 | 223/244 = 91.4 % |
| `lexer.rs` | `/tmp/opencode/mb1-sql-lexer3` | 173 | 30 | **0** | 3 | **173/203 = 85.2 %** |
| `normalize/literals/lru` | `/tmp/opencode/mb1-sql-normalize` | 108 | 1 | **0** | 15 | 108/109 = 99.1 % |
| `statements/tables` | `/tmp/opencode/mb1-sql-statements` | 168 | **0** | **0** | 7 | **168/168 = 100 %** |
| `learn.rs:65x` | `/tmp/opencode/mb1-sql-learn` | 4 | 0 | 0 | 0 | 4/4 |

Every number quoted in the RESULTs matches the files, and `cargo mutants --list` on `51dced9` returns
the same set sizes (637 / 921 / 206 / 124 / 175), so the measurements were taken on source that is
still the shipped source (`git log 7a41f61..51dced9 -- crates/blastit-manifest/src` is empty).

### Spot-check 1 — `uid.rs:62` "equivalent" (MB.1.2 §4): **honest**

`UidLayout::view` is `if uid > u32::MAX as u64 { u32::MAX } else { uid as u32 }` with
`uid = base + 1000 + index` as `u64`. At `uid == u32::MAX` the real code returns `uid as u32 ==
u32::MAX` and the `>=` mutant returns `u32::MAX`; below and above, both arms agree. Note
`base.saturating_add(..)` cannot overflow past `u64::MAX` into a differing answer either. No test can
distinguish it — correctly recorded as unkillable, not as a kill.

### Spot-check 2 — the 30 `lexer.rs` survivors (MB.1.3 follow-up): **real, but mis-attributed**

The follow-up says "the largest cluster is `grouped_digits` (`lexer.rs:353..362`, `delete !` and two
`+= → *=`)". The file says otherwise:

```console
$ awk -F: '{print $1":"$2}' …/missed.txt | sed -E 's/.*lexer\.rs:([0-9]+).*/\1/' | awk \
    '{ if($1==126) f="lex"; else if($1>=268 && $1<=304) f="number_end";
       else if($1>=329 && $1<=331) f="radix_end"; else f="grouped_digits"; n[f]++ }
     END{for(k in n) print k, n[k]}'
number_end       21
radix_end         4
grouped_digits    4
lex               1
```

26 of the 30 live in `number_end`/`radix_end`/`lex`; only 4 are in `grouped_digits`. They are genuine
(not equivalent): `grouped_digits`'s loop is `for c in rest`, so `len += 2 → len *= 2` truncates the
numeric literal instead of hanging — which is why `timeout.txt` is empty and why the loop
restructuring did remove the timeout class. The defect class they leave open is numeric-literal
extent (`0x1_2`, `0b_1`, `$1E`) in a lexer that decides token/statement boundaries (`F4`).

### Probe — the protected sqlpolicy tier is blind to a widening normalize change (`F1`)

In `/tmp/opencode/mb1-curate-probe`, `is_placeholder_list` returning `true`:

```console
$ cargo test -p blastit-sqlpolicy --test critical_literals --test critical_learn_policy
test result: ok. 10 passed; 0 failed          # critical_learn_policy
test result: ok. 25 passed; 0 failed          # critical_literals

$ cargo test -p blastit-sqlpolicy --test policy_module_edges
test result: FAILED. 9 passed; 1 failed
    an_in_list_is_one_shape_whatever_its_length

# same mutated tree, tests/probe_widen.rs (scratch only, not shipped):
$ normalize("SELECT id FROM t WHERE id IN ('x')")  = "SELECT id FROM t WHERE id IN ($?, ...)"
$ normalize("SELECT id FROM t WHERE id IN ($?)")   = "SELECT id FROM t WHERE id IN ($?, ...)"
$ grant "..., id IN ($?)" checks "..., id IN ('x')" -> Allowed     # an inline literal the grant never approved
```

That is a real allowlist **widening** (a literal reaches the database under a placeholder grant), it is
caught only by the non-critical file, and `tools/critical_guard.py` would not notice the pair being
deleted. `AUDIT-MA.3` `F8` described the same shape of gap; MB.1.4/MB.1.5 moved the oracle *further*
out of the protected tier rather than into it.

## Protected suites that were extended

| File | Diff | Verdict |
|---|---|---|
| `crates/blastit/tests/critical_config_and_presets.rs` | **+40 lines, 0 removed**, `#[test]` 11 → 12, appended last | **contract intact.** The new test drives the real binary in a fresh sandbox and asserts *both* orders (empty dir names `blastit.toml` and never `BLASTIT_APP`, stdout empty; a manifest with no `app` names `BLASTIT_APP` and never `No such file`). Same fixture style as its neighbours. |
| `python/tests/critical/test_check_contract.py` | **+10 lines, 0 removed**, 22 tests before and after | **contract intact.** `import time`, one bounded `while server.statements != [literal]` loop (≤ 10 s, 10 ms step) ahead of the unchanged `assertEqual`. A timeout can only make the existing assertion fail; nothing is skipped or relaxed. |

`crates/blastit-manifest/tests/critical_*.rs`, `crates/blastit-sqlpolicy/tests/critical_*.rs`,
`crates/blastit/tests/{critical_check_engine,critical_dev_and_fail_closed}.rs`,
`crates/blastit-py/tests/critical_check_schema.rs` and the other Python critical files are byte-identical
across the range. No `#[ignore]`, `pytest.mark.skip`, `xfail` or `#[mutants::skip]` was added anywhere
in scope.

## `edit_branches.rs` — does it do what it claims? (`MB.1.1`, question 4)

Yes, and it is not a happy-path restatement. 17 tests, each round-tripping the edited text through
`Manifest::from_toml_str` before asserting:

- **in-string `[[`** — `a_bracket_inside_a_quoted_statement_does_not_hide_the_next_route`: a
  `queries = ['... "[[\"...']` entry must not be counted as a header, proven by the resulting route
  order (`products, orders, fallback`).
- **`queries =` inside a string** — `a_line_that_looks_like_an_assignment_inside_a_string_is_not_the_assignment`:
  a `'''`-quoted value containing `queries = ['fake']`; asserts exactly one `queries = [` line and that
  the author's statement survives.
- **list shapes** — multi-line folded to one line (and no orphan `[`/`]`/entry lines), one-line with a
  trailing comment kept byte-for-byte, `[]` and a trailing-comma list, a `[route.db]` with no
  assignment, a file with no trailing newline, a foreign `[proxy.backup]` table between the route and
  its grant table (one `[route.db]`, no duplicate).
- **refusals** — duplicate proposed route names, empty pattern (by rule index), empty statement
  (not echoed), a path with no file name, each with the message fragment the docs promise.
- **not covered here**: the escape/triple-quote state (`walk_reads_escapes_strings_and_brackets`,
  `tripled`) lives only in `src/edit.rs`'s inline `mod tests`, and the wrong-route attach case has no
  case (`F5`).

Two cases are security-shaped and belong in `critical_manifest_edit.rs`: a patcher that loses string
state writes into a grant's literal, and one that mis-locates `[route.db]` attaches a learned grant to
a route the operator did not approve. The re-parse + `verify()` backstop catches corruption but cannot
catch a well-formed grant on the wrong route.

## Findings

| # | Sev | Finding | Evidence |
|---|---|---|---|
| `F1` | **MAJOR** | `tests/policy_module_edges.rs` (10 tests) is the only oracle for `normalize`'s collapsed shapes. A widening mutant passes both protected sqlpolicy suites and fails only this file; delete the file and the check together and `critical_guard` is green. Belongs in a protected `tests/critical_policy_surface.rs`. | probe above; `normalize.rs:614` inline tests are the other oracle and live in the file being mutated |
| `F2` | **MAJOR** | `tests/policy_module_tables.rs` (18 tests) is the only oracle for table/column grant extents: join alias, subquery, `INSERT…SELECT` read side, star-vs-list, read-vs-write, lock-without-column. The protected table coverage is a table-name case, a star grant and one column denial (`critical_learn_policy.rs:264,295,371`), with **no** join/subquery/alias line at all. A `tables.rs` under-report mutant that widens what a grant reaches is therefore unguarded. | `grep -Eic 'join\|subquery\|alias'` → **0** in `critical_learn_policy.rs`, 5 in the new file |
| `F3` | **MAJOR** | `tests/audit_group_hint.rs` (4 tests) is the only executable statement of the `add_group` contract and of its `SELECT 1 → add_query` twin. `critical_check_engine.rs` contains **zero** occurrences of `add_group`, so a change making every `missing_grant` an `add_group` passes every protected suite; `docs/design/ma-lean-dx.md:392` claims the opposite (R33's `add_group` guarantee is attributed to `critical_check_engine.rs`). | `grep -c add_group` → 0 in `critical_check_engine.rs`, `critical_check_schema.rs`, `test_check_contract.py`; the applied shape itself does validate (probe: group table + `groups = ["connection"]` → `parse_and_validate` OK, 0 warnings) |
| `F4` | MINOR | `tests/lexer_progress.rs` (lexer boundaries, 200 KB hostile inputs) is standard-tier, and 30 real survivors remain in `number_end`/`radix_end`/`grouped_digits`. `MB.1.3`'s follow-up calls them "genuine coverage debt for a security-surface parser" but no plan node exists. | §Mutation evidence, spot-check 2 |
| `F5` | MINOR | The manifest scanner's string/escape state is oracle'd by inline `src/edit.rs` tests (same file as the code, unprotected) plus two standard-tier cases; the two `write_atomic` bare-relative-name mutants (reclassified as a **real** gap by the lead addendum) and `parse.rs:197/220` have no task. | `mutants.out/missed.txt` lines 384/411, 197, 220; `edit.rs:1690` `walk_reads_escapes_strings_and_brackets` |
| `F6` | MINOR | Two `RESULT.md` statements are inaccurate. (a) `MB.1.5`: "921 = the crate's entire mutant set, so every module is now measured" — `learn.rs` lists 96 mutants and only the 4 in the refactored `parse_form` region were re-measured; the other 92 rest on MA's pre-refactor 90/98. (b) `MB.1.3`: "30 real survivors in `grouped_digits`" (26 are in `number_end`/`radix_end`/`lex`) and "8 tests" for a 6-`#[test]` file (8 was the `fn` count). Neither changes a verdict. | `cargo mutants -p blastit-sqlpolicy -f '**/src/learn.rs' --list` → 96; `grep -c '#\[test\]' lexer_progress.rs` → 6 |
| `F7` | MINOR | The startup reordering exists at two sites (`stage0.rs:444` `run`, `:666` `run_dev`); only `dev` is pinned. The production leg is proven once by hand (`sudo -n`) and by no automated test, so a regression at `run` alone would ship. | diff `cab377e..51dced9 -- crates/blastit/src/stage0.rs`; `critical_config_and_presets.rs` calls `dev` only |
| `F8` | MINOR | Three hygiene changes are proven by one-shot manual commands with no regression test: the wheel's absolute README links (`scripts/build-wheel.sh`), the light-scheme logo contrast (`docs/brand/logo.svg:10`), and `scripts/dev-version.sh`'s lagging-requirement guard. `scripts/check-long-description.py` runs only in the publish workflow; `docs-unit`/`docs-test` assert neither the SVG nor the dev-version guard. | `grep -rn 'logo.svg\|readme-absolute\|dev-version' python/tests/` → nothing; `Makefile` has no target for them |
| `F9` | INFO | `mutation_gaps.rs`'s `only_tables_and_groups_cover_a_query` pins a *check-side* predicate (`audit_v1`/`blastit-py` suppress a finding when it says covered). A wrong `true` hides a missing grant (fail-closed at runtime); it cannot widen. Standard tier is correct. | `model.rs:655`, callers in `audit_v1.rs:821,1368`, `audit.rs:758`, `dev_learn.rs:549` |
| `F10` | INFO | `check_negative_boot` runs in the local rootless driver only (the droplet image lives on the droplet), and no test compares `e2e/grants.py`'s output with the committed `e2e/demo/blastit.toml`; the e2e run does prove the committed manifest behaves. | `e2e/rootless/run.py` `local()`; `MB.1.8` follow-ups 2 and 4 |

## Gaps → tasks

Grouped so each task owns one protected file and one acceptance proof. Paths are the ones the
implementer may touch (a new `critical_*.rs` is always allowed by `tools/critical_guard.py`).

### T1 — Protect the sqlpolicy policy surface (`F1` + `F2`)

- **Paths:** `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` (new, protected),
  `crates/blastit-sqlpolicy/tests/policy_module_edges.rs`,
  `crates/blastit-sqlpolicy/tests/policy_module_tables.rs`,
  `crates/blastit-sqlpolicy/src/normalize.rs`, `crates/blastit-sqlpolicy/src/tables.rs`
- **Acceptance:** the protected file holds (a) the normalize pairs — reformatted-allowed vs
  comment-splice/case/literal-for-placeholder/NBSP-denied, `IN`/`VALUES` shapes that collapse only when
  identical; (b) the table extents — star-vs-list, read-vs-write, join alias, subquery,
  `INSERT…SELECT` read side, lock needs one column; every case an allowed/denied twin.
  `cargo test -p blastit-sqlpolicy --test critical_policy_surface --locked` green, and the probe
  mutant `is_placeholder_list -> true` **fails** it (paste before/after; it currently passes
  `critical_literals` + `critical_learn_policy`). Keep the rendering/serde cases in the standard files.

### T2 — Protect the check fix-action contract (`F3`)

- **Paths:** `crates/blastit/tests/critical_check_engine.rs` (or new
  `crates/blastit/tests/critical_audit_fix_actions.rs`), `crates/blastit/tests/audit_group_hint.rs`,
  `crates/blastit/src/audit_v1.rs`, `docs/design/ma-lean-dx.md`
- **Acceptance:** protected twins for `SELECT set_config(…)`/`SET`/`SHOW` → `add_group`, `SELECT 1` and
  a plain `DELETE` → `add_query`, redaction and `SAVEPOINT` → `observe`; apply the `add_group` fix
  (group table + `groups = ["connection"]`) and assert `Manifest::parse_and_validate` accepts it with
  zero warnings; making every `missing_grant` an `add_group` fails the protected run;
  `docs/design/ma-lean-dx.md:392` names the file that really holds it.

### T3 — Close the scanner survivors and re-measure the two crates (`F4`, `F5`, `F6`)

- **Paths:** `crates/blastit-manifest/src/{edit,parse}.rs`,
  `crates/blastit-manifest/tests/edit_branches.rs`,
  `crates/blastit-sqlpolicy/src/lexer.rs`,
  `crates/blastit-sqlpolicy/tests/lexer_progress.rs` (promote to `critical_lexer_boundaries.rs`),
  `crates/blastit-sqlpolicy/src/learn.rs`
- **Acceptance:** a `set_current_dir`-isolated binary covers the bare-filename `write_atomic` branch
  (no flake, no `--test-threads=1` dependency); `parse.rs:197/220` killed by a route/grant test; the
  numeric-literal survivors (`0x1_2`, `0b_1`, `$1E`, `_`-separated runs) pinned with allowed **and**
  denied twins; `make mutants-rust PKG=blastit-manifest` misses ≤ 11 and `cargo mutants -p
  blastit-sqlpolicy -f '**/src/lexer.rs'` ≥ 95 %; `cargo mutants -p blastit-sqlpolicy -f
  '**/src/learn.rs'` (96) re-measured at ≥ 75 % with 0 timeouts, counts pasted; the RESULT texts
  corrected (learn tally, survivor locations, test count).

### T4 — Pin the production startup ordering end to end (`F7`)

- **Paths:** `e2e/**`, `.github/workflows/ci.yml`
- **Acceptance:** in an empty directory, a root boot with `--cap-drop ALL` prints
  `./blastit.toml: No such file or directory` as an `app.output` event on **stdout**, exits `1`, and
  never mentions `BLASTIT_APP`; the check runs in `make e2e` and in the CI `smoke (root)` job; the
  existing `dev` assertion in `critical_config_and_presets.rs` stays untouched.

### T5 — Regression-test the release hygiene trio (`F8`)

- **Paths:** `python/tests/runnable_docs/harness.py` (wheel metadata), `python/tests/test_docs_assets.py`
  (new, SVG contrast), `python/tests/test_dev_version_script.py` (new, tmp-copy guard)
- **Acceptance:** `make docs-test` fails when a `dist/*.whl` `METADATA` long description contains
  `](docs/` or `](ARCHITECTURE.md`; a docs-unit test computes WCAG contrast from the SVG's
  light-scheme fill against `#f4efe3` and requires ≥ 3.0:1; a test runs `scripts/dev-version.sh` on a
  `cp -a` copy with one lagging `version = "0.1.0", path` requirement and asserts a non-zero exit
  naming `file:line`. All three must run offline in `make check`.

## Follow-ups

- `e2e/demo/blastit.toml` is still hand-regenerated; either add an offline shape assertion on
  `grants.py`'s `EMPTY_GRANT` emission or accept that the next harvest is a manual review diff
  (`MB.1.8` follow-up 4).
- `MB.1.3`'s lexer run lists 206 mutants where the plan text still says 298; `MB.1.4` 124 vs 128 and
  `MB.1.5` 175 vs 170. The epic's "1019" is a pre-refactor count — restate it as "everything the tree
  lists (921)" in `plan/STATE.md`, as `MB.1.5` already argues.
- `mutants.out/` in the worktree root is gitignored and complete; keep it until the `MB` milestone gate
  re-reads the numbers, then `MB.1.11`'s `make mutants-clean` owns pruning.
- The tree is not clean (`plan/{BUDGET,JOURNAL}.md`, `MB.1/LOG.md` modified and
  `plan/nodes/MB/MB.1/MB.1.11/` untracked, a live `MB.1.11` run) — none of it affects the audited
  sources; the diff `cab377e..51dced9` is what was judged.

VERDICT: PASS at the epic gate — the mutation bar (97.5 % manifest whole crate, 85–100 % per
`sqlpolicy` module) and both protected-suite extensions are honest and green; the protected tier must
still adopt the sqlpolicy policy surface (`F1`, `F2`) and the `add_group` contract (`F3`) before the
`MB` milestone gate (tasks T1–T2). No BLOCKER.
