---
id: MB.53
title: "Preserve caller PYTHONPATH precedence in check.rs and prove full pytest-plugin suite green in Docker"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/src/check.rs, "python/blastit/**", "python/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T12:58"
updated: "2026-11-20T13:13"
---

# Goal
Fix `crates/blastit/src/check.rs:1004` `PYTHONPATH` ordering (`{existing}:{directory}` instead of `{directory}:{existing}`) and prove the **entire** `ci.yml` `pytest-plugin` command (`/opt/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures`) passes with **0 failed, 0 errors**!

# Root Cause
- In `crates/blastit/src/check.rs:994-1007`, for a wheel install (`.../site-packages/blastit/_bin/blastit`), `bundled_python()` returns `.../site-packages`.
- Joining `format!("{directory}:{existing}")` put `.../site-packages` **ahead of** the caller's explicit `PYTHONPATH` (`{existing}`), inverting Python's `sys.path` contract and causing real `site-packages/pytest` to shadow `test_check_contract.py`'s `project.root/stub/pytest` (`no tests ran in 0.05s`, 10 failures in `ci.yml`'s `pytest-plugin` job)!
- Putting `{existing}:{directory}` keeps the caller's explicit `PYTHONPATH` first while still placing `bundled_python()` ahead of the child interpreter's stdlib/`site-packages` (so `blastit._check_pytest` still imports when `blastit` is installed globally via `.deb`/`.rpm` or `uv tool`).
- Also check `python/blastit/pytest_plugin.py` `_serveable_project` if the full `pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures` run in Docker hits any remaining `/tmp` provenance failure!

# Acceptance
- Rebuild wheel (`scripts/build-wheel.sh --require-shims py`), install into `/opt/blastit-tests` in Docker (`modelw/base:2026.07` + throwaway `postgres:18`), and prove `/opt/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures` exits `0` (`0 failed, 0 errors`)!
- `make fast-check`
