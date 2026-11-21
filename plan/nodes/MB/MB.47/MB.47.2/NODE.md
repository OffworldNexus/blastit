---
id: MB.47.2
title: "CI py-sapi shims & root provenance, wheel --require-shims, standalone .tar.gz CPython shims"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.47.1]
paths: [Makefile, ".github/workflows/**", "scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T07:50"
updated: "2026-11-20T08:34"
---

# Goal

Make CI and the release archive build (and ship) the CPython shims, so no CI job runs `cargo test` without a runtime and no `blastit-<target>.tar.gz` installs without Python WSGI support. Two concrete fixes, all in `Makefile`, `.github/workflows/**` and `scripts/**`:

1. **`.github/workflows/ci.yml` runtime shims & root provenance.**
   - `test` and `smoke (root)`: add `astral-sh/setup-uv@v10.3.2`, `uv python install 3.13 3.14`, and `make python-runtime` **before** `cargo test`, so the shim-dependent tests have interpreters.
   - `smoke` and `pytest-plugin`: add `sudo --non-interactive chmod -R go-w "$HOME/.local"` alongside the existing `/opt` line, so root provenance trusts `uv`'s CPython (pass `HOME`/`BLASTIT_PY_SHIM_DIR` only where actually needed).
   - Bump `smoke` job `timeout-minutes: 30` and the root-run step `timeout-minutes: 20` for the 28 test binaries; update the step comment/warning threshold accordingly.
   - `e2e`, `rootless`, `wheel-smoke`, `pytest-plugin`, `docs-gate`: ensure `astral-sh/setup-uv@v10.3.2` + `uv python install 3.13 3.14` + `make py-sapi` run **before** `scripts/build-wheel.sh --require-shims py`, so a CI wheel is never built with 0 CPython shims.
   - `Makefile:docs-test`: depend on `py-sapi` (or export `BLASTIT_WHEEL_REQUIRE_SHIMS=py` only after `py-sapi`), same guarantee for the docs gate.

2. **Bundle `libblastit_py3{13,14}` in `blastit-<target>.tar.gz`.**
   - `scripts/build-standalone-in-image.sh`: run `make py-sapi PY_MINORS="3.14 3.13" PY_SAPI_PROBE=0` — `manylinux_2_28` ships `/opt/python/cp313-cp313` and `/opt/python/cp314-cp314`, so both build without downloads.
   - `.github/workflows/publish-pypi.yml`, `binaries_musl` (alpine) job: `uv python install 3.13 3.14` then the same `make py-sapi …`.
   - `scripts/build-standalone.sh`: stage `libblastit_py3*.so` beside `libblastit_php*.so` in the archive, and update the archive `README` so `install.sh --user` and Alpine `.tar.gz` installs serve Python WSGI out of the box.
   - Keep the existing "cannot build, cannot ship" behaviour: a missing shim must fail the build loudly, never produce a silently Python-less archive.

# Context

- Reference for the shim layout: `docs/deploy/compose-traefik.md` § "4. The image" (the wheel brings `_bin/blastit` plus shims; the tarball must match).
- `scripts/build-wheel.sh --require-shims py` is the existing lever: it fails a wheel with no CPython shims. Use it rather than adding a new guard.
- Do **not** edit `e2e/**` or the two `crates/blastit/tests/critical_*.rs` files here — `MB.47.1` owns them.

# Acceptance

```bash
make fast-check
make workflow-check
make packaging-gates-test
uv run --no-project scripts/platform_matrix.py --check
uv run --no-project tools/critical_guard.py --base origin/develop --node MB.47.2
```

Also confirm by inspection (grep is fine) that every `scripts/build-wheel.sh --require-shims py` call site in `.github/workflows/ci.yml` is preceded by `setup-uv` + `uv python install 3.13 3.14` + `make py-sapi`, and that `publish-pypi.yml`'s `binaries_musl` produces `libblastit_py313.so` + `libblastit_py314.so` in the archive.

# Notes

- Keep `RESULT.md` short: command + output tail per acceptance item; put out-of-scope discoveries under `## Follow-ups`.
