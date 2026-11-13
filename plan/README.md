# Plan system (quick reference)

Full specification: skill `plan-system`.

```text
plan/
  ROADMAP.md      milestones (MA..MD), versions, exit criteria and standing policies (strategist only)
  STATE.md        generated dashboard: `uv run --no-project tools/plan.py state`
  HUMAN.md        inbox of requests for the owner (decisions, credentials, release PR merge checkpoints)
  ESCALATIONS.md  every call to the expensive tier (strategist, architect, expert), with reason
  JOURNAL.md      append-only one-line event log (written by tools/plan.py)
  BUDGET.json     owner-edited hard cap on cumulative expensive-tier spend
  BUDGET.md       generated spend report (written by .opencode/plugins/autopilot)
  AUTOPILOT       "on" or "off": keep-alive plugin re-prompts conductor while "on"
  nodes/MA/...    the hierarchical Markdown plan tree
```

Common commands (always pass `--no-project` and `--agent <your-role>`):

```bash
uv run --no-project tools/plan.py state
uv run --no-project tools/plan.py tree --depth 2
uv run --no-project tools/plan.py next --limit 15
uv run --no-project tools/plan.py show MA --log
uv run --no-project tools/plan.py validate
uv run --no-project tools/plan.py stuck
uv run --no-project tools/plan.py human open
```
