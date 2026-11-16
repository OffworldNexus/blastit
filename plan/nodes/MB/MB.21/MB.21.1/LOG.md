# Log MB.21.1

## 2026-11-16T00:38 [planner]

created (task) Write the Blastfile basic guide and complete language reference, register them, pass dx + ai-ergonomics critics

## 2026-11-16T00:40 [lead]

status ready -> active

## 2026-11-16T00:40 [lead]

attempt 1/3 started

## 2026-11-16T00:40 [lead]

base f1f933a5edd55bf1e0f9a85f8c3705e45d295702

## 2026-11-16T01:10 [lead]

final ai-ergonomics review: 0 BLOCKER / 5 MAJOR (--json contract naming, fix.to vocabulary, PHP gate not runnable unprivileged, guide links to 0.1.1 pages, raw upload undeployable) -> fix round to doc-writer; owner decisions taken by lead: docs keep the CURRENT v1 --json kind names + schema_version 1 (no pre-announced rename; MB.21.4.3 owns any rename), fix.to documented as optional and still node-spelled until MB.21.4.3, raw deployments stated as impossible in 0.2.0, 0.1.1 links labelled until MB.21.5.1

## 2026-11-16T01:12 [lead]

owner correction: acknowledged raw IS deployable (validate.rs:835-878 refuses only Raw && !allow_raw; ADR-0011). Blastfile spelling settled: 'body raw' = unacknowledged (check raw_ingress + prod boot refusal, dev warns), 'body raw unvalidated' = acknowledged (allow_raw true, deploys with boot warning, never written by --learn). Supersedes my earlier 'no acknowledgement spelling' wording; MB.21.2 must pin it in ADR-0012

## 2026-11-16T01:23 [lead]

closing pass complete after 2 acceptance-round reviews; owner decisions enforced in text: workers REMOVED (not deferred), raw acknowledgement = 'body raw unvalidated', fix.* documented in target Blastfile terms (MB.21.4.3 re-spells payloads), PHP gate = fmt --check 0 + check 2/3 codes, no_observations instead of unexercised_route on an empty suite

## 2026-11-16T01:23 [lead]

status active -> review

## 2026-11-16T01:31 [lead]

status review -> done

## 2026-11-16T01:31 [lead]

committing

## 2026-11-16T01:55 [lead]

commit c2d89e4
