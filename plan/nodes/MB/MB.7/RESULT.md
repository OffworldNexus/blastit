# Result MB.7

## Summary

Green the release CI by not handing root a test whose premise root cannot create. PR #64's
`smoke (root)` job failed `critical_static_cli::a_root_the_caller_cannot_list_is_refused`
(`crates/blastit/tests/critical_static_cli.rs:542`, 11 passed / 1 failed): the assertion needs a
static root the caller cannot LIST, and as uid 0 every directory is listable, so root is an invalid
input to a correct test. The test is protected and was not touched; `.github/workflows/ci.yml` is
the only changed file.

MB.10's mechanism now carries both exceptions as a name → reason map, exactly
`{critical_dev_and_fail_closed, critical_static_cli}`, with a new `EXCLUDED_COUNT = 2` tripwire so a
third exclusion cannot slip in without bumping it in the same commit, on top of the existing guard
that fails when a listed suite disappears from cargo's output. Each excluded name prints its own
reason and where it is covered unprivileged. Both workflow comments were rewritten to describe two
suites and why; the root run step's `timeout-minutes` went 12 → 15 (six kept binaries × 120 s = 12
min exactly, so 15 keeps slack) and the build step's budget-warning threshold moved 720 → 900 to
match.

## Evidence

### Acceptance 1 — `critical_static_cli` excluded, comment + log line, guard still strict

`git diff .github/workflows/ci.yml` (excerpt):

```text
+          EXCLUDED = {
+              "critical_dev_and_fail_closed": (
+                  "its assertion is only valid unprivileged: production refuses "
+                  ...
+              "critical_static_cli": (
+                  "its assertion is about an unprivileged caller: it refuses a "
+                  "static root the caller cannot list, and as root every directory "
+                  "is listable, so the precondition cannot exist"
+              ),
+          }
+          EXCLUDED_COUNT = 2
...
-          if len(excluded) != 1:
+          if len(EXCLUDED) != EXCLUDED_COUNT:
+          unnamed = sorted(name for name in EXCLUDED if not EXCLUDED[name].strip())
...
-                  "  reason: its assertion is only valid unprivileged (production "
+                  f"  reason: {EXCLUDED[name]}"
```

The workflow comment above the build step names the reason verbatim ("as uid 0 every directory is
listable, so the precondition cannot exist"), and the log block prints per name:

```text
excluded from the root phase: <name>
  reason: <its reason from EXCLUDED>
  covered unprivileged: the `test` job (cargo test --workspace --all-targets) in CI, and `make test-critical` locally
```

Real binary set against cargo's own JSON:

```console
$ cargo test -p blastit --locked --no-run --message-format=json
8 test binaries built as the runner user; 6 run as root:
excluded from the root phase: critical_dev_and_fail_closed
  reason: ...as root it serves, so the spawned child never exits and the suite blocks
  covered unprivileged: the `test` job ... and `make test-critical` locally
excluded from the root phase: critical_static_cli
  reason: ...as root every directory is listable, so the precondition cannot exist
  covered unprivileged: the `test` job ... and `make test-critical` locally
exit 0
```

A suite that vanishes still fails the step (dead exclusion, false coverage claim):

```console
$ # same script, artifacts.json with the `critical_static_cli` artifact removed
the unprivileged-only suite(s) ['critical_static_cli'] did not appear in cargo's output: ... the exclusion is dead ...
exit 1
```

### Acceptance 2 — no test skipped, weakened or edited; workflow only

```console
$ git diff 0a2778c --stat
 .github/workflows/ci.yml | 123 +++++++++++++++++++++++------------------
 plan/BUDGET.md           |   2 +-
 plan/JOURNAL.md          |   6 +++
$ git diff 0a2778c -- crates/blastit/tests/
# (empty)
```

The two `plan/` entries are lead bookkeeping, not this node. No file under `crates/*/tests/` or
`python/tests/` changed: `crates/blastit/tests/critical_static_cli.rs` is byte-identical to base
`0a2778c`.

### Acceptance 3 — `make check`, `plan.py validate`, `bash -n`

```console
$ make check
exit 0   # 80 s
$ uv run --no-project tools/plan.py validate
plan OK
$ # bash -n on every `run: |` block of the parsed workflow
11/11 clean
```

### Acceptance 4 — what the next `smoke (root)` run must show

See Follow-ups (a): 6 kept binaries each `test result: ok`, both excluded names with reason and
coverage line, no `::error::`, no budget `::warning::` (6 × 120 s = 720 s < 900 s).

## Files

- `.github/workflows/ci.yml` (modified)

## Follow-ups

(a) **Verify on the next `smoke (root)` run** (not yet observed — this node was fixed locally):
the build step's log must show `8 test binaries built as the runner user; 6 run as root:` followed
by `excluded from the root phase: critical_dev_and_fail_closed` and
`excluded from the root phase: critical_static_cli`, each with its `reason:` and
`covered unprivileged:` line, then `test result: ok` for the six kept binaries, no `::error::`, and
no `::warning::` about the root-phase budget (720 s worst case inside the 15-minute bound).

(b) **PROPOSED separate node, not implemented here:** make `critical_static_cli` root-meaningful
instead of excluded by running the unlistable-root assertion in a dropped-privilege child (setuid to
a non-root uid) so a root caller can exercise it. That edits a protected critical test
(`crates/blastit/tests/critical_static_cli.rs`) and needs its own review; until then `make
test-critical` plus the `test` job cover the assertion unprivileged, which is what the exclusion
prints.
