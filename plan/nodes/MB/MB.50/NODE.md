---
id: MB.50
title: "Fix final 5 PR #64 CI items (wheel-smoke entry_points spacing, host.rs symlink-independent test, empty-PATH POSIX fallback, hostedtoolcache chown & py_sapi_leg gnu/musl)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/src/host.rs, "crates/blastit-py-sapi/**", "scripts/**", ".github/workflows/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T10:28"
updated: "2026-11-20T10:50"
---

# Goal
Close the final 5 GitHub Actions items from PR #64 (`3f0325b`, where `docs-gate`, `e2e`, `e2e rootless`, `wheel-smoke`, `sdist`, `standalone` gnu+musl, `lsp` x5, and `php-sapi` x16 are already 100% GREEN!):

1. **`scripts/wheel-smoke.sh` + `scripts/build-wheel.sh` (`Publish` `wheel` 4 legs):**
   - `wheel-smoke.sh`: maturin writes `blastit=blastit.pytest_plugin` (no spaces around `=`), while `wheel-smoke.sh` grepped for `blastit = blastit.pytest_plugin` with spaces! Match `^blastit[[:space:]]*=[[:space:]]*blastit\.pytest_plugin$` (and verify `wheel-smoke.sh` against the built wheel!).
   - `build-wheel.sh`: wrap `RUN dnf install -y --setopt=install_weak_deps=False clang && dnf clean all` inside `quay.io/pypa/manylinux_*` in a 3-attempt retry loop (`for attempt in 1 2 3; do dnf install ... && break; dnf clean all; sleep 2; done`) so transient AlmaLinux 8 mirror glitches on `aarch64` don't abort the container build.

2. **`crates/blastit/src/host.rs:1050` (`CI` `test` job — 283/284 passed):**
   - `host::tests::every_named_program_that_carries_capabilities_is_refused` passed literal `/usr/local/bin/python3.13`, and `placement()` called real `fs::read_link("/usr/local/bin/python3.13")` — which followed the `/usr/local/bin/python3.13 -> .../uv-python-dir/...` symlink on CI runners! Use synthetic nonexistent paths (`/nonexistent/blastit-test/python`, `/nonexistent/blastit-test/python3`, `/nonexistent/blastit-test/python3.13`) in that pure unit test so host symlinks never alter its assertion.

3. **`crates/blastit-py-sapi/src/discovery` (`CI` `smoke (root)` & `test` `critical_dev_reload` with `env_clear()`):**
   - `critical_dev_reload.rs` spawns `blastit dev` after `.env_clear()`, leaving `PATH` unset (`None`). `blastit-py-sapi`'s `PATH` search searched 0 directories (`(searched )`) when `PATH` was `None`!
   - When `PATH` is unset (`None`), fall back to standard POSIX `/usr/local/bin:/usr/bin:/bin` (matching `libc::execvp` / `_CS_PATH`) in `blastit-py-sapi`'s discovery so `env -i blastit` (and `critical_dev_reload`) searches `/usr/local/bin`, `/usr/bin`, and `/bin`! Add a unit test for `PATH = None`.

4. **`.github/workflows/ci.yml` `pytest-plugin` & `smoke` (`libpython` ownership under root):**
   - `actions/setup-python` extracts `/opt/hostedtoolcache/Python/...` owned by runner `uid 1001`, causing production root boot in `pytest-plugin` (`test_live_server`) to refuse `/opt/hostedtoolcache/Python/3.14.10/x64/lib/libpython3.14.so.1.0` (`owned by uid 1001`)!
   - Add `sudo --non-interactive chown -R root:root /opt/hostedtoolcache "${UV_PYTHON_INSTALL_DIR:-$HOME/.local}"` alongside `chmod -R go-w` in `pytest-plugin` and `smoke`!

5. **`.github/workflows/py-sapi.yml` & `scripts/py_sapi_leg.sh` (`Python SAPI` 8 legs):**
   - In `py-sapi.yml` `legs` job: add `- uses: dtolnay/rust-toolchain@stable` and `- uses: Swatinem/rust-cache@v2` (and in `packages.yml` / `nightly.yml` `py_shims` job if it shares the same pattern — check rule 21 in `check_workflows.py`!).
   - In `scripts/py_sapi_leg.sh`:
     * On `musl` (`docker run ... "$image"`): bare `alpine:3.21` has `/bin/sh`, NOT `bash`, before `apk add`! Invoke `docker run --rm -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" -v "$root:/src" -w /src "$image" sh -eu -c 'trap "chown -R $HOST_UID:$HOST_GID /src/target 2>/dev/null || true" EXIT; apk add --no-cache bash >/dev/null; exec bash /src/scripts/py_sapi_leg.sh --inside ...'` — and test `bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc musl` locally in Docker!
     * On `gnu`: if `PY_SAPI_PYTHON_$digits` is unset and `uv python find --no-project "$minor"` fails, run `uv python install "$minor" >/dev/null` and export `PY_SAPI_PYTHON_$digits="$(uv python find --no-project "$minor")"`!

# Acceptance
- `bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu --out /tmp/opencode/py-sapi-test`
- `bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc musl --out /tmp/opencode/py-sapi-test`
- `bash scripts/wheel-smoke.sh dist/blastit-*-py3-none-*.whl` (or `--python "$(uv python find 3.13)"`)
- `make fast-check && make workflow-check && make packaging-gates-test && uv run --no-project scripts/platform_matrix.py --check`
