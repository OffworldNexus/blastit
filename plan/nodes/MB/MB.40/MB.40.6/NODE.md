---
id: MB.40.6
title: "Visual documentation hierarchy pass: Python+PHP+static README, Mermaid diagrams, admonitions, command-first guides & runnable-docs check"
kind: task
status: ready
owner: writer
priority: 1
depends_on: [MB.40.5]
paths: [README.md, "docs/**", zensical.toml, "python/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T00:36"
---

# Goal
Upgrade `README.md` and `docs/**` visual hierarchy (Mermaid diagrams, `!!!` admonitions, command-first happy paths, fixed headings, non-empty framework `Blastfile` grants) and pass the full `make check` gate (including `zensical build --strict` and runnable-docs execution).

# Exact Targets (`file:line` pre-diagnosed in `/tmp/opencode/audit-ux.md` Part B)
1. **`README.md` (`:11` and quickstart):** Present Blastit `0.2.0`'s full scope (Python WSGI, embedded NTS PHP, and static sites — not Python-only!), update boot banner / `blastit check` output snippets to match Slice 5, and keep the 3-step zero-`sudo` quickstart crisp.
2. **Command-first openings & visual hierarchy (`docs/index.md`, `docs/getting-started.md`, `docs/local.md`, `docs/ci.md`, `docs/blastfile.md`, `docs/security.md`):**
   - `docs/getting-started.md`: Move the 10-line platform/wheel matrix (`:12-21`) out of the middle of Step 1 into a collapsed `??? note "Supported platforms & standalone binary"` below the 3 steps, and add a compact Mermaid request-flow diagram.
   - `docs/local.md`: Lead immediately with `uv run blastit dev` / `./blastit dev` commands (`:3-9`'s 5 defensive caveats move into a concise `!!! warning` admonition *after* Command 1!), plus a Mermaid diagram of live reload / `--learn`.
   - `docs/ci.md`: Add a Mermaid `sequenceDiagram` (`pytest -> blastit check -> dbproxy -> audit report`), verify store-less + Postgres examples, and add a `## Where next` forward link to `deploy/index.md` at the bottom!
   - `docs/index.md`, `docs/blastfile.md`, `docs/security.md`: Add a trust-boundary / architecture Mermaid diagram to `index.md` and `security.md`, add `!!! note`/`warning` callouts, and purge roadmap language (`"Until Milestone C's CDR"`, `"until the HTTP proxy milestone"`) from `docs/security.md:83-85,167`.
3. **Deploy & Framework pages (`docs/deploy/{digitalocean-paas,compose-traefik}.md`, `docs/frameworks/**`):**
   - Replace diary heading `## What we thought the config would be` (`digitalocean-paas.md:48`, `compose-traefik.md:49`) with `## Preset defaults` (or `## Recommended configuration`) and trim redundant prose toward <=150 lines.
   - In `docs/frameworks/index.md`, surface the architecture overview + Mermaid request-flow diagram up front and highlight PHP's 0.2.0 `dev --learn` workflow clearly in a `!!! note` callout.
   - In `docs/frameworks/python/index.md:17-19` and `docs/frameworks/php/index.md:24-29` (and `django.md`, `flask.md`, `symfony.md`, `laravel.md`), replace empty `postgres {}` blocks (which deny all SQL!) with real non-empty `postgres { query 'SELECT ...' }` grants and add compact Mermaid diagrams where helpful.
4. **Runnable-docs alignment (`python/tests/runnable_docs/**` if any console output assertion checks the old banner/help format):** Ensure `make docs-check` (`zensical build --strict`) and `make docs-test` pass 100%!

# Acceptance
- `make docs-check`
- `make check`
