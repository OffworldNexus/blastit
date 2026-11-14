# Log MB.3.21

## 2026-11-14T16:36 [lead]

created (task) check --learn must not drop a route's executor block: edit.rs expected_routes loses [route.static] today and [route.php] next

## 2026-11-14T16:36 [lead]

created on owner instruction (2026-11-14) after the finding was folded into MB.3.6.4 once and stayed there: a --learn rewrite that silently deletes a [route.static]/[route.php] block costs a user their config. EXISTING bug -- expected_routes drops static_policy TODAY; it would drop [route.php] as soon as that block exists (it does, MB.3.5.4). Paths chosen so the round-trip test and the CLI-level check can both live here.
