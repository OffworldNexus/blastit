---
id: ME
title: "Coding agent integrations: llms.txt, portable manifest skill, plugins/MCP (Claude Code, OpenCode, Codex, Junie) (0.5.x)"
kind: milestone
status: ready
priority: 4
depends_on: [MD]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T19:57"
updated: "2026-11-14T19:57"
---

# Goal

Make Blastit first-class for coding agents: an agent-consumable publication (llms.txt, llms-full.txt, per-page raw markdown), a portable **manifest-writing skill**, and plugins/MCP tools for Claude Code, OpenCode, Codex and Junie — so an agent can adapt an app, fill a `Blastfile` and verify completeness without reading the HTML site or guessing at diagnostics.

# Context

The human DX is now documented (Local / CI / Deploy). Agents are the other half of the audience named in `docs/VISION.md`, and today they have only `blastit check --json` and prose. The grounding already exists: the nine capability slots (`MB.19`), the `--json` contract shipped in 0.1.x, and the comment-preserving surgical editor (`edit.rs`, `MB.2.2.2`/`MB.3.21`). This milestone adds the agent-facing surface on top of those, not a parallel implementation.

DX-first applies with the same force: the agent-facing documentation for each integration is written and critiqued (`dx` + `ai-ergonomics`) before any plugin code.

# Acceptance

- **Publication:** `llms.txt` generated at the site root as the agent-facing sitemap — title, canonical URL and a one-line description per page — with **bidirectional URL parity against `sitemap.xml`** (which stays, for crawlers); `llms-full.txt` concatenation free of secrets/PII; per-page raw `.md` sources reachable; `robots.txt` referencing `llms.txt` and not blocking the target agents.
- **Skill:** a portable manifest-writing skill that encodes the nine capability slots and the **finding → fix mapping** — `missing_grant` → `add_query`/`add_group`, `dead_grant` → remove (never `unsafe`), `unrouted` → declare `[[route]]`, missing `[route.ingress]` → body policy — reusing the single `edit.rs` editor.
- **Plugins / MCP:** ≤3-step, zero-`sudo` integrations for Claude Code, OpenCode, Codex and Junie, exposing `check --json`, `learn` and a comment-preserving manifest edit.
- **Contract:** versioned `--json` schema, CI conformance test against emitted output, and a documented deprecation rule.
- **Proof:** golden transcripts per agent (CI where scriptable, documented where not) of a fixture app adapted from scratch with only the skill and `check --json`.
- **Agent-safe:** never teaches `unrestricted = true`/`unsafe`, never prints secrets, fails closed on an incomplete manifest.

# Notes

- Depends on `MD` (proxies) for a complete backend story, but the publication and skill work can start earlier if prioritised.
- The skill and plugins are thin: all intelligence stays in `check --json` and `edit.rs`; the integration must not fork manifest logic.
