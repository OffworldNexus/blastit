---
name: Design docs & DX specs (elephant drafts, parallel UX + arch reviews)
description: How Blastit writes, reviews and accepts User-Facing DX Specifications and Technical Design Documents + ADRs using `writer`/`architect` and parallel `review-ux` + `review-arch` passes.
---

# Design docs & DX specs: Drafts and Parallel Specialist Reviews

The author (`writer` for user DX guides/specs, `architect` for major UX surfaces and security/runtime architectures) writes a crisp draft. Fresh-context specialist reviewers (`review-ux` and `review-arch`) review in parallel with zero conversation history.

In Blastit, there are **two distinct artefacts**, always ordered DX-first:

1. **User-Facing DX Documentation (`docs/**` + `README.md` or `docs/design/dx-<slug>.md`)** — `<130` lines/page, immediate 3-step zero-`sudo` happy path before any caveats, Mermaid diagram + `!!!` admonition, showing **Local (`blastit dev` / `--learn`)**, **CI/CD (`blastit check` / `--json`)**, and **Deployments (`--preset` / fail-closed)**.
2. **Technical Design Document (`docs/design/<slug>.md` + `docs/decisions/ADR-NNNN-<slug>.md`)** — Concrete Rust types, Constructor IoC from `BootMode`, cohesive `<500` LOC module boundaries, `blastit-view-*` protocol crates over `view.chan` frames, WASM memory/fuel budgets, CVE analysis, and **3–4 coarse vertical implementation slices**.

## 1. Parallel Specialist Review (`review-ux` + `review-arch`)

Spawn `review-ux` and `review-arch` in parallel against the draft:

| Reviewer | What it checks |
|---|---|
| `review-ux` | **Human + AI Developer Experience & Visual Hierarchy.** Immediate 3 zero-`sudo` commands before caveats (`!!! warning` / `??? note`), Mermaid `flowchart`/`sequenceDiagram`, `<130`-line leanness, valid `Blastfile` syntax (single top-level `entrypoint`, nested `view` grants, non-empty `postgres` blocks, clean `"/*"` / `regex '...'`), actionable CLI output (`NO_COLOR`, clickable `http://`, dead-grant `sql`/`proxy`/`fix.to`), zero diary headings or roadmap apologies. |
| `review-arch` | **True Constructor IoC, Cohesion, Deduplication & Zero-Trust Security.** Single composition root (`BootMode`), zero `OnceLock`/`Mutex` globals or `env::var` leaks, zero single-impl mock traits, `<500` LOC modules, reuse of canonical workspace crates (`sqlpolicy`, `elf`, `blastit-frame`), WASM `O(1 page)` fuel/memory caps, stage-0 `execve` secret barrier, real socket/frame lab test plan (both allowed + denied/adversarial `php2shell`/`node2shell`/SQLi/polyglots, zero `Mock*Server`, zero `scripts/check_*.py` regex guards). |

Findings are graded `[BLOCKER]` or `[MAJOR]` (actionable defects with exact `file:line` and concrete fix).

## 2. Technical design doc template (`docs/design/<slug>.md`)

```markdown
---
status: Draft            # Draft | In review | Accepted | Superseded by <slug>
owner: architect
node: <plan node id>
---
# <Title>

## 1. Developer Experience Target (Local, CI/CD, Deploy)
(Link to or summarize the exact user-facing commands, `Blastfile` syntax, and CLI output this design serves.)

## 2. Goals & Non-Goals
## 3. Requirements (R1..Rn, numbered and testable)
## 4. Architecture & Constructor IoC (Mermaid diagrams, concrete types, `view.chan`/wire/WASM contracts, `<500` LOC module layout, resource budgets)
## 5. Security & CVE Analysis (trust boundaries, parser differential prevention, `php2shell`/`node2shell`/polyglot/CGI-parameter smuggling neutralization)
## 6. Alternatives Considered (>= 2, especially why-not extra config knobs or mock-trait seams)
## 7. Lab-First Validation & Critical Tests Plan (real `UnixStream::pair()`/`UnixListener` allowed + adversarial tests; zero `Mock*Server`, zero source-grep scripts)
## 8. Vertical Implementation Slices (3–4 coarse slices with explicit `paths`, `owner`, and `make fast-check` / `make docs-check` acceptance)
## 9. Review Resolution
```

## 3. Resolution & Acceptance

1. Resolve every genuine `[BLOCKER]` and `[MAJOR]` finding and record concise dispositions under `## Review Resolution`.
2. Core DX shifts and `architect`-owned docs are accepted by `strategist` (`plan.py escalate`).
3. Record lasting architectural or UX choices as 1-page ADRs in `docs/decisions/ADR-NNNN-<slug>.md` (`Proposed` -> `Accepted`).
