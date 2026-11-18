# Log MB.36.6

## 2026-11-17T14:07 [planner]

created (task) Finish the Cargo feature matrix (default php+static, wheel python+php+static), teach the guards the new members, and rewrite ARCHITECTURE.md's crate map while proving no libpython/libphp DT_NEEDED

## 2026-11-17T15:57 [planner]

edited fields

## 2026-11-18T12:34 [lead]

edited fields

## 2026-11-18T12:55 [lead]

edited fields

## 2026-11-18T13:15 [lead]

edited fields

## 2026-11-18T14:24 [lead]

design correction owed here: `docs/design/view-crates-architecture.md` §4.4 D1 predicts "exactly two `Privileges::Shared` (the dev and check boots)"; MB.36.4 measured ONE, because dev and check share stage1.rs's single `dev.is_none()` branch (line ~394). Fix the design sentence to "one, serving both `dev` and `check`" so the census in the record matches the grep.

## 2026-11-18T14:34 [lead]

CORRECTION to the D1 census note above (reviewer, MB.36.4 round 1): `check` never boots stage 1 and forks no view, so the design sentence must read "one `Privileges::Shared` — stage 1's dev branch only (`check` constructs neither) — and one `Filter::Off`", not "serving both dev and check".

## 2026-11-18T16:17 [lead]

edited fields

## 2026-11-18T16:17 [lead]

status ready -> active

## 2026-11-18T16:17 [lead]

attempt 1/3 started

## 2026-11-18T16:17 [lead]

attempt 1/3 started; base b42a6c9. Paths widened with crates/blastit-view-php/src/discovery.rs (the duplicated-noun refusal sentence MB.36.5 recorded) and crates/blastit/src/stage0.rs (the `no_php_support` sentence the php-less build prints).

## 2026-11-18T16:38 [lead]

status active -> review

## 2026-11-18T16:52 [lead]

REVIEW FAIL (round 1): MAJOR - ARCHITECTURE.md:358 claims the static transport is blastit-view, but the scheduler uses only blastit_view::path and the head leaves as blastit-frame RespHead/RespBody through its own Outbound. MINORs: archive README omits static; static-and-spa.md still points the mutation audit at the scheduler; build-standalone.sh refuses pyo3 but nothing refuses a dropped static; stale census line numbers; two stale "blastit-view still defaults python" comments outside paths.

## 2026-11-18T16:52 [lead]

status review -> ready

## 2026-11-18T16:52 [lead]

status ready -> active

## 2026-11-18T16:52 [lead]

attempt 2/3 started

## 2026-11-18T17:14 [lead]

status active -> done
