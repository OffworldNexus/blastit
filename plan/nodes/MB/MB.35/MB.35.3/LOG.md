# Log MB.35.3

## 2026-11-17T13:17 [planner]

created (task) Amend ADR-0013 and design 8.3: blastit-lsp crate boundary, portability invariant, three-step resolution and the five-target matrix table

## 2026-11-18T05:52 [lead]

status ready -> active

## 2026-11-18T05:52 [lead]

attempt 1/3 started

## 2026-11-18T05:52 [lead]

base 143afaf

## 2026-11-18T06:31 [lead]

status active -> review

## 2026-11-18T06:31 [lead]

owner done: 4 artefacts (ADR-0013 amendment +208/-3, ADR-0010 lsp family row, design §1/§8.3a/§11/§14 +457/-8, ARCHITECTURE §2.7 15-row table); 15 critic reports across 4 lenses, 0 BLOCKER open after round 2, every MAJOR resolved or handed to a named leaf; platform-matrix ok, packaging-gates 49/49. Deviation: critical_lsp.rs keeps assertions but 4 include_str! literals move.

## 2026-11-18T06:51 [lead]

REVIEW FAIL (attempt 1): MAJOR ADR-0013 Consequences :84/:94 still assert the two premises the amendment reverses and the amendment does not name them as superseded; MAJOR the ledger counts are unreproducible (implementability r1 2/5/5 not 4/9/15; testability r1 2/6/5 not 3/18/12, and :3061 cites M1-M13 which do not exist). MINOR: platform_matrix CLAIM walk still scans *.review-*.md (root cause of the earlier gate red) — handed to MB.35.5, which owns that script; ADR-0013 label; ARCHITECTURE dbproxy row.

## 2026-11-18T06:51 [lead]

status review -> ready

## 2026-11-18T06:51 [lead]

attempt 2/3 started

## 2026-11-18T07:02 [lead]

reviewer 2: VERDICT PASS. Five fixes verified on disk; packaging-gates now 54 cases (MB.35.5 added 5); one MINOR — tokio-postgres is a dev-dep and must not sit in the "can be built where" column (fixing).

## 2026-11-18T07:06 [lead]

status ready -> done

## 2026-11-18T07:06 [lead]

committed ef93363
