---
id: MB.40
title: "GEPA G4 harness overhaul, P0 bugfixes, deduplication, god-file split, CLI craft & visual docs"
kind: epic
status: ready
priority: 1
depends_on: []
paths: ["**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T00:16"
---

# Goal
Install the GEPA G4 evolved `.opencode/` agent architecture and execute the 6 pre-diagnosed 0.2.0 engineering, P0 bugfix, deduplication, god-file breakup, CLI UX, and visual documentation slices with zero re-research.

# Context
All exact `file:line` targets were established empirically in `/tmp/opencode/audit-{cov,arch,ux,history}.md` and validated across 238 isolated OpenCode runs in `/home/remy/.cache/blastit-gepa/results/`.

# Acceptance
- `uv run --no-project tools/plan.py validate`
- `uv run --no-project tools/guard.py`
- `make check`
