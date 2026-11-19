---
id: MB.41
title: "Global multi-language golden-path documentation overhaul (Linux + cloud app-server framing + linked Python/PHP/Static tabs) & Docker php-sapi fix"
kind: epic
status: done
priority: 1
depends_on: []
paths: ["**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T12:44"
updated: "2026-11-19T14:11"
---

# Goal
Overhaul the entire public documentation site (`zensical.toml`, `README.md`, and all 27 public `docs/**/*.md` pages) around:
1. **True product positioning:** Blastit is a **Linux zero-trust application server** built to sit behind a cloud ingress / load balancer (Traefik, Cloudflare, DigitalOcean App Platform, nginx) — NOT an edge proxy (remove defensive HTTP/2 & WebSocket disclaimers!). It manages **framework interfaces**: **WSGI** for Python, **PSR-7 / PSR-15 & front controllers** for PHP, and **static files / SPAs**.
2. **Linux stated right away** at the top of `README.md`, `docs/index.md`, `docs/getting-started.md`, and `docs/local.md` (with Docker/VM guidance for macOS/Windows laptops, plus native cross-platform `blastit-lsp`).
3. **Synchronized Multi-Language Golden Paths (`content.tabs.link`):** Linked content tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) across `index.md`, `getting-started.md`, `blastfile.md`, `local.md`, `ci.md`, and `deploy/*` so Python, PHP, and Static developers each get a complete, first-class golden path!
4. **Full gate + Docker `php-sapi` verification:** Update `docs_fences.rs` and `python/tests/runnable_docs/` to parse/verify 4-space indented fences inside `=== "..."` tabs, pass browser `visual-checker`, and pass `--security-opt seccomp=unconfined` in `Makefile:rs-test-php-image` so all 65 CI checks are green.

# Acceptance
- `make check`
- `make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 && make rs-test-php-image`
