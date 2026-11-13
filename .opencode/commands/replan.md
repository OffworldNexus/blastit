---
description: Ask the strategist (expensive tier) to replan a subtree or the roadmap with the owner's instructions
agent: conductor
---

The owner requests a replan. Instructions: $ARGUMENTS

1. Record escalation: `uv run --no-project tools/plan.py --agent conductor escalate <closest node id or MA> --to strategist --reason "owner-requested replan"`.
2. Spawn `strategist` in Replan or Restructure mode, passing the owner's instructions verbatim plus pointers to `plan/STATE.md` and the affected subtree.
3. Run `uv run --no-project tools/plan.py validate` and `state`, commit `plan/` and doc updates, and resume the Main loop if autopilot is `on`.
