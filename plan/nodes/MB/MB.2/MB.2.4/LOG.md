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
