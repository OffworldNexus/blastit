---
id: MB.41.1
title: "Rewrite core onboarding spine (zensical.toml, README, Home, Getting started, Blastfile, Local, CI/CD) with Linux + cloud app-server framing & linked Python/PHP/Static tabs"
kind: task
status: done
owner: writer
priority: 1
depends_on: []
paths:
  - zensical.toml
  - README.md
  - docs/index.md
  - "docs/getting-started.md"
  - docs/blastfile.md
  - docs/local.md
  - docs/ci.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T12:44"
updated: "2026-11-19T13:00"
---

# Goal
Rewrite the core onboarding spine (`zensical.toml`, `README.md`, `docs/index.md`, `docs/getting-started.md`, `docs/blastfile.md`, `docs/local.md`, `docs/ci.md`) around Blastit's true cloud application-server identity, upfront Linux requirement, and synchronized linked content tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`).

# Exact Requirements (Owner Feedback — Mandatory!)
1. **`zensical.toml` (`:10`):**
   - Update `site_description` to cover Python (WSGI), PHP (PSR / front controllers), and static sites on Linux (replacing `"A zero-trust WSGI harness..."`). Keep `content.tabs.link` and `pymdownx.tabbed.alternate_style = true` intact!
2. **Cloud Application-Server Positioning & Zero HTTP/2 Apology (`docs/index.md`, `README.md`):**
   - Delete `docs/index.md:86-93` (`"Yes for... No for HTTP/2, WebSockets or an upload codec..."`)!
   - Frame Blastit clearly: **Blastit is a Linux application server & per-route sandbox built to sit behind your cloud ingress or load balancer** (Traefik, DigitalOcean App Platform, Cloudflare, nginx — TLS and HTTP/2 terminate at the ingress; Blastit isolates your application routes and database credentials behind it).
   - Frame around **application interfaces**, never raw HTTP plumbing:
     * **Python (`WSGI`):** Django, Flask, SQLAlchemy, psycopg 3 (CPython 3.13+).
     * **PHP (`PSR-7` / `PSR-15` & front controllers):** Symfony, Laravel, Slim (`public/index.php` via embedded NTS PHP 8.3, 8.4, 8.5).
     * **Static files & SPAs:** Built-in zero-interpreter serving (`static` mounts with `ETag`, `immutable`, SPA `fallback`).
   - State **Linux** right at the top of both `README.md` and `docs/index.md` (`blastit-lsp` runs natively on Linux, macOS, and Windows editors; on macOS/Windows laptops, run `blastit` in Docker/Compose or a Linux dev VM).
   - In `docs/index.md`, use synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) under `## One file, no code changes` showing each golden path's 10-line `Blastfile` + CLI commands!
3. **`docs/getting-started.md`:**
   - Replace line 3 (`"Linux, CPython 3.13 or newer, no root"`) with a crisp upfront **Linux** statement + 3 clear bullet points:
     * **For Python (WSGI):** CPython 3.13+ (`uv add blastit`)
     * **For PHP:** PHP 8.3, 8.4, or 8.5 NTS embed library (`apt install libphp-embed` / `apk add php84-embed` / `dnf install php-embedded`) + standalone `blastit` binary
     * **For static files & SPAs:** Standalone `blastit` binary (no language runtime required)
   - Use synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) across **1. Install**, **2. Write two files (`Blastfile` + app)**, and **3. Run `blastit dev` & test**!
   - IMPORTANT: Check `scripts/platform_matrix.py` before editing platform counts/claims in `getting-started.md` so `make platform-matrix-check` stays 100% green!
4. **`docs/blastfile.md`:**
   - Use synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) for `## The file`, `## Local development`, and `## CI/CD` so each language sees its own `entrypoint`, `Blastfile`, and CLI commands (`uv run blastit ...` vs standalone `blastit ...`) as a first-class golden path!
5. **`docs/local.md`:**
   - State **Linux** right away at the top (plus a compact `??? note "Developing on macOS or Windows?"` showing Docker/Compose dev + native `blastit-lsp` in Zed).
   - Use synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) for starting `blastit dev`, connecting PostgreSQL + `DATABASE_URL`, single-pass `--learn`, and live `fanotify` reload! Keep the Zed `settings.json` fence intact (`make zed-check` verifies it!).
6. **`docs/ci.md`:**
   - Replace the Python-only opening with synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) covering each language's exact CI golden path (including GitHub Actions workflow YAML per tab: `uv run blastit check` for Python; `blastit fmt --check` + `blastit check` manifest/SQL/libphp preflight + HTTP smoke run against `blastit dev` for PHP; `blastit fmt --check` + `blastit check` root verification for Static & SPA), ending with `## Where next` -> [Deployments](deploy/index.md)!

# Acceptance
- `make docs-check platform-matrix-check zed-check`
