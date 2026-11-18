---
name: Plan system
description: How the Blastit Markdown plan tree works. Covers milestones (MA..ME), coarse 3-4 vertical slices, statuses, fast-check vs check gates, stuck detection, and every `uv run --no-project tools/plan.py` command. Load before reading or mutating `plan/`.
---

# Plan system

## Principles

1. **State on disk, never only in context.** Any session can be interrupted or compacted; the next agent resumes from `plan/` alone.
2. **Deterministic mutations via `tools/plan.py`.** Never hand-edit managed YAML frontmatter fields (`status`, `attempts`, `updated`). Run `uv run --no-project tools/plan.py ...`.
3. **DX-First coarse vertical slices.** Feature epics decompose into **3–4 coarse vertical slices** (NEVER 15–25 micro-tasks!): Slice 1 user DX docs (`writer`), Slice 2 core crate + `Blastfile` + critical tests (`implementer`), Slice 3 CLI/supervisor `--learn` & `check` + real lab E2E (`implementer`).
4. **1 Sequential Cargo Writer & Gate Split.** At most 1 Rust/wheel/Docker writer runs at a time per worktree. Leaf slices verify with `make fast-check` (or `make docs-check`); full `make check` runs **once** at epic/milestone gates.
5. **Parallel specialist review with noise filtering.** `review-arch` and `review-ux` run in parallel on `<base sha>`; `lead` filters out `RESULT.md` line-number nits, `scripts/check_*.py` regex-guard demands, and `MINOR` style churn.
6. **Cheap first, explicit escalation.** DeepSeek Flash handles almost everything; Gemini 4 Carbon (`strategist`, `architect`, `expert`) is called via the escalation ladder and recorded with `plan.py escalate`.

## Node anatomy

`plan/nodes/MB/MB.40/` holds:

- `NODE.md`: managed YAML frontmatter + Markdown body (`# Goal`, `# Context` / `# Exact Targets`, `# Acceptance`).
- `LOG.md`: append-only work log (`plan.py log`).
- `RESULT.md`: concise summary + command evidence written by the slice's owner before final verification.

Frontmatter fields:

```yaml
id: MB.40.6
title: Visual documentation hierarchy pass
kind: task            # milestone | epic | task
status: ready         # proposed|ready|active|review|done|blocked|failed|superseded
owner: writer         # active owners: writer|implementer|researcher|planner|visual-checker|architect|strategist|expert|human
priority: 1           # 1 (highest) .. 5
depends_on: []        # node ids
paths: ["docs/**", "README.md"]
attempts: 0
max_attempts: 3
escalation: none
```

`RESULT.md` template (keep concise — never 2,000-word essays!):

```markdown
# Result <id>

## Summary
<3-6 lines: what was delivered and how it serves DX/security>

## Evidence
<each # Acceptance item -> exact command run -> exit status & output tail>

## Files
<added/modified/deleted repo-relative paths>

## Follow-ups
<out-of-scope discoveries or "none">
```

## Statuses and transitions

| Transition | Who | When |
|---|---|---|
| (new) -> `ready` / `proposed` | `planner`, `strategist`, `conductor` | Node created |
| `ready` -> `active` | `lead`, `conductor` | Before dispatching owner (also run `bump` and log `base <sha>`) |
| `active` -> `review` | `lead` | Worker reported `STATUS: done` |
| `review` -> `done` | `lead` | Parallel `review-arch` + `review-ux` passed (after at most 1 consolidated fix pass for real `[BLOCKER]`/`[MAJOR]` bugs), `make fast-check` + guards passed, and `lead` committed |
| `active` -> `blocked` | `lead` | Waiting on `HUMAN.md` answer or external dependency |
| `*` -> `failed` | `lead` | Attempts exhausted including `expert` escalation |
| `failed` / `blocked` -> `ready` | `strategist`, `conductor` | After replan or `HUMAN.md` answer |
| `*` -> `superseded` | `strategist`, `planner`, `expert` | Consolidated or replaced during replanning |
| container -> `done` | `lead` (epic) / `strategist` (milestone) | All children closed and `make check` gate passed |

## Decomposition rules (`planner`)

- Decompose an epic into **3–4 coarse vertical slices** (never 15–25 fragmented micro-tasks, never separate "write tests later" tasks, and never `scripts/check_*.py` source-grep tasks):
  1. **Slice 1 (`writer`):** Lean `<130`-line user DX doc with Mermaid diagram, admonitions, and Local + CI + Deploy commands.
  2. **Slice 2 (`implementer`):** Core crate + workspace `Cargo.toml`/`Cargo.lock` + `Blastfile` parser/validator + allowed & adversarial critical tests.
  3. **Slice 3 (`implementer`):** `BootMode` composition root + `--learn` + `blastit check` (`--json` & human `fix.to`) + real `UnixStream::pair()`/`UnixListener` E2E tests.
  4. *(Optional Slice 4)*: Runnable-docs & CVE regression verification.

## Gates

- **Epic gate (`lead`):**
  1. Run **full `make check` once** (plus `make e2e` if `crates/blastit`, brokers, or `e2e/` changed, and `make mutants-rust PKG=<crate>` if security-critical policy crates changed).
  2. If `docs/` layout/diagrams changed, verify `make docs-check` and run `visual-checker` when visual browser QA is needed.
  3. Write `plan/nodes/.../<epic>/RESULT.md`, mark `<epic>` `done`, regenerate `state`, and commit `[<epic>] Gate passed: <title>`.
- **Milestone gate (`strategist`, expensive tier):**
  1. Verify every `plan/ROADMAP.md` exit criterion with evidence (`make check`, `make e2e`, CVE suite, runnable docs, `cargo-mutants` on critical policy crates).
  2. Write `plan/nodes/<M>/RESULT.md`, verify `Cargo.toml` version and dated `CHANGELOG.md` entry (`## <version> (YYYY-MM-DD)`), refresh the next milestone's `NODE.md`, mark `<M>` `done`, and return `RELEASE_READY: <version>` so `conductor` pushes `release/<version>` and opens the Release PR into `develop`.

## Command cheat sheet

Always run with `--no-project` and `--agent <your-role>`:

```bash
uv run --no-project tools/plan.py tree [--under ID] [--depth N] [--all]
uv run --no-project tools/plan.py next [--under ID] [--limit N]
uv run --no-project tools/plan.py show ID [--log] [--result]
uv run --no-project tools/plan.py --agent planner new PARENT --kind task --owner implementer \
  --title "Implement ..." --depends MB.4.1 --paths "crates/blastit-view-node/**" --priority 1 --body-file /tmp/opencode/body.md
uv run --no-project tools/plan.py --agent lead set ID active
uv run --no-project tools/plan.py --agent lead bump ID
uv run --no-project tools/plan.py --agent implementer log ID "message"
uv run --no-project tools/plan.py --agent planner edit ID [--owner writer --kind epic --reset-attempts]
uv run --no-project tools/plan.py validate
uv run --no-project tools/plan.py stuck [--stale-hours 6]
uv run --no-project tools/plan.py state
uv run --no-project tools/plan.py --agent lead human add "question" [--node ID]
uv run --no-project tools/plan.py human open | answered
uv run --no-project tools/plan.py --agent conductor human close H-001
uv run --no-project tools/plan.py --agent lead escalate ID --to strategist|architect|expert --reason "..."
uv run --no-project tools/plan.py --agent conductor autopilot [on|off] [--reason "..."]
```
