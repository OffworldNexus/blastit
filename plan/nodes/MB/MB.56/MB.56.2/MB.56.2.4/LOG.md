# Log MB.56.2.4

## 2026-11-20T17:15 [lead]

created (task) Consolidate blastit-sqlpolicy (<=1.5k) and blastit-http (<=800) §7 suites

## 2026-11-20T17:43 [lead]

NOTE from MB.56.2.2: 'body policies' §7 stays owned by crates/blastit-ingress/tests/critical_ingress.rs (blastit-http does not depend on blastit-ingress, so codec refusals are unreachable from http_framing.rs). Do NOT delete the ingress suite; keep blastit-http <=800 LOC for framing/smuggling/size only.
