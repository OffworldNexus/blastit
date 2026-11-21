# Log MB.2.6

## 2026-11-13T15:11 [planner]

created (task) Wire static routes into blastit dev and blastit check diagnostics

## 2026-11-13T20:16 [lead]

acceptance extended at the MB.2.1 gate: own the all-static check path (no pytest, root audit, exit 0/2) promised by docs/frameworks/static-and-spa.md

## 2026-11-13T20:44 [lead]

edited fields

## 2026-11-13T20:44 [lead]

edited fields

## 2026-11-13T20:52 [planner]

edited fields

## 2026-11-13T23:01 [lead]

status ready -> active

## 2026-11-13T23:01 [lead]

attempt 1/3 started

## 2026-11-13T23:01 [lead]

base cb7a1e4

## 2026-11-13T23:09 [implementer]

app optional iff is_all_static in dev+blastit (MB.1.6 order kept); check_static_roots + responder-uid probe before listeners; all-static check audits roots, no pytest, exit 0/2, v1 --json unchanged, --learn exit 2; mixed-tree static root never requested = one stderr line not a finding; new protected critical_static_cli.rs (8 cases, mutation-proven); banner left as the page/design §1.2 requires (follow-up)

## 2026-11-13T23:21 [implementer]

attempt 2: R13 step-5 probe asks the kernel in-process when probe==geteuid (dev/check) so an owned 0700 $HOME no longer refuses a readable root; production keeps the dropped-to probe. New tests: 0700-ancestor accepted (mutation-proven), chmod 000 refused, file-shaped root refused, check names the route, all three banner lines, BLASTIT_APP on an all-static manifest refused by name (app_rule). 12 cases green; clippy/doc/fmt/guard clean

## 2026-11-13T23:21 [lead]

status active -> review

## 2026-11-13T23:35 [implementer]

attempt 3 (gate regression): blastit check never required an app — split the R13 rule: static_app_refusal (all-static + set BLASTIT_APP refused by name) is check's only half, app_rule (adds 'required when not all-static') is the boots'. Removed the test that encoded the regression, added check_without_an_app_still_runs_the_suite (mixed manifest, no app, no PATH -> reaches the pytest preflight, never names BLASTIT_APP) alongside the existing all-static refusal test. make check exit 0 (Ran 50 python critical tests OK, 47 fences 0 failed, plan OK); critical_guard's only violation is MB.2.4.3's in-flight critical_static_resolve.rs

## 2026-11-13T23:35 [lead]

GATE: make check caught a regression the review could not (18 python contract failures: check demanded BLASTIT_APP). Fixed by splitting app_rule (boots) from static_app_refusal (check). make check exit 0, 'Ran 50 tests ... OK'. Reviewer PASS attempt 2; lead gate PASS.

## 2026-11-13T23:35 [lead]

status review -> done
