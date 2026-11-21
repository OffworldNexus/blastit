# Result MB.1

## Summary

Epic complete: 17 leaves (`MB.1.1`–`MB.1.17`) close every `MA`-gate follow-up it inherited, and two
`test-curator` audits of the diff agree it passes the epic gate. `blastit-manifest` moves from 550/564
(97.5 %) to **558/564 = 98.94 %** whole-crate mutation kill (637 mutants, **6 proven-equivalent
survivors**, **0 timeouts**), and `blastit-sqlpolicy` is measured end to end at **921 mutants,
84.4–100 % per module, 0 timeouts anywhere** — the bounded-loop refactors that removed the timeout
class also removed/added step mutants, which is why the epic's "1019" is superseded by a measured 921.

The security-shaped oracles the audits found in the unprotected tier now live in
`tools/critical_guard.py`-protected suites (`critical_policy_surface.rs`, `critical_lexer_boundaries.rs`,
`critical_check_engine.rs`), startup names the missing `./blastit.toml` before `BLASTIT_APP` at **both**
entry points (dev by a protected test, production by a Docker e2e), and `e2e/` permanently asserts the
`queries = []` (42501) and `--cap-drop ALL` negatives. No BLOCKER; the two honest discrepancies are
stated under Evidence (`### Two deviations`).

## Evidence

One row per `# Acceptance` bullet of `MB.1/NODE.md`.

| # | Acceptance bullet (abridged) | Status | Command / artefact | Source |
|---|---|---|---|---|
| 1 | `make mutants-rust PKG=blastit-manifest` ≥ 75 % whole crate, `verify()` + comment/bracket patcher branches, 0 cursor-loop timeouts | **met** — 97.5 % → **98.94 %**, 0 timeouts | `make mutants-rust PKG=blastit-manifest MUTANTS_TMP=/var/tmp/mb115-mut` → `637 mutants tested in 19m: 6 missed, 558 caught, 73 unviable`; `test ! -s mutants.out/timeout.txt` → `0 timeouts`; `grep -i verify mutants.out/missed.txt` → none | `MB.1.2` §1–§4, `MB.1.15` §4, `AUDIT-MB.1.md` C1 |
| 2 | All `blastit-sqlpolicy` mutants measured at ≥ 75 % **per module**, cursor-loop timeouts bounded | **met** — 921 measured, 84.4–100 %, 0 timeouts (count superseded, see `(a)`) | file-scoped `cargo mutants -p blastit-sqlpolicy -f …/-F …` runs; `cargo mutants -p blastit-sqlpolicy --list` = 921 | `MB.1.3`, `MB.1.4`, `MB.1.5`, `MB.1.14`; `AUDIT-MB1.md` C2 |
| 3 | Missing `./blastit.toml` reported before `BLASTIT_APP` (`stage0.rs:444,666`), pinned in `critical_config_and_presets.rs`, `docs/troubleshooting.md` reconciled | **met** — dev pinned, production pinned by e2e | `cargo test -p blastit --test critical_config_and_presets` → `12 passed`; `make e2e` → both `D6: the rooted production boot …` blocks `ok` | `MB.1.6` §1–§2, §5; `MB.1.16`; `AUDIT-MB1.md` C4 |
| 4 | `build-wheel.sh` rewrites README links + restores on exit, `CARGO_NET_OFFLINE=true`, `blastit-events` drops `serde`, `plan_request_body` returns `AllowEmptyOnly` for `policy.is_none()` | **met** | `! grep -rn 'CARGO_NET_OFFLINE=1' Makefile scripts .github`; `test -z "$(git status --porcelain README.md)"` after a build; `head_allocs` → `no policy 66.02` vs `body = "none" 69.02` | `MB.1.7` §1–§2, `MB.1.6` §4; `AUDIT-MB1.md` C6–C9 |
| 5 | e2e `queries = []` (42501) + `--cap-drop ALL` negatives, deterministic learn refusal, logo light-mode contrast, `add_group` guidance, `make check`/`make e2e` green | **met** | gate `make check` exit 0; gate `make e2e` exit 0 (356 `ok`); see the per-item table below | `MB.1.8`, `MB.1.7`, `MB.1.6`, `MB.1.10`, `MB.1.13`; gate logs |

### 1. Mutation — `blastit-manifest`, whole crate

| Run | Mutants | caught | missed | timeout | unviable | Killed (viable) |
|---|---|---|---|---|---|---|
| `MB.1.2` (`/tmp/opencode/mb12-mut-tmp`) | 637 | 550 | 14 | **0** | 73 | **97.52 %** (550/564) |
| `MB.1.15` (`/var/tmp/mb115-mut`) | 637 | **558** | **6** | **0** | 73 | **98.94 %** (558/564) |

`edit.rs` 233/244 → **239/244 (97.95 %)**, `parse.rs` 113/115 → **115/115**; `model.rs`, `validate.rs`,
`error.rs`, `routing.rs` at 100 %, `uid.rs` 10/11 with one equivalent. All six survivors are proved
equivalent (no input distinguishes them) in `MB.1.15` §"Survivors left"; `verify()` is covered directly
by five refusal cases plus the accept path, so `grep -i verify missed.txt` is empty (`MB.1.1` §4).
The former `parse.rs:252` / `edit.rs:738,1116..1369` timeouts are structurally impossible: every scan
takes its step from an iterator position (`MB.1.1` loop inventory), and no `--timeout`, `#[ignore]` or
loop guard was added.

### 2. Mutation — `blastit-sqlpolicy`, every module (921 = `--list`)

| Module | Mutants | caught | missed | timeout | Killed | Source |
|---|---|---|---|---|---|---|
| `lexer.rs` | 206 | 202 | 1 | 0 | **99.5 %** (202/203) | `MB.1.14` |
| `learn.rs` | 96 | 90 | 1 | 0 | **98.9 %** (90/91) | `MB.1.14` |
| `normalize.rs` | 110 | 97 | 1 | 0 | **99.0 %** (97/98) | `MB.1.4` |
| `literals.rs` | 4 | 4 | 0 | 0 | **100 %** | `MB.1.4` |
| `lru.rs` | 10 | 7 | 0 | 0 | **100 %** (7/7) | `MB.1.4` |
| `statements.rs` | 84 | 84 | 0 | 0 | **100 %** | `MB.1.5` |
| `tables.rs` | 91 | 84 | 0 | 0 | **100 %** (84/84) | `MB.1.5` |
| `analysis.rs` | 299 | 232 | — | 0 | **84.4 %** (232/275), unchanged file | `AUDIT-MB.1.md` C2 |
| `lib.rs` | 21 | 18 | — | 0 | **90.0 %** (18/20), unchanged file | `AUDIT-MB.1.md` C2 |

Every `timeout.txt` is empty (lexer, learn, normalize/literals/lru, statements/tables). The nine
cursor-loop timeouts MA recorded (`learn.rs:661` ×1, `lexer.rs:179..230` ×8 — `MA/RESULT.md` §2) are
gone and the mutants that caused them are **caught**, not merely absent (`MB.1.3`; `MB.1.5` proves the
same for its six `statements.rs` hangs from the run's `caught.txt`). Survivors are **3** in total, each
proven equivalent: `normalize.rs:629` `trailing_word &&`→`||` (`MB.1.4` §4), `lexer.rs:126` `<`→`<=`
(`MB.1.14` §3), and `learn.rs:563`'s `Value::Array` arm, unreachable for a single-statement tree
(`MB.1.14` §4.1). The 30 real numeric-extent survivors `MB.1.3` left open are closed by extent tests
that assert the **index** a literal ends at.

### 3. Startup ordering

`stage0::run` and `stage0::run_dev` read the manifest before `Config::from_env`, so an empty directory
says `./blastit.toml: No such file or directory` and a manifest without `app` still says `BLASTIT_APP`.
Pinned both ways by
`critical_config_and_presets::an_empty_directory_names_the_missing_manifest_before_the_app` (12 tests,
+1, 0 existing lines touched) and, for production — which an unprivileged boot never reaches because
the capability gate precedes it — by `e2e/rootless/run.py::check_rooted_startup`, which asserts the
`app.output` **stream** and the absence of the other message in both directions. `docs/troubleshooting.md`
now has the missing-file row, the "manifest exists but declares no `app`" row, and one `blastit check`
connection-setup row (`SET`, `SHOW`, `set_config(…)` → `[group.<name>.db]`).

### 4. Build, scheduler and release hygiene

- `scripts/build-wheel.sh` rewrites the README with `scripts/readme-absolute-links.py` before
  `maturin build` and restores it via `trap restore_readme EXIT` on success **and** failure.
- `CARGO_NET_OFFLINE=1` no longer exists in `Makefile`, `scripts/` or `.github/` (strict-bool comment
  points at `LESSONS.md`); maturin rejects `1`.
- `crates/blastit-events/Cargo.toml` has no `serde` dependency and `Cargo.lock` lost exactly that edge.
- `Scheduler::plan_request_body` returns `Ok(BodyPlan::AllowEmptyOnly)` before building the method or
  reading a header: **66.02** allocations per request with no policy vs 69.02 with
  `[route.ingress] body = "none"` — an exact 3-per-request delta, reversed-order runs fail.
- All four are now regression-tested offline in `make check` (`python/tests/test_release_scripts.py`,
  `python/tests/test_docs_assets.py`, `scripts/dev-version.sh --check`), each proved by a revert probe.

### 5. Harness, brand and `add_group`

| Item | Evidence |
|---|---|
| `queries = []` route denies with `42501` | gate `make e2e`: `ok a route granting \`queries = []\` refuses an ordinary statement (SQLSTATE 42501)` (root + rootless) and `ok the \`queries = []\` route refused an ordinary statement with SQLSTATE 42501` |
| `--cap-drop ALL` negative boot | gate `make e2e`: `D6: an unprivileged boot with no capabilities is refused` → `ok the message names the uid and every capability it lacks (init runs as uid 10000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid)` |
| Production ordering e2e | `MB.1.16` `make e2e` exit 0: `D6: the rooted production boot names the missing manifest first` + `… with a manifest names BLASTIT_APP, not the file`, each `ok`, `stderr carries 0 bytes` |
| Deterministic learn refusal | protected `python/tests/critical/test_check_contract.py` +10/−0 lines: one bounded wait (≤ 10 s, 10 ms step) before the unchanged assertion; 3× `make test-critical` green; `AUDIT-MB1.md` C12 |
| Logo light-mode contrast | `docs/brand/logo.svg` inline `@media (prefers-color-scheme: light) { #head-cutout { fill: #161a20 } }`; computed WCAG ≥ 3.0:1 pinned by `test_docs_assets.py`; residual toggle case is in Follow-ups |
| `add_group` for connection-setup SQL | `audit_v1::Fix::add_group` → `[group.connection.<proxy>].queries`, `SELECT 1`/`DELETE` keep `add_query`, tx/redaction stay `observe`; protected oracle in `critical_check_engine.rs` (19 tests), docs in `docs/ci.md` (all 8 emitted actions) + `docs/design/ma-lean-dx.md` R33/R38 |
| `make check` green | exit 0 at `c65289b` (54 s), `9c523e2` (136 s), `2f55b19` (162 s) — `/tmp/opencode/mb-gate{2,3,4}.log`, `/tmp/opencode/mb-merge-check.log` (guard OK, clippy `-D warnings`, rustdoc `-D warnings`, all workspace tests, 50 Python critical tests, docs-unit, `zensical build --strict`, wheel verified, 38 doc fences 0 failed, `plan OK`) |
| `make e2e` green | exit 0 at `9c523e2` + dist 0.1.2 (130 s, 356 `ok`, 0 `FAIL`) — `/tmp/opencode/mb-gate-e2e.log`; `MB.1.16` repeated it at the tip with the rooted pair added |
| Fresh confirmation at the tip | `make check` on `ccef48f` — see `### Gate re-run` below |

### Merge of `origin/develop` (v0.1.2) — `9c523e2`

Nine mechanical conflicts: 8 × `crates/*/Cargo.toml` → develop's 0.1.2, `scripts/dev-version.sh` →
ours (the hardened version is a superset). Verified `cargo metadata --locked --no-deps`,
`scripts/check-release-tag.sh v0.1.2`, `scripts/release-notes.sh --require-dated v0.1.2`, then the
gate runs above (`MB.1/LOG.md` 18:52).

### Audits and their gaps — all closed

| Audit | Verdict | Gaps | Closed by |
|---|---|---|---|
| `docs/testing/AUDIT-MB.1.md` (committed `9018a8b`, F1–F10) | PASS at the epic gate, no BLOCKER | F1/F2 protected sqlpolicy surface, F3 `add_group`, F4/F6 lexer+learn, F5 scanner survivors, F7 production ordering, F8 hygiene tests, F9/F10 INFO | `MB.1.12`, `MB.1.13`, `MB.1.14`, `MB.1.15`, `MB.1.16`, `MB.1.17` |
| `docs/testing/AUDIT-MB1.md` (untracked, G1–G7) | PASS on correctness and the mutation bar, no BLOCKER | G1/G2 (same as F1/F3), G3, G4, G5, G6, G7 = `make check`/`make e2e` at the tip | same leaves; G7 by the lead gate runs |

`critical_guard.py` authorizes both protected edits (`MB.1.6`, `MB.1.8`) and all later protected
additions; no `#[ignore]`, `skip`, `xfail` or `#[mutants::skip]` was added in scope.

### Two deviations (stated, not hidden)

**(a) "1019 mutants" is stale; the shipped set is 921.** The epic text counts the crate before the
bounded-loop refactor. Making a hand-stepped `i += 1` structural changes the mutant surface —
`lexer.rs` 298→206, `learn.rs` 103→96, `normalize.rs` 114→110, `statements.rs` 79→84 — and
`cargo mutants -p blastit-sqlpolicy --list` on the shipped tree returns **921**, the sum of the nine
module runs above with **every** module measured, not sampled. The same effect moved `blastit-manifest`
from 630 to **637**. The acceptance measure ("≥ 75 % killed per policy module, timeouts bounded") is met
with room; only the remembered constant is wrong (`MB.1.5` follows-up, `AUDIT-MB1.md` G3).

**(b) Two audits of the same diff exist.** `AUDIT-MB.1.md` (F1–F10, committed with `MB.1`) and
`AUDIT-MB1.md` (G1–G7, still untracked) were written concurrently by two curator sessions; both
re-derived the numbers from the artefacts, both reach PASS with no BLOCKER, and `AUDIT-MB1.md` records
the duplication itself. Keeping one is a housekeeping follow-up, not a correctness question; the gap
taxonomy differs, and every gap in both tables is closed by a leaf.

### Gate re-run

`make check` on this tree (`ccef48f` + this file, unprivileged, offline) — **exit 0**, 2 837 lines
(`/tmp/opencode/mb1-epic-gate-check.log`):

```console
$ make check > /tmp/opencode/mb1-epic-gate-check.log 2>&1; echo "exit=$?"
exit=0
$ grep -E "guard: OK|critical-guard|test result: FAILED|No issues found|fence|plan OK|^exit=" …
guard: OK (1 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 4 changed file(s)
  no change to a protected critical test
92 × test result: ok, 0 × test result: FAILED      # cargo test --workspace
Ran 50 tests … OK                                  # python/tests/critical
63 passed, 57 subtests passed                      # docs-unit (was 46 before MB.1.17's 17)
No issues found                                    # zensical build --strict
📦 Built wheel … dist/blastit-0.1.2-…whl            # then the long-description test + installed-wheel fences
38 fences: 26 executed, 12 accounted, 0 failed
plan OK
exit=0
```

The changed files the guard saw when it ran are the then-untracked `AUDIT-MB1.md` and four plan
bookkeeping files (`BUDGET.md`, `MB.1/LOG.md`, `MB.1.11/{LOG,RESULT}.md`); this `RESULT.md` was written
while the run was in flight, and the run's closing `plan.py validate` (`plan OK`) already covers it. No
source and no protected file was touched by the gate, and nothing was committed.

## Files

The epic's deliverables, by area (this gate wrote only `plan/nodes/MB/MB.1/RESULT.md`):

- `crates/blastit-manifest/src/{edit,parse,model}.rs`, `crates/blastit-manifest/tests/{edit_branches,mutation_gaps,critical_manifest_edit}.rs`
- `crates/blastit-sqlpolicy/src/{lexer,learn,normalize,lru,statements,tables}.rs` (test/inline-test additions plus the bounded scans), `tests/{critical_policy_surface,critical_lexer_boundaries,policy_module_edges,policy_module_tables}.rs` (`lexer_progress.rs` moved into the protected file)
- `crates/blastit/src/{stage0,audit_v1}.rs`, `crates/blastit-scheduler/src/scheduler.rs`, `crates/blastit-events/Cargo.toml`, `crates/*/Cargo.toml` (0.1.2 requirements), `Cargo.lock`
- `crates/blastit/tests/{critical_config_and_presets,critical_check_engine,audit_group_hint}.rs`, `crates/blastit-scheduler/tests/head_allocs.rs`
- `e2e/{scenario.py,check_logs.py,grants.py,rootless/run.py,rootless/README.md,README.md,demo/blastit.toml,demo/demo/urls.py,demo/notes/views.py}`
- `scripts/{build-wheel.sh,dev-version.sh,mutants-run.sh,mutants-scratch.sh}`, `Makefile`, `.gitignore`
- `python/tests/{test_docs_assets.py,test_release_scripts.py}`, `python/tests/critical/test_check_contract.py`
- `docs/{ci.md,troubleshooting.md,brand/logo.svg,knowledge/LESSONS.md,design/ma-lean-dx.md}`
- `docs/testing/{AUDIT-MB.1.md,AUDIT-MB1.md}`, `plan/nodes/MB/MB.1/**`

## Follow-ups

**Residue the audits deliberately left open (none blocks the gate):**

1. **Equivalent mutants, proofs only in RESULTs.** 6 in `blastit-manifest` (`uid.rs:62`,
   `edit.rs:490/743/794/815/878`) with proofs in `MB.1.15` §"Survivors left", and 3 in
   `blastit-sqlpolicy` (`normalize.rs:629`, `lexer.rs:126`, `learn.rs:563`'s unreachable `Value::Array`
   arm, each proved in `MB.1.4`/`MB.1.14`). No test can distinguish them; a future node could re-spell the
   predicate (`MB.1.4` suggests `checked_sub` for `trailing_word`) to trade an unkillable mutant for a
   killable one, but that is churn, not coverage.
2. **The droplet leg runs neither refusal check.** `check_negative_boot` and `check_rooted_startup` run
   in the local rootless driver (`make e2e`); a droplet twin needs the image on the droplet (`MB.1.16`
   follow-up).
3. **Logo `<img>` limitation.** The asset's `prefers-color-scheme: light` rule follows the **OS**
   preference; the site's light toggle on a dark OS still draws the paper cutout, because
   `data-md-color-scheme` cannot reach inside an `<img>`. Closing it needs the logo inlined
   (`brand.css`) or a `<picture>` pair — both outside `MB.1.7`/`MB.1.17`.
4. **`dist/` two-wheel collision after a version bump.** At the gate, `make e2e` refused to start with
   "a prebuilt e2e image installs exactly one wheel from dist, found 2" (stale `0.1.1` beside `0.1.2`);
   `rm -f dist/blastit-0.1.1-*.whl` fixed it (`MB.1/LOG.md` 18:57). The wheel-metadata test also asserts
   *every* wheel in `dist/`, so one stale/broken artefact fails `docs-unit`. Worth a `dist` preflight
   (prune wheels whose version ≠ workspace version) so a version bump is not a manual `rm`.
5. **`LESSONS.md` bullets the leaves could not write (`docs/` outside their `paths`), recorded only in
   RESULT text:**
   - `MB.1.15`: `cargo mutants` puts `CARGO_TEST_ARGS` into `cargo test` **verbatim without `--`** (a
     scoped probe needs `-- -- --exact <name>`); `rustfmt --edition 2024` vs the editor auto-format
     disagree on nested import order; the session's shell tool cannot `cd` into `/var/tmp`, so probe
     copies are driven with `--manifest-path` from the repository.
   - `MB.1.16`: the editor runs `ruff format` with **default** settings on write (no `[tool.ruff]`), so
     editing `e2e/rootless/run.py` reformats the whole file (heredoc avoids it); `docker run` inherits an
     image's `BLASTIT_APP`/`BLASTIT_MANIFEST`, and `-e NAME=` is **not** an unset.
   - `MB.1.17`: a test must not copy the live `README.md` — `build-wheel.sh` rewrites it in place during
     the build, so the copy races; use a fixture.
   - `MB.1.13`: the editor hook re-sorts imports ASCII-first where `cargo fmt` is version-first.
   - Gate (this file, `MB.1/LOG.md`): the `dist/` two-wheel collision above.
   (`MB.1.10` already wrote the four bullets it owned — allocation oracle, stage-0 `app.output` stream,
   `cargo-mutants` outcome files, concurrent `MUTANTS_TMP` — and `MB.1.11`/`MB.1.7`/`MB.1.14` their own.)

**Other out-of-scope discoveries:**

6. **Keep one audit file at merge.** `AUDIT-MB.1.md` is committed (`9018a8b`); `AUDIT-MB1.md` is still
   untracked and is the file the new task bodies reference. Pick one before the `MB` milestone gate —
   this gate deliberately committed neither and left the working tree as it found it.
7. **`blastit-env` has 9 surviving mutants** (`error.rs::is_empty`, `EnvErrors::into_iter`,
   `postgresql.rs` `"password" | "ca_cert"`, `secret.rs::Drop`, `template.rs` ×4) — found while proving
   `MB.1.11`'s memory bound; needs a `test-curator`/`implementer` task, outside this epic's crates.
8. **Remaining `/tmp` defaults** of the same class `MB.1.11` fixed: `e2e/rootless/run.py` (SSH key
   scratch), `python/tests/runnable_docs/harness.py`, `docs/brand/check_seams.py`.
9. **Test-tier duplication.** `MB.1.12` promoted 21 cases that still exist in the standard-tier files
   (deliberate: the node forbade deleting assertions); a later curation pass can keep only the
   rendering/serde twins there. Likewise `MB.1.13`'s `add_group` fix is asserted as a document but never
   *applied* by a test, and `MB.1.14`'s `number_end`/`radix_end` are asserted at offset 0 only.
10. **Plan text still carries stale snippets.** `MB.1.1/NODE.md` instructs
    `awk '$NF=="caught"' mutants.out/outcomes.txt`, a file cargo-mutants 27.1.0 never writes (the
    `LESSONS.md` bullet says so; the terminal node still misleads). `MB.1.11`'s `paths` frontmatter lacks
    the `.gitignore` its directive added. `e2e/demo/blastit.toml` generated-vs-committed is still a
    manual review diff (`make e2e` proves the committed manifest behaves).
11. **Run the gate's `critical_guard` with a `--node` per protected-touching leaf.** `make check`'s guard
    has no `--node`, so an authorized protected edit reads as a violation there; the enforced form is
    `tools/critical_guard.py --base <base> --node MB.1.6 --node MB.1.8 --node MB.1.12 …` (repeatable).
