# Escalations to the expensive model

Every call to an expensive-tier agent (`strategist`, `architect`, `expert`) is recorded here **before** spawning it, via `uv run --no-project tools/plan.py --agent <caller> escalate <node> --to <agent> --reason "..."`.

| time | node | to | by | reason |
|---|---|---|---|---|
| 2026-11-12T01:49 | MA.5 | architect | conductor | core 0.1.1 DX/runtime design (unprivileged blastit dev simulation, wire-level --learn, check/test completeness, manifest precedence, presets) derived from accepted DX docs; core UX surface per dispatch routing table |
