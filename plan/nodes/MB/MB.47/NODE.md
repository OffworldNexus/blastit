---
id: MB.47
title: "Align rootless e2e, ci.yml py-sapi/wheel shims, venv-smoke py3-none glob, root-safe critical tests & standalone archive Python shims"
kind: epic
status: done
priority: 1
depends_on: []
paths:
  - Makefile
  - ".github/workflows/**"
  - "scripts/**"
  - "e2e/**"
  - crates/blastit/tests/critical_handoff.rs
  - crates/blastit/tests/critical_python_shim.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T07:49"
updated: "2026-11-20T08:40"
---

# Goal

Epic for the 5 CI/release gaps found during the `release/0.2.0` pre-push rehearsal, split into two sequential vertical slices: **MB.47.1** aligns the rootless Docker e2e with the shipped `ADR-0017` `blastit-init` file-capability layout, fixes the `e2e/venv-smoke.sh` `py3-none` wheel glob and makes the two root-sensitive critical tests pass as `uid 0` (`e2e/**`, `crates/blastit/tests/critical_{handoff,python_shim}.rs`); **MB.47.2** gives CI its CPython shims and root provenance, makes every CI wheel build use `--require-shims py`, and bundles `libblastit_py3{13,14}` into the standalone `.tar.gz` (`Makefile`, `.github/workflows/**`, `scripts/**`). No production code; every path belongs to exactly one slice.

# Acceptance

- **MB.47.1** done: `e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt --port 28080 --tls-port 28443` passes 100%; `critical_handoff` + `critical_python_shim` pass as `uid 0` in `debian:trixie-slim`; `make fast-check`.
- **MB.47.2** done: `make fast-check && make workflow-check && make packaging-gates-test && uv run --no-project scripts/platform_matrix.py --check`; the `binaries_musl` archive carries `libblastit_py313.so` and `libblastit_py314.so`.
- `uv run --no-project tools/critical_guard.py --base origin/develop` clean for both nodes; no path glob overlaps between them.
