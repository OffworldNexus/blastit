---
name: Dispatch
description: How Blastit agents spawn subagents. Covers the routing table, standalone brief and report templates, parallelism rules, and the escalation ladder to the expensive tier. Load before calling `subagent`.
---

# Dispatch protocol

## Routing table

| Agent | Tier / Model | Responsibility | Can spawn |
|---|---|---|---|
| `conductor` | cheap (flash) | Top-level loop across milestones (`MA`..`MD`). Never edits code/docs. | `lead`, `planner`, `strategist`, `architect`, `expert`, `researcher`, `test-curator`, `reviewer` |
| `lead` | cheap (flash) | Orchestrates one subtree (`ROOT=<id>`). Runs `execute -> review -> commit` and epic gates. | `lead` (sub-epics), `planner`, `doc-writer`, `designer`, `implementer`, `researcher`, `reviewer`, `critic`, `test-curator`, `visual-checker`, `architect`, `expert`, `strategist` |
| `planner` | cheap (flash) | Decomposes containers into DX-first child nodes (`tools/plan.py`). | `researcher`, `explore` |
| `doc-writer` | cheap (flash) | Writes lean user-facing DX docs (`docs/**`, `README.md`, `CHANGELOG.md`) before & after code; runs DX critics. | `critic`, `researcher`, `explore` |
| `designer` | cheap (flash) | Drafts technical design docs (`docs/design/*.md`) & ADRs from accepted DX specs; runs critics. | `critic`, `researcher`, `explore` |
| `implementer` | cheap (flash) | Implements one Rust/Python/e2e task + critical tests. Never commits. | `explore`, `researcher` |
| `researcher` | cheap (flash) | Investigates protocols, frameworks, WASM codecs, CVEs -> `docs/research/`. | `researcher`, `explore` |
| `reviewer` | cheap (flash) | Goldfish code/docs review + re-runs every acceptance command. Read-only. | `explore` |
| `critic` | cheap (flash) | Goldfish single-lens review (`dx`, `ai-ergonomics`, `security`, `implementability`, `testability`, `consistency`). | `explore` |
| `test-curator` | cheap (flash) | Goldfish test audit + mutation testing -> `docs/testing/AUDIT-*.md` + gap tasks. | `explore` |
| `visual-checker` | multimodal flash | Opens built Zensical site in Chrome (`chrome-devtools`), inspects light/dark/mobile screenshots. | — |
| `architect` | **expensive** (Gemini 4 Carbon) | Core DX blueprints & major security/runtime architecture designs. | `critic`, `researcher`, `explore` |
| `strategist` | **expensive** (Gemini 4 Carbon) | Owns `plan/ROADMAP.md`; runs milestone gates, accepts core designs, replans stuck subtrees. | `planner`, `researcher`, `critic`, `test-curator`, `explore` |
| `expert` | **expensive** (Gemini 4 Carbon) | Last-resort solver when cheap attempts fail on hard Rust/Linux/WASM/protocol bugs. | `explore`, `researcher` |

## Brief template (parent -> child)

Child sessions start with **zero context**. Keep briefs ≤ 40 lines; point to files rather than pasting them:

```text
ROLE: you are the <agent> for Blastit node <ID>.
NODE: plan/nodes/.../<ID>/NODE.md   (read first; source of truth)
READ ALSO: docs/VISION.md, docs/knowledge/LESSONS.md, <specific design/doc files>, skills <dx-first, ...>
ATTEMPT: <n>/<max>. <If n>1: "Previous attempt failed review. Findings: <bullets>. Read LOG.md.">
SCOPE: only touch <paths from NODE.md>. Record out-of-scope discoveries under Follow-ups.
DONE MEANS: every # Acceptance command run and verified; RESULT.md written.
DO NOT: commit, mutate plan statuses, read .env, push/tag git, or ask the human.
REPORT: reply ONLY with the dispatch report template (<= 25 lines).
```

## Report template (child -> parent)

Reply with **only** this block (≤ 25 lines; put details in `RESULT.md` / `LOG.md`):

```text
STATUS: done | needs_split | blocked | failed
NODE: <ID>
SUMMARY: <2-4 lines>
EVIDENCE: <one line per # Acceptance item: PASS/FAIL + command>
FILES: <changed paths, comma-separated>
FOLLOWUPS: <bullets or "none">
BLOCKER: <only if blocked/failed: exact cause and what unblocks it>
SPLIT: <only if needs_split: proposed child tasks, one line each>
```

## Parallelism rules

- A `lead` may run **up to 3 background subagents** simultaneously **only** when their `paths` do not overlap and at most **one** of them builds wheels / runs Docker Compose (`e2e/`), to avoid `target/` lock contention and Docker network/container name collisions.
- `Cargo.toml` (workspace root) and `Cargo.lock` changes must never run in parallel with another Rust task. Put new workspace dependency additions in a dedicated sequential task first.
- Commits are strictly serialized: only `lead` and `conductor` commit, staging only the completed node's `paths` and `plan/` files.

## Escalation ladder & budget (`plan/BUDGET.json`)

| Trigger | Action |
|---|---|
| Attempt 1 fails review | Retry with the same cheap owner, passing the reviewer's exact findings. |
| Attempt 2 fails, or identical review finding repeats | Escalate once to `expert`: run `uv run --no-project tools/plan.py --agent lead escalate <ID> --to expert --reason "..."`, bump `max_attempts`, and dispatch `expert`. |
| `expert` fails | `set <ID> failed` and report up; `conductor` escalates the parent subtree to `strategist` for a replan. |
| `needs_split` | Convert task to epic and dispatch `planner` (cheap — no escalation needed). |
| Core architecture / major UX surface (`owner: architect`) | Escalate to `architect`, then `strategist` for acceptance. |
| Milestone gate | Escalate to `strategist` (mandatory at every milestone boundary). |

- **Hard cap:** `plan/BUDGET.json` (`expensive_cap_usd`, default `$100`) is enforced mechanically by `.opencode/plugins/autopilot`. If launching `strategist`, `architect` or `expert` is denied due to budget exhaustion: **do not retry**; open one `HUMAN.md` request (`plan.py human add`), mark expensive-dependent nodes `blocked`, and continue cheap work.
- **Target ratio:** ≤ 1 non-gate escalation per 15 completed tasks.
