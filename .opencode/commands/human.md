---
description: Tell the conductor that the owner answered items in plan/HUMAN.md (and resume)
agent: conductor
---

The owner has answered items in `plan/HUMAN.md`. Extra message (may be empty): $ARGUMENTS

!`uv run --no-project tools/plan.py human answered`

1. Load skills `dx-first`, `plan-system` and `dispatch`.
2. For each answered item: read it in `plan/HUMAN.md`, apply it (unblock nodes, or dispatch a `planner` or `strategist` if it changes the plan/DX), then run `human close H-xxx`.
3. Switch autopilot `on` and resume your Main loop.
