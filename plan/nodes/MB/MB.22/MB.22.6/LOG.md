# Log MB.22.6

## 2026-11-16T09:16 [planner]

created (task) Audit and mutation-test blastit lsp, the manifest spans, the grammar queries and the editors guard (test-curator)

## 2026-11-16T09:16 [planner]

edited fields

## 2026-11-16T09:16 [planner]

edited fields

## 2026-11-17T07:02 [lead]

status ready -> active

## 2026-11-17T07:02 [lead]

attempt 1/3 started

## 2026-11-17T07:02 [lead]

base d08d04c

## 2026-11-17T07:21 [lead]

Reviewed the audit in place: it is complete (verdict, requirements matrix, seven proved regressions, mutation scope, protected-suite comparison, gap nodes MB.22.7..MB.22.11). Marking done; the five gap children stay proposed for post-0.2.0.

## 2026-11-17T07:21 [lead]

status active -> done

## 2026-11-17T07:23 [test-curator]

Audit written to docs/testing/AUDIT-MB.22.md (base d08d04c). Tier PASS, teeth PASS (6/7 regressions red; grammar.js drift is gap MB.22.7), mutation PASS 25/27 tested = 92.6% (error.rs 94.4%, sites.rs 87.5%, mod.rs 100%; cargo-mutants campaigns killed by host load, 11 hand-applied mutants named in §3.4). Gaps filed: MB.22.7 grammar drift, MB.22.8 cli.md lsp rows, MB.22.9 S10 capability refusal, MB.22.10 critical_guard base, MB.22.11 guard negative twins, MB.22.12 endpoint-level body raw site. No test, crate, script or SCM file edited; tree clean.
