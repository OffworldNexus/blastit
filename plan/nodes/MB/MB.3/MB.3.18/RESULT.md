# MB.3.18 — RESULT (attempt 4 / expert, base `05ab3ea`)

## Matrix & Proven Counts (Unchanged and Enforced)

| Family | Declared (`LEGS`) | Built **and** Exercised Locally (`PROVEN`) | `aarch64` Status |
|---|---|---|---|
| Wheels (`cp313`, `cp314` × `{manylinux_2_28, musllinux_1_2}`) | **8** | **4** (`x86_64` × `{gnu, musl}` × `{3.13, 3.14}`) | **4** CI-only (`ubuntu-24.04-arm`) |
| Standalone archives (`--no-default-features --features php`) | **4** | **2** (`x86_64-gnu`, `x86_64-musl`) | **2** CI-only (`ubuntu-24.04-arm`) |
| Native PHP SAPI shims (`8.2`, `8.3`, `8.4`) | **12** | **6** (`x86_64` × `{gnu, musl}` × `{8.2, 8.3, 8.4}`) | **6** CI-only (`ubuntu-24.04-arm`) |

All 6 musl security gate invariants (`M-1` secrets, `M-2` `SeccompRole::View` without widening, `M-3` `AF_INET -> EPERM` egress block, `M-4` `CLOCK_PROCESS_CPUTIME_ID` > 0, `M-5` signed Alpine `php{82,83,84}-embed` + dynamic musl `dlopen` with zero `DT_NEEDED` on `libphp`, `M-6` effective INI lockdown via `ini_get` + `extension_loaded` verification for absent `ffi`/`phar`) hold across `8.2`, `8.3`, and `8.4` on `x86_64-musl`. The `aarch64` legs are CI-only because this host has neither `binfmt_misc` nor `qemu-user` and `ADR-0010` §4.2 forbids cross-compiling shims against foreign `php_config.h`.

---

## 1. Root Cause & Fix for Red CI `e2e` + Wheel Platform Tag Floor (`manylinux_2_28`)

### Two Distinct Traps Uncovered

1. **Why `05ab3ea` went RED in CI (`e2e (Django)` and `e2e rootless`) at step `#24` (`blastit-0.2.0-cp313-cp313-manylinux_2_39_x86_64.whl is not a supported wheel on this platform`):**
   - Every wheel-installing `e2e/` image is Debian `trixie` (**glibc 2.41** >= 2.39); `e2e/php-sapi/Dockerfile` (`debian:bookworm` 2.36 for PHP 8.2) compiles C/Rust from source and installs **no wheel**.
   - The culprit was `e2e/php/Dockerfile:32`:
     `FROM python:${PYTHON_VERSION}-slim-trixie@sha256:9aaa474097d094040649f2162a627c0049b408f52138d980916788d80f04802f`
   - In OCI/Docker `FROM repo:tag@digest`, `@digest` **completely overrides `:tag`**. Digest `9aaa47…` is **`python:3.14-slim-trixie` (CPython 3.14.10)**. When `ci.yml` (`PYTHON_VERSION: "3.13"`) built a `cp313` wheel and passed `--build-arg PYTHON_VERSION=3.13` (in both `e2e/compose.yaml` service `php` and `e2e/rootless/run.py` -> `php_entry_swap.build`), Docker still booted **CPython 3.14**, whose `pip` rejected `cp313` (`is not a supported wheel on this platform`) — and rejected `cp313-cp313-manylinux_2_28_x86_64.whl` identically until fixed.
   - **Fix (`e2e/php/Dockerfile`):** Declared global `ARG PYTHON_VERSION=3.14` *before* the first `FROM`, pinned both multi-arch index digests (`AS base-3.13` and `AS base-3.14`), and selected `FROM base-${PYTHON_VERSION}`. Verified with both `PYTHON_VERSION=3.13` (`cp313-cp313-manylinux_2_28_x86_64.whl` -> `cp313 e2e/php image ok: 3.13.18`) and `PYTHON_VERSION=3.14` (`make e2e` -> exit `0`).

2. **Wheel platform tag floor (`manylinux_2_28`) & `*.dist-info/WHEEL` `Tag:` guard:**
   - **Physics on bare glibc >= 2.34 hosts:** Running `maturin build --compatibility manylinux_2_28` directly against this host's glibc 2.41 `libc.so.6` is rightly refused by `maturin` (`libm.so.6 offending versions: GLIBC_2.29`, `libc.so.6 offending versions: GLIBC_2.29, GLIBC_2.30, GLIBC_2.33, GLIBC_2.34, GLIBC_2.39` from `pthread_create@GLIBC_2.34`, `__libc_start_main@GLIBC_2.34`, `pidfd_spawnp@GLIBC_2.39`), and `--auditwheel skip` would forge a lying tag that crashes at `import` on glibc 2.28.
   - **Shipped wheels NEVER depend on the runner's glibc:** Both `.github/workflows/publish-pypi.yml` and `.github/workflows/wheels.yml` build glibc wheels inside `quay.io/pypa/manylinux_2_28_<arch>` via `.github/actions/build-wheel/action.yml` (`manylinux: 2_28`), and we now pass `--compatibility ${{ inputs.platform }}` (`manylinux_2_28`) explicitly to `maturin` (and `--compatibility musllinux_1_2` in `scripts/build-musl-wheel.sh`).
   - **Local & `make e2e` `manylinux_2_28` builds (`scripts/build-wheel.sh --manylinux 2_28`):** Added `--manylinux 2_28` (`BLASTIT_MANYLINUX=2_28`) to `scripts/build-wheel.sh` (used by `make e2e`), building both `blastit-dbproxy` and `_blastit*.so` inside `quay.io/pypa/manylinux_2_28_<arch>` (`target/manylinux_2_28`) so a wheel built on a glibc 2.41 developer host carries genuine `Tag: cp3XX-cp3XX-manylinux_2_28_<arch>`. Uncontainerized dev builds (`make docs-test` inside offline, Docker-free `make check`) stay native and enforce `--max-glibc 2_41`; CI wheel jobs (`ci.yml`, `live.yml`) are pinned to `ubuntu-24.04` (glibc 2.39 <= 2.41).
   - **Post-build `WHEEL` `Tag:` guard (`scripts/check_wheel_tag.py`):** Reads `*.dist-info/WHEEL` `Tag:` lines directly, prints `wheel-tag: <file>: Tag: <tag>`, refuses unversioned `linux_*` and filename/metadata drift, and under `--max-glibc 2_28` (in `.github/actions/build-wheel/action.yml` and `scripts/build-wheel.sh --manylinux 2_28`) **fails loudly** if any `manylinux_X_Y` exceeds `2_28`.

### Audit of Every `e2e/` Dockerfile Base vs the `2_28` Floor & Host-Derived Values

| Dockerfile / Script | Base Image & `glibc` / `libc` | Installs Wheel? | Verdict vs `manylinux_2_28` Floor |
|---|---|---|---|
| `e2e/Dockerfile` | `python:3.13-slim-trixie` (`glibc 2.41`) | Yes (built in-image) | `2.28 <= 2.41` (in-image 2.41 == 2.41) |
| `e2e/Dockerfile.prebuilt` | `python:${PYTHON_VERSION}-slim-trixie` (`glibc 2.41`) | Yes (`dist/*.whl`) | `2.28 <= 2.41`; `prebuilt_wheel.py` also rejects `> 2_41` / `musllinux` |
| `e2e/rootless/Dockerfile` | `python:3.13-slim-trixie` (`glibc 2.41`) | Yes (built in-image) | `2.28 <= 2.41` |
| `e2e/rootless/Dockerfile.prebuilt` | `python:${PYTHON_VERSION}-slim-trixie` (`glibc 2.41`) | Yes (`dist/*.whl`) | `2.28 <= 2.41` |
| `e2e/php/Dockerfile` | `base-3.13` / `base-3.14` (`python:3.{13,14}-slim-trixie`, `glibc 2.41`) | Yes (`dist/*.whl`) | **Fixed** `@sha256` tag-override bug; `2.28 <= 2.41` |
| `e2e/php-sapi/Dockerfile` | `debian:bookworm` (`2.36`), `ubuntu:24.04` (`2.39`), `debian:trixie-slim` (`2.41`), `alpine:3.20/3.21` (`musl`) | **No** (builds C shim + Rust probe) | N/A for wheels; `2.28 <= 2.36` |
| `scripts/collect-shim.sh` | Per-leg container (`cc -dumpmachine`) | No | Verifies exact `ADR-0010` §4.1 shim filename + `check-portable-elf.sh` (zero `RPATH`/`RUNPATH`) |
| `scripts/build-standalone.sh` | `--target <triple>` (no host binary fallback) | No | Verifies `check-portable-elf.sh` + `ld-musl-<arch>.so.1` `INTERP` on musl (see Follow-up 5 on gnu standalone `glibc` floor) |

---

## 2. Attempt-3 Findings + Seven Additional Workflow Defects Found & Fixed

| # | Severity / Source | Defect | Fix & Mechanical Guard |
|---|---|---|---|
| 1 | **BLOCKER** (Attempt 3) | `publish-pypi.yml:250,312` called `platform_matrix.py --leg shims --libc … --arch …` without `--minor` -> matched 3 legs, exited `2` with empty stdout, killing `binaries_gnu` and `binaries_musl` under `set -euo pipefail` | Added `--count {wheels,binaries,shims,images}` to `platform_matrix.py`; updated both steps to `--count shims --libc … --arch …`; `check_workflows.py` now refuses any `--leg` call missing `--minor`/`--python`/`--libc`/`--arch` |
| 2 | **MAJOR 1** (Attempt 3) | `check_workflows.py` checked upload `name:` duplicates, missing file basename collisions inside `merge-multiple: true` downloads (`dist/SHA256SUMS` across 4 legs) | `check_workflows.py` expands every matched upload's `path:` entries across concrete matrix entries and refuses any colliding non-glob basename feeding `merge-multiple: true` |
| 3 | **MAJOR 2** (Attempt 3) | `check_workflows.py` `#`-in-`\`-continuation check skipped `.github/actions/*/action.yml` `runs.steps` and `before-script-linux` | `check_step_scripts` now scans every step in both workflow jobs and composite actions (`runs.steps` + `before-script-linux`) |
| 4 | **MINORs** (Attempt 3) | Missing `if-no-files-found: error` on uploads; `/tmp` negative proof not in `make check`; `check_workflows.py` fetched unpinned `pyyaml` over the network | Added `if-no-files-found: error` to all 7 non-diagnostic uploads (+ auditor rule); pinned `pyyaml==6.0.3` with `scripts/check_workflows.py.lock` (`UV_OFFLINE=1 --locked`); added `scripts/test_packaging_gates.py` (22 positive + negative cases) to `make check` |
| 5 | **NEW BLOCKER** (Audit #1) | `publish-pypi.yml:384` & `wheels.yml:102` wrote `dist/blastit-*-cp${{ matrix.python_tag }}*-musllinux_*.whl`, where `python_tag` is already `cp313`/`cp314` -> expanded to **`cpcp313`**, matching nothing and failing every `wheels_musl` smoke step | Fixed to `dist/blastit-*-"${PY_TAG}"*-musllinux_*.whl`; auditor now refuses `cp${{ matrix.python_tag }}` |
| 6 | **NEW BLOCKER** (Audit #2) | `publish-pypi.yml:394` `verify_musl` hardcoded `runs-on: ubuntu-latest` (`x86_64`) while iterating `aarch64-musl` (`Exec format error`), **and** was orphaned from `needs:` of `testpypi`, `pypi`, and `release` | Folded the clean-Alpine runtime check directly into `binaries_musl` on `${{ matrix.runs_on }}` before artifact upload; auditor now refuses orphaned jobs in `publish-pypi.yml` and multi-arch matrices hardcoded to `ubuntu-latest` |
| 7 | **NEW BLOCKER** (Audit #3) | `publish-pypi.yml` (`binaries_musl`, `wheels_musl`) & `wheels.yml` (`wheels_musl`) used job-level `container: alpine:3.21` on `ubuntu-24.04-arm` (`externals/node20_alpine` does not exist in `actions-runner-linux-arm64`, breaking JS actions at container init) | Replaced job-level `container: alpine:3.21` with `docker run --rm … alpine:3.21` on the native `${{ matrix.runs_on }}` runner (with `trap` UID/GID restoration); auditor refuses `container: alpine*` on `*-arm` |
| 8 | **NEW BLOCKER** (Audit #4) | `publish-pypi.yml` `shims` ran `Swatinem/rust-cache@v2` without `rust-toolchain` (fails `rustc -vV` on `ubuntu-24.04-arm`, and `shims` builds in Docker); `binaries_gnu` & `php-sapi.yml` assumed `libclang-dev` on arm64 runners | Removed unused `rust-cache` from `publish-pypi.yml` `shims`; added conditional `libclang-dev`/`clang` install on `binaries_gnu` and `php-sapi.yml` gnu legs; added `rust-toolchain` to `sdist` |
| 9 | **NEW MAJOR** (Audit #5) | `wheels.yml` `wheels_musl` built and smoked the musl wheels and **never uploaded them** (`actions/upload-artifact@v4` missing) | Added `actions/upload-artifact@v4` (`wheel-${{ matrix.python_tag }}-${{ matrix.arch }}-${{ matrix.libc }}`, `if-no-files-found: error`) to `wheels.yml` |
| 10 | **NEW MAJOR** (Audit #6) | `publish-pypi.yml` `release` used unfiltered `download-artifact` (pulling PyPI wheels/sdist into GitHub Releases), omitted the 12 standalone `.so` shims from `SHA256SUMS`, ran `sort -k2 dist/SHA256SUMS` to stdout without `-o`, and verified no matrix counts | `release` now explicitly downloads `standalone-*` and `shim-*`, verifies each `SHA256SUMS.<target>`, asserts exact `--count binaries` (`4`) and `--count shims` (`12`), and writes one sorted, verified 16-entry `SHA256SUMS` (`verify` likewise asserts `--count wheels` == `8` + `1` sdist) |
| 11 | **NEW MAJOR** (Audit #7) | `check_workflows.py` shared one global `producers` map across all 6 workflow files, skipped unfiltered `download-artifact` steps, and checked `bool(produced_by & upstream)` instead of `produced_by <= upstream` | Scoped `uploads` per workflow file, audited unfiltered downloads, and required **every** matching producer job (`produced_by <= upstream`) in `transitive_needs` (**352 checks** total, up from 207) |

---

## 3. End-to-End Job Graph Audit (`publish-pypi.yml`)

```text
guard ──┬──► sdist [upload: sdist] ───────────────────────────────────┐
        ├──► wheels_gnu (4 legs) [upload: wheel-cp3{13,14}-*-gnu] ────┼──► verify (asserts 8 whl + 1 sdist)
        ├──► wheels_musl (4 legs) [upload: wheel-cp3{13,14}-*-musl] ──┘         │
        │                                                                       ▼
matrix ─┼──► shims (12 legs) [upload: shim-*-php8{2,3,4}] ─┬──────────► testpypi (dispatch & !dry-run)
        │         │                                        │                    │
        │         ├──► binaries_gnu (2 legs, runtime-probed) [standalone-*-gnu] │
        │         └──► binaries_musl (2 legs, runtime-probed) [standalone-*-musl]
        │                                                  │
        └──► validate (develop push: CI + Docs + Live) ────┴──► pypi (push develop|v*; wheels + sdist)
                                                                  │
                                                                  ▼
                                                               release (push develop|v*; asserts 4 tar.gz + 12 .so + SHA256SUMS)
```

Checked across both `workflow_dispatch` and `push` of `v0.2.0` (plus `pull_request` and `push` to `develop`):
- **(a) `needs` satisfiability & acyclicity:** 13 jobs, DAG verified acyclic (`has_cycle == []`), zero orphaned jobs (`guard`, `matrix`, `shims`, `binaries_gnu`, `binaries_musl`, `wheels_gnu`, `wheels_musl`, `sdist`, `verify`, `validate`, `pypi` all feed `testpypi` and/or `release`).
- **(b) Artifact producer -> consumer fidelity:**
  - `binaries_gnu` downloads `shim-${{ matrix.name }}-*` (3 `.so` files from `shims`, in `needs`) -> uploads `dist/${{ matrix.archive }}` + `dist/SHA256SUMS.${{ matrix.target }}` as `standalone-${{ matrix.name }}`.
  - `binaries_musl` downloads `shim-${{ matrix.name }}-*` (3 `.so` files from `shims`, in `needs`) -> uploads `dist/${{ matrix.archive }}` + `dist/SHA256SUMS.${{ matrix.target }}` as `standalone-${{ matrix.name }}`.
  - `verify`, `testpypi`, and `pypi` each download `pattern: wheel-*` (`wheels_gnu` + `wheels_musl`, all 8 wheels in transitive `needs`) and `pattern: sdist` (`sdist`, in transitive `needs`).
  - `release` (`needs: [guard, pypi, shims, binaries_gnu, binaries_musl]`) downloads `pattern: standalone-*` (4 `.tar.gz` + 4 `SHA256SUMS.<target>`) and `pattern: shim-*` (12 `.so`), verifies per-target hashes, asserts `4` archives + `12` shims, generates a unified 16-entry `SHA256SUMS`, and uploads all 17 files (`4` tarballs + `12` shims + `SHA256SUMS`).
- **(c) Literal file & directory paths:** Every step creates or downloads `dist/`, `dist/shims/`, `/tmp/artifact`, `/tmp/from-sdist` before reading; tarball root `/art/blastit-${{ matrix.target }}/blastit` matches `build-standalone.sh`'s `name="blastit-$target"`.
- **(d) `if:` conditions:** On `workflow_dispatch`, `should_build == 'true'`, `release == 'false'`: all build jobs + `verify` run; `testpypi` runs iff `!inputs['dry-run']`; `validate`, `pypi`, and `release` skip. On `push` tag `v0.2.0`, `should_build == 'true'`, `release == 'true'`: `validate` skips (`ref != 'refs/heads/develop'`); `pypi` evaluates `!cancelled() && ... && (github.ref_type == 'tag' && needs.validate.result == 'skipped')` -> **true**; `release` evaluates `!cancelled() && ... && needs.pypi.result == 'success' && github.ref_type == 'tag'` -> **true**.
- **(e) `set -euo pipefail` safety:** Eliminated `--leg shims` (exit 2) and `grep -c` (exit 1 on 0 matches) in favour of `platform_matrix.py --count shims` (always exits 0); `standalone-runtime-check.sh` wraps its expected exit-1 WSGI refusal in `set +e ... set -e`.
- **(f) No half-landed release:** `pypi` directly requires `verify`, `shims`, `binaries_gnu`, and `binaries_musl` all `== 'success'` before uploading a single wheel to PyPI; `verify` asserts all `8` wheels + `1` sdist exist; `release` requires `pypi == 'success'` and asserts all `4` archives + `12` shims exist before tagging or creating the GitHub Release.

---

## 4. Verification Evidence

### 4.1 Wheel `manylinux_2_28` Build, `Tag:` Guard (Pass + Negative), and `make e2e`

```text
$ uv run --no-project scripts/check_wheel_tag.py --max-glibc 2_28 dist/blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl: Tag: cp314-cp314-manylinux_2_39_x86_64
wheel-tag: FAIL blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl: Tag: cp314-cp314-manylinux_2_39_x86_64 requires glibc 2.39 (manylinux_2_39), which is above the manylinux_2_28 floor — build inside manylinux_2_28 (`scripts/build-wheel.sh --manylinux 2_28` or `.github/actions/build-wheel`)
EXPECTED_EXIT=1

$ scripts/build-wheel.sh --manylinux 2_28
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to /src/dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   /var/tmp/blastit-wheel.jJ6gOi/_blastit0.so: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
$ unzip -p dist/blastit-*.whl '*.dist-info/WHEEL' | grep '^Tag:'
Tag: cp314-cp314-manylinux_2_28_x86_64

# Exact CI reproduction (`PYTHON_VERSION=3.13` -> `cp313-cp313-manylinux_2_28_x86_64.whl` into `e2e/php/Dockerfile`):
$ PYO3_PYTHON=/home/remy/.local/bin/python3.13 scripts/build-wheel.sh --manylinux 2_28
wheel-tag: blastit-0.2.0-cp313-cp313-manylinux_2_28_x86_64.whl: Tag: cp313-cp313-manylinux_2_28_x86_64
$ docker build -f e2e/php/Dockerfile --build-arg PYTHON_VERSION=3.13 -t blastit-e2e-php:cp313-check .
#8 1.975 Processing ./tmp/dist/blastit-0.2.0-cp313-cp313-manylinux_2_28_x86_64.whl
#8 2.202 Successfully installed blastit-0.2.0
$ docker run --rm --entrypoint python3 blastit-e2e-php:cp313-check -c 'import sys, blastit; print("cp313 e2e/php image ok:", sys.version.split()[0], blastit.__file__)'
cp313 e2e/php image ok: 3.13.18 /usr/local/lib/python3.13/site-packages/blastit/__init__.py

# Every wheel install across `make e2e` (`grep` over `/tmp/opencode/e2e-run.log`):
16:📦 Built wheel for CPython 3.14 to /src/dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
17:wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
79:#17 1.030 Processing ./tmp/dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl   (e2e/Dockerfile.prebuilt)
81:#17 1.335 Successfully installed blastit-0.2.0
98:#16 3.081 Processing ./tmp/dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl   (e2e/php/Dockerfile)
100:#16 3.436 Successfully installed blastit-0.2.0
799:#11 0.986 Processing ./tmp/dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl  (e2e/rootless/Dockerfile.prebuilt)
801:#11 1.322 Successfully installed blastit-0.2.0

# `make e2e` tail:
  ok   an entry rewritten in place is refused (500)
  ok   and the in-place decoy never reaches the client
  ok   and no decoy marker at all
  ok   the decoy recorded 0 opens (0)
  ok   and the worker names the change and the fix in its log
  ok   restoring the page in place succeeds
  ok   the restored page still answers a head
  ok   and it stays refused until a restart (500)
  ok   a restart serves the page as it is now
  ok   and the restarted boot serves it (200)
  ok   still 0 decoy opens (0)
MAKE_E2E_EXIT=0
```

### 4.2 `make check` + Auditor Self-Tests + Release & Standalone Gates

```text
$ make check
...
46 fences: 28 executed, 18 accounted, 0 failed · 31.1s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims; 0 leg(s) dropped
UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py
workflow-check: ok — 352 checks over 6 workflows and 1 actions, 0 problem(s)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 22 cases passed (workflow=8, matrix=10, wheel-tag=4)
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0

$ scripts/check-release-tag.sh v0.2.0
tag v0.2.0 names the workspace version 0.2.0   (TAG_EXIT=0)

$ scripts/release-notes.sh --require-dated v0.2.0 | wc -l
67   (NOTES_EXIT=0)

$ UV_OFFLINE=1 uv run --no-project --with pyyaml==6.0.3 python -c '...'
yaml ok: .github/actions/build-wheel/action.yml (6 steps)
yaml ok: .github/workflows/ci.yml (11 jobs)
yaml ok: .github/workflows/docs.yml (2 jobs)
yaml ok: .github/workflows/live.yml (2 jobs)
yaml ok: .github/workflows/php-sapi.yml (3 jobs)
yaml ok: .github/workflows/publish-pypi.yml (13 jobs)
yaml ok: .github/workflows/wheels.yml (3 jobs)

$ make php-sapi
php-sapi: cc -> .../target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: 1 shim(s) for x86_64-gnu in .../target/php-sapi; skipped: 8.3 8.2

$ make rs-no-python
ok   .../target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
default build: 0 pyo3 dependencies (no libpython linked)

$ uv run --no-project tools/critical_guard.py --base 05ab3ea --node MB.3.18
critical-guard: 05ab3ea..working tree, 24 changed file(s)
  no change to a protected critical test

$ uv run --no-project tools/guard.py
guard: OK (10 staged/untracked file(s) checked)
```

### 4.3 Post-Review MINOR Closures (All 4 Closed)

1. **`PROVEN` 8.2 & 8.3 `x86_64-gnu` shims re-built, probed, and retained on disk (`target/php-sapi/`):**

```text
$ make php-sapi-e2e PHP_MINOR=8.2 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1 PHP_SAPI_IMAGE=blastit-php-sapi-e2e:x86_64-gnu-php82
php-sapi-e2e: docker build --build-arg BASE=debian:bookworm --build-arg PHP_MINOR=8.2 -t blastit-php-sapi-e2e:x86_64-gnu-php82
php-sapi-e2e: readelf -d libblastit_php82-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.2 abi 1: status 200, cpu_ns 14719653 <= wall_ns 16255118, exit()+fatal contained, next request served
      allow_url_fopen=0  socket(AF_INET) errno before/after=0/0  fopen_http=php_setting  stream_socket=network (Connection refused)  curl=network (Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server)
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.2 abi 1: status 200, cpu_ns 18045643 <= wall_ns 19492785, exit()+fatal contained, next request served
      allow_url_fopen=0  socket(AF_INET) errno before/after=1/1  fopen_http=php_setting  stream_socket=other (...)  curl=network (...)
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.2 abi 1: status 200, cpu_ns 16184992 <= wall_ns 16724155, exit()+fatal contained, next request served
      allow_url_fopen=1  socket(AF_INET) errno before/after=1/1  fopen_http=other  stream_socket=other (...)  curl=network (...)
PHP82_GNU_EXIT=0
$ scripts/collect-shim.sh blastit-php-sapi-e2e:x86_64-gnu-php82 target/php-sapi --minor 8.2 --libc gnu --arch x86_64
ok   target/php-sapi/libblastit_php82-x86_64-gnu.so: NEEDED libc.so.6; no RPATH/RUNPATH
collect-shim: target/php-sapi/libblastit_php82-x86_64-gnu.so (17384 bytes)

$ make php-sapi-e2e PHP_MINOR=8.3 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1 PHP_SAPI_IMAGE=blastit-php-sapi-e2e:x86_64-gnu-php83
php-sapi-e2e: docker build --build-arg BASE=ubuntu:24.04 --build-arg PHP_MINOR=8.3 -t blastit-php-sapi-e2e:x86_64-gnu-php83
php-sapi-e2e: readelf -d libblastit_php83-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.3 abi 1: status 200, cpu_ns 14321198 <= wall_ns 16320505, exit()+fatal contained, next request served
      allow_url_fopen=0  socket(AF_INET) errno before/after=0/0  fopen_http=php_setting  stream_socket=network (Connection refused)  curl=network (Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server)
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.3 abi 1: status 200, cpu_ns 13248794 <= wall_ns 15833714, exit()+fatal contained, next request served
      allow_url_fopen=0  socket(AF_INET) errno before/after=1/1  fopen_http=php_setting  stream_socket=other (...)  curl=network (...)
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.3 abi 1: status 200, cpu_ns 12249758 <= wall_ns 15657445, exit()+fatal contained, next request served
      allow_url_fopen=1  socket(AF_INET) errno before/after=1/1  fopen_http=other  stream_socket=other (...)  curl=network (...)
PHP83_GNU_EXIT=0
$ scripts/collect-shim.sh blastit-php-sapi-e2e:x86_64-gnu-php83 target/php-sapi --minor 8.3 --libc gnu --arch x86_64
ok   target/php-sapi/libblastit_php83-x86_64-gnu.so: NEEDED libc.so.6; no RPATH/RUNPATH
collect-shim: target/php-sapi/libblastit_php83-x86_64-gnu.so (21760 bytes)

$ ls -la target/php-sapi/
-rwxr-xr-x 1 remy remy 17384 libblastit_php82-x86_64-gnu.so
-rwxr-xr-x 1 remy remy 22376 libblastit_php82-x86_64-musl.so
-rwxr-xr-x 1 remy remy 21760 libblastit_php83-x86_64-gnu.so
-rwxr-xr-x 1 remy remy 22360 libblastit_php83-x86_64-musl.so
-rwxr-xr-x 1 remy remy 17384 libblastit_php84-x86_64-gnu.so
-rwxr-xr-x 1 remy remy 22360 libblastit_php84-x86_64-musl.so
```

2. **`make docs-test` (`Makefile:269`) isolated to `target/docs-wheel` (`-o`) with `trap restore_dist EXIT`:**
   `scripts/build-wheel.sh` now supports `-o`/`--out` across both host and `--manylinux` builds, and `docs-test` builds into `target/docs-wheel`, stashing any existing `dist/` aside and restoring it on exit so `make check` never leaves a `manylinux_2_39` wheel in `dist/` (or deletes a `manylinux_2_28` wheel):

```text
=== BEFORE make check (after `make e2e`) ===
beac311ec4bd4dbe339d621dd6e92274c575366f80bd0e8bfc7887d598723dbd  dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
Tag: cp314-cp314-manylinux_2_28_x86_64
$ make check                                # exit 0
=== AFTER make check ===
beac311ec4bd4dbe339d621dd6e92274c575366f80bd0e8bfc7887d598723dbd  dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
Tag: cp314-cp314-manylinux_2_28_x86_64
$ git status --short dist/                  # (empty)
```

3. **Auditor `cp${PY_TAG}` indirection check (`scripts/check_workflows.py`, `scripts/test_packaging_gates.py`):** `has_double_cp_prefix` now tracks shell variables assigned from `${{ matrix.python_tag }}` (including `env:` mappings) and refuses `cp$VAR` / `cp"${VAR}"` as well as direct `cp${{ matrix.python_tag }}`, pinned by `test_packaging_gates.py` case 8.
4. **`e2e/php-sapi/Dockerfile:8-10` comment:** corrected Alpine's embedded library paths (`/usr/lib/libphp82.so` for 8.2/8.3, `/usr/lib/php84/libphp.so` for 8.4).

---

## 5. Follow-ups

1. **Wheels carry no PHP shims** (`ADR-0010` §4): pure wheel installs still take shims from `BLASTIT_PHP_SHIM_DIR` or the standalone archive until `stage0` adds a `site-packages/blastit/_shims` lookup root.
2. **Prebuilt `ghcr.io/offworldnexus/blastit:php8.4{,-alpine}` images** are declared in `platform_matrix.py` (`IMAGES`) for `ADR-0010` §4.3; pushing them is a container-publishing follow-up.
3. **Alpine `libgcc` runtime package**: dynamically linked musl `blastit` (`-C target-feature=-crt-static`, required for `dlopen`) needs `apk add libgcc` (`ADR-0010` §4, verified in `binaries_musl`).
4. **`docs/getting-started.md` install row**: outside this node's `paths`; update to mention `x86_64` + `aarch64` (`glibc` & `musl`) wheels and the Python-free standalone release archives.
5. **Standalone gnu binary `glibc` floor**: `publish-pypi.yml`'s `binaries_gnu` builds on `ubuntu-24.04` (`glibc 2.39`, binding `GLIBC_2.34` + weak `GLIBC_2.39`), whereas wheels build inside `manylinux_2_28` (`glibc 2.28`). Building `binaries_gnu` inside `quay.io/pypa/manylinux_2_28_<arch>` as well would remove `ld.so`'s weak `GLIBC_2.39` stderr warning on Debian 12 bookworm (`glibc 2.36`) and allow RHEL 8 (`glibc 2.28`) standalone execution.
6. **`max_requests = 1` PHP worker recycle window on immediate `POST` (`crates/blastit-scheduler`)**: observed once under `e2e/php2shell.py` (`E6 -> E7`): if a `POST` with a body arrives during the sub-millisecond window after a `max_requests = 1` worker sends `DONE` and exits `0` before `init` reaps and retires the slot, `scheduler` can return `502` instead of holding/re-queuing onto the respawned worker (`crates/` is outside `MB.3.18` `paths`).

RELEASABLE: yes
