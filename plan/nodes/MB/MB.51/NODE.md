---
id: MB.51
title: "Fix final 3 PR #64 CI items (Requires-Python >=3.12 in check-long-description.py, setup-uv before chown in ci.yml & BLASTIT_REQUIRE_ROOT_TESTS in critical_handoff)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "scripts/check-long-description.py"
  - .github/workflows/ci.yml
  - crates/blastit/tests/critical_handoff.rs
  - "python/tests/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T11:15"
updated: "2026-11-20T11:38"
---

# Goal
Close the final 3 CI items on PR #64 (`c057655`, where `Python SAPI` x8, `PHP SAPI` x16, `Docs`, `docs-gate`, `e2e`, `e2e rootless`, `wheel-smoke`, all 4 universal `py3-none` wheels, all 4 standalone `.tar.gz` archives, `sdist`, and all 5 `blastit-lsp` targets are now 100% GREEN!):

1. **`scripts/check-long-description.py` (`Publish` `description, as PyPI renders it` job):**
   - `check-long-description.py` still expected `Requires-Python` to be `>=3.13`, whereas `pyproject.toml` (`ADR-0017`) is now `>=3.12`. Update `check-long-description.py` to expect `>=3.12` (or read `requires-python` from `pyproject.toml` so they can never drift!) and verify `check-long-description.py` locally against `dist/*.whl`!

2. **`.github/workflows/ci.yml` step ordering in `smoke (root)` & `pytest-plugin` (`EACCES: permission denied, mkdir '/opt/hostedtoolcache/uv'`):**
   - `sudo chown -R root:root /opt/hostedtoolcache` ran *before* `astral-sh/setup-uv@v10.3.2`, preventing `setup-uv` (running as `runner`) from creating `/opt/hostedtoolcache/uv`!
   - Put `astral-sh/setup-uv@v10.3.2` + `uv python install 3.13 3.14` *before* the `chown`/`chmod` step, and `chown -R root:root /opt/hostedtoolcache/Python "${UV_PYTHON_INSTALL_DIR:-$HOME/.local/share/uv/python}"` (plus `chmod go-w /opt` and `chmod -R go-w /opt/hostedtoolcache`) *after* `setup-uv` and `uv python install` finish!

3. **`crates/blastit/tests/critical_handoff.rs:504` + `ci.yml` `smoke` (authorized protected-test update):**
   - GitHub Actions sets `CI=true` on the **unprivileged** `test` job (`uid 1001`) as well as `smoke (root)` (`uid 0`), so checking `std::env::var_os("CI").is_some()` panicked in the unprivileged `test` job (`15 passed; 1 failed` — the ONLY failure in `test`!).
   - Gate that panic on `BLASTIT_REQUIRE_ROOT_TESTS` (`std::env::var_os("BLASTIT_REQUIRE_ROOT_TESTS").is_some()`) instead of ambient `CI`, and pass `"BLASTIT_REQUIRE_ROOT_TESTS=1"` in `ci.yml`'s `smoke (root)` binary runner so `smoke (root)` still strictly enforces root execution of `a_binary_owned_by_another_uid_is_refused` while the unprivileged `test` job passes cleanly with `CI=true`!

# Acceptance
- `CI=true cargo test -p blastit --test critical_handoff --locked` passes unprivileged AND `BLASTIT_REQUIRE_ROOT_TESTS=1` panics unprivileged / passes as root
- `uv run --with readme-renderer[md] --no-project scripts/check-long-description.py dist/blastit-*.whl` (or offline equivalent) passes
- `make fast-check && make workflow-check && make packaging-gates-test`
