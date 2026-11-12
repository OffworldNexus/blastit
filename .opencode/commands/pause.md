---
description: Pause the autonomous Blastit loop (autopilot off) after the current step
agent: conductor
---

The owner asks to pause.

1. Run `uv run --no-project tools/plan.py --agent conductor autopilot off --reason "paused by owner"`.
2. Let running background subagents finish, without starting new work.
3. Commit any pending `plan/` changes.
4. Reply with a 5-line status summary, then end your turn.
