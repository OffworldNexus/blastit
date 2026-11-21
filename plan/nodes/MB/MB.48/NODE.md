---
id: MB.48
title: "Set BLASTIT_EXEC in docs/deploy/compose-traefik.md rootless Dockerfile and assert in docs_deploy.py"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["docs/deploy/compose-traefik.md", e2e/docs_deploy.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T08:42"
updated: "2026-11-20T08:45"
---

# Goal
Set `BLASTIT_EXEC=/usr/share/blastit/lib/blastit` in `docs/deploy/compose-traefik.md` §4's `Dockerfile` (`ENV PYTHONUNBUFFERED=1 BLASTIT_EXEC=/usr/share/blastit/lib/blastit`, plus 1 brief clause in the paragraph below explaining that `--role=dbproxy` runs the capability-free shared binary while `blastit-init` carries the file capabilities; keep `/usr/share/blastit/lib` because `install.sh` on `debian:trixie-slim` installs the `.deb` package into `/usr/share/blastit/lib`, and keep `compose.yaml`'s `environment: *app-env` untouched for `e2e/docs_deploy.py`). Also add a check in `e2e/docs_deploy.py` asserting `BLASTIT_EXEC=/usr/share/blastit/lib/blastit` appears in `compose-traefik.md`'s `Dockerfile` block!

# Acceptance
- `uv run --no-project e2e/docs_deploy.py`
- `make docs-check && make docs-unit`
