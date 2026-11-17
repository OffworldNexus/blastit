---
id: MB.26.12
title: "Give critical_guard a per-node base: the release-branch merge base reports every earlier node as a violation"
kind: task
status: proposed
owner: planner
priority: 3
depends_on: []
paths: [tools/critical_guard.py, plan/nodes/MB/MB.22/MB.22.6/NODE.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:22"
updated: "2026-11-17T07:22"
---

## Goal

Make `uv run --no-project tools/critical_guard.py --base ... --node <id>` answer the question it
exists for ("did *this* node weaken a protected test?") on a release branch with many nodes.

## Context

Moved from `MB.22.10` at the `0.2.0` release gate: post-0.2.0 work, re-homed from the `MB.22` epic.

`AUDIT-MB.22.md` §4. `MB.22.6`'s `# Acceptance` runs
`--base "$(git merge-base HEAD origin/develop)" --node MB.22`. On `release/0.2.0` that base is
`ec47752`, the branch's fork point, so the guard reports **9 violations** that belong to
`MB.21.4.1`, `MB.21.4.3`, `MB.21.4.5.x`, `MB.21.4.7`, `MB.21.4.8`, `MB.21.4.10`, `MB.21.3.5`,
`MB.1.15` and `MB.3.27` — all earlier nodes, none of them `MB.22`'s. The guard's own docstring
says `--base <commit-before-node>`, so the acceptance line and the tool disagree. Measured
2026-11-17: `critical-guard: FAILED: 9 protected critical test(s) changed …` for a node whose own
protected diff is two added files (2 277 lines, 0 deletions).

## Acceptance

```console
$ uv run --no-project tools/critical_guard.py --base <the commit before this node> --node MB.22
$ uv run --no-project tools/plan.py validate
```

- Either the acceptance lines in the node templates use the node's own start commit (a
  `--base` the tool can derive, e.g. `--node-base` reading the node's first commit), or the guard
  gains a `--since <node>` mode that resolves it. Pick one and state it once.
- A self-test proves both directions: a node whose own commit weakens a protected file is red; a
  node that only adds files, on a branch where earlier nodes edited protected files, is green.
- Every `NODE.md` that carries the old line is updated (grep for `merge-base HEAD origin/develop`).
