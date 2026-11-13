---
id: MB.4
title: "Vite & Node HTTP Upstream & node2shell Defense (0.2.2)"
kind: epic
status: ready
priority: 2
depends_on: [MB.3]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-13T10:42"
---

# Goal

Ship **Vite & Node** HTTP upstream backend support (`0.2.2`) with seamless static + SSR split for modern 12-factor frameworks (SvelteKit, React Router/Remix, Nuxt, Hono, Next standalone), wire-level `--learn` / `blastit check`, and `node2shell` defense, starting Docs-First.

# Context

- Mandatory DX-First sequence (`docs/frameworks/node.md` + `inventory.toml` + `dx`/`ai-ergonomics` critics -> HTTP upstream & SSR/asset split design + ADR -> leaf implementation + protected `critical_*.rs` tests -> `e2e/` + `node2shell` exploit regression).
- Split process-lifecycle/cleanup tasks from `--learn` wire attribution tasks (`MA` escalation lesson).

# Acceptance

- [ ] `docs/frameworks/node.md` covering Local (zero `sudo`), CI/CD (`blastit check`), and Deployments accepted by `dx` and `ai-ergonomics` critics before Rust code begins.
- [ ] Per-route Node HTTP upstream workers run behind stage-0 `execve` secret stripping, `NODE_OPTIONS`/env lockdown, seccomp `egress = false` default, and per-route `0600` PostgreSQL sockets, with unified `blastit dev --learn` and `blastit check`.
- [ ] Automated `e2e/` `node2shell` exploit suite proves reverse-shell egress, metadata SSRF, env injection, and Vite `/@fs/` traversal probes fail harmlessly; `make check`, `make e2e`, and epic `test-curator` audit pass.
