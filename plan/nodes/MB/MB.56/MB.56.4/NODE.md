---
id: MB.56.4
title: "Consolidate container & multi-platform GHA E2E suites (Laravel/Symfony PHP + Postgres + static, Django/Flask, Static, root/rootless prod & packaging)"
kind: epic
status: done
priority: 1
depends_on: [MB.56.3]
paths: ["e2e/**", "scripts/**", ".github/**", Makefile, "docs/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T22:55"
---

# Goal
Consolidate `e2e/` (`59` files -> clean, real-framework user-story suites) and wire `.github/workflows/` so **every user story in `TESTING_PLAN.md` §2, §3, and §6 runs end-to-end**, leveraging GitHub Actions for multi-platform matrices!

1. **Application & Runtime E2E (`TESTING_PLAN.md` §2 — `e2e/apps/` + `e2e/run_apps.py`):**
   - **Laravel / Symfony-style PHP Front-Controller App + PostgreSQL + Public Static Files:** Build a realistic multi-view PSR/Laravel/Symfony-shaped PHP fixture (`public/index.php` + `public/assets/app.css` static mount + multiple views with **distinct per-view PostgreSQL table & column rights** — e.g. public `GET "/products/*"` reading `products(id, name, price)`, `POST "/orders"` inserting `orders`, and verifying a compromised public view is denied `SELECT` on `users(password_hash)` or `INSERT` on `orders` — PLUS `exit()`/fatal recovery and `php2shell` exploit neutrality!). Run it against real PostgreSQL 18, and wire `php-sapi.yml` / GHA to verify PHP **8.3, 8.4, 8.5, and 8.6** (`x86_64` & `aarch64`, `gnu` & `musl`)!
   - **Django & Flask / SQLAlchemy Multi-View Apps:** Verify multi-view read/write PostgreSQL table/query/trait grants + static mounts across **Python 3.13 and 3.14**, proving each view can execute only its own declared SQL and sees zero credentials.
   - **Standalone Static & SPA Container:** Run in bare `debian:trixie-slim` / `alpine` with **neither Python nor PHP installed**, proving `immutable` caching, `ETag`/`Range`, SPA `fallback`, and path confinement (`..`, symlinks, dotfiles, `.php` source blocked).

2. **Production Zero-Trust Isolation E2E (`TESTING_PLAN.md` §6 — `e2e/run_prod.py`):**
   - **Root preset (`--preset digitalocean-paas`):** Docker Compose with PostgreSQL 18 verifying per-view OS UIDs (`>= 20000`), zero DB credentials in `/proc/<pid>/environ`, blocked outbound TCP & shell execution, cross-view DB policy isolation, fair queuing under slow views, and zero-drop `SIGTERM` drain.
   - **Rootless capability preset (`--preset compose-traefik`):** Docker Compose behind Traefik v3 + PgBouncer + PostgreSQL 18 verifying `0500` `blastit-init` file capabilities, zero capabilities + `NO_NEW_PRIVS` in all child workers, worker UID `10001` refused `blastit-init`, trusted `X-Forwarded-For` / `X-Forwarded-Proto`, and tampered PHP entry-file refusal.

3. **Installation & Packaging E2E (`TESTING_PLAN.md` §3 — `e2e/run_packaging.py` + GHA):**
   - Verify `.deb` (`apt`), `.rpm` (`dnf`), `install.sh --user` + Alpine `musl` (with checksum mismatch refusal), universal `py3-none` wheel (Python 3.12 CLI/check + 3.13/3.14 WSGI), and Composer wrapper (`vendor/bin/blastit` + corrupt archive refusal), leveraging GHA (`ci.yml`, `publish-pypi.yml`, `packages.yml`, `py-sapi.yml`, `php-sapi.yml`) for `x86_64` + `aarch64` (`gnu` & `musl`) and macOS/Windows `blastit-lsp`!
   - Remove deleted/obsolete `ci.yml` jobs (including the old host-tree `smoke (root)` job now replaced cleanly by containerized `e2e/run_prod.py`!) and delete superseded legacy files in `e2e/`.

# Acceptance
- Local Docker E2E runs (`e2e/run_apps.py`, `e2e/run_prod.py`, `e2e/run_packaging.py` on free ports `28080+`, never stopping `n2f-vuln`) pass 100%
- `make check` green
