# Log MB.2.2

## 2026-11-13T15:11 [planner]

created (task) Design static serving, rewrite precedence and path safety (ADR-0007)

## 2026-11-13T20:22 [lead]

status ready -> active

## 2026-11-13T20:22 [lead]

attempt 1/3 started

## 2026-11-13T20:22 [lead]

base 3ea3c59

## 2026-11-13T20:29 [designer]

Draft docs/design/static-and-spa.md (643 lines, 152 table rows) + ADR-0007 (Proposed) + both README index rows; designs the [route.static] grammar, total rewrite order, openat2/RESOLVE_BENEATH boundary, 31-row attack matrix, all-static check contract and the R1-R15 -> critical-suite oracle map. Three fresh-context critics (security/implementability/testability) spawned in parallel. docs-check + docs-unit green.

## 2026-11-13T20:43 [designer]

Design + ADR + three lens artefacts PASS after 3 passes (1 BLOCKER+15 MAJOR pass 1, 2 MAJOR+1 BLOCKER pass 2, 0 open pass 3). Settled: [route.static] grammar (5 ManifestError variants, check_static_roots separate so docs_fences stays green), 11-step total rewrite order with refusals final, openat2 RESOLVE_BENEATH|NO_SYMLINKS|NO_MAGICLINKS with O_PATH probe + (dev,ino)-checked read open + injectable Walk fallback, total errno table, 36-row attack matrix (per-fixture, exact statuses), answer-kind cache key, bounded spawn_blocking pump, app-less dev/check contract, R1-R15 oracle map, MB.2.7 fence map. 4 preconditions for the lead: MB.2.6 += stage1.rs, MB.2.4 += tests/common/static_fixture.rs, new docs leaf MB.2.9, brief re-sync. docs-check + docs-unit green.

## 2026-11-13T20:44 [lead]

status active -> review

## 2026-11-13T20:44 [lead]

lead applied its plan follow-ups: MB.2.6 paths += crates/blastit/src/stage1.rs; MB.2.4 paths += crates/blastit-scheduler/tests/common/static_fixture.rs; docs leaf MB.2.9 created (security/how-it-works/ci/cli/manifest app row). The MB.2.4/MB.2.5 a/b split is deferred to their dispatch (implementer can return needs_split).

## 2026-11-13T20:49 [lead]

status review -> ready

## 2026-11-13T20:49 [lead]

REVIEW FAIL attempt 1: 5 MAJOR + 3 MINOR. M1 design:359 'empty segments dropped' contradicts matrix rows 5-6 (/%2Fetc/passwd, //etc/passwd -> 400) -> add 'exactly one leading /' to R11+segments(). M2 design:499-503 static_reads global gate but pump_static per-stream with no re-drive pinned -> a completable request can 504. M3 design:825-828+RESULT:119 land work on MB.2.4a/b,2.5a/b which do not exist -> planner must create them first. M4 ADR:54-55 'never 403, no existence oracle' contradicts design:635-638. M5 design:786 R11 (dev,ino) TOCTOU check names no assertion -> needs a seam or an explicit mutants-only note. m6 ADR:29,61 pre-boot fork claim false for dev/check. m7 missing NFC/NFD + magic-link matrix rows. m8 RESULT cites Follow-ups 10/11 that do not exist; §8 MB.2.9 row omits docs/reference/manifest.md.

## 2026-11-13T21:16 [designer]

Attempt 2: all 8 review findings fixed + converged through 8 critic re-runs (passes 4-11), each artefact 0 BLOCKER/0 MAJOR, VERDICT: PASS. Fixes: leading-// Absolute rule in segments; bounded reads = slots<=max_workers (counter/re-drive removed); ADR admits status-only disclosure; same_inode tuples + resolve_with substitution oracle; ADR probe sentence; 40-row matrix (NFC/NFD/magic-link/alias); real ids MB.2.4.1/2.5.1/2.4.2/2.5.2 with chain and static_response.rs depth-overlap override; Forward releases its slot and re-enters the ordinary path; plan_body Err stored as body_plan_error (Serve 405/413, Forward 400); static admission gate + queue branch; stored-error releases like Reject; expire_queue for a stalled body; MB.2.9 owns manifest.md. docs-check + docs-unit green; acceptance tails in RESULT.

## 2026-11-13T21:16 [lead]

attempt 2/3 started

## 2026-11-13T21:16 [lead]

status ready -> review

## 2026-11-13T21:20 [designer]

Attempt 3: fixed the 4 remaining findings only. RESULT Plan state item 2 now records that MB.2.4.1/2.5.1/2.4.2/2.5.2 exist under epics MB.2.4/MB.2.5 with chain 2.3->2.4.1->2.5.1->2.4.2->2.5.2->2.6; added #[doc(hidden)] pub const STATIC_RESOLVE_FLAGS: ResolveFlags and named it in 5.3/7.2/8; R11 now names the resolve_with substitution test as the single owner of the identity-check call site; Follow-ups 6/7 moved under ## Follow-ups (item 7 drops the epic ordering the lead fixed). Re-anchored the three artefacts, all still VERDICT: PASS; docs-check + docs-unit green.

## 2026-11-13T21:23 [lead]

REVIEW PASS attempt 3 (1 MINOR residue: stale 'Follow-up N' refs in historical prose at RESULT:188,199,204,207 and design:684).

## 2026-11-13T21:23 [lead]

status review -> done
