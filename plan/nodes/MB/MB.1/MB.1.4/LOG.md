# Log MB.1.4

## 2026-11-13T14:44 [planner]

created (task) Measure blastit-sqlpolicy normalize, literals and lru mutants

## 2026-11-13T16:16 [lead]

status ready -> active

## 2026-11-13T16:16 [lead]

attempt 1/3 started

## 2026-11-13T16:16 [lead]

base 9311ff3

## 2026-11-13T16:38 [implementer]

file-scoped campaign: 128 mutants, 110 caught / 1 equivalent missed / 2 timeouts / 15 unviable; normalize 99/102 = 97.1 %, literals 4/4 and lru 7/7 = 100 %; new tests/policy_module_edges.rs (10) + 3 inline lru tests; no production change

## 2026-11-13T16:40 [lead]

status active -> review

## 2026-11-13T18:12 [lead]

COLLISION: lead reset the sqlpolicy spillover (src/{lru,statements,tables}.rs + tests/policy_module_{edges,tables}.rs) at ~17:00 while this node's implementer was mid-campaign; RESULT.md (128 mutants 110 caught, 1 equivalent missed, 2 timeouts, 15 unviable; normalize 99/102, literals 4/4, lru 7/7) is on disk but tests/policy_module_edges.rs and the 3 inline lru tests are NOT. Status back to ready: recreate the tests, re-verify the numbers from artefacts or re-run, and bound the 2 normalize timeouts to 0.

## 2026-11-13T18:12 [lead]

status review -> ready

## 2026-11-13T18:12 [lead]

status ready -> active

## 2026-11-13T18:12 [lead]

attempt 2/3 started

## 2026-11-13T18:12 [lead]

base 686b5b7

## 2026-11-13T18:25 [implementer]

attempt 2: 0 timeouts. Both code-region scans in normalize.rs step with enumerate+resume (no counter to mutate) => 124 mutants tested in 7m: 108 caught / 1 equivalent missed / 0 timeout / 15 unviable; normalize 97/98=99.0%, literals 4/4, lru 7/7=100%; tests/policy_module_edges.rs (10) and 3 inline lru tests recreated; make rs-lint + make rs-doc green

## 2026-11-13T18:26 [lead]

status active -> done

## 2026-11-13T18:26 [lead]

lead verification: cargo test -p blastit-sqlpolicy 224+10+25+6+10+18+8 green; artefacts caught 108 / missed 1 / timeout 0 / unviable 15 = 99.1%%; single survivor normalize.rs:629:33 documented equivalent
