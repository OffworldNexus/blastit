# Log MB.34.6

## 2026-11-17T13:17 [planner]

created (task) Tree-sitter: tokenize query SQL (keywords, $?/$n, ...), capture it in highlights.scm, regenerate parser.c and re-pin the Zed grammar rev

## 2026-11-18T01:25 [lead]

edited fields

## 2026-11-18T04:11 [lead]

status ready -> active

## 2026-11-18T04:11 [lead]

attempt 1/3 started

## 2026-11-18T04:11 [lead]

base 5f293985cc355b29dbac08421482b8ea84d1c677

## 2026-11-18T04:24 [lead]

edited fields

## 2026-11-18T04:24 [lead]

owner done: sql_string + 7 children, 'query' out of , 33/33 corpus, 23 Blastfiles parse with 0 ERROR/MISSING as a permanent guard, 46 properties / 23 self-tests. Two-step commit is the lead's. Path added: editors/tree-sitter-blastfile/highlights.scm (stale '…' claim).

## 2026-11-18T04:24 [lead]

edited fields

## 2026-11-18T04:24 [lead]

owner done: sql_string + 7 children; "query" removed from keyword; 33/33 corpus; 23 Blastfiles parse with 0 ERROR/MISSING as a permanent guard; 46 properties / 23 self-tests. Two-step commit is the lead job. Path added: editors/tree-sitter-blastfile/highlights.scm (stale "..." claim about raw strings covering query).

## 2026-11-18T04:27 [lead]

status active -> review

## 2026-11-18T04:27 [lead]

follow-up done: grammar highlights.scm now mirrors Zed's five SQL patterns; new guard property 'highlights: copies agree' (one-directional) with a negative twin; 47 properties / 24 tests. Reviewer dispatched (two-step commit stays with the lead).

## 2026-11-18T04:39 [lead]

REVIEW FAIL (attempt 1): MAJOR - query_decl requires a value, so `functions query` and `select query` (both legal, fmt-canonical) now ERROR; the base grammar split them into two statements. MINOR - the 23-file loop silently notes+passes without the pinned binary; assert_red does not prove "only its own property". Reviewer supplied the verified fix.

## 2026-11-18T04:39 [lead]

status review -> ready

## 2026-11-18T04:39 [lead]

attempt 2/3 started

## 2026-11-18T05:02 [lead]

committed 5bbd244 (A) + 0e79aba (B)

## 2026-11-18T05:02 [lead]

status ready -> done
