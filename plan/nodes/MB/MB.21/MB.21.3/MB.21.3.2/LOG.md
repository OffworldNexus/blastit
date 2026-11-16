# Log MB.21.3.2

## 2026-11-16T00:38 [planner]

created (task) LL(1) Blastfile parser producing the existing Manifest, with the v1 model-equality oracle

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T03:49 [lead]

edited fields

## 2026-11-16T03:56 [lead]

status ready -> active

## 2026-11-16T03:56 [lead]

attempt 1/3 started

## 2026-11-16T03:56 [lead]

base 58c8ed9760ab3bd4510cc8edc96e6ab333dea405

## 2026-11-16T04:25 [lead]

Attempt 1 done. Deviations reviewed by the lead and accepted: D1 one catch-all "expected …, found <class>" sentence for the non-word residue (H-012 for the owner; it never echoes a value) -- the parser must totalise somehow and §18 pins no row; D2 both regex sentences, split on the following token; D3/D4 unknown-trait and trait-level unrestricted belong to lower/validate by §11.2/§11.3; D5 an empty entrypoint-python block and a second preload are accepted because §6.7 pins no sentence; D6 the depth counter is proved by driving open_block (a third block is unreachable through the grammar); D7 size overrun accepted (37 documented variants + 21 productions as tests + 32 refusal rows).

## 2026-11-16T04:25 [lead]

status active -> review

## 2026-11-16T04:31 [lead]

REVIEW FAIL (attempt 1): 2 MAJOR, both test/pinned-sentence gaps, plus 5 MINOR. MAJORa: a lower-case method inside a view block yields the generic "not a directive here" sentence where §6.3 pins "`get` is not a method: methods are upper-case, `GET`", and no test covers that position. MAJORb: the duplicate rows' "the first is on line <l>" slot is only ever asserted with the first declaration on line 1, so the slot is unproven. MINOR: RESULT evidence arithmetic; BlockTooDeep sentence differs between §4.5 and §6.7 row 26 (lead ruling: §4.5 wins -- it owns the budget rows -- and the design inconsistency is an epic follow-up); §6.3 prose vs its table on `regex` as a name (table wins); syntax.rs's every_element_is_visited_once asserts thresholds, not equality; §20/§21 assign critical_blastfile_parser.rs to 3.2 but it is 3.8's (already fixed by widening 3.8's paths).

## 2026-11-16T04:31 [lead]

status review -> ready

## 2026-11-16T04:31 [lead]

status ready -> active

## 2026-11-16T04:31 [lead]

attempt 2/3 started

## 2026-11-16T04:31 [lead]

base 58c8ed9760ab3bd4510cc8edc96e6ab333dea405 (attempt 2; no commits since)

## 2026-11-16T04:38 [lead]

status active -> review

## 2026-11-16T04:38 [lead]

attempt 2 reported done; re-review (fresh reviewer, base 58c8ed9) on both MAJORs and a regression sweep.

## 2026-11-16T04:45 [lead]

edited fields

## 2026-11-16T04:45 [lead]

reviewer verdict PASS (attempt 2): both MAJORs verified closed, the keyword split pinned both ways. 3 MINOR: (1) top-level `root`/`body`/... answer "is not a keyword" where §6.7 row 3 pins "not a directive here"; (2) `is_keyword` omits `unrestricted`/`any`/`json`/`form`/`none`/`raw`/`unvalidated`, so `view v { json }` answers the method sentence -- both handed to MB.21.3.8, whose acceptance already requires every §6.7 row through the public entry point (parser.rs is in its paths); (3) LESSONS.md added to paths.

## 2026-11-16T04:45 [lead]

status review -> done

## 2026-11-16T04:47 [lead]

commit d230370
