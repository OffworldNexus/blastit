---
name: Plan system
description: How the Blastit Markdown plan tree works. Covers milestones (MA..MD), node kinds, statuses, the DX-first decomposition rules, gates, stuck detection, and every `uv run --no-project tools/plan.py` command. Load before reading or mutating `plan/`.
---

# Plan system

## Principles

1. **State on disk, never only in context.** Any session can be interrupted or compacted; the next agent resumes from `plan/` alone.
2. **Deterministic mutations via `tools/plan.py`.** Never hand-edit managed YAML frontmatter fields (`status`, `attempts`, `updated`). Run `uv run --no-project tools/plan.py ...`.
3. **DX-First ordering.** Every milestone or feature epic begins with lean user documentation (`local`, `CI/CD`, `deployments`) and a fresh-context `dx` + `ai-ergonomics` critic review **before** internal design or Rust/Python implementation.
4. **Small leaves with executable acceptance.** A leaf task finishes in one session and proves completion by running verifiable commands.
5. **Fresh eyes ("goldfish") for review.** `reviewer` and `critic` subagents start with zero conversation history and judge only what is on disk.
6. **Cheap first, explicit escalation.** DeepSeek Flash handles almost everything; Gemini 4 Carbon (`strategist`, `architect`, `expert`) is called only via the escalation ladder and recorded with `plan.py escalate`.

## Node anatomy

`plan/nodes/MA/MA.1/` holds:

- `NODE.md`: managed YAML frontmatter + Markdown body (`# Goal`, `# Context`, `# Acceptance`, `# Notes`).
- `LOG.md`: append-only work log (`plan.py log`).
- `RESULT.md`: written by the task's owner upon completion.

Frontmatter fields:

```yaml
id: MA.1
title: Rewrite lean Getting Started and local/CI/deploy DX spec
kind: task            # milestone | epic | task
status: ready         # proposed|ready|active|review|done|blocked|failed|superseded
owner: doc-writer     # doc-writer|designer|implementer|researcher|planner|test-curator|visual-checker|architect|strategist|expert|human
priority: 1           # 1 (highest) .. 5
depends_on: []        # node ids
paths: ["docs/**", "README.md"]
attempts: 0
max_attempts: 3
escalation: none
```

`RESULT.md` template:

```markdown
# Result <id>

## Summary
<3-6 lines: what was delivered and how it serves the DX/security goal>

## Evidence
<each # Acceptance item -> exact command run -> output tail>

## Files
<added/modified repo-relative paths>

## Follow-ups
<out-of-scope discoveries or proposed child/sibling nodes>
```

## Statuses and transitions

| Transition | Who | When |
|---|---|---|
| (new) -> `ready` / `proposed` | `planner`, `strategist` | Node created |
| `ready` -> `active` | `lead` | Before dispatching owner (also run `bump`) |
| `active` -> `review` | `lead` | Worker reported `STATUS: done` |
| `review` -> `done` | `lead` | `reviewer` returned `VERDICT: PASS`, then `lead` committed |
| `review` -> `ready` | `lead` | `reviewer` returned `FAIL`; findings logged in `LOG.md` |
| `active` -> `blocked` | `lead` | Waiting on `HUMAN.md` answer or external dependency |
| `*` -> `failed` | `lead` | Attempts exhausted including `expert` escalation |
| `failed` / `blocked` -> `ready` | `strategist`, `conductor` | After replan or `HUMAN.md` answer |
| `*` -> `superseded` | `strategist`, `planner` | Replaced during replanning |
| container -> `done` | `lead` (epic) / `strategist` (milestone) | All children closed and gate checks passed |

## Decomposition rules (`planner`)

- A container (`milestone` or `epic`) shows up as `decompose` in `plan.py next` when `ready`, unblocked, and childless.
- Decompose into **3–8 children** (introduce epics if larger; never leave a 1-child container).
- **Mandatory DX-First sequence inside every feature epic / milestone:**
  1. **Research** (`researcher`) if any protocol, framework idiom, WASM codec license, or CVE detail is unverified.
  2. **User-facing DX & Documentation** (`doc-writer` or `architect` for major UX shifts), covering Local machine, CI/CD, and Deployments, reviewed by `critic` (`dx` + `ai-ergonomics` lenses).
  3. **Technical Design Doc** (`designer` or `architect`), derived from the accepted DX docs, reviewed by `critic` (`security`, `implementability`, `testability`, `consistency`).
  4. **Implementation + Critical Tests** (`implementer`), broken into small crate/module leaves (≤ 5–8 files, ≤ 600 changed lines, acceptance runs in < 10 min). Tests live in the same task as the code they protect!
  5. **Runnable-Docs, Visual & E2E/CVE Verification** (`implementer` / `doc-writer` / `visual-checker`), proving every documented command works verbatim and the Zensical site builds & renders cleanly.
- Set disjoint `paths` on independent siblings so `lead` can run up to 3 tasks in parallel.

## Gates

- **Epic gate (`lead`):**
  1. Verify every item in the epic's `# Acceptance` by checking child `RESULT.md` files and running verification commands (`make check`, plus `make e2e` if process/proxy/container behaviour changed).
  2. If the epic touched Rust/Python code, run a `test-curator` audit of the touched crates/modules. Open critical gaps become new child tasks before the epic closes.
  3. If the epic touched `docs/`, verify `make docs-check` (`zensical build --strict`) and run `visual-checker` when layout, diagrams, navigation or brand changed.
- **Milestone gate (`strategist`, expensive tier):**
  1. Verify every `plan/ROADMAP.md` exit criterion with evidence (`make check`, `make e2e`, CVE proof suite, runnable docs check).
  2. Run a milestone `test-curator` audit (including `cargo-mutants` on critical security/policy crates).
  3. Write `plan/nodes/<M>/RESULT.md`, update `CHANGELOG.md` / `ARCHITECTURE.md` if needed, refresh the next milestone's `NODE.md` body with concrete pointers, mark `<M>` `done`, and open a `HUMAN.md` release-ready notice so the owner can push `develop` and tag the release (`v0.1.1`, `v0.2.x`, etc.).

## Command cheat sheet

Always run with `--no-project` (fast stdlib execution, zero build/network) and `--agent <your-role>`:

```bash
uv run --no-project tools/plan.py tree [--under ID] [--depth N] [--all]
uv run --no-project tools/plan.py next [--under ID] [--limit N]
uv run --no-project tools/plan.py show ID [--log] [--result]
uv run --no-project tools/plan.py --agent planner new PARENT --kind task --owner implementer \
  --title "Implement ..." --depends MA.1 --paths "crates/blastit-cli/**" --priority 2 --body-file /tmp/opencode/body.md
uv run --no-project tools/plan.py --agent lead set ID active
uv run --no-project tools/plan.py --agent lead bump ID
uv run --no-project tools/plan.py --agent implementer log ID "message"
uv run --no-project tools/plan.py --agent planner edit ID [--kind epic --reset-attempts] [...]
uv run --no-project tools/plan.py validate
uv run --no-project tools/plan.py stuck [--stale-hours 6]
uv run --no-project tools/plan.py state
uv run --no-project tools/plan.py --agent lead human add "question" [--node ID]
uv run --no-project tools/plan.py human open | answered
uv run --no-project tools/plan.py --agent conductor human close H-001
uv run --no-project tools/plan.py --agent lead escalate ID --to strategist|architect|expert --reason "..."
uv run --no-project tools/plan.py --agent conductor autopilot [on|off] [--reason "..."]
```
