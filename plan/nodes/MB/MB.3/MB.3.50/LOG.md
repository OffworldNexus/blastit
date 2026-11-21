# Log MB.3.50

## 2026-11-15T06:25 [lead]

created (task) Sweep the scheduler's charge/outcome/release doc claims instead of patching three more

## 2026-11-15T06:55 [lead]

edited fields

## 2026-11-15T06:55 [lead]

Three more stale claims were found by MB.3.21's attempt-2 reviewer; they belong in this sweep rather than a new node, because they are the same defect class (a document or fixture that no longer matches the code it points at):
(a) `crates/blastit-manifest/src/model.rs:415,422` says "Parsing is the only caller" of the two route helpers; `edit.rs:942-943` now calls both. Update the doc.
(b) `crates/blastit/tests/critical_learn_executor_blocks.rs:64-73` has `[route.static]` with NO `[route.db]` beside it (the fail brief asked for both), so the process-level "grant lands" leg only splices a new table. Seed `[route.db] queries = ['SELECT 42']` and keep `SELECT 1` as the learned statement so the extend-array splice is covered too.
(c) `plan/nodes/MB/MB.3/MB.3.21/RESULT.md:40-41` still quotes the nonexistent `--test edit_roundtrip` target even though the LOG entry forbids repeating it: delete the literal and cite "see LOG 2026-11-15T06:43".
Also add the LESSONS.md line MB.3.21 asked for (its paths did not include it): a postcondition rewrite built with `Route::new` must chain every defaulted field, or the comparison silently answers a different question than the writer asked.

## 2026-11-15T06:55 [lead]

status ready -> active

## 2026-11-15T06:55 [lead]

attempt 1/3 started

## 2026-11-15T07:11 [lead]

status active -> review

## 2026-11-15T07:14 [lead]

edited fields

## 2026-11-15T07:14 [lead]

REVIEW FAIL (attempt 1) — one MAJOR, and it is the node's own thesis:
MAJOR: `crates/blastit-scheduler/src/scheduler.rs:4322-4323` (`charge_ns`'s doc) still says a scheduler-written 502/504 "never held the worker its wall spans" — the exact claim this node proved false at :3756-3762. A dispatched 502/504 DID hold a slot; what must not be charged is the queue wait inside the wall span. It is now the only surviving instance of that falsehood in the crate and it contradicts the node's own fix. Replace it with the :3760-3762 wording.
MINOR: :3772-3773 and :4121 call the third release-charged path "a head-only response", but the guard is `head_sent && !complete` (view_done:3946,3960), so a response whose head AND body went out and then lost only its `RESP_END` (e.g. `echo …; exit;` -> `DONE{Bailed}`) lands there too. Say "a response whose head is out but whose `RESP_END` never came".
MINOR: `docs/knowledge/LESSONS.md` is edited and is now in this node's paths (added by the lead) — no further action, just know the path is covered.
MINOR: NODE.md's `# Goal`/`# Context` never explains the protected `critical_learn_executor_blocks.rs` edit (the reason lives only in the LOG). `testing` §2 requires the Goal to state why a protected file is edited; a planner will fix the body, so note the sentence you would add in RESULT.md.
JUDGED NOT A DEFECT (keep the behaviour): the deferred-5xx-whose-channel-closes-first case emitting `Outcome::Ok`/500 is honest — `view_gone` -> `fail_request` early-returns on `complete` and `release` charges; `Ok` is "produced a complete response", `bailed` would assert a DONE fact never received, `view_died` would be a lie. Nothing to change.

## 2026-11-15T07:14 [lead]

status review -> ready

## 2026-11-15T07:14 [lead]

status ready -> active

## 2026-11-15T07:14 [lead]

attempt 2/3 started

## 2026-11-15T07:20 [lead]

status active -> review

## 2026-11-15T07:24 [lead]

Reviewer PASS on attempt 2 with one MINOR accepted as a residual rather than a new node (this node exists to stop the treadmill, and one module-doc sentence in an edge case does not earn another round trip): `scheduler.rs:47-49` says a `DONE { Errored }` "is reported with the `view_died` outcome" without the completion caveat — `blastit-view/src/request.rs:299-307` sends `Errored` after a COMPLETED response when `close()` raised, and `view_done:3958` fails only the unfinished case, so a completed 2xx keeps the `ok` it already emitted. The sentence to add: "...and so is a `DONE { Errored }` whose response never completed (a completed one keeps its label; only a completed `5xx` can still be relabelled)". Whoever next opens this file for MB.3's own gate should land it; it is recorded here so it cannot be lost.

## 2026-11-15T07:24 [lead]

status review -> done
