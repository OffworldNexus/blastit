# RESULT MA.4 — Fix `make check` on this host: PyO3 test binaries need `PYTHONHOME`

## What changed

`Makefile` (only `RUST_ENV`, plus comments):

```make
# Install prefix (parent of bin/): embedded interpreters (PyO3 test binaries) need it as PYTHONHOME
# at runtime, otherwise they cannot find the uv-managed stdlib ("No module named 'encodings'").
PYO3_PREFIX := $(if $(PYO3_BIN),$(abspath $(dir $(PYO3_BIN))/..),)
RUST_ENV = $(if $(PYO3_PREFIX),PYTHONHOME="$(PYO3_PREFIX)") PYO3_PYTHON="$(PYO3_BIN)" LD_LIBRARY_PATH="$(PYO3_LIBDIR)$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}"
```

The prefix is derived from the already-resolved `$(PYO3_BIN)` (parent of `bin/`, i.e. the same
directory whose `../lib` feeds `PYO3_LIBDIR`). No host-specific path is hardcoded, and the
`$(if ...)` guard keeps it empty (harmless) if interpreter discovery ever fails. Because the
export lives in the shared `RUST_ENV`, `rs-lint`, `rs-doc`, `rs-test`, `rs-test-critical` and
`mutants` all inherit it; it is inert for non-embedding targets.

`docs/knowledge/LESSONS.md`: new PyO3 bullet documenting that `PYO3_PYTHON` + `LD_LIBRARY_PATH`
are not enough for an *embedded* interpreter — `PYTHONHOME` must be the install prefix, and the
failure mode is `Could not find platform independent libraries <prefix>` /
`ModuleNotFoundError: No module named 'encodings'`.

No test was modified, skipped, weakened or newly `#[ignore]`d; nothing under
`crates/*/tests/critical_*.rs` or `python/tests/critical/` was touched
(`git status` shows only `Makefile` + `docs/knowledge/LESSONS.md`).

## Reproduction before the fix (clean env)

```console
$ PYO3_PYTHON=…/cpython-3.14.0-linux-x86_64-gnu/bin/python3.14 \
  LD_LIBRARY_PATH=…/cpython-3.14.0-linux-x86_64-gnu/lib \
  cargo test -p blastit --lib immortal --locked        # tail
test immortal::tests::versions_outside_3_12_to_3_14_are_refused ... ok
Could not find platform independent libraries <prefix>
Could not find platform dependent libraries <exec_prefix>
Fatal Python error: Failed to import encodings module
Python runtime state: core initialized
ModuleNotFoundError: No module named 'encodings'

Current thread 0x000075eaa85f96c0 [immortal::tests] (most recent call first):
  <no Python frame>
error: test failed, to rerun pass `-p blastit --lib`
note: test exited abnormally; to see the full output pass --no-capture to the harness.
```

## Acceptance evidence

### 1. `make rs-test` (clean shell: `env -u PYTHONHOME -u PYO3_PYTHON -u LD_LIBRARY_PATH`)

```console
$ make rs-test
…
test immortal::tests::a_skipped_pass_walks_nothing ... ok
test immortal::tests::an_immortalised_object_stops_counting_references ... ok
test immortal::tests::an_object_created_afterwards_is_unaffected ... ok
test immortal::tests::the_running_interpreter_follows_the_same_decision ... ok
test immortal::tests::the_running_interpreter_is_probed ... ok
test immortal::tests::the_3_14_wrap_is_survived_by_the_marker ... ok
test immortal::tests::the_walk_reaches_untracked_objects ... ok
test immortal::tests::marker_child ... ignored, the child of the_3_14_wrap_is_survived_by_the_marker: this binary, re-run with BLASTIT_IMMORTAL_CHILD

test result: ok. 131 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.25s
…
(54 × "test result: ok" overall; unit, integration and doc test suites all pass)
EXIT=0
```

All 20 immortal-related tests execute (17 `immortal::tests::*` — 16 pass, `marker_child` stays
ignored by design — plus 3 `config::tests::*immortalis*`), and the binary no longer aborts.

### 2. `make check` (clean shell, exit code)

```console
$ env -u PYTHONHOME -u PYO3_PYTHON -u LD_LIBRARY_PATH make check > /tmp/opencode/ma4-check.log 2>&1
EXIT=0
$ tail -5 /tmp/opencode/ma4-check.log
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.17s
uv run --no-project tools/plan.py validate
plan OK
```

`grep -E "encodings|FAILED|error: test failed|aborted"` over the full log: no matches.

### 3. Expanded env (what every Rust target now gets)

```console
$ make -n rs-test | head -1
PYTHONHOME="/home/remy/.local/share/uv/python/cpython-3.14.0-linux-x86_64-gnu" PYO3_PYTHON="/home/remy/.local/share/uv/python/cpython-3.14.0-linux-x86_64-gnu/bin/python3.14" LD_LIBRARY_PATH="/home/remy/.local/share/uv/python/cpython-3.14.0-linux-x86_64-gnu/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" cargo test --workspace --all-targets --locked
```

## Follow-ups

None. `make check` is green on this host without any manual environment export.
