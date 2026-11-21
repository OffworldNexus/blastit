# Log MB.3.31

## 2026-11-14T20:33 [planner]

created (task) Purge the retired PHP transport vocabulary from live plan briefs (one-and-only-one-way invariant)

## 2026-11-14T20:33 [lead]

RECONCILIATION (owner 2026-11-14): the earlier owner-dispatched planner stood down WITHOUT touching anything (no edits, no commit). Its read-only survey is the target set: 63 nodes match the retired tokens across 94 files -- 18 done, 13 superseded, 30 ready, 1 proposed, 1 active. So this pass is the 30 ready + MB.3.20 (proposed) + MB.3.28 (active, becomes done); the 31 done/superseded are EXEMPT and untouched (no banners unless the owner asks -- the guard skips them anyway). Guard id is MB.3.30, purge id is MB.3.31 (authoritative). HEAD note: this worktree also carries 2ad39c3 (owner/strategist VISION-ROADMAP+ME), which no node here edits.

## 2026-11-14T20:33 [lead]

OWNER 2026-11-14: add the HISTORICAL banner to done/superseded nodes that PRESCRIBE the retired mechanism (not every token match; MB.3.1/.2/.3/.4*/.5.1/.6.1-.6.3*/.9.1-.9.5 are the named set). Exact line in the body; skip attack-class-only nodes and MB.3.17 (the removal record is still current). Substance untouched; report banner count and skip reasons.

## 2026-11-14T20:37 [lead]

OWNER clarification: also purge 'retired key/legacy/deprecated/migration' framing about PHP from live briefs (nothing was ever released). Banner text stays verbatim; attack-class stays.

## 2026-11-14T21:04 [lead]

ADDITION: live briefs that cite docs/design/php-fastcgi.md (e.g. MB.9.2.2 Context) must be repointed to docs/design/backend-runtimes.md §4.2 + ADR-0009, because MB.3.32 deletes that file; the guard's crates dead-link scan covers code, and this purge covers plan briefs.

## 2026-11-14T22:14 [lead]

status ready -> active

## 2026-11-14T22:14 [lead]

attempt 1/3 started

## 2026-11-14T22:14 [lead]

base 136f2d376089ac581de7794c08ea8689f42336c3

## 2026-11-14T22:20 [lead]

status active -> review

## 2026-11-14T22:22 [lead]

REVIEW FAIL (attempt 1): (1) MAJOR MB.3.5.4 is done but still prescribes the retired fallback (lines 29,48,80) and was skipped for the banner; (2) MAJOR MB.3.8:63,66,79 invents an ADR-0010 amendment of ADR-0007 and gates on its banner; (3) MINOR MB.3.6 gate/paths name critical_php_dispatch.rs/conformance_php.rs with no live owner; (4) MINOR MB.3.22 item 5 was softened, leaving MB.3.5.4:61 stale; (5) MINOR MB.3.8:53-60 adds new technical content. [6]=not a finding.

## 2026-11-14T22:22 [lead]

status review -> ready

## 2026-11-14T22:22 [lead]

status ready -> active

## 2026-11-14T22:22 [lead]

attempt 2/3 started

## 2026-11-14T22:24 [lead]

REVIEW FAIL (attempt 2): (4) MB.3.22 item 5 orders deletion of MB.3.5.4:61, which is the LIVE [route.php] entry/workers acceptance; the stale clause it describes no longer exists (replaced by the unknown-key bullet at :63). MINOR: MB.3.8:60-62 should cite ADR-0010 s1 AND s2; RESULT:77 stale line numbers; RESULT:154 wrong status wording.

## 2026-11-14T22:24 [lead]

status active -> ready

## 2026-11-14T22:24 [lead]

status ready -> active

## 2026-11-14T22:24 [lead]

attempt 3/3 started

## 2026-11-14T22:26 [lead]

status active -> done

## 2026-11-14T22:29 [lead]

commit 66fccd5 (plan-only, 33 live briefs + 27 banners)
