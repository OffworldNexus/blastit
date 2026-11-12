---
description: Start or resume the autonomous Blastit DX-first loop (conductor + autopilot)
agent: conductor
---

Start (or resume) the Blastit autonomous loop now.

1. Load skills `dx-first`, `plan-system` and `dispatch`.
2. Follow your Boot procedure, then your Main loop, exactly as described in your agent instructions.
3. Remember the #1 rule: before designing internals or writing code for any feature, its lean user-facing documentation across Local machine, CI/CD, and Deployments must be written, critic-reviewed (`dx` + `ai-ergonomics`), and accepted first.
4. Do not stop to report progress. Keep working autonomously until the roadmap is complete or every actionable item is blocked on the human.

Extra instructions from the owner (may be empty): $ARGUMENTS

Current state:

!`uv run --no-project tools/plan.py state`
