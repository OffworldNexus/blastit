---
name: Dispatch
description: How Blastit orchestrators spawn subagents. Covers the GEPA G4 routing table, crisp standalone briefs, 1-Cargo-writer rule, fast-check vs check gate split, and escalation ladder.
---

# Dispatch protocol

## Routing table

| Agent | Tier / Model | Responsibility | Can spawn |
|---|---|---|---|
| `conductor` | cheap (flash) | Top-level loop across milestones (`MA`..`ME`) on `release/<version>` branches; opens Release PRs into `develop` when ready. Never edits code/docs. | `lead`, `planner`, `strategist`, `architect`, `expert`, `researcher`, `review-arch`, `review-ux`, `writer`, `implementer` |
| `lead` | cheap (flash) | Orchestrates one subtree (`ROOT=<id>`). Runs 1 sequential Cargo writer, parallel `review-arch` + `review-ux`, noise filtering, `make fast-check` commits per slice, and `make check` once at epic gates. | `implementer`, `writer`, `review-arch`, `review-ux`, `planner`, `researcher`, `visual-checker`, `expert`, `architect`, `strategist`, `lead` |
| `planner` | cheap (flash) | Decomposes epics/milestones into **3–4 coarse vertical slices** via `tools/plan.py`. | `explore` |
| `implementer` | cheap (flash) | Implements Rust/Python features, refactors, and lab-first socket/frame tests (`make fast-check`). Never commits. | `explore` |
| `writer` | cheap (flash) | Writes lean (`75–95` lines), visual (Mermaid + admonitions), command-first Blastit user docs & DX specs (`make docs-check`). Never touches `crates/`. | — |
| `review-arch` | cheap (flash) | Repo-aware architecture, Constructor IoC, deduplication, security & lab-test reviewer (`git diff <base>`). Read-only. | — |
| `review-ux` | cheap (flash) | Specialist reviewer for CLI terminal craft, visual Mermaid docs & `Blastfile` DX purity (`git diff <base>`). Read-only. | — |
| `researcher` | cheap (flash) | Investigates protocols, frameworks, WASM codecs & licenses, CVEs -> `docs/research/`. | `researcher`, `explore` |
| `visual-checker` | multimodal flash | Opens built Zensical site in Chrome (`chrome-devtools`), inspects Mermaid diagrams & light/dark/mobile screenshots. Read-only. | — |
| `architect` | **expensive** (Gemini 4 Carbon) | Core DX blueprints, `Blastfile`/CLI UX surfaces & security/runtime architectures. | `review-arch`, `review-ux`, `researcher`, `explore` |
| `strategist` | **expensive** (Gemini 4 Carbon) | Owns `plan/ROADMAP.md`; runs milestone gates, accepts core designs, replans stuck subtrees. | `planner`, `researcher`, `review-arch`, `review-ux`, `writer`, `implementer`, `visual-checker`, `explore` |
| `expert` | **expensive** (Gemini 4 Carbon) | Last-resort solver when cheap attempts fail on hard technical bugs or cross-cutting surgery. | `explore`, `researcher` |

## Brief template (parent -> child)

Child sessions start with **zero context** and **zero boot ceremony** (do NOT ask leaf agents to load skills or snoop `.opencode/`). Keep briefs <= 35 lines and name exact target files:

```text
ROLE: You are `<agent>` for Blastit node `<ID>`.
NODE: `plan/nodes/.../<ID>/NODE.md` (read in Step 1 along with target files and `docs/knowledge/LESSONS.md`).
TARGETS: <exact file paths to read/edit>
ATTEMPT: <n>/<max>. <If retry/fix: list ONLY filtered genuine [BLOCKER]/[MAJOR] defects.>
SCOPE: Only touch `<paths from NODE.md>`.
VERIFY: Write brief `RESULT.md`, then run `<make fast-check | make docs-check | acceptance cmd>` ONCE at the end.
DO NOT: Run `git add`, `git commit`, `git push`, mutate plan statuses, read `.env`, or edit files after final check!
REPORT: Reply immediately with the <= 20-line dispatch report block.
```

## Report template (child -> parent)

```text
STATUS: done | needs_split | blocked | failed
NODE: <ID>
SUMMARY: <2-4 lines>
EVIDENCE: <one line per verification command: PASS/FAIL + command>
FILES: <changed paths, comma-separated>
FOLLOWUPS: <bullets or "none">
RELEASE_READY: <optional: version e.g. 0.2.0 when a milestone or sub-release gate passed>
BLOCKER: <only if blocked/failed: exact cause and what unblocks it>
SPLIT: <only if needs_split: 3-4 coarse vertical slices, one line each>
```

## Concurrency & Gate Deduplication Rules

- **1 Sequential Cargo Writer per Worktree:** Run **at most 1** Rust/wheel/Docker task (`implementer` or `expert`) at a time so `Cargo.lock` and `target/debug/blastit` never collide mid-test.
- **Parallel Read-Only Reviews:** Always spawn `review-arch` and `review-ux` in parallel against `<base sha>`.
- **Gate Split (`fast-check` vs `check`):** Leaf slices run `make fast-check` (or `make docs-check`). Full `make check` runs **once** at epic/milestone gates — never 3x per leaf!
- **Commits:** Strictly serialized by `lead` and `conductor`, staging explicit paths (`git add -- <paths>`, never `git add -A`).

## Escalation ladder & budget (`plan/BUDGET.json`)

| Trigger | Action |
|---|---|
| Review finds genuine `[BLOCKER]`/`[MAJOR]` bug | Filter out `RESULT.md` nits, regex-guard requests, and `MINOR` style churn; dispatch **at most 1** consolidated fix pass to the cheap owner. |
| Fix pass fails, or identical genuine bug repeats | Escalate once to `expert`: `uv run --no-project tools/plan.py --agent lead escalate <ID> --to expert --reason "..."`, bump `max_attempts`, dispatch `expert`. |
| `expert` fails | `set <ID> failed` and report up; `conductor` escalates the parent subtree to `strategist` for a replan. |
| `needs_split` | Convert task to epic and dispatch `planner` for 3–4 coarse vertical slices (cheap). |
| Core architecture / major UX surface (`owner: architect`) | Escalate to `architect`, then `strategist` for acceptance. |
| Milestone gate | Escalate to `strategist` (mandatory at every milestone boundary). |

- **Hard cap:** `plan/BUDGET.json` (`expensive_cap_usd`) is enforced mechanically by `.opencode/plugins/autopilot`. If denied due to budget exhaustion, do not retry: open one `HUMAN.md` request (`plan.py human add`), mark expensive-dependent nodes `blocked`, and continue cheap work.
