# Log MB.2.3

## 2026-11-13T15:11 [planner]

created (task) Parse and validate [static] route blocks in blastit-manifest

## 2026-11-13T21:23 [lead]

status ready -> active

## 2026-11-13T21:23 [lead]

attempt 1/3 started

## 2026-11-13T21:23 [lead]

base 31c9739

## 2026-11-13T21:32 [implementer]

Landed [route.static] grammar (StaticPolicy, Route::static_policy, Manifest::{is_all_static,static_roots,check_static_roots}, five exact errors, dead-declaration rule), new protected suite (15 tests, every twin) + docs_fences 7 with a shape canary + reference entry (app now no*). Deviations named in RESULT: is_all_static follows R13 (not §4.1's loose comment), no index-existence/hashed-pattern/mixed-app denials (design forbids them), 615 production+docs lines. Follow-ups: tests/parse.rs ROUTE_KEY_EXPECTED omits static; MB.2.2 reconcile is_all_static wording; critical-guard BASE=develop trips on pre-existing MB.1 files (passes at 31c9739).

## 2026-11-13T21:32 [lead]

edited fields

## 2026-11-13T21:32 [lead]

lead extended paths with crates/blastit-manifest/tests/parse.rs: its ROUTE_KEY_EXPECTED message must now name the reserved 'static' key (the node's own follow-up).

## 2026-11-13T21:33 [lead]

status active -> review

## 2026-11-13T21:40 [lead]

REVIEW PASS attempt 1 (4 MINOR: R4 inherited-groups accepted case missing; R4 dead-declaration vs BLASTIT_APP precedence -> reconcile in MB.2.6/MB.2.9; [route.static] groups hint not actionable; NODE acceptance critical-guard line should use BASE=<node base> not develop).

## 2026-11-13T21:40 [lead]

status review -> done
