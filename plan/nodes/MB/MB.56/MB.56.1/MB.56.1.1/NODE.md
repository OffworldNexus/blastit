---
id: MB.56.1.1
title: "Retire legacy Python modules + --internal-json RPC bridge; slim blastit wheel to a pure-stdlib launcher"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "python/**"
  - pyproject.toml
  - Makefile
  - crates/blastit/src/cli.rs
  - "crates/blastit/src/cli/**"
  - crates/blastit/src/main.rs
  - crates/blastit/tests/critical_internal_json.rs
  - crates/blastit/tests/critical_handoff.rs
  - scripts/repack_universal_wheel.py
  - "scripts/check-bundle-layout.sh"
  - scripts/test_packages.sh
  - "scripts/wheel-smoke.sh"
  - "scripts/wheel-drive.sh"
  - e2e/universal_binary.py
  - e2e/dx_replay.py
  - .github/workflows/docs.yml
  - .github/workflows/ci.yml
  - "docs/reference/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T16:04"
updated: "2026-11-20T16:24"
---

# Goal
Retire the legacy `0.1.0` Python surface and the `--internal-json` RPC bridge so the published wheel is nothing but a tiny pure-stdlib `execvp` launcher for the shipped Rust binaries.

# Context
- `python/blastit/pytest_plugin.py` (2646), `fold.py` (1351) and `logs.py` (528) are dead weight: `_check_pytest.py` never issues `_blastit` RPCs.
- `crates/blastit/src/cli/internal_json.rs` (224) + `crates/blastit/tests/critical_internal_json.rs` (358) exist only to serve `python/blastit/_blastit.py`; the flag is also parsed in `crates/blastit/src/cli.rs` / `cli/parse.rs` and mentioned in `cli/handoff.rs`.
- Target end state: `python/blastit/` = `__init__.py`, `__main__.py`, `_blastit.py` (launcher), plus whatever `_check_pytest`/`check` entry points are genuinely called by the CLI; `python/tests/` ≤ 500 LOC total.
- `TESTING_PLAN.md` is human-approved and READ-ONLY: never list or modify it.

# Work
1. Delete `python/blastit/pytest_plugin.py`, `fold.py`, `logs.py`; slim `_blastit.py` and `__main__.py` to a pure-stdlib launcher that `execvp`s `_bin/blastit` / `_bin/blastit-lsp` (forwarding argv, including `--role=...`); console scripts are exactly `blastit` and `blastit-lsp`.
2. `pyproject.toml`: drop `[project.entry-points.pytest11]`, the `blastit-manifest` script and the `pytest --blastit-manifest` prose; `dependencies = []`. Do **not** change `build-backend` here (still `blastit_build`, still staging `_bin/blastit-dbproxy`) — that is slice `.2`.
3. Delete `crates/blastit/src/cli/internal_json.rs` and `crates/blastit/tests/critical_internal_json.rs`, and every `--internal-json` flag/arm/helper/test reference in `crates/blastit/src/cli.rs`, `crates/blastit/src/cli/parse.rs`, `crates/blastit/src/cli/handoff.rs`.
4. Delete retired `python/tests/*` (`runnable_docs/`, `test_docs_assets.py`, `test_fold*.py`, `test_pytest_plugin*.py`, `test_live*.py`, `test_logs.py`, `test_release_scripts.py`, `test_runnable_docs.py`, `critical/test_lsp_stdio.py`, `critical/test_fold_literals_and_atomic.py`) keeping only a minimal wheel-launcher + `_check_pytest` suite; update the Makefile `docs-unit`/`docs-test`/`py-lint` recipes accordingly.
5. Remove the retired `blastit-manifest fold` section from `docs/reference/cli.md` and `docs/reference/log-events.md`; update the console-script table in `scripts/repack_universal_wheel.py`.

# Acceptance
- `make fast-check` green
- `make docs-check` green
- `rg -n "pytest11|internal-json|internal_json|blastit-manifest" python/ pyproject.toml crates/blastit/src crates/blastit/tests scripts/repack_universal_wheel.py` returns nothing
- `python/blastit/` contains no module > 150 LOC; `python/tests/` ≤ 500 LOC

# Notes
- Keep `python/blastit/_blastit.py` public API stable enough that `python/blastit/__init__.py` and the CLI entry points keep importing it.
