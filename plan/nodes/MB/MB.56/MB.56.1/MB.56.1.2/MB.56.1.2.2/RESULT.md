# RESULT — MB.56.1.2.2

**One ELF per install.** The duplicate `blastit-dbproxy` program is gone: every
bundle and wheel ships `blastit` (+ `blastit-lsp`), and stage 0 runs the
credentials-holding role as `_bin/blastit --role=dbproxy`.

## What changed

### 1. The second binary is deleted
- `crates/blastit-dbproxy/src/main.rs` **deleted** (its `--role=dbproxy` grammar
  and its four unit tests were already pinned by `crates/blastit/src/cli.rs`'s
  `the_dbproxy_command_line_parses`, `seccomp_is_on…`).
- `crates/blastit-dbproxy/Cargo.toml`: the `[[bin]] name = "blastit-dbproxy"`
  block is gone; the crate stays a **library** (protocol + `role::dbproxy`).
- `blastit_build.py` **deleted**; `pyproject.toml` is `build-backend =
  "maturin"` with no `backend-path`, and `[tool.maturin] include` loses the
  `python/blastit/_bin/blastit-dbproxy` and `blastit_build.py` entries (the
  sdist keeps `crates/**/*` for the workspace-resolution reason).

### 2. Every consumer points at the one image
- `crates/blastit/src/check/dbproxy.rs`: `dbproxy_program()` **deleted** (its
  `current_exe().with_file_name("blastit-dbproxy")` fallback was the last
  `_bin/blastit-dbproxy` producer). The check now spawns
  `host.dbproxy().program` (+ its prefix) with `--role=dbproxy`, the same empty
  environment, the same descriptors and `--seccomp=1`, unchanged.
- `host.rs` (`BLASTIT_EXEC` doc), `roles.rs`, `stage0/fork_exec.rs`,
  `check/mod.rs`, `crates/blastit-dbproxy/src/role.rs`: prose now says the role
  is the same image, not a second program.
- `visibility.rs`: the two `execve`-permission fixtures name `blastit`.
- **The security invariant is kept**: `blastit --role=dbproxy` still runs
  `blastit_sys::caps::hold_none()` **before** `roles::dbproxy` (the deleted
  `main.rs` wrapper was a duplicate of exactly that; `cli.rs::dispatch` is the
  surviving call site), the child is still `env_clear()`ed, and the shipped
  binary still links no interpreter (`make rs-no-python` leg in `fast-check`).

### 3. Packaging
- `scripts/build-wheel.sh`: both `cargo build -p blastit-dbproxy --bin
  blastit-dbproxy` + `install … _bin/blastit-dbproxy` steps removed (manylinux
  container **and** host paths), the required-member loop now asks for
  `blastit/_bin/blastit` + `blastit-lsp`, and a **fail-closed** assertion refuses
  a wheel that still carries the retired program.
- `scripts/repack_universal_wheel.py`, `scripts/check-bundle-layout.sh`:
  comments/bookkeeping only (the repack relocates `maturin`'s one binary).
- `scripts/wheel-smoke.sh`: step 2 is now "`_bin/` holds one program" — the
  binary is executable and the retired path must **not** exist.
- `Makefile` (`test-critical` staging): the `_bin/blastit-dbproxy → blastit`
  symlink is gone; `_bin/blastit` is the one staged program.
- `python/blastit/_blastit.py`: `_PROGRAMS` is `("blastit", "blastit-lsp")`
  (the tuple exists to `chmod` `_bin/` members a wheel installed `0644`).

### 4. GHA + e2e
- `.github/actions/build-wheel/action.yml`: the two assertions became
  "`_bin/blastit` is `-rwxr-xr-x`" + "no second program".
- `.github/workflows/publish-pypi.yml`: the sdist comment no longer claims a
  backend prebuild, and the post-install step asserts `blastit --help` **and**
  that nothing named `blastit-dbproxy` exists in the venv.
- `e2e/rootless/run.py`: the capability probe no longer looks for a `dbproxy`
  file (the capable copy is still the only file with `cap_*`).
- `e2e/venv-smoke/run.sh`: the packaged-mode leg measures the one program
  (`_bin/blastit`), and the `BLASTIT_EXEC` stub legs are unchanged.
- `crates/blastit/tests/critical_artifacts.rs`: `WHEEL_BINARIES` is
  `blastit` + `blastit-lsp`, the fixture wheels carry those two, and a **new**
  refusal (`the_universal_wheel_shape_is_refused` case 11) rejects any third
  program under `blastit/_bin/` — the one-ELF invariant now has its own failing
  fixture.

## Evidence

```
$ cargo test -p blastit --locked --features python --test critical_artifacts
running 7 tests
test inflate::tests::a_stream_that_grows_past_its_declared_size_is_refused ... ok
test inflate::tests::deflate_vectors_decode ... ok
test the_universal_wheel_ships_no_interpreter_and_no_php ... ok
test the_wheel_without_an_extension_is_refused ... ok
test the_trait_scan_catches_a_planted_name ... ok
test the_universal_wheel_shape_is_refused ... ok
test the_deleted_transport_trait_stays_deleted ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.22s
```

```
$ make fast-check        # green end to end (make stops at the first failing target)
workflow-check: ok — 716 checks over 9 workflows and 1 actions, 0 problem(s)
plan OK
env -u PYO3_PYTHON -u PYTHONHOME cargo build -p blastit --locked
scripts/check-portable-elf.sh target/debug/blastit
ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
cargo test -p blastit --locked --features python --test critical_handoff
test result: ok. 16 passed; 0 failed
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

The `check-portable-elf.sh` line is the security invariant: the image that now
plays `--role=dbproxy` links `libc`/`libm`/`libgcc_s`/`ld-linux` only — no
interpreter, no `RUNPATH`.

```
$ bash -n scripts/build-wheel.sh scripts/check-bundle-layout.sh scripts/wheel-smoke.sh e2e/venv-smoke/run.sh
ok (all four)
$ uvx --offline ruff format --check e2e/rootless/run.py scripts/repack_universal_wheel.py python/blastit/_blastit.py
1 file already formatted / 1 file already formatted / 1 file already formatted
```

`dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl` (dated 13:00, i.e.
built **before** this change) still carried `_bin/blastit-dbproxy`; the artifact
test refused it by name, which is the new rule working. It was a generated,
gitignored file, and it was removed so the gate reads no pre-change artifact.

### Review fix (`MB.56.1.2.2` follow-up)

- `.github/workflows/publish-pypi.yml`: `pull_request.paths` no longer lists the
  deleted `blastit_build.py` (`pyproject.toml` + `scripts/**` already cover it).
- `scripts/build-wheel.sh` + `scripts/wheel-smoke.sh`: the `_bin/` gate is now an
  **exact allow-list** read from the artifact (`blastit`, `blastit-lsp`,
  `libblastit_py*.so`, `libblastit_php*.so`), not a grep for the retired literal
  name — a stray `blastit/_bin/blastit-extra` now fails `build-wheel.sh` and
  `wheel-smoke.sh` by name, instead of only `certify_universal_wheel`.

```
$ bash -n scripts/build-wheel.sh scripts/wheel-smoke.sh && rg -n 'blastit_build' --glob '!target' --glob '!plan' --glob '!docs/design' .
ok (both scripts parse); the workflow path filter is clean. `blastit_build` survives only in ARCHITECTURE.md:183 and docs/knowledge/LESSONS.md:7 (docs, outside this node's `paths` — see Follow-ups).
$ make fast-check
green (make stops at the first failing target; the same targets as the block above plus the allow-list scripts under `bash -n`)
```

## Notes / deviations

- `e2e/rootless/run.py` was rewritten by the workspace's format-on-write hook
  (`ruff format`); the file was **not** `ruff-format`-clean at HEAD (`ruff
  format --check` on the HEAD copy: *1 file would be reformatted*), so the diff
  carries that mechanical reformat in addition to the three semantic lines.
  Semantics are unchanged (`ruff format --check` now passes).
- `python/blastit/_blastit.py` is outside this node's `paths`, but its
  `_PROGRAMS` tuple named the retired file; leaving it would keep a `chmod`
  target that no longer exists and contradict "one ELF", so the one entry was
  removed.
- `rg 'blastit-dbproxy\b'` still matches, deliberately: the crate name
  (`Cargo.toml`/`Cargo.lock`, `blastit-manifest/tests`), the role prose in
  `python/blastit/_check_pytest.py`, and the **absence** assertions that must
  name the retired path (`scripts/build-wheel.sh`, `scripts/wheel-smoke.sh`,
  `.github/actions/build-wheel/action.yml`, `publish-pypi.yml`).

## Follow-ups

- `docs/`, `ARCHITECTURE.md`, `ROADMAP.md`, `CHANGELOG.md`, `AGENTS.md` still
  describe a `_bin/blastit-dbproxy` (out of this node's `paths`; a docs node owns
  them).
- `e2e/Dockerfile`, `e2e/dx_replay.py`, `e2e/rootless/{Dockerfile,README.md}`,
  `e2e/venv-smoke/Blastfile` carry the same stale prose (comments only, no
  assertion).
- **Pre-existing red, not this node's:** `make packaging-gates-test` →
  `test_repack_cases` expects a `blastit-manifest` console script, which
  `CONSOLE_SCRIPTS` in `scripts/repack_universal_wheel.py` has never defined
  (both files are identical at `HEAD`; verified with `git show`). `make check`'s
  gate will need that one of the two to move.
