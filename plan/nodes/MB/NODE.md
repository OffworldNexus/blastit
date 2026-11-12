---
id: MB
title: "Modern 12-Factor Backends: Static, PHP-FPM, Vite/Node, Tomcat (0.2.x)"
kind: milestone
status: ready
priority: 2
depends_on: [MA]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

Expand Blastit across focused `0.2.x` sub-releases into a **multi-backend zero-trust application harness** supporting modern 12-factor ecosystems beyond Python WSGI:
1. Safe built-in **static asset serving** and clean front-controller / SPA **URL rewriting** (no nginx regex soup).
2. **PHP-FPM** (`FastCGI` protocol, PSR-7/15 front controllers, Symfony & Laravel idioms, locked-down `SCRIPT_FILENAME` & FastCGI params).
3. **Vite & Node** (HTTP upstream per route/worker, seamless Vite asset + SSR split for SvelteKit, Remix/React Router, Nuxt, Hono, Next standalone).
4. **Tomcat / JVM** (modern 12-factor Servlet / Spring Boot integration) and other evaluated popular 12-factor HTTP backends.
5. Unified language-agnostic `blastit dev --learn` and `blastit check` workflows across Python, PHP, Node, and JVM, plus baseline `php2shell` and `node2shell` exploit neutralization tests.

# Context

- Vision & rules: `docs/VISION.md`, skill `dx-first`, skill `security-invariants`.
- **Mandatory DX-First law:** Before designing internals or coding any sub-release (`0.2.0`, `0.2.1`, `0.2.2`...), **start by writing the lean user documentation** showing how that backend looks on the **local machine** (zero `sudo`), in **CI/CD** (completeness verification), and in **deployments** — critiqued by `dx`, `ai-ergonomics`, and `security` goldfish critics!
- Modern 12-factor scope only: play natively with PSR, modern frameworks, static assets, and URL rewriting, without supporting legacy un-12-factorizable monoliths like WordPress.

# Acceptance

- [ ] Each `0.2.x` sub-epic begins with accepted lean user documentation (`docs/frameworks/...`) covering Local, CI/CD, and Deployments, verified by `dx` and `ai-ergonomics` critics before Rust code is written.
- [ ] Static asset serving + URL rewriting handle SPA fallbacks, hashed immutable assets, and PHP `public/index.php` front-controller routing safely (immune to path traversal, null bytes, symlink escapes, and dotfile leaks).
- [ ] PHP-FPM FastCGI backend serves real PSR / Symfony- or Laravel-style routes isolated per route, strips real DB secrets via stage-0 `execve`, enforces `blastit.toml` PostgreSQL grants, and blocks `php2shell` / CVE-2024-4577 FastCGI/CGI argument & `PHP_VALUE` injection attacks in automated `e2e/` tests.
- [ ] Vite + Node SSR/API backend serves static + dynamic routes cleanly in `blastit dev` and production containers, enforces per-route DB grants & egress lockdown, and blocks `node2shell` reverse-shell / env / `/@fs/` traversal vectors in `e2e/`.
- [ ] Tomcat / JVM 12-factor backend runs cleanly under Blastit with per-route sandboxing/routing, env secret swapping, and wire-level `--learn` + `blastit check`.
- [ ] `make check`, `make docs-check`, `visual-checker`, `make e2e`, and `test-curator` milestone audit all pass.

# Notes

- May be shipped as multiple `0.2.x` sub-releases (`0.2.0`, `0.2.1`, `0.2.2`); `strategist` refreshes concrete pointers here at the `MA` gate.
