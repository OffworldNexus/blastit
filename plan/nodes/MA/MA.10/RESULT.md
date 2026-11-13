# Result MA.10

## Summary

`smoke (root)` now runs exactly the five test binaries whose assertions are valid as uid 0 and
deliberately withholds the one that is not, `critical_dev_and_fail_closed`, whose
`unprivileged_production_has_no_single_uid_fallback` waits for the production binary to refuse —
true only for an unprivileged caller, so as root the child serves and `Command::output()` blocks.

The exclusion is explicit, commented, printed, and guarded: the step fails if the excluded set is
not exactly `{critical_dev_and_fail_closed}`, if that stem is missing from cargo's output
(renamed/removed ⇒ dead exclusion, false coverage claim), if it would ever skip more than one
binary, or if a binary name cannot be stemmed. Everything else is default-included, so a new test
target is run as root automatically. MA.9's preflight, build-as-runner/run-as-root split,
per-binary `timeout --kill-after`, `--nocapture`, `--test-threads=1`, `stdbuf`+`tee` streaming,
124/137 `::error::`, doc-test step and failure-log upload are untouched, and the step comment's
arithmetic now matches the kept set (2 min worst case × 5 = 10 min inside a 12-minute bound).

## Evidence

### The real binary set (cargo's own JSON)

```console
$ cargo test -p blastit --locked --no-run --message-format=json > /tmp/opencode/ma10-artifacts.json   # 6 compiler-artifact(test) lines
$ # filter output, annotated with each target's name/kind/src from the same JSON
6 test binaries built as the runner user; 5 run as root:
  …/out/blastit-40841d6f9f4794cc                          -> stem blastit                      (src/lib.rs,  kinds ['lib'])
  …/out/blastit-a0eb3b2a97f4e4c1                          -> stem blastit                      (src/main.rs, kinds ['bin'])
  …/out/critical_check_engine-820647768b49cfb5            -> stem critical_check_engine        (tests/critical_check_engine.rs)
  …/out/critical_config_and_presets-bb327fcd8e8a409f      -> stem critical_config_and_presets  (tests/critical_config_and_presets.rs)
  …/out/smoke-98997e28a8a88429                            -> stem smoke                        (tests/smoke.rs)
excluded from the root phase: critical_dev_and_fail_closed
  reason: its assertion is only valid unprivileged (production refuses because capability detection fails; as root it serves, so the spawned child never exits and the suite blocks)
  covered unprivileged: the `test` job (cargo test --workspace --all-targets) in CI, and `make test-critical` locally
```

Order is cargo's sorted path order (`blastit` lib, `blastit` bin, `critical_check_engine`,
`critical_config_and_presets`, `smoke`); the two `blastit-*` entries prove the stem is not a unique
key, which is why the kept list is a list and only the documented stem is removed.

### Acceptance 1 — only root-valid binaries; comment, log line, guard

Command (extracts every `run:` script from the parsed workflow, runs `bash -n`, compiles the filter,
runs it against the real JSONL and five tampered variants, then drives the run loop with a stub
`sudo`):

```console
$ uv run --no-project /tmp/opencode/ma10_verify.py
PASS bash -n step 2 (None)
PASS bash -n step 3 ('Make /opt and the tool cache non-world-writable (as root)')
PASS bash -n step 6 ('Preflight (sudo is non-interactive, cargo runs as root)')
PASS bash -n step 7 ('Build the test binaries (runner user, no root)')
PASS bash -n step 8 ('Run every test binary as root, one at a time')
PASS bash -n step 9 ("Run the crate's doc tests as root")
PASS filter heredoc compiles
PASS 6 built, 5 kept, 1 excluded :: 6/5/['critical_dev_and_fail_closed']
PASS kept stems are exactly the root-valid set :: ['blastit', 'blastit', 'critical_check_engine', 'critical_config_and_presets', 'smoke']
PASS log prints the excluded stem, the reason and the unprivileged job
PASS build step exits 0 end to end
PASS build step wrote kept and excluded files :: critical_dev_and_fail_closed
PASS removed suite => step fails and names it dead :: the unprivileged-only suite(s) ['critical_dev_and_fail_closed'] did not appear in cargo's output: renamed, removed, or not built? The root phase's exclusion is ...
PASS renamed suite => step fails
PASS a grown exclusion set => step fails :: the exclusion would skip 2 test binaries (['critical_dev_and_fail_closed', 'smoke2']), not exactly one ...
PASS an unstrippable name => step fails rather than guess :: notahashedbinary does not look like a cargo test binary name (<target>-<hash>) ...
PASS no binaries => step fails
37 PASS, 0 FAIL
ALL CHECKS PASSED
```

- **Comment:** new `# MA.10:` block above `Build the test binaries (runner user, no root)` names the
  test, the reason (production does not refuse as root by design, so the test blocks) and the guard.
- **Visible line:** printed by the build step (above) *and* echoed by the run step
  (`not run as root (excluded in the build step): critical_dev_and_fail_closed`), both in the job log.
- **Guard:** `set(excluded) != EXCLUDED` (dead exclusion), `len(excluded) != 1` (silent growth),
  `stem()` rejects an unstrippable name, plus the pre-existing "no test binary" error.
- **Default-include:** `kept = [b for b, s in zip(binaries, stems) if s not in EXCLUDED]` — a new
  target is run as root with no edit here.

### Acceptance 2 — per-binary timeout, `--nocapture`, preflight kept

```console
$ uv run --no-project /tmp/opencode/ma10_verify.py   #  (excerpt)
PASS sudo preflight step intact
PASS build-as-runner step intact
PASS run-as-root step intact
PASS doc-test step intact
PASS failure log upload intact
PASS no step uses continue-on-error
PASS the only step that builds tests does so as the runner, with --no-run
PASS run step bound is 12 min
PASS doc-test step still --test-threads=1 and --doc
PASS loop: two healthy binaries exit 0
PASS loop: the excluded stem is echoed in the run step too
PASS loop: counts the kept binaries
PASS loop: streams --nocapture output into the log
PASS loop: a wedged binary is a 124 ::error:: naming it :: ::error::... did not finish as root within 2s: the last 'test <name> ...' line in root-tests.log is the test that blocked (MA.9 candidate 3), not a deadlock in the job.
PASS loop: a failing binary keeps its exit code and gets an ::error:: :: rc=3
```

The run loop was executed for real (fake bin dir + stub `sudo` that shifts to `env`): ordering,
`stdbuf -oL -eL`, `tee -a`, `PIPESTATUS`, the 124/137 branch and the non-zero branch all behave.

### Acceptance 3 — `make check`, `plan.py validate`, `bash -n`, diff scope

```console
$ make check
...
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet ...
38 fences: 26 executed, 12 accounted, 0 failed · 23.4s
uv run --no-project tools/plan.py validate
MA: done but has open children
make: *** [Makefile:54: plan-validate] Error 1
make check exit=2
```

Every target is green and grep counts **0** failing tests (`grep -cE 'FAILED' -> 0`): `guard`,
`critical-guard`, `rs-fmt`, `rs-lint`, `rs-doc`, `rs-test`, `test-critical` (Rust + Python),
`py-lint`, `docs-unit`, `docs-check`, `docs-test` (`38 fences: 26 executed, 12 accounted, 0 failed`).
The single `Error` in the log is `plan-validate`, whose cause is the plan-state defect below.
**Acceptance 3's `make check exit 0` is therefore not met, and cannot be met from this node:** the
only red target is `plan-validate`, which reads `plan/` statuses alone, and `implementer` may not
mutate plan statuses (dispatch rule). Everything else in `make check` — including the full
unprivileged run of the excluded suite — is green.

Coverage of the excluded suite, proved by the same run (unprivileged, `make test-critical`):

```console
cargo test -p blastit --test critical_dev_and_fail_closed
running 19 tests
...
test unprivileged_production_has_no_single_uid_fallback ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```

`plan.py validate`:

```console
$ uv run --no-project tools/plan.py validate
MA: done but has open children
validate exit=1        # NOT caused by this node, see Follow-ups
```

Proof the `validate` failure is pre-existing and outside `paths` (a clean worktree at the base SHA,
which has no MA.10 node at all):

```console
$ git worktree add --detach /tmp/opencode/ma10-base dc553ad && cd /tmp/opencode/ma10-base
$ uv run --no-project tools/plan.py validate
plan OK
exit=0
```

The failure appears the moment `plan/nodes/MA/MA.10/NODE.md` exists with `status: active` under
milestone `MA` whose status is `done` (created by `conductor` at 12:15, logged in `LOG.md`).
`implementer` may not mutate plan statuses, so this node cannot make it pass; `lead` must reopen
`MA` (`plan.py --agent lead set MA active`).

```console
$ bash -n <each extracted smoke run script>   # see Acceptance 1: 6/6 PASS
$ git diff --name-only
.github/workflows/ci.yml
$ git diff --stat -- .github/workflows/ci.yml
 .github/workflows/ci.yml | 115 +++++++++++++++++++++++++++++++++++++++++++++++------
 1 file changed, 99 insertions(+), 16 deletions(-)
```

(`plan/BUDGET.md`, `plan/JOURNAL.md` and untracked `plan/nodes/MA/MA.10/` are the lead's/conductor's
bookkeeping, present before this session; no file outside `.github/workflows/ci.yml` was written.)

### Acceptance 4 — what the next `smoke (root)` run must show

Green means, in the job log, in this order and with **no** `::error::` line:

1. Build step: `6 test binaries built as the runner user; 5 run as root:` followed by exactly five
   paths (`blastit` ×2 for lib+bin, `smoke`, `critical_check_engine`, `critical_config_and_presets`)
   and the `excluded from the root phase: critical_dev_and_fail_closed` block.
2. Run step: `not run as root (excluded in the build step): critical_dev_and_fail_closed`, then
   `running 5 test binaries as uid 0, one at a time`, then five `==> root: <path>` blocks each
   ending in `test result: ok. N passed; 0 failed; 0 ignored` — today `smoke` 31, `critical_config_and_presets`
   11, `critical_check_engine` 16, plus both `blastit` unit binaries. No
   `::error::<path> did not finish as root within 90s` and no `did not finish` at all.
3. Doc tests: `running 0 tests ... ok`.
4. No `Upload the root test logs` step in the run (it is `if: failure()`), job conclusion **success**,
   wall time ~4-6 min with a warm `rust-cache` (well under the 20-minute ceiling; the build step is
   the only long one on a cold cache).
5. In the `test` job, `critical_dev_and_fail_closed` must still run and pass unprivileged
   (`cargo test --workspace --all-targets`) — that is the coverage this exclusion relies on.

Any other shape (an extra `==> root:` line, a missing `excluded from the root phase:` line, a
`critical_dev_and_fail_closed` path in the kept list) means the filter changed and the guard was
bypassed — treat it as red.

### Acceptance 5 — can the suite legitimately run as root?

No evidence found. `crates/blastit/tests/critical_dev_and_fail_closed.rs:1257-1295` asserts
`output.status.code() == Some(1)`, that the refusal mentions `blastit dev`, and that
`"privilege":"unprivileged-dev"` / `"event":"ready"` never appear; its own comment states *"`R17`:
production fails closed. The suite runs unprivileged, so the capability detection is what refuses."*
There is no `needs_root` inverse marker (the `Makefile` only greps for the skip/`needs_root` pattern
as a forbidden construct) and no env switch that would make production refuse as root — passing
`SUDO_UID` would require editing the protected test, which is forbidden. The exclusion is therefore
the only correct fix, and MT: the file was read only.

## Files

- `.github/workflows/ci.yml` (only change): build-step filter + guard + exclusion comment and log
  line, run-step comment/echo/arithmetic, run-step `timeout-minutes: 10 -> 12`.
- `plan/nodes/MA/MA.10/RESULT.md` (this file).

## Follow-ups

1. **Plan state blocks `plan.py validate`:** milestone `MA` is `status: done` while its child
   `MA.10` is `active` (`MA: done but has open children`). Reproduced identically in a clean
   worktree at `dc553ad` (where `plan OK`), so it is not this node's diff; `lead` must reopen `MA`
   (or `conductor` re-file MA.9/MA.10 under an open milestone). No acceptance item in this node can
   fix it without mutating plan statuses, which is forbidden for `implementer`.
2. **A second unprivileged-only critical suite would need the same treatment** (e.g. anything else
   asserting on capability-detection refusal). The guard deliberately fails the job rather than
   silently widening, so the next author gets a named error instead of a 90 s "blocked test".
3. **The smoke job's step bounds sum past its 20-minute ceiling** (1+2+10+12+3 = 28 min). This is
   pre-existing from MA.9 and only matters when both the build and the run go long; the healthy path
   and any single wedged binary stay far inside it. Worth a future node if a cold build ever collides
   with a wedged binary.

---

# Attempt 2 — reviewer MINOR findings (all three fixed)

Scope unchanged: `.github/workflows/ci.yml` only; no step/phase bound changed, no exclusion logic,
stems, pass/fail condition, run loop or MA.9 preflight touched, no `continue-on-error`.

## F1 — job comment claimed the bounds were "well inside 20 minutes" (now says the truth)

The job comment above `timeout-minutes: 20` now says each phase is *independently* bounded (a
regression is a red phase with its own `::error::`; healthy run ~5 min), then states plainly that
`1 + 2 + 10 + 12 + 3 = 28` worst-case minutes exceeds the 20-minute job ceiling, so the job ceiling —
not a step — is the outer bound in a pathological run where several phases go long at once.

```console
$ grep -n -A9 'MA.9: this job once sat' .github/workflows/ci.yml | sed -n '4,10p'
    # Every phase below is independently bounded, so a regression is a red phase
    # whose log and `::error::` name what blocked (a healthy run is ~5 minutes).
    # The bounds do not sum to a promise, though: 1 + 2 + 10 + 12 + 3 = 28
    # worst-case minutes, so the 20-minute `timeout-minutes` below - not any
    # single step - is the outer bound in a pathological run where several
    # phases go long at once. (A cold `rust-cache` is the one legitimate way to
    # approach it: the build step below is then the long one.)
```

`timeout-minutes: 20` and every step bound are untouched (`run step bound is 12 min :: 12`).

## F2 — "silently grown" message contradicted itself (now names the list size)

The guard's behaviour is identical; only the wording changed. It now prints the *actual* excluded
stems observed and the exclusion list's own size, so a two-entry list no longer reads as
"2 … not exactly one (2)".

```console
$ # extracted heredoc, EXCLUDED grown to {"critical_dev_and_fail_closed","smoke2"}
PASS a grown exclusion set => step fails with the corrected wording ::
the exclusion would skip 2 test binaries (['critical_dev_and_fail_closed', 'smoke2']) but the
exclusion list has 2 entries, not exactly one: every extra exclusion needs its own proof and comment
```

(Singular "1 entry" is used when the list has one entry, e.g. two binaries colliding on the same stem;
the grown case above is the reviewer's exact tamper input.)

## F3 — sixth/seventh target was only promised, never surfaced (now a non-fatal `::warning::`)

The build step's Python prints a GitHub `::warning::` (never fails the job) when
`len(kept) * 120 >= 720`, naming the count, the per-binary budget (`120 s = 90 s + 30 s kill-after`),
the product and the exact step whose bound to raise. The run-step parenthetical now points at that
warning instead of promising an unenforced rule.

```console
$ # healthy 6 built (5 kept): no warning, exit 0
PASS healthy run prints no ::warning:: about the run bound
$ # 7th target added (6 kept): warning, exit 0
PASS a seventh target (six kept) => ::warning:: naming count and bound, exit 0 ::
::warning::the root phase now runs 6 test binaries at up to 120 s each = 720 s, at or beyond the
run step's 720 s (12-minute) bound: raise `timeout-minutes` on 'Run every test binary as root, one
at a time' with the count so every binary still reaches its own ::error::
```

One deliberate one-character reading of the brief: the literal predicate `len(kept) * 120 > 720`
would **not** fire on the 7th built target (6 kept × 120 = 720 exactly), contradicting both the
finding's own rationale ("a wedged sixth kept binary would be cut off by the step") and its VERIFY
line ("7th target => warning"). The guard therefore uses `>= 720`, which fires at 6 kept (7 built) and
above; the reviewer's 7th-target case below passes.

## Re-verification (all after the final edit)

```console
$ uv run --no-project /tmp/opencode/ma10_verify2.py
...
PASS job still named 'smoke (root)'
PASS bash -n step 2 (None)   # + steps 3, 6, 7, 8, 9 — every changed run block
PASS filter heredoc compiles
PASS 6 built, 5 kept, 1 excluded :: 6/5/['critical_dev_and_fail_closed']
PASS job comment no longer claims the bounds are 'well inside 20 minutes'
PASS healthy run prints no ::warning:: about the run bound
PASS removed suite => step fails and names it dead ::
  the unprivileged-only suite(s) ['critical_dev_and_fail_closed'] did not appear in cargo's output:
  renamed, removed, or not built? ...
PASS a grown exclusion set => step fails with the corrected wording ::
  the exclusion would skip 2 test binaries (['critical_dev_and_fail_closed', 'smoke2']) but the
  exclusion list has 2 entries, not exactly one: ...
PASS a seventh target (six kept) => ::warning:: naming count and bound, exit 0
PASS loop: a wedged binary is a 124 ::error:: naming it
PASS loop: a failing binary keeps its exit code and gets an ::error:: :: rc=3
40 PASS, 0 FAIL
ALL CHECKS PASSED
exit=0
```

```console
$ uv run --no-project tools/plan.py validate
plan OK
validate exit=0
$ make check
...
38 fences: 26 executed, 12 accounted, 0 failed · 21.2s
uv run --no-project tools/plan.py validate
plan OK
make check exit=0          # 38 PASS targets, 0 FAIL/FAILED/Error lines
$ git diff --stat dc553ad -- .github/workflows/ci.yml
 .github/workflows/ci.yml | 144 +++++++++++++++++++++++++++++++++++++++++++++++-------
 1 file changed, 124 insertions(+), 20 deletions(-)
```

No commit, no push. The `plan/` modifications in `git status` are the lead's/conductor's bookkeeping
(present before this attempt); this attempt wrote only `.github/workflows/ci.yml` and this section.

## Attempt 3 — reviewer MINOR (message text contained the literal `::error::`)

Reworded only the tail of the non-fatal budget `::warning::` at `ci.yml:271` from ``own ::error::`` to `own timeout error annotation`, so a grep-based "no `::error::` in the job log" check cannot read that line as red; count/`720`/step-name content, the `>= 720` condition and the non-fatal behaviour are unchanged. Verified: `grep -n '::error::' .github/workflows/ci.yml` now matches only genuine `echo "::error::...` emissions (145, 152, 156, 168, 343, 347, 560, 565, 689, 700, 704, 709) plus three YAML comments (92, 264, 307, never logged); the heredoc compiles and emits `::warning::...reaches its own timeout error annotation` with exit 0 and 0 `::error::` at 6 kept, and is silent at 5 kept; `bash -n` 6/6 extracted smoke scripts; `/tmp/opencode/ma10_verify2.py` 40 PASS / 0 FAIL; `plan.py validate` `plan OK` exit 0; `make check` exit 0 (38 fences: 26 executed, 12 accounted, 0 failed, 0 FAIL/Error lines). Diff still limited to `.github/workflows/ci.yml` plus this file; no commit, no push.
