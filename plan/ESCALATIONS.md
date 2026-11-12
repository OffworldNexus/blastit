# Escalations to the expensive model

Every call to an expensive-tier agent (`strategist`, `architect`, `expert`) is recorded here **before** spawning it, via `uv run --no-project tools/plan.py --agent <caller> escalate <node> --to <agent> --reason "..."`.

| time | node | to | by | reason |
|---|---|---|---|---|
| 2026-11-12T01:49 | MA.5 | architect | conductor | core 0.1.1 DX/runtime design (unprivileged blastit dev simulation, wire-level --learn, check/test completeness, manifest precedence, presets) derived from accepted DX docs; core UX surface per dispatch routing table |
| 2026-11-12T12:58 | MA.5 | strategist | lead | core DX/runtime design acceptance: R1..R42 design + 6 ADRs are the source of truth for MA.2 |
| 2026-11-12T15:31 | MA.2.3.1 | expert | lead | attempt 2 failed review: fold literal redaction must be backslash-aware for E'..'/U&'..' spans (secret leak on stderr) plus mode preservation on atomic write |
| 2026-11-12T16:21 | MA.2.3.3 | expert | lead | attempt 2 failed review: dev runtime root leaks on every post-adopt fatal exit path and the binding critical twin is missing |
| 2026-11-12T17:37 | MA.2.3.3 | expert | lead | attempt 4 fixed attribution but re-review found the learn fallback still reuses a declared route name, dropping that route's statements when an unrouted request exists |
