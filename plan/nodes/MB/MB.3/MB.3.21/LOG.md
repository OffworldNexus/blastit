# Log MB.3.21

## 2026-11-14T16:36 [lead]

created (task) check --learn must not drop a route's executor block: edit.rs expected_routes loses [route.static] today and [route.php] next

## 2026-11-14T16:36 [lead]

created on owner instruction (2026-11-14) after the finding was folded into MB.3.6.4 once and stayed there: a --learn rewrite that silently deletes a [route.static]/[route.php] block costs a user their config. EXISTING bug -- expected_routes drops static_policy TODAY; it would drop [route.php] as soon as that block exists (it does, MB.3.5.4). Paths chosen so the round-trip test and the CLI-level check can both live here.

## 2026-11-14T23:17 [lead]

edited fields

## 2026-11-14T23:17 [lead]

lead: paths corrected before dispatch — there is no crates/blastit/src/edit.rs; expected_routes/apply_edits/write_atomic live in crates/blastit-manifest/src/edit.rs, and the existing protected suite is crates/blastit-manifest/tests/critical_manifest_edit.rs. New round-trip test goes in a NEW critical_manifest_edit_*.rs file, not in that suite.

## 2026-11-15T06:27 [lead]

status ready -> active

## 2026-11-15T06:27 [lead]

attempt 1/3 started

## 2026-11-15T06:27 [lead]

base 59dd162

## 2026-11-15T06:36 [lead]

status active -> review

## 2026-11-15T06:43 [lead]

REVIEW FAIL (attempt 1), findings to close in attempt 2:
MAJOR 1: NODE.md's acceptance command `cargo test -p blastit --test edit_roundtrip` names a target that does not exist (exit 101: no test target named edit_roundtrip), and RESULT never reports it. The real command is `cargo test -p blastit-manifest --test critical_manifest_edit_executor --locked` — this LOG entry is the correction of record; the RESULT must state the command actually run and never the stale one.
MAJOR 2: the "user-visible level" acceptance bullet is NOT met, and the claim that it cannot be met offline is WRONG. `dev --learn` IS reachable offline: `mod common`, Dev + MockPgServer, a WSGI app whose route carries `[route.static] root = "public"` plus `[route.db]` (legal per validate.rs:519-520), request the ungranted SQL path, SIGTERM, then assert the static block lines are byte-identical while the learned query landed. Add `crates/blastit/tests/critical_learn_executor_blocks.rs` for exactly that; it runs in `make check` (rs-test-critical adds --features python) and is the only thing that covers `supervise.rs:889-899`.
MINOR: `critical_manifest_edit_executor.rs:306-315`'s "never written" assertion is tautological (no writer is invoked) — drive `LearnRun::finish`/`write_atomic` for the refused input, or keep only the `EditError::Source` check.
MINOR: RESULT.md:11-15 says the bug "would drop the blocks ... reports success"; this revision does not — it fails closed with exit 1. State the observed pre-fix behaviour (the refusal) in the Summary.

## 2026-11-15T06:43 [lead]

status review -> ready

## 2026-11-15T06:43 [lead]

status ready -> active

## 2026-11-15T06:43 [lead]

attempt 2/3 started

## 2026-11-15T06:48 [lead]

status active -> review

## 2026-11-15T06:54 [lead]

status review -> done
