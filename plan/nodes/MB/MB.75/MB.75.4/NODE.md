---
id: MB.75.4
title: "MB.75.4 Real 3.15/3.16 shim builds, 3.15 boot probe, stable-vs-nightly diff, install legs"
kind: task
status: ready
owner: implementer
priority: 4
depends_on: [MB.75.3]
paths:
  - Makefile
  - scripts/py_sapi_leg.sh
  - "scripts/build-musl-wheel.sh"
  - e2e/universal_binary.py
  - "crates/blastit-py-shim/build.rs"
  - "crates/blastit-py-shim/src/**"
  - "crates/blastit-py-sapi/build.rs"
  - "crates/blastit-py-sapi/src/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T11:41"
updated: "2026-11-23T11:41"
---

# Goal

Prove the channel split with **real builds**, not tables: 3.15 and 3.16 shims build against the real interpreters and pass the ELF contract, one boot-level probe serves WSGI on 3.15, the stable/nightly matrices differ by exactly 3.16, and the MB.71/MB.73 derivation legs still pass with the new counts.

# Scope

- Interpreter resolution for a pre-release: `Makefile` (`PY_SAPI_PYTHON_315/316`), `scripts/py_sapi_leg.sh` and `scripts/build-musl-wheel.sh` must handle `cpython-3.16.0a2` without an unscoped `uv python install 3.16` failure. Keep the alpha pin in one named variable/comment, not scattered literals, and keep `uv python find 3.16` working once the alpha is installed.
- `e2e/universal_binary.py`: the python legs are the table's, so 3.15 and 3.16 are real legs; make sure a nightly-only 3.16 shim in the bundle is selectable while the stable artifact set stays 8 names/32 artifacts. Fix only what the real run exposes; do not add a `Mock*Server`.
- `crates/blastit-py-shim/**` and `crates/blastit-py-sapi/**` only if a real build exposes a bug (e.g. an alpha-interpreter check).

# Acceptance (record command + output tail in RESULT.md)

- `uv python install cpython-3.15.2 cpython-3.16.0a2`
- `make py-sapi PY_MINORS="3.15" PY_SAPI_REQUIRE_ALL=1 PY_SAPI_PYTHON_315="$(uv python find cpython-3.15.2)"` builds 1 shim; the target's own readelf/nm asserts `DT_SONAME`, `UND Py_Initialize`, exported `blastit_py_*`.
- `make py-sapi PY_MINORS="3.16" PY_SAPI_REQUIRE_ALL=1 PY_SAPI_PYTHON_316="$(uv python find cpython-3.16.0a2)"` builds `target/py-sapi/libblastit_py316-*.so` with the same contract.
- Stage a bundle (`BUNDLE_REQUIRE_PHP_SHIMS=0 make install PREFIX=/tmp/opencode/blastit-315`) and run `uv run --no-project e2e/universal_binary.py --binary /tmp/opencode/blastit-315/bin/blastit --python 3.15 --static` with zero SKIP (real socket, real interpreter).
- Stable-vs-nightly diff: the sorted minor set from `platform_matrix.py --matrix py-shims --channel nightly` minus stable is exactly `3.16`; stable package set remains 8 names / 32 artifacts.
- `uv run --no-project e2e/run_packaging.py --print-legs` shows the MB.73 install-coverage legs with the new counts; run `sh scripts/test_install.sh`; run `make e2e-packaging` if Docker is available (record the leg), otherwise record the derived-leg output and say why.
- `make fast-check` and `make docs-check`; `uv run --no-project tools/plan.py validate`.

# Notes

Slice 4 of 4, last. Builds are serialized: one Cargo writer, no parallel `make py-sapi`. Report out-of-scope findings under `## Follow-ups`, never widen `paths` silently. No commit.
