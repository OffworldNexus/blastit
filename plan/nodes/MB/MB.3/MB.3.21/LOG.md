# Log MB.3.21

## 2026-11-14T16:36 [lead]

created (task) check --learn must not drop a route's executor block: edit.rs expected_routes loses [route.static] today and [route.php] next

## 2026-11-14T16:36 [lead]

created on owner instruction (2026-11-14) after the finding was folded into MB.3.6.4 once and stayed there: a --learn rewrite that silently deletes a [route.static]/[route.php] block costs a user their config. EXISTING bug -- expected_routes drops static_policy TODAY; it would drop [route.php] as soon as that block exists (it does, MB.3.5.4). Paths chosen so the round-trip test and the CLI-level check can both live here.

## 2026-11-14T23:17 [lead]

edited fields

## 2026-11-14T23:17 [lead]

lead: paths corrected before dispatch — there is no crates/blastit/src/edit.rs; expected_routes/apply_edits/write_atomic live in crates/blastit-manifest/src/edit.rs, and the existing protected suite is crates/blastit-manifest/tests/critical_manifest_edit.rs. New round-trip test goes in a NEW critical_manifest_edit_*.rs file, not in that suite.
