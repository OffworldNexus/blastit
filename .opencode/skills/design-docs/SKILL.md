---
name: Design docs & DX specs (elephant drafts, goldfish reviews)
description: How Blastit writes, critiques and accepts both User-Facing DX Specifications and Technical Design Documents + ADRs. Uses lens-based fresh-context goldfish critics (`dx`, `ai-ergonomics`, `security`, `implementability`, `testability`, `consistency`).
---

# Design docs & DX specs: Elephant drafts, Goldfish reviews

The **elephant** (`doc-writer`, `designer`, `architect`) holds context and writes the draft. **Goldfish** (`critic` subagents) review with *zero* conversation history, seeing only the document and the repo files it cites.

In Blastit, there are **two distinct artefacts**, always written in this order:

1. **User-Facing DX Documentation (`docs/**` + `README.md` or `docs/design/dx-<slug>.md`)** — How it looks and feels on the **local machine**, in **CI/CD**, and across **deployment scenarios**.
2. **Technical Design Document (`docs/design/<slug>.md` + `docs/decisions/ADR-NNNN-<slug>.md`)** — How the Rust crates, WASM sandboxes, wire brokers, and Python CLI implement that exact DX safely.

## 1. Critic lenses (`critic` subagent)

Spawn **≥ 2** (and **≥ 3** for core designs: always include `dx`, `ai-ergonomics`, and `security`) `critic` subagents in parallel, giving each **only** the document path(s), its single lens name, and the review output path (`docs/design/<slug>.review-<lens>.md`):

| Lens | What the goldfish asks |
|---|---|
| `dx` | **Human Developer Experience.** Count the steps on (a) local laptop, (b) CI/CD, (c) deployment! Does local dev work with **zero `sudo`** and zero `chmod`/`setcap` pain? Is the prose ultra-lean (no kernel lectures in user guides, ≤ 3 steps to hello world, clear tables)? Does it fit modern 12-factor idioms (Django/Flask, Symfony/Laravel PSR, Vite/Node, Tomcat) natively? Do errors tell the user the exact fix? |
| `ai-ergonomics` | **AI Agent Experience.** Imagine an LLM coding agent asked to integrate Blastit or add a feature to an app. How does it populate `blastit.toml`? How does it verify in one pass (`--learn` / `blastit check --json`) that **zero** SQL queries, outgoing calls, routes, or input schemas were forgotten? Could any syntax or error message confuse an agent into reaching for `unrestricted = true`? |
| `security` | **Zero-Trust & CVE Resistance.** Does every byte reaching the backend get decoded and re-encoded in memory-safe / budgeted WASM code (where applicable)? Are WASM memory and fuel strictly bounded (O(1 page/chunk), never O(N pages))? Are parser differentials impossible? Do real credentials stay strictly behind the stage-0 `execve` barrier? Would `php2shell`, `node2shell`, polyglot uploads, HTTP/FastCGI header smuggling, or SSRF be stopped cold even on an unpatched runtime? Does production `blastit` fail closed if mistakenly run without privileges? |
| `implementability` | **Cheap-Model Buildability.** Are crate boundaries, Rust traits, wire frames, and CLI flags specified unambiguously so a cheap `implementer` can build each task in ≤ 600 lines without guessing? Are new dependencies justified and permissively licensed (Apache-2.0 / MIT / BSD)? |
| `testability` | **Verification & Oracles.** Does every requirement `R1..Rn` map to a fast unprivileged test, a protected critical test, and (for security/CVE claims) an exploit-reproduction integration test? Can every code block in `docs/` be executed automatically? |
| `consistency` | **Coherence.** Does the doc agree with `docs/VISION.md`, `ARCHITECTURE.md`, accepted ADRs, and `blastit.toml` conventions? Are there naming mismatches between CLI flags, env vars, presets, and TOML keys? |

Each critic writes `docs/design/<slug>.review-<lens>.md` with findings graded:
- `BLOCKER`: breaks DX simplicity, leaks secrets/sandbox, allows CVE bypass, or contradicts `VISION.md`.
- `MAJOR`: significant UX friction, prose bloat, missing AI verification path, unbounded resource, or underspecified seam.
- `MINOR`: polish item.

## 2. Technical design doc template (`docs/design/<slug>.md`)

```markdown
---
status: Draft            # Draft | In review | Accepted | Superseded by <slug>
owner: <designer|architect>
node: <plan node id>
reviewers: []
---
# <Title>

## 1. Developer Experience Target (Local, CI/CD, Deploy)
(Link to or summarize the exact user-facing commands, `blastit.toml` syntax, and error messages this design serves.)

## 2. Goals & Non-Goals
## 3. Requirements (R1..Rn, numbered and testable)
## 4. Architecture & Data Flow (Mermaid diagrams, Rust traits, wire/WASM contracts, resource budgets)
## 5. Security & CVE Analysis (threat boundaries, parser differential prevention, `php2shell`/`node2shell`/polyglot impact)
## 6. Alternatives Considered (≥ 2, with why-not — especially why-not more config knobs!)
## 7. Validation & Critical Tests Plan (mapping R1..Rn -> unit, critical, runnable-docs, e2e/CVE tests)
## 8. Implementation Plan (ordered leaf tasks with paths and acceptance commands)
## 9. Review Resolution
```

## 3. Resolution & Acceptance

1. For every critic finding, record `accepted -> updated §X`, `rejected -> reason`, or `deferred -> node` under `## Review Resolution`.
2. If a `BLOCKER` or `MAJOR` change substantially altered the design or UX, re-run that lens once.
3. Core DX shifts and `architect`-owned docs are accepted by `strategist` (recorded via `plan.py escalate`, gate-exempt). Scoped crate designs are accepted by `lead` once zero `BLOCKER`/`MAJOR` findings remain.
4. Record lasting architectural or UX choices as 1-page ADRs in `docs/decisions/ADR-NNNN-<slug>.md` (`Proposed` -> `Accepted`).
