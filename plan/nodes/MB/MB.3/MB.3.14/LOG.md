# Log MB.3.14

## 2026-11-14T14:43 [lead]

created (task) Record the design-doc collision in LESSONS: one writer per design doc, incremental .part writes, dispatch order

## 2026-11-14T14:43 [lead]

status ready -> active

## 2026-11-14T14:43 [lead]

attempt 1/3 started

## 2026-11-14T14:43 [lead]

base 4cd727b

## 2026-11-14T14:52 [lead]

status active -> done

## 2026-11-14T14:52 [lead]

bullet is IN HEAD (verified: git show HEAD:docs/knowledge/LESSONS.md contains it) but it landed inside dc05a48 [MB.15] -- another agent's commit swept my working-tree bullet while staging docs/** broad. Content correct (+2/-0). Plan bookkeeping (this node's RESULT/STATE) stays uncommitted until MB.3.11 settles: make check must not run against the implementer's mid-edit crates/**, which is exactly how the earlier red gate happened.

## 2026-11-14T14:52 [lead]

status done -> ready

## 2026-11-14T14:52 [lead]

status ready -> active

## 2026-11-14T14:52 [lead]

attempt 2/3 started

## 2026-11-14T15:30 [lead]

status active -> ready

## 2026-11-14T15:30 [lead]

status ready -> active

## 2026-11-14T15:30 [lead]

attempt 3/3 started

## 2026-11-14T15:31 [lead]

attempt 3: two more bullets appended from MB.3.12 (php_execute_script swallows the bailout -> false is the signal; CLOCK_PROCESS_CPUTIME_ID is tick-granular -> a trivial fixture can measure 0 ns, so cpu_ns>0 flakes on fast hosts). NODE.md body rewritten to describe all four bullets (the node previously described only the design-doc one). docs-check green, plan OK; full make check deferred to MB.3.12's commit which covers the same tree.

## 2026-11-14T15:31 [lead]

status active -> done

## 2026-11-14T15:52 [lead]

status done -> ready

## 2026-11-14T15:52 [lead]

status ready -> active

## 2026-11-14T15:52 [lead]

attempt 4/3 started

## 2026-11-14T15:52 [lead]

status active -> done

## 2026-11-14T15:52 [lead]

attempt 4 refused by the attempt cap (4/3, ESCALATE) -- correctly: a one-bullet docs addition is not worth an expensive-tier escalation. The third trap is carried by MB.3.17 instead (docs/knowledge/LESSONS.md is in its paths and it already appends a LESSONS sentence).
