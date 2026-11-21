# Log MB.2.4

## 2026-11-13T15:11 [planner]

created (task) Resolve and serve assets safely beneath the root (openat2, NUL, symlink, dotfiles, immutable)

## 2026-11-13T20:44 [lead]

edited fields

## 2026-11-13T20:44 [lead]

edited fields

## 2026-11-13T20:52 [planner]

edited fields

## 2026-11-13T20:52 [planner]

Split into MB.2.4.1 (resolve: segments/classify/StaticRoot open+resolve/openat2+Walk/errno table/TOCTOU identity, config fields, tests/critical_static_resolve.rs) and MB.2.4.2 (respond: ETag/304/cache rule/Content-Type/http_date/bounded body in a new static_response.rs, plus the one 36-row critical_static_assets.rs matrix). Design docs/design/static-and-spa.md §8 names the halves MB.2.4a/b; plan IDs are numeric because tools/plan.py's ID_RE rejects a letter suffix. This node is now an epic gate; its paths are the union of the two children so make critical-guard NODE=MB.2.4 still authorizes the new protected suites. Order: MB.2.4.1 then MB.2.5.1 then MB.2.4.2 (the matrix asserts statuses through rewrite::decide, so the design's 2.4b-before-2.5a sequence would have forced a duplicate status mapping in the test).

## 2026-11-13T22:29 [lead]

GATE EVIDENCE (lead, at b8e8806): cargo test -p blastit-scheduler --test critical_static_assets -> 11 passed; cargo test -p blastit-scheduler --locked -> all suites ok (18/14/14/1/11). critical-guard with the full authorizing node set (MB.1.6 MB.1.8 MB.1.12 MB.1.13 MB.1.14 MB.1.15 MB.1.16 MB.2.4.1 MB.2.4.2, --base origin/develop) -> 'OK: all 10 protected change(s) are authorized'; the acceptance block's narrow 'BASE=develop NODE=MB.2.4' cannot pass because the branch carries protected edits made by those MB.1 nodes before this epic started. test-curator audit of blastit-scheduler to follow before the epic closes.

## 2026-11-13T22:44 [lead]

GUARD RULING (owner): the branch-wide 'critical-guard BASE=develop NODE=<one epic>' line cannot pass once several nodes legitimately edited protected files; the guard's contract is 'these nodes authorize these paths', so the gate invocation uses the FULL authorizing node set for the branch: --base origin/develop --node MB.1.6 MB.1.8 MB.1.12 MB.1.13 MB.1.14 MB.1.15 MB.1.16 MB.2.4.1 MB.2.4.2 -> 'OK: all 10 protected change(s) are authorized'. Recorded here so no later reader thinks the check was dodged. Not weakened, not skipped.

## 2026-11-14T00:32 [lead]

status ready -> done
