# Log MB.21.5.1

## 2026-11-16T00:38 [planner]

created (task) Rewrite the documentation site and README for Blastfile and retire reference/manifest.md

## 2026-11-16T01:49 [planner]

edited fields

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T03:50 [conductor]

Confirmed H-011 diagnostic wording from MB.21.3.1: 'the file is not a regular file; Blastfile must be a plain file' and 'the file could not be read: <os sentence>'.

## 2026-11-16T21:45 [lead]

status ready -> active

## 2026-11-16T21:45 [lead]

attempt 1/3 started

## 2026-11-16T21:45 [lead]

base 1f3ffc83f26510c24255b21c98b91ce0c34d750a

## 2026-11-16T21:55 [lead]

edited fields

## 2026-11-16T21:57 [doc-writer]

site-wide sweep: reference/manifest.md deleted, 8 pages teach Blastfile, links repointed, 7 live briefs re-spelled, docs_fences rows moved, all docs gates green

## 2026-11-16T21:57 [lead]

status active -> review

## 2026-11-16T22:00 [lead]

REVIEW FAIL (attempt 1): ARCHITECTURE.md:223 still teaches 'inline grants, its groups and the global groups'; :708 Mermaid repeats it. MINOR plan/nodes/MB/NODE.md:55 archaic 'queries = []'. Reviewer also found out-of-paths Rust doc comments still naming [route.ingress]/[route.db] (blastit-ingress/src/lib.rs:4,16,85,99,101; blastit-dbproxy/tests/postgres.rs:673) -> needs an owner after this node

## 2026-11-16T22:01 [doc-writer]

review fix: ARCHITECTURE group-model claims re-spelled as traits (223, 708); MB brief lines 20/73/78 de-retired

## 2026-11-16T22:01 [lead]

attempt 2/3 started

## 2026-11-16T22:01 [lead]

status review -> review

## 2026-11-16T22:04 [doc-writer]

review pass 2: whole-file ARCHITECTURE audit — 16 retired-model rows fixed (routing path=, php workers/max_requests, manifest-order, proxy/group prose); 9 justified hits remain

## 2026-11-16T22:04 [doc-writer]

audit count: 8 passages / 15 retired-model token hits fixed; 9 hits justified as product vocabulary (worker processes, process.retired max_requests, report fix.allow_raw)

## 2026-11-16T22:05 [lead]

attempt 3/3 started

## 2026-11-16T22:05 [lead]

status review -> review

## 2026-11-16T22:08 [lead]

status review -> done

## 2026-11-16T22:11 [lead]

commit 809143d (+3452a7a for the ME brief)
