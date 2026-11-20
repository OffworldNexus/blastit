---
id: MB.56.4.4
title: "Wire workflows: drop the host-tree smoke (root), run every §2/§3/§6 story on the matrix"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.4.3]
paths: [".github/**", "e2e/**", "scripts/**", Makefile, "docs/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T21:07"
updated: "2026-11-20T21:07"
---

# Goal
Wire `.github/workflows/` so every `TESTING_PLAN.md` §2, §3 and §6 story actually runs on the platform matrix, and delete the superseded CI shapes.

1. `ci.yml`: remove the old host-tree `smoke (root)` job (replaced by containerized `e2e/run_prod.py`), and run `e2e/run_apps.py` / `e2e/run_prod.py` / `e2e/run_packaging.py` where their Docker is available.
2. `php-sapi.yml`: PHP **8.3, 8.4, 8.5, 8.6** across `x86_64` and `aarch64`, `gnu` and `musl`, with the boot-level `e2e_dev` PHP legs and a hard failure when they skip.
3. `py-sapi.yml`: CPython **3.13 and 3.14** WSGI legs.
4. `wheels.yml` / `packages.yml` / `publish-pypi.yml`: `.deb`/`.rpm`/`install.sh`/`py3-none` wheel/Composer on `x86_64` + `aarch64` (`gnu` & `musl`), macOS/Windows `blastit-lsp` legs.
5. Every lifted step is replaced, not duplicated: no job may name a script or test target that no longer exists, and `scripts/platform_matrix.py` stays the single matrix source.
6. Update `e2e/README.md` and any `docs/` page that describes the harness.

# Acceptance
- Every workflow parses, `needs:` resolve, and no step names a removed file; `make fast-check` green; `make docs-check` green.
