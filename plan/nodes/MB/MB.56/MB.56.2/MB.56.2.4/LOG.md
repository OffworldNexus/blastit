# Log MB.56.2.4

## 2026-11-20T17:15 [lead]

created (task) Consolidate blastit-sqlpolicy (<=1.5k) and blastit-http (<=800) §7 suites

## 2026-11-20T17:43 [lead]

NOTE from MB.56.2.2: 'body policies' §7 stays owned by crates/blastit-ingress/tests/critical_ingress.rs (blastit-http does not depend on blastit-ingress, so codec refusals are unreachable from http_framing.rs). Do NOT delete the ingress suite; keep blastit-http <=800 LOC for framing/smuggling/size only.

## 2026-11-20T18:09 [lead]

status ready -> active

## 2026-11-20T18:09 [lead]

attempt 1/3 started

## 2026-11-20T18:09 [lead]

base 6bcd3d9857e17b23b156eedcaf53cc6ef31f94de

## 2026-11-20T18:22 [lead]

REVIEW FIX (ux): http: 431 header-cap, 408 head timeout, 417 bad Expect, 408 body_timeout+cancel, 503 over max_connections, 100-continue interim (and none for CL:0), incremental body forwarding before all bytes arrive - all documented in crates/blastit-http/src/lib.rs:32-38 and settings.md:101, pins deleted. sqlpolicy: add RefuseReason::Unparseable render-time scrub test (Display strips literal; 'at or near' + quoted-span branches). Keep http <=800 LOC by trimming redundant ported checks.

## 2026-11-20T18:27 [lead]

status active -> done
