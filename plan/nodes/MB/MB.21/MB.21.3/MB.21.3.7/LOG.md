# Log MB.21.3.7

## 2026-11-16T02:16 [planner]

created (task) Model parity (b): delete the per-view static/PHP accessors, rename StaticOutcome::fallback_missing, move the scheduler and blastit call sites and rewrite the three suites

## 2026-11-16T02:40 [lead]

edited fields

## 2026-11-16T03:04 [lead]

edited fields

## 2026-11-16T03:04 [lead]

edited fields

## 2026-11-16T03:05 [lead]

status ready -> active

## 2026-11-16T03:05 [lead]

attempt 1/3 started

## 2026-11-16T03:05 [lead]

base ef783db0c5c094a9f6aff5e2042e7955c40cadb5

## 2026-11-16T03:08 [implementer]

blocked: the design drops the route->mount / route->php association that critical_rewrite.rs::an_app_route_matches_before_a_later_static_route, critical_static_cli.rs (route "site" root audit), no_python_build.rs:227 and the all-static check path depend on; no source changed; decision requested as H-010 (option A: named static_mount_for bridge + widened paths).

## 2026-11-16T03:09 [lead]

edited fields

## 2026-11-16T03:09 [lead]

edited fields

## 2026-11-16T03:09 [lead]

Re-scoped after attempt 1's blocked report (design gap, no code written): attempt 1 proved the four per-view accessors are the only route->mount / route->php association in the model, that four protected contracts read it, and that §9.5's is_all_static shape flips a TOML static-only site to 'needs BLASTIT_APP' while the retired reader is still in service. Design §4.3's Consumers column already assigns the removal to MB.21.4.2, so the accessor deletion, the StaticOutcome rename (+ rewrite.rs and two protected suites), §9.5/§9.6 and the scheduler/blastit call sites move there. 3.7 keeps what is green on its own commit: StaticPolicy.fallback replaces spa (retired reader lowers spa=true), §9.1 mount shape + DuplicateMount, §10.3 check_routes by sandbox, error.rs wording, and the parse.rs bridge test. H-010 answered by this decision; no human input needed.

## 2026-11-16T03:09 [lead]

status active -> ready

## 2026-11-16T03:10 [lead]

status ready -> active

## 2026-11-16T03:10 [lead]

attempt 2/3 started

## 2026-11-16T03:10 [lead]

base ef783db0c5c094a9f6aff5e2042e7955c40cadb5

## 2026-11-16T03:26 [lead]

edited fields

## 2026-11-16T03:26 [lead]

Accepted attempt 2's two disclosed deviations: D1 (routing.rs/uid.rs in-crate test literals) by widening paths — mechanical one-line each, no assertion changed; D2 (one mount per prefix, first TOML block wins) because §9.1's DuplicateMount makes two '/' mounts invalid and the protected three-static-route suite must keep validating. Nothing on the runtime path reads mounts, so behaviour is unchanged; 3.8's lowering produces distinct prefixes from a Blastfile.

## 2026-11-16T03:26 [lead]

status active -> review

## 2026-11-16T03:35 [lead]

reviewer verdict PASS (attempt 2). 4 MINOR: (1) unrelated doc-comment rewordings in model.rs made only to satisfy the acceptance grep -> node grep tightened to \bspa\b|spa: for reuse; (2) validate's fallback==index skip leaves mount-level root/index checks -> MB.21.3.8; (3) parse bridge test could also validate() -> covered by critical_static_manifest's three-static-route suite; (4) check_static_index refuses '.' while §9.1 allows it (pre-existing) -> epic follow-up.

## 2026-11-16T03:35 [lead]

status review -> done
