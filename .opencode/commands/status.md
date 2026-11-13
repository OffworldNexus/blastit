---
description: Show Blastit plan status (state, tree, stuck items, open human requests) without changing anything
agent: conductor
---

Summarize the project status for the owner in <= 20 lines using only the output below:

- where we are (current milestone and active DX/design/code phase);
- what is actionable next;
- what is blocked or stuck and why;
- what the owner should do (if anything, e.g. answer HUMAN.md or review/merge an open Release PR into `develop`).

Do NOT start the loop, spawn subagents, or modify any file. End your turn right after the summary.

!`uv run --no-project tools/plan.py state`

!`uv run --no-project tools/plan.py tree --depth 2`

!`uv run --no-project tools/plan.py stuck`

!`uv run --no-project tools/plan.py human open`
