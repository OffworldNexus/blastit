---
id: MB.49
title: "Fix all 9 PR #64 GitHub Actions failures (py-shim clippy, self-handoff, CI uv python PATH, php Dockerfile, venv-smoke chown, pytest11 entry_point, docs-gate dry-run, wheel host-py fallback & musl TARGET)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-py-shim/**"
  - crates/blastit/src/cli/handoff.rs
  - crates/blastit/tests/critical_handoff.rs
  - ".github/workflows/**"
  - ".github/actions/**"
  - "scripts/**"
  - "e2e/**"
  - "python/tests/runnable_docs/**"
  - Makefile
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T09:10"
updated: "2026-11-20T10:09"
---

# Goal
Fix all 9 GitHub Actions failures observed on PR #64 (`ca3d5be`) in one clean, locally-verified pass:

1. **`py-sapi.yml` (`fmt + clippy` on `crates/blastit-py-shim/src/buffer.rs:19,22`):**
   - `PANIC` and `REFUSED` constants in `crates/blastit-py-shim/src/buffer.rs` are unused when compiling `--all-targets` without `--features extension-module` (`-D dead-code`). Scope them to `#[cfg(feature = "extension-module")]` (or exercise them in unit tests) and verify BOTH `cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings` AND `cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings` pass with 0 warnings!

2. **`handoff.rs` self-image check + `scripts/standalone-runtime-check.sh` (`ci.yml` `standalone` job):**
   - In `crates/blastit/src/cli/handoff.rs`: check `same_file` (`(st_dev, st_ino)` against `/proc/self/exe`) **before** refusing on ownership/mode even for `BLASTIT_BIN`! When `BLASTIT_BIN=/usr/local/bin/blastit` and `/proc/self/exe` IS `/usr/local/bin/blastit` (as in `ci.yml`'s `docker run -v .../blastit:/usr/local/bin/blastit -e BLASTIT_BIN=/usr/local/bin/blastit`), it is ALREADY running that exact inode and must NOT refuse handoff to itself! Also unset `BLASTIT_BIN` when invoking `"$binary" dev` inside `scripts/standalone-runtime-check.sh` (`env -u BLASTIT_BIN BLASTIT_NO_LOCAL_HANDOFF=1 "$binary" dev ...`), and add a unit/critical test for `BLASTIT_BIN == /proc/self/exe` owned by another uid!

3. **`ci.yml` `test` & `smoke (root)` jobs (`no CPython 3.13 or 3.14 interpreter was found`):**
   - `uv python install 3.13 3.14` on GitHub runners installs into `$UV_PYTHON_INSTALL_DIR` (NOT on `/usr/local/bin` / `PATH`!).
   - In both `test` and `smoke (root)` in `.github/workflows/ci.yml`, right after `uv python install 3.13 3.14`:
     `sudo --non-interactive ln -sfn "$(uv python find 3.13)" /usr/local/bin/python3.13`
     `sudo --non-interactive ln -sfn "$(uv python find 3.14)" /usr/local/bin/python3.14`
     and in `smoke (root)` (and `pytest-plugin`), also `sudo --non-interactive chmod -R go-w "${UV_PYTHON_INSTALL_DIR:-$HOME/.local}"` so root's provenance check trusts the `uv`-installed interpreters!

4. **`e2e/php/Dockerfile` (`ci.yml` `e2e` & `e2e rootless` jobs):**
   - Remove the stale `=8.4.27-1~deb13u1` point-version pin on `libphp8.4-embed` and `php8.4-dev` in `e2e/php/Dockerfile` (Debian `trixie` security repo superseded `8.4.27-1~deb13u1`, returning `E: Version '8.4.27-1~deb13u1' ... was not found`; keep the base image `@sha256:...` digest pins and update the comment). Test `docker build --no-cache -f e2e/php/Dockerfile ...` (or pull check) to prove `apt-get install` succeeds against live Debian repos!

5. **`e2e/venv-smoke.sh` & `e2e/venv-smoke/run.sh` (`ci.yml` `wheel-smoke` job):**
   - `work=$(mktemp -d)` created `/tmp/tmp.XXXX` owned by `user` (`uid 1000`) inside sticky `/tmp` (`1777`), causing production `sudo .venv/bin/blastit` (`uid 0`) to refuse `_bin/libblastit_py314-x86_64-gnu.so` (`owned by uid 1000` / `/tmp` ancestor writable)!
   - Create the work directory under `/var/lib/blastit-venv-smoke` (via `sudo mkdir -p` / `chmod 0755`, NOT `/tmp`) and `sudo chown -R root:root "$work/.venv"` before starting production `sudo env ... "$work/.venv/bin/blastit"` (cleaning up with `sudo rm -rf` on exit).

6. **`scripts/repack_universal_wheel.py` (`ci.yml` `pytest plugin` job — REAL USER BUG!):**
   - `repack_universal_wheel.py` overwrote `*.dist-info/entry_points.txt` with ONLY `[console_scripts]`, dropping `[pytest11]
blastit = blastit.pytest_plugin` from `pyproject.toml`!
   - Preserve all non-`console_scripts` sections from maturin's `entry_points.txt` (including `[pytest11]
blastit = blastit.pytest_plugin`) when writing the repacked `entry_points.txt`, and assert `[pytest11]` in `scripts/test_packaging_gates.py` + `scripts/wheel-smoke.sh`!

7. **`python/tests/runnable_docs/harness.py` & `ci.yml` `docs-gate` (`EXTERNALLY-MANAGED` uv Python 3.14):**
   - Pass `--system` to `uv pip install --python <interp> pytest==9.2.0 --dry-run` in `python/tests/runnable_docs/harness.py` (it's `--dry-run`, only probing cache presence, and `uv`-managed base interpreters carry `EXTERNALLY-MANAGED`!), AND in `ci.yml`'s `docs-gate`, symlink `3.14` or warm `pytest==9.2.0` for both `3.13` and `3.14` (and key the cache cleanly) so whichever minor `harness.py` selects finds `pytest==9.2.0` in `UV_CACHE_DIR`!

8. **`scripts/build-wheel.sh` (`publish-pypi.yml` `wheel (x86_64-gnu)` & `wheel (aarch64-gnu)`):**
   - Line 352 `py_bin=$(uv python find --no-project '>=3.13')` failed with exit 2 on `ubuntu-24.04` runners where only system Python 3.12 was installed on the host (even though `--manylinux` builds inside the manylinux container's own `/opt/python/cp313-cp313` & `cp314-cp314`!). Guard `uv python find --no-project '>=3.13' 2>/dev/null || true` and fall back to `py_minor="3.13"` when empty!

9. **`.github/workflows/publish-pypi.yml` `binaries_musl` (`TARGET: unbound variable`):**
   - Check `publish-pypi.yml` `binaries_musl` (`standalone (x86_64-musl)` & `standalone (aarch64-musl)`): fix the unbound `$TARGET` variable inside the Alpine `docker run` script (ensure `-e TARGET="${{ matrix.target }}"` is passed to `docker run` and referenced safely, mirroring `build-musl-wheel.sh` which passed on both `x86_64-musl` and `aarch64-musl`!).

# Acceptance
- `cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings`
- `cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings`
- Repacked wheel `entry_points.txt` contains both `[console_scripts]` and `[pytest11]
blastit = blastit.pytest_plugin`
- `docker build --pull -f e2e/php/Dockerfile -t blastit-e2e-php:test .` succeeds
- `make fast-check && make workflow-check && make packaging-gates-test && uv run --no-project scripts/platform_matrix.py --check`
