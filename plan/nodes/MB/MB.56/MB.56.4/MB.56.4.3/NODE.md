---
id: MB.56.4.3
title: "e2e/run_packaging.py: deb/rpm/install.sh/wheel/composer"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.4.2]
paths: ["e2e/**", "scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T21:07"
updated: "2026-11-20T22:45"
---

# Goal
Consolidate the installation/packaging E2E into `e2e/run_packaging.py` for `TESTING_PLAN.md` §3.

1. `.deb` (`apt`), `.rpm` (`dnf`), `install.sh --user` + Alpine `musl` (checksum-mismatch refusal), the universal `py3-none` wheel (CPython 3.12 CLI/`check`, 3.13/3.14 WSGI), and the Composer wrapper (`vendor/bin/blastit`, corrupt archive refusal).
2. Absorb `e2e/packages/*`, `scripts/test_packages.sh`'s legs, `e2e/prebuilt_wheel.py`, `e2e/venv-smoke/*`, `e2e/php-sapi/*` image runners and `e2e/universal_binary.py` where they overlap; keep one runner per story and delete the rest.
3. Every installer runs as a real user where the story says so (zero `sudo`), and a tampered archive fails closed with a sentence.

# Acceptance
- `uv run --no-project e2e/run_packaging.py` passes on this host; `make fast-check` green.
