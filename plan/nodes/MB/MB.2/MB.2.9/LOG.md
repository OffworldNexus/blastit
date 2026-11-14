# Log MB.2.9

## 2026-11-13T20:44 [lead]

created (task) Document the static threat list, mechanics, CLI reference and the all-static check path

## 2026-11-13T21:23 [lead]

edited fields

## 2026-11-13T21:23 [lead]

depends moved MB.2.2 -> MB.2.6: the page's two central claims (app optional for an all-static tree, check runs no pytest) become true only when MB.2.6 lands, so documenting them earlier would document fiction.

## 2026-11-13T23:44 [lead]

status ready -> active

## 2026-11-13T23:44 [lead]

attempt 1/3 started

## 2026-11-13T23:44 [lead]

base 0796921

## 2026-11-13T23:48 [doc-writer]

security/how-it-works/ci/cli/manifest pages now match the shipped static serving; docs-unit, docs-check, docs-test green (47 fences, 0 failed); no fence added so inventory.toml untouched

## 2026-11-13T23:49 [lead]

status active -> review

## 2026-11-13T23:56 [doc-writer]

attempt 2: 5 reviewer accuracy fixes (scheduler read scope, spa/app miss qualification, malformed escape + 255-byte segment, bind mounts, all-static --json/exit-2 wording); docs-unit 63 passed, docs-check clean, docs-test 0 failed twice

## 2026-11-13T23:56 [lead]

attempt 2/3 started

## 2026-11-13T23:57 [lead]

status review -> review

## 2026-11-14T00:02 [doc-writer]

attempt 3: scheduler 'cannot' half replaced with its uid+seccomp boundary (no path-prefix confinement claim); check exit-2 names the route for every filesystem refusal; docs-unit 63 passed, docs-check clean, docs-test 0 failed

## 2026-11-14T00:03 [lead]

REVIEW PASS attempt 3 (attempts 1-2 failed on the security.md scheduler row: first it denied the scheduler's read scope, then over-corrected into claiming a filesystem confinement the uid lacks; final wording is bounded by uid + socket-less seccomp, matching design 5.1).

## 2026-11-14T00:03 [lead]

status review -> done
