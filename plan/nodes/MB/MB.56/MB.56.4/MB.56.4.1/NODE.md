---
id: MB.56.4.1
title: "e2e/apps + run_apps.py: PHP/PostgreSQL/static, Django & Flask, interpreter-free Static"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T21:07"
updated: "2026-11-20T21:07"
---

# Goal
Consolidate the application/runtime E2E into `e2e/apps/` + `e2e/run_apps.py` so every `TESTING_PLAN.md` §2 story runs end-to-end against real frameworks and a real PostgreSQL 18.

1. **PHP front controller (Laravel/Symfony shaped) + public static files**: `public/index.php` + `public/assets/app.css`, several views with **distinct per-view PostgreSQL table/column rights** — public `GET "/products/*"` reading `products(id, name, price)`, `POST "/orders"` inserting `orders`, and proof a compromised public view is denied `SELECT` on another table's columns and denied `INSERT`. Plus `exit()`/fatal recovery and `php2shell` neutrality.
2. **Django and Flask/SQLAlchemy multi-view apps**: shared `trait` grants, parameterized read/write, static mounts, each view only its declared SQL, zero credentials in the worker environment; across CPython 3.13 and 3.14.
3. **Standalone Static & SPA container**: bare `debian:trixie-slim` / `alpine` with **neither Python nor PHP installed** — `immutable`, `ETag`/`304`, SPA `fallback`, confinement (`..`, symlinks, dotfiles, `.php`).
4. Absorb the app-side files (`e2e/scenario.py`, `e2e/grants.py`, `e2e/sqlalchemy_check.py`, `e2e/static_assets.py`, `e2e/php/*`, `e2e/sqlalchemy/*`, `e2e/static_site/*`, `e2e/demo/*`, `e2e/check_logs.py`) and delete every superseded one; keep `e2e/run.py` as the entry point or delete it in favour of `run_apps.py`.
5. Every leg uses free ports `28080+` and `/tmp/opencode/...`, never stops foreign containers (`n2f-vuln`, `orange_poc_*`).

# Acceptance
- `uv run --no-project e2e/run_apps.py` passes on this host (Docker available); `make fast-check` green.
