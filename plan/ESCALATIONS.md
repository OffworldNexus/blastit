# Escalations to the expensive model

Every call to an expensive-tier agent (`strategist`, `architect`, `expert`) is recorded here **before** spawning it, via `uv run --no-project tools/plan.py --agent <caller> escalate <node> --to <agent> --reason "..."`.

| time | node | to | by | reason |
|---|---|---|---|---|
| 2026-11-12T01:49 | MA.5 | architect | conductor | core 0.1.1 DX/runtime design (unprivileged blastit dev simulation, wire-level --learn, check/test completeness, manifest precedence, presets) derived from accepted DX docs; core UX surface per dispatch routing table |
| 2026-11-12T12:58 | MA.5 | strategist | lead | core DX/runtime design acceptance: R1..R42 design + 6 ADRs are the source of truth for MA.2 |
| 2026-11-12T15:31 | MA.2.3.1 | expert | lead | attempt 2 failed review: fold literal redaction must be backslash-aware for E'..'/U&'..' spans (secret leak on stderr) plus mode preservation on atomic write |
| 2026-11-12T16:21 | MA.2.3.3 | expert | lead | attempt 2 failed review: dev runtime root leaks on every post-adopt fatal exit path and the binding critical twin is missing |
| 2026-11-12T17:37 | MA.2.3.3 | expert | lead | attempt 4 fixed attribution but re-review found the learn fallback still reuses a declared route name, dropping that route's statements when an unrouted request exists |
| 2026-11-12T19:55 | MA.2.3.5 | expert | lead | pytest-django lifecycle/credential-leak bug: cheap attempts 1-2 failed on R30 handshake (env leak when no DB test; teardown drop failure); needs deep pytest-django hook work |
| 2026-11-13T03:12 | MA | strategist | conductor | milestone gate: MA Lead DX & Show-the-World Polish 0.1.1 — all 5 children done, epic gates passed, make check + make e2e green, visual-checker PASS; needs ROADMAP exit-criteria verification, test-curator/mutant audit sign-off (cargo-mutants absent H-002), CHANGELOG/version/release readiness and HUMAN v0.1.1 tag sign-off |
