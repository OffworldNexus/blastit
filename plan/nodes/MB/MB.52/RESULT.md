# MB.52 — stage the CPython shims beside `blastit`, move the pytest venv to `/opt`

## What changed

### `crates/blastit/tests/smoke.rs`
1. `RunDir::binary` now also copies the per-minor CPython shims beside the copy:
   a single cohesive helper, `copy_python_shims_beside`, reads them from
   `CARGO_BIN_EXE_blastit`'s own directory (the `smoke (root)` job stages them
   there) or `<target>/py-sapi` (`make python-runtime`), copies each as `0644`
   with no file capabilities, and is idempotent so the second `binary()` call
   (`blastit`) does not have to re-copy. A deployment boot discovers
   `libblastit_py3<m>-<arch>-<libc>.so` **beside the running binary only**
   (`DiscoveryEnv::Production` -> `ProductionEnv::exe_dir`), so the rootless copy
   used to refuse with `NoShim` before forking anything.
2. `Fixture::new` creates its directory in `fixture_root()`: as root that is the
   first of `/opt`, `/usr/local`, `/srv` whose `host::facts_of`/`host::protected`
   accept it, and `/tmp` for an unprivileged run. The root half of the suite
   drives a **deployment** boot, and `host::Provenance` refuses a manifest
   directory under `/tmp` (1777): *this was the second, previously hidden
   refusal the shim error had been masking*.
3. `a_writable_working_directory_is_refused` keeps its manifest and app in the
   (protected) fixture directory and makes a `cwd/` **subdirectory** the `0777`
   /`1777` hole — otherwise the manifest's own provenance refusal answers before
   the `sys.path` check the test is about.

### `.github/workflows/ci.yml`
1. `smoke (root)`: a new bounded step, after the runner-owned build and before
   the root run, `install -m 0644 target/py-sapi/libblastit_py3*.so
   target/debug/`, chowns only those `.so` files to root, `chmod go-w target
   target/debug`, then walks every ancestor of `target/debug` and **fails
   naming** one that is group/other-writable. The checkout above belongs to the
   runner; a recursive `chown` there would break every later runner step, and
   ordinary `0755` directories already satisfy provenance.
2. `pytest-plugin`: the venv moved from `/tmp/blastit-tests` (1777, uid 1001) to
   `/opt/blastit-tests`, created as the runner (`chown "$(id -u):$(id -g)"`),
   filled, then `chown -R root:root` + `chmod -R go-w`. Every step is bounded
   and `set -euo pipefail`, one `sudo --non-interactive` per line.

### `python/tests/test_live_server.py`
`run()` stages the project copy where a **production** boot will authorise it:
`_stage_for_production` copies it under `/opt` (`PROTECTED_ROOT`), hands it to
root and removes `go-w`, and passes an absolute `--blastit-report` in the test
user's `tmp_path` (the copy itself is readable, not writable, which is the point
of being root's). Without root *and* sudo the fixture still serves the project
through `blastit dev` from a plain `tmp_path` copy. This was required: with only
the venv moved, the live server failed on
`the manifest directory (/tmp/blastit-live-…/live_project) could be replaced by
another uid … owned by uid 1000 … a directory above it is group- or other-writable`.

## Why

Production stage 0 authorises every path a worker `dlopen`s (`libpython`, the
per-minor shim) and every tree it reads (`manifest_dir`, …) before a listener
binds (`host::Provenance`, `R7`/`B2-R5`). A shim or project another uid could
replace is code that uid chooses; `/tmp` and the wheel's old `/tmp` venv are
exactly that. Both jobs now hand root root-owned, non-group/other-writable paths,
which is what a container image gives `/app` and `_bin/`.

## Proof (all in `modelw/base:2026.07`, throwaway `postgres:18` on 25432)

### 1. `smoke` as uid 0 — 31/31
```
docker run --rm --user root --network host -v "$PWD:$PWD" -w "$PWD" \
  --cap-add=SETFCAP --cap-add=SYS_PTRACE --security-opt seccomp=unconfined \
  -e BLASTIT_TEST_PG_URL=postgresql://blastit:ci-not-secret@127.0.0.1:25432/blastit \
  … modelw/base:2026.07 \
  sh -c 'target/debug/build/blastit/e22e9785c72e879a/out/smoke-e22e9785c72e879a --test-threads=1 --nocapture'
```
```
test a_rootless_standalone_install_runs_dbproxy_from_the_capability_free_copy ... ok
test a_writable_working_directory_is_refused ... ok
test the_real_credential_is_in_no_environment ... ok
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.84s
```
(`--cap-add=SYS_PTRACE` is the container-only fact `docs/knowledge/LESSONS.md`
records: root in a container cannot read a `PR_SET_DUMPABLE(0)` role's
`/proc/<pid>/fd` or `environ` without it; a CI VM has it. `BLASTIT_PYTHON`
pointed at the image's `/opt/python/global` CPython 3.14, whose `libpython` is
the one the shims load.)

### 2. `python/tests/test_live_server.py` with the root-owned `/opt` venv — 5/5
```
sudo --non-interactive chown -R root:root /opt/blastit-tests
sudo --non-interactive chmod -R go-w /opt/blastit-tests
BLASTIT_TEST_PG_URL=… /opt/blastit-tests/bin/pytest python/tests/test_live_server.py -q
```
```
.....                                                                    [100%]
5 passed in 11.26s
```
The inner production server really ran: before `python/tests/test_live_server.py`
was fixed, the same command failed with the provenance sentence above; the
`sudo` leg was exercised (the venv is root-owned and the plugin's `sudo -n true`
probe succeeds).

### 3. Full `python/tests` (the job's own command)
```
/opt/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures
```
```
12 failed, 446 passed, 1 skipped, 65 subtests passed in 93.87s (0:01:33)
```
The 12 failures are all in the protected `python/tests/critical/test_check_contract.py`
and are **not** caused by this node: the identical 12 fail in the same image with
the *old* `/tmp/blastit-tests` venv and with `pytest==8.4.2` (`blastit check`
exits 3 with `no tests ran in 0.05s`, i.e. the gate's runner choice in that
image, not shim provenance). No other tier failed.

### 4. Gates
```
make fast-check            -> FASTCHECK_EXIT=0   (install-test: all cases passed; 0 `error|panicked` lines)
make workflow-check        -> workflow-check: ok — 716 checks over 9 workflows and 1 actions, 0 problem(s)
make packaging-gates-test  -> packaging-gates-test: ok — 131 cases passed (…)
```

## Review fix round (lead review)

1. `PROTECTED_ROOT = Path("/opt")` replaced by `_protected_root()` (`functools.cache`):
   the first of `/opt`, `/usr/local`, `/srv` that exists **and** whose own mode and
   every ancestor's up to `/` satisfy `mode & 0o022 == 0` (`_unprotected_reason`,
   plain `os.stat`). With none qualifying the suite fails through `pytest.fail`
   naming every rejected candidate and its reason (`does not exist`, `group- or
   other-writable (mode 0775)`, `cannot be inspected (…)`) — it never falls back to
   `blastit dev` and pretends production ran.
2. `_stage_for_production` wraps the whole staging body in
   `try/except BaseException: _remove_stage(target); raise`, so a failing
   `copytree`/`chown` no longer leaks `/opt/blastit-live-*`;
   `_remove_stage` removes only a `blastit-live-*` directory directly under the
   resolved root, never asserts, and prints (rather than raises) a failed `rm`, so
   cleanup cannot replace a test's real result with a fixture error.
3. `python/tests/README.md`: the recipe now creates the venv at
   `$HOME/.venvs/blastit-tests` and the privilege text is truthful — no skip (R39 →
   `blastit dev`), and production needs the venv tree root-owned and
   non-group/other-writable (`sudo --non-interactive chown -R root:root` +
   `sudo --non-interactive chmod -R go-w`), because a `/tmp` venv is refused by
   provenance.

### Re-proof (same image, throwaway `postgres:18` on 25432, removed afterwards)
```
sudo --non-interactive chown -R root:root /opt/blastit-tests
sudo --non-interactive chmod -R go-w /opt/blastit-tests
BLASTIT_TEST_PG_URL=… sudo --non-interactive -u user -E \
  /opt/blastit-tests/bin/pytest python/tests/test_live_server.py -q
```
```
.....                                                                    [100%]
5 passed in 12.62s
```
`make fast-check` — green (see the tail pasted by the implementer's final run).

## Follow-ups
- `python/tests/critical/test_check_contract.py`'s 12 failures under
  `modelw/base:2026.07` (both venv locations, pytest 8.4.2 and 9.2.0): the gate
  reports `no tests ran in 0.05s` instead of running the project's stub `pytest`.
  Needs a `test-curator` look with the CI image's exact interpreter before a pin
  is blamed.
- `a_non_root_init_without_its_capabilities_names_them` self-skips when run as
  uid 0 (as the root job does); its assertion has no witness in `smoke (root)`.
- The `/opt/blastit-tests` venv's `bin/python` symlink resolves to a `0777`
  interpreter path in `modelw/base`; discovery only checks the execute bit, but a
  writable interpreter is the same defect class as a writable shim.
