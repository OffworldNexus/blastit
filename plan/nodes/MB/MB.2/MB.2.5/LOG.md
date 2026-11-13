# Log MB.2.5

## 2026-11-13T15:11 [planner]

created (task) Wire static serving and SPA/front-controller precedence into scheduler dispatch

## 2026-11-13T20:52 [planner]

edited fields

## 2026-11-13T20:52 [planner]

Split into MB.2.5.1 (rewrite.rs Rewrite/decide + RouteInfo::with_static + the pure half of tests/critical_rewrite.rs) and MB.2.5.2 (scheduler.rs: run opens the roots, static admit/dispatch_static, pump_static, bounded static_reads, release-by-read-result, 503/slot rules; the flow half of tests/critical_rewrite.rs). Design §8 names them MB.2.5a/b; IDs are numeric because ID_RE rejects a letter suffix. Node is now an epic gate with the children's paths as its union. Strict order MB.2.4.1 -> MB.2.5.1 -> MB.2.5.2, so the shared tests/critical_rewrite.rs is never edited concurrently. MB.2.6 was re-pointed from MB.2.5 to MB.2.5.2.

## 2026-11-13T21:19 [lead]

edited fields
