---
id: MB.56.1.3
title: "Delete critical_guard.py, mutants-*.sh and all scripts/ meta-linters; fold concise ELF/wheel-zip assertions inline and rewire Makefile/GHA/AGENTS"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.1.2.2]
paths:
  - Makefile
  - "tools/**"
  - "scripts/**"
  - ".github/**"
  - AGENTS.md
  - ARCHITECTURE.md
  - RELEASING.md
  - ".opencode/**"
  - "editors/zed/**"
  - crates/blastit/tests/critical_artifacts.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:04"
updated: "2026-11-20T16:25"
---

# Goal
Delete the meta-linter bureaucracy (`tools/critical_guard.py`, mutant runners, YAML/prose gate scripts) and its Makefile / GitHub Actions / AGENTS wiring, folding the few genuinely useful artifact assertions inline into the scripts that build those artifacts.

# Context
- Meta-linters to delete: `tools/critical_guard.py`, `scripts/mutants-run.sh`, `scripts/mutants-scratch.sh`, `scripts/check_workflows.py` (+`.lock`), `scripts/test_packaging_gates.py`, `scripts/test_publish_gates.py`, `scripts/check_zed_extension.py` (+ its test), `scripts/check-no-test-seam.sh`, `scripts/check_wheel_shims.py`, `scripts/check_wheel_tag.py`, and `crates/blastit/tests/critical_artifacts.rs`.
- `scripts/platform_matrix.py` must survive as the compact JSON matrix generator only — no `STALE_CLAIMS` prose grepping.
- Invariants must be enforced by crate/type boundaries and real E2E tests, never 400-line source-grep regex guards.
- `TESTING_PLAN.md` is human-approved and READ-ONLY: never list or modify it. §7 Zed/LSP coverage stays.

# Work
1. Delete the 11 scripts + `tools/critical_guard.py` + `crates/blastit/tests/critical_artifacts.rs`; fold concise inline ELF-matrix / wheel-zip assertions directly into `scripts/build-standalone.sh` and `scripts/build-wheel.sh` where the artifacts are produced.
2. Strip `STALE_CLAIMS` (and its prose-grepping) from `scripts/platform_matrix.py`; keep it emitting the JSON matrix consumed by GHA.
3. `Makefile`: remove the `critical-guard`, `workflow-check`, `packaging-gates-test`, `seam-check`, `platform-matrix-check` targets and their `fast-check`/`check` prerequisites; keep `zed-check` but back it with the real `editors/zed` grammar/highlight fixtures instead of the deleted script.
4. `.github/workflows/*.yml`: drop the `critical-test guards` and `pytest-plugin` jobs from `ci.yml`, drop every call to a deleted script from `publish-pypi.yml` / the wheel action, and let `platform_matrix.py` remain the only matrix source.
5. Update `AGENTS.md`, `.opencode/`, `RELEASING.md` and `ARCHITECTURE.md` to remove `critical_guard.py` / `mutants-*` / `seam-check` / `workflow-check` references and describe the type-boundary + E2E enforcement instead.

# Acceptance
- `make fast-check` green
- `make docs-check` green
- `rg -n "critical_guard|critical-guard|mutants-run|mutants-scratch|check_workflows|packaging-gates-test|seam-check|check_wheel_shims|check_wheel_tag|STALE_CLAIMS" Makefile tools/ scripts/ .github/ AGENTS.md RELEASING.md ARCHITECTURE.md .opencode/` returns nothing
- `make check` green

# Notes
- `scripts/build-wheel.sh --require-shims py` (referenced by the epic) is measured against the new `maturin`-only layout from slice `.2`.
