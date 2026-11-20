---
id: MB.42.3.3
title: "Ecosystem wrappers: 4 universal py3-none wheels + console scripts + py-sapi.yml + Composer package"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.3.2]
paths:
  - pyproject.toml
  - Makefile
  - "scripts/**"
  - "e2e/**"
  - "packages/**"
  - ".github/workflows/py-sapi.yml"
  - pyproject.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T01:33"
updated: "2026-11-20T01:33"
---

# Goal
The two **ecosystem lockfile wrappers**: 4 universal `py3-none-*` wheels (one per arch+libc, valid for CPython 3.12/3.13/3.14) and the `packages/composer-blastit/` Composer package — plus the CI that builds and smokes them.

Authoritative detail: `plan/nodes/MB/MB.42/MB.42.3/NODE.md` §3 and §5, and `docs/design/universal-distribution.md` §6.1. Read them.

# Exact Deliverables
1. **Universal wheel layout.** `pyproject.toml` currently uses `bindings = "bin"` (interim, and it forced `[project.scripts]` out). Move to the §6.1 shape: `maturin build --bindings bin` in `manylinux_2_28_<arch>` / `musllinux_1_2`, then `scripts/repack_universal_wheel.py` relocates `blastit`, `blastit-lsp` and all 6 shims (4 PHP `83`–`86` + 2 Python `313`/`314`) into `blastit/_bin/`, rewrites `RECORD`, and sets `Root-Is-Purelib: false` + the `py3-none-<platform>` tag.
2. **Restore the console scripts** lost by the interim `bindings = "bin"` wheel: `blastit`, `blastit-lsp` (`blastit.__main__:main_lsp` — currently dead code) and `blastit-manifest`. Prove all three exist on `PATH` after install.
3. `scripts/platform_matrix.py`: real `packages` (4), `wheels` (4 `py3-none-*`), `py_shims` (8) legs with `PROVEN` rows, and `wheel_legs()` keys that `e2e/prebuilt_wheel.py` and `scripts/check_wheel_tag.py` actually consume (drop `PYTHONS`/`runtime_image` and test one wheel in `python:3.12-slim`, `3.13-slim`, `3.14-slim`).
4. New `.github/workflows/py-sapi.yml` mirroring `php-sapi.yml`: builds the CPython shims with `make py-sapi PY_SAPI_REQUIRE_ALL=1` and gates on the artifact-level proof (`nm -D --defined-only` carries the three `blastit_py_*` exports; `DT_SONAME` matches; `UND Py_InitializeEx`; zero `NEEDED libpython`).
5. `packages/composer-blastit/`: `composer.json` for `offworldnexus/blastit` with `"bin": ["bin/blastit"]`; `bin/blastit` is POSIX `sh` that resolves the pinned version's `blastit-<arch>-<libc>.tar.gz`, verifies it against an **embedded** SHA-256 table (or `$BLASTIT_SHA256` / a local `SHA256SUMS` when `BLASTIT_RELEASE_BASE_URL` is set — failing closed otherwise), caches to `${BLASTIT_COMPOSER_CACHE}` -> `<pkg>/.runtime` -> `${XDG_CACHE_HOME:-$HOME/.cache}/blastit/composer`, then `BLASTIT_HANDOFF_DONE=1 exec`s it. Prove it in a `composer:2` container against a local fixture base URL.
6. Wire `make wheel-smoke` / `make py-sapi` / a composer smoke into the gates you judge appropriate, with `##` help lines.

# Notes
- Container-only for PHP/Python/composer installs; **never** write `/home/remy/.local` or `/usr/bin/blastit`.
- `scripts/check-portable-elf.sh --allow-origin-rpath` must pass on every embedded artifact; zero `libpython` `NEEDED` on `blastit`.
- Never touch `docs/**`.

# Acceptance
- 4 wheels built; one installed and driven (`blastit check`, `fold`, `blastit --version`, `blastit-lsp --help`) on CPython 3.12, 3.13 **and** 3.14
- `uv run --no-project scripts/platform_matrix.py --check` green with the new leg counts
- `make py-sapi PY_SAPI_REQUIRE_ALL=1` green, and `py-sapi.yml` proof lines pasted
- Composer wrapper installed and `bin/blastit --version` runs in `composer:2` from a local fixture
- `make fast-check`

# Carried in from MB.42.11
`crates/blastit/src/check.rs`'s third bundle root, `exe_dir.join("../../..")`, is **dead**: from a wheel's `<site-packages>/blastit/_bin` it resolves one level *above* `site-packages`, so it can never contain a `blastit/` package (`../..` is what a wheel root would need). Either make it load-bearing for the repacked `py3-none` wheel layout (and prove it with a test), or delete it as a dead fallback — the charter forbids keeping unexercised fallbacks. State which in `RESULT.md`.
