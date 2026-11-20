---
id: MB.56.4.2
title: "e2e/run_prod.py: root + rootless presets, isolation, fair queueing, drain"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.4.1]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T21:07"
updated: "2026-11-20T21:07"
---

# Goal
Consolidate the production zero-trust E2E into `e2e/run_prod.py` covering `TESTING_PLAN.md` §6 in real containers.

1. **Root preset (`--preset digitalocean-paas`)**: Docker Compose + PostgreSQL 18 proving per-view OS UIDs (>= 20000), zero DB credentials in `/proc/<pid>/environ`, blocked outbound TCP and shell execution, cross-view DB policy isolation, fair queuing under a slow view, and zero-drop `SIGTERM` drain.
2. **Rootless preset (`--preset compose-traefik`)**: Traefik v3 + PgBouncer + PostgreSQL 18 with `0500` file capabilities on `blastit-init`, zero capabilities + `NO_NEW_PRIVS` in every child worker, worker UID `10001` refused `blastit-init`, trusted `X-Forwarded-For`/`X-Forwarded-Proto`, tampered PHP entry refusal.
3. Absorb `e2e/rootless/*` (`run.py`, Dockerfiles, `php_entry_swap.py`, README) and the prod-side legs of `e2e/scenario.py`; delete every superseded file.
4. Split stores (`postgres { host $DB_HOST … }`) must appear in at least one preset leg so the env rewrite is proven end-to-end.
5. Free ports `28080+`, `/tmp/opencode/...`, never stop `n2f-vuln`/`orange_poc_*`.

# Acceptance
- `uv run --no-project e2e/run_prod.py` passes on this host; `make fast-check` green.
