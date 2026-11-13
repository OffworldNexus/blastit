---
id: MB.2
title: "Static Asset Serving & Clean URL Rewriting (0.2.0)"
kind: epic
status: ready
priority: 2
depends_on: [MB.1]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-13T10:42"
---

# Goal

Ship safe built-in **static asset serving** and clean SPA / front-controller **URL rewriting** (`0.2.0`) without external nginx regex boilerplate, starting Docs-First.

# Context

- Mandatory order: user-facing DX docs (`docs/` covering Local zero-`sudo`, CI/CD `blastit check`, Deployments, registered in `python/tests/runnable_docs/inventory.toml` in the same commit) + `dx` & `ai-ergonomics` critics -> technical design + ADR + `security`/`implementability` critics -> Rust implementation behind the supervisor/`http`/`scheduler` + protected `critical_*.rs` tests -> `e2e/` + `0.2.0` sub-release prep.
- Security bar: immune to path traversal (`..`, encoded slashes), NUL bytes, symlink escapes outside the declared asset root, and dotfile leaks (`/.env`, `/.git`).

# Acceptance

- [ ] Lean user documentation written and accepted by `dx` and `ai-ergonomics` critics before internal design or Rust implementation begins.
- [ ] Built-in static asset serving and SPA / front-controller URL rewriting implemented with protected `critical_*.rs` positive/negative tests (traversal, NUL, symlink escape, dotfile denial, immutable hashed caching).
- [ ] `blastit dev`, `blastit check` (`--learn` / `--json`), runnable-docs fences, `zensical build --strict`, `make check`, `make e2e`, and epic `test-curator` audit all pass.
