---
id: MB.41.2
title: "Align Deployments, Frameworks, Reference, How-it-works, Security & Troubleshooting with multi-language golden paths & cloud app-server framing"
kind: task
status: ready
owner: writer
priority: 1
depends_on: [MB.41.1]
paths:
  - "docs/deploy/**"
  - "docs/frameworks/**"
  - "docs/reference/**"
  - "docs/how-it-works.md"
  - docs/security.md
  - docs/troubleshooting.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T12:44"
updated: "2026-11-19T12:44"
---

# Goal
Audit and update the remaining public documentation pages (`docs/deploy/*`, `docs/frameworks/**`, `docs/reference/*`, `docs/how-it-works.md`, `docs/security.md`, `docs/troubleshooting.md`) so the entire site shares one consistent Linux + cloud-ingress application-server model and multi-language golden path (Python WSGI, PHP PSR/front-controller, Static & SPA).

# Exact Targets
1. **`docs/deploy/{index,digitalocean-paas,compose-traefik,anything-else}.md`:**
   - In `deploy/index.md`: reinforce the cloud-ingress / load-balancer -> Blastit application server -> PgBouncer pooler -> PostgreSQL architecture cleanly; remove negative filler (`"answers 501 to Upgrade"`, `"0.2.0's only proxy protocol"`).
   - In `digitalocean-paas.md`, `compose-traefik.md`, and `anything-else.md`: use synchronized tabs (`=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`) where container/install/start snippets differ (`uv` wheel vs standalone `blastit` archive + `libphp-embed`), keeping preset tables crisp and scannable!
2. **`docs/frameworks/{index,python/*,php/*,static-and-spa}.md`:**
   - Frame `frameworks/index.md` cleanly around **application interfaces** (**WSGI** for Python, **PSR-7 / PSR-15 & front controllers** for PHP, **Static & SPA**), replacing the dry 9-question numbered list with a scannable capability overview table!
   - Ensure `python/{index,django,flask}.md` and `php/{index,symfony,laravel}.md` each read as a clean, consistent golden path (`Blastfile` with real non-empty `postgres` grants + static assets + `blastit dev --learn` + CI + deploy).
   - **CRITICAL:** In `docs/frameworks/static-and-spa.md`, preserve the exact `Blastfile` and `blastit dev` banner snippet lines that `e2e/static_assets.py` replays in CI (`grep` `e2e/static_assets.py` before touching `static-and-spa.md`!).
3. **`docs/how-it-works.md`, `docs/security.md`, `docs/troubleshooting.md`, `docs/reference/{cli,blastfile,settings}.md`:**
   - `how-it-works.md`: make the request sequence diagram & text runtime-neutral (`WSGI callable or PHP front controller`, never `"routing happens before Python"`!).
   - `troubleshooting.md`: remove line 65 (`"501 on an Upgrade request | WebSockets and HTTP/2 are not implemented"`), and add crisp PHP symptoms (`no usable NTS libphp`, ZTS vs NTS, `.venv` CPython minor mismatch).
   - `reference/{cli,blastfile,settings}.md`: ensure CLI examples and invocation notes treat standalone `blastit` (PHP / static / `make install`) and `uv run blastit` (Python wheel) as equal peers, and check `scripts/platform_matrix.py --check`!

# Acceptance
- `make docs-check platform-matrix-check zed-check`
