# Log MB.2.10

## 2026-11-13T22:13 [lead]

created (task) Sync the static design with the shipped API surface and status rules

## 2026-11-13T22:13 [lead]

status ready -> active

## 2026-11-13T22:13 [lead]

attempt 1/3 started

## 2026-11-13T22:13 [lead]

base d37f765

## 2026-11-13T22:33 [designer]

Synced docs/design/static-and-spa.md + ADR-0007 with shipped code (6 findings): §4.2 head_segments/declares_body/STATIC_ALLOW + MB.2.4.2 surface; §4.4 refusal terminal for every method (400/404, never 405/413); §8 shared 1,600-line critical_rewrite ceiling; §5.2 F/N no-cache cells; §7.1-§7.2 24-case response matrix ownership. docs-check + docs-unit green; MB.2.5.2 brief resync is Follow-up 1.

## 2026-11-13T22:34 [lead]

status active -> review

## 2026-11-13T22:40 [designer]

Attempt 2: re-read the three suites and replaced the wrong oracle map. critical_static_assets.rs (11 tests) owns 24 200 cases (numbered rows 18,28,30,32-35,37,40) + one decide status test (400 for 1/2/6/7; 404 for 15/16/22/25/26); critical_static_resolve.rs (14) owns rows 1-40 over both backends; critical_rewrite.rs (13) owns rows 1-13 400 (GET/HEAD/POST), 15-17/20-26/39 404 (GET/POST), miss rows 14/19/27/29/31/38, row 37 and RespAbort. Also: 24 cases are all 200 (statuses are a separate test); canned(405) wart no longer pinned (MB.2.5.2 owns the fix). docs-check + docs-unit green.

## 2026-11-13T22:40 [lead]

attempt 2/3 started

## 2026-11-13T22:40 [lead]

status review -> review

## 2026-11-13T22:43 [designer]

Attempt 3: four mechanical corrections to the oracle map. Row 20 is 400 (not 404) in R5/R6/§9; R7 negative is outcome-only (no status); fixture-D cache clause has / and /dashboard only (/docs is an S case); shared critical_rewrite.rs is 1,365 lines (not 1,353). docs-check 0.33s No issues; docs-unit 63 passed, 57 subtests.

## 2026-11-13T22:43 [lead]

attempt 3/3 started

## 2026-11-13T22:43 [lead]

status review -> review

## 2026-11-13T23:01 [lead]

status review -> done
