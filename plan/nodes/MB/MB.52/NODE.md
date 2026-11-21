---
id: MB.52
title: "Stage CPython shims beside blastit for smoke (root) and move pytest-plugin venv to root-owned /opt/blastit-tests"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/tests/smoke.rs, .github/workflows/ci.yml, "python/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T12:24"
updated: "2026-11-20T12:55"
---

# Goal
Close the final 2 red jobs across all 5 workflows on PR #64 (`f8eb15a` — 68/70 checks are already 100% GREEN, leaving only `ci.yml`'s `smoke (root)` and `pytest-plugin`!):

1. **`ci.yml` `pytest-plugin` (`443 passed`, only `test_live` failed on `/tmp/blastit-tests` shim provenance):**
   - `ci.yml:679` created the test virtualenv at `/tmp/blastit-tests` (owned by `runner` `uid 1001` inside world-writable `/tmp` `1777`). When `test_live.py` started production `sudo /tmp/blastit-tests/bin/blastit` (`uid 0`), production provenance rightly refused `/tmp/blastit-tests/.../_bin/libblastit_py314-x86_64-gnu.so` (`owned by uid 1001; /tmp is writable`)!
   - Move the test venv from `/tmp/blastit-tests` to `/opt/blastit-tests`:
     `sudo --non-interactive mkdir -p /opt/blastit-tests && sudo --non-interactive chown "$USER:$USER" /opt/blastit-tests`
     `python -m venv /opt/blastit-tests`
     `/opt/blastit-tests/bin/pip install dist/blastit-*.whl pytest pytest-django pytest-xdist django 'psycopg[binary]'`
     `sudo --non-interactive chown -R root:root /opt/blastit-tests && sudo --non-interactive chmod -R go-w /opt/blastit-tests`
     and run `/opt/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures`!
     (Also inspect `python/tests/test_live*.py` to ensure any temporary manifest/app directory `test_live` passes to `sudo .../bin/blastit` isn't chowned/writable in a way that fails production provenance when run from `/opt/blastit-tests`!)

2. **`ci.yml` `smoke (root)` & `crates/blastit/tests/smoke.rs` (`NoShim` in production mode):**
   - In production mode (`DiscoveryEnv::production`), `blastit` looks for `libblastit_py3*.so` **beside the `blastit` binary** (`target/debug/libblastit_py3*.so`), NOT in dev-only `target/py-sapi/`, AND verifies root/init ownership + non-writable ancestors!
   - Two places need the shims staged beside `blastit`:
     a) In `crates/blastit/tests/smoke.rs` (`RunDir::binary`, line 595, plus any other helper in `smoke.rs` that copies `CARGO_BIN_EXE_blastit` into a scratch dir for rootless `blastit-init` tests!): also copy `target/py-sapi/libblastit_py3*.so` (or sibling `libblastit_py3*.so` beside `CARGO_BIN_EXE_blastit`) into `self.path` (owned by the same uid as `self.path`, mode `0644` / `0755`, no file caps!) so `a_rootless_standalone_install_runs_dbproxy_from_the_capability_free_copy` finds the CPython shim beside `blastit-init`!
     b) In `.github/workflows/ci.yml` `smoke (root)` (after `Build the test binaries` as runner user, right before `Run every test binary as root`):
        copy `install -m 0644 target/py-sapi/libblastit_py3*.so target/debug/` and ensure `target/debug/libblastit_py3*.so` (and every directory along `$PWD/target/debug` up to `/`) is owned by `root:root` (or add a clean `chown root:root` on `target/debug/libblastit_py3*.so` + ancestor `chown root:root` / `chmod go-w` — note the post-job cleanup or runner does not care at end of `smoke`, OR even cleaner: stage `blastit` + `libblastit_py3*.so` or `chown root:root` on `target/debug/libblastit_py3*.so` — verify what `visibility.rs` / `blastit-py-sapi` checks on ancestor directories: `mode & 0o022 == 0` on ancestors, owner `root || init_uid` on the `.so` file itself!).

3. **PROVE BOTH LOCALLY IN DOCKER BEFORE COMMITTING:**
   - Run the built `smoke` test binary as `uid 0` inside Docker (with `BLASTIT_TEST_PG_URL` pointing at a real Postgres container/host Postgres and `--security-opt seccomp=unconfined` / `privileged` for `setcap` + `setuid`) and confirm **ALL 31 `smoke.rs` tests pass (`0 failed`)**!
   - Run `pytest python/tests/test_live*.py` inside Docker with `/opt/blastit-tests` owned by `root:root` and `sudo` available, confirming **`0 failed`**!

# Acceptance
- Full `smoke` test binary passes `31/31` (`0 failed`) as `uid 0` in Docker
- `test_live` passes with `/opt/blastit-tests` root-owned venv
- `make fast-check && make workflow-check && make packaging-gates-test`
