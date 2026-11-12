# Escalations to the expensive model

Every call to an expensive-tier agent (`strategist`, `architect`, `expert`) is recorded here **before** spawning it, via `uv run --no-project tools/plan.py --agent <caller> escalate <node> --to <agent> --reason "..."`.

| time | node | to | by | reason |
|---|---|---|---|---|
