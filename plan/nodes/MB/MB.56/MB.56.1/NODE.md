---
id: MB.56.1
title: "Delete retired 0.1.0 Python/Rust code, --internal-json, blastit-audit crate, duplicate blastit-dbproxy bin & meta-linter scripts"
kind: epic
status: ready
priority: 1
depends_on: []
paths:
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - pyproject.toml
  - blastit_build.py
  - AGENTS.md
  - ARCHITECTURE.md
  - RELEASING.md
  - "crates/**"
  - "python/**"
  - "scripts/**"
  - "tools/**"
  - "docs/reference/**"
  - ".github/**"
  - ".opencode/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T16:03"
---

# Goal
Delete useless legacy `0.1.0` Python/Rust code, the `--internal-json` RPC bridge, the duplicate `blastit-dbproxy` binary, `critical_guard.py`, `runnable_docs/`, and all YAML/script/prose meta-linter bloat:

1. **Retire legacy Python (`python/blastit/` & `python/tests/`):**
   - Delete `python/blastit/pytest_plugin.py`, `python/blastit/fold.py`, and `python/blastit/logs.py` (`4,525` LOC; note `_check_pytest.py` never called `_blastit` RPCs!).
   - Slim `python/blastit/_blastit.py` and `python/blastit/__main__.py` to a tiny pure-stdlib `execvp` launcher for `_bin/blastit` and `_bin/blastit-lsp` (`dependencies = []`, console scripts `blastit` and `blastit-lsp` only; remove `[project.entry-points.pytest11]` and `blastit-manifest` from `pyproject.toml` & `repack_universal_wheel.py`; remove `-p no:blastit` comment references where obsolete; remove the retired `blastit-manifest fold` section from `docs/reference/cli.md` and `docs/reference/log-events.md`).
   - Delete `python/tests/runnable_docs/`, `python/tests/test_docs_assets.py`, `python/tests/test_fold*.py`, `python/tests/test_pytest_plugin*.py`, `python/tests/test_live*.py`, `python/tests/test_release_scripts.py`, and `python/tests/critical/test_lsp_stdio.py`; keep only a clean minimal Python wheel-launcher & `_check_pytest` test (`<= 500` LOC total in `python/tests/`).

2. **Delete `--internal-json`, retire `crates/blastit-audit` crate, and drop duplicate `blastit-dbproxy` `[[bin]]`:**
   - Delete `crates/blastit/src/cli/internal_json.rs` and `crates/blastit/tests/critical_internal_json.rs` (zero callers remain!).
   - Move only the live `blastit check` grant-diff & human/JSON report modules needed by `crates/blastit/src/check.rs` into `crates/blastit/src/check/` (deleting the retired `0.1.0` `audit()`/`covering()`/`blastfile_model()` shims and removing `crates/blastit-audit/` from the workspace!).
   - Delete `crates/blastit-dbproxy/src/main.rs` (`[[bin]] name = "blastit-dbproxy"`), `blastit_build.py`, and all `_bin/blastit-dbproxy` wheel staging (`pyproject.toml` switches cleanly to `build-backend = "maturin"`; `check.rs` and stage 0 run `_bin/blastit --role=dbproxy` directly, cutting every wheel's size in half!).

3. **Delete `tools/critical_guard.py`, `mutants-*.sh`, and all `scripts/` meta-linters:**
   - Delete `tools/critical_guard.py`, `scripts/mutants-*.sh`, `scripts/check_workflows.py`, `scripts/check_workflows.py.lock`, `scripts/test_packaging_gates.py`, `scripts/test_publish_gates.py`, `scripts/check_zed_extension.py`, `scripts/test_check_zed_extension.py`, `scripts/check-no-test-seam.sh`, `scripts/check_wheel_shims.py`, `scripts/check_wheel_tag.py`, and `crates/blastit/tests/critical_artifacts.rs` (fold concise inline ELF/zip assertions directly into `build-standalone.sh` and `build-wheel.sh` where artifacts are built!).
   - Remove `STALE_CLAIMS` prose-grepping from `scripts/platform_matrix.py` (keep `platform_matrix.py` strictly as the compact JSON matrix generator for GHA!).
   - Update `Makefile`, `.github/workflows/*.yml` (drop `critical-test guards` & `pytest-plugin` jobs from `ci.yml`, drop deleted script calls from `publish-pypi.yml`), `AGENTS.md`, and `.opencode/` removing `critical_guard.py` and `mutants-*` references.

# Acceptance
- `make check` green
- all 3 sub-slices (`MB.56.1.1`, `MB.56.1.2`, `MB.56.1.3`) `done`
