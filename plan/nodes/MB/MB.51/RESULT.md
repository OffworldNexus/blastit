# MB.51 — Fix the final 3 red PR #64 CI items

## Fix round (7 reviewer defects, items A–G)

Reviewers found seven defects in the first attempt; only these were changed.

**A. `ci.yml` `pytest-plugin` orphaned `/opt` comment.** The two lines "The
runner image ships /opt and its tool cache world-writable…" sat directly above
`- uses: dtolnay/rust-toolchain@stable`, reading as if rust-toolchain owned
`/opt`. Removed there; they are now the opening two lines of the `/opt`
`chmod`/`chown` step, merged with the `MB.51` `EACCES` ordering rationale, so
comment and step are one unit.

**B. `ci.yml` `smoke (root)` job comment.** The arithmetic said
`1 + 2 + 10 + 10 + 20 + 3 = 46` for 6 phases; the job now has 8 bounded steps.
It reads `5 + 1 + 2 + 10 + 2 + 10 + 20 + 3 = 53`, plus one clause: the `5` is
`uv python install`, the one phase that can stall on the index without being
wrong, which is why it is capped at 5 (`MB.51`).

**C. `ci.yml` `pytest-plugin` bounds.** Its three reordered steps were unbounded
and its `/opt` step was a 180-character `&&` chain whose first two `sudo` calls
were not `--non-interactive`, while `smoke` bounds every phase and argues that a
prompt-blocking sudo must be impossible. Now: `timeout-minutes: 5` on
`uv python install`, `timeout-minutes: 2` on the `/opt` step, `timeout-minutes:
2` on the interpreters step, and the `/opt` step is `run: |` +
`set -euo pipefail` with three separate `sudo --non-interactive` lines.

**D. `check-long-description.py` config-error handling.** `required_fields()`
was called inside `check_metadata()`, so a broken `pyproject.toml` was reported
once per artifact — each prefixed with a wheel name — under "the description
PyPI would show is not the one we want", exit 1. Now:
- `declared_requires_python(path: pathlib.Path = PROJECT_FILE) -> str` is a
  stdlib-only module-level function, and `import readme_renderer.markdown` is
  lazy (inside `check_description`), so the helper is importable without the
  third-party dependency. The PEP 723 header and its dependency list are
  unchanged.
- `main()` resolves it **once**, before the per-artifact loop; `except Failure`
  prints the failure to stderr and returns `2` (configuration error, not a
  description verdict), then passes the dict down to
  `check_metadata(headers, required)`.
- Messages: missing key → `` f"{path} has no `[project].requires-python`: it is
  the source of the wheel's `Requires-Python`" ``; wrong type/empty → names the
  value and asks for a PEP 440 specifier such as `>=3.12`; malformed/undecodable
  → names the path and the parse error. `ValueError` (a superset of
  `tomllib.TOMLDecodeError` and `UnicodeDecodeError`) is caught alongside
  `OSError, TypeError`; `KeyError` is separate because it is a `LookupError`,
  not a `ValueError`.
- The mismatch failure still prints both values and now appends
  `(from pyproject.toml's [project].requires-python)`.

**E. Reverted the unrelated reflow.** The first attempt reformatted ~50 lines of
`check-long-description.py` (`metadata_of`, `check_metadata`'s content-type
block, `check_description`, `main`'s usage/print lines) that this fix does not
touch. Their original wrapping is restored: the file's diff is now 64 insertions
/ 9 deletions, all semantic. `ci.yml` and the Rust file were not reformatted.
Note for a future attempt: the editor's format-on-write hook runs `ruff format`
(line-length 88) on `*.py`, which is what produced that reflow; `scripts/` is
deliberately not in `make py-format` (`ruff check|format python tools`), so the
file was installed verbatim instead.

**F. New test** `python/tests/test_release_scripts.py`
(`importlib.util.spec_from_file_location`, stdlib only — the lazy renderer
import is what makes that possible): `declared_requires_python` (1) follows a
temporary `pyproject.toml` declaring `>=3.99`, (2) raises `Failure` naming the
path when the file is missing, (3) raises when `[project].requires-python` is
absent, (4) raises on invalid TOML, (5) raises on a non-string value. Five tests,
`tmp_path` only, nothing else in the file touched (73 insertions, 0 deletions).

**G. `ci.yml` `BLASTIT_REQUIRE_ROOT_TESTS` rationale.** Added to the comment
block above `Run every test binary as root, one at a time`:
`BLASTIT_REQUIRE_ROOT_TESTS=1` makes a root-only fixture's skip fatal
(`critical_handoff::a_binary_owned_by_another_uid_is_refused`); ambient `CI` is
not a root signal because GitHub sets it on the unprivileged `test` job too.

### Fix-round evidence

```
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_release_scripts.py -q
.......................                                                  [100%]
23 passed in 0.59s
```

(the 18 pre-existing tests plus the 5 new ones; `make docs-unit` runs this file)

Real wheel, unchanged result:

```
$ uv run --with 'readme-renderer[md]' --no-project scripts/check-long-description.py dist/blastit-*.whl
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: ok

1 file(s), rendered the way PyPI renders them
exit=0
```

Configuration error, from a throwaway copy of the script with a
`pyproject.toml` that has no `[project].requires-python` (the real one was never
edited) — one message, no wheel prefix, exit 2:

```
$ /tmp/opencode/mb51.040O7x/pyproject.toml has no `[project].requires-python`: it is the source of the wheel's `Requires-Python`
exit=2
```

Drifting value (same throwaway copy, `>=3.99`) — both values plus the origin:

```
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: `Requires-Python` is ['>=3.12'], expected `>=3.99` (from pyproject.toml's [project].requires-python)
exit=1
```

Non-UTF-8 `pyproject.toml` (loaded with `spec_from_file_location`, stdlib only)
— a `Failure`, not a traceback:

```
Failure: /tmp/opencode/mb51u.Ktbmzv/pyproject.toml is not valid project metadata: 'utf-8' codec can't decode byte 0xff in position 39: invalid start byte
```

Protected root test, unchanged by this round:

```
$ CI=true cargo test -p blastit --test critical_handoff --locked --features python
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
```

Final combined gate (run last, after this file): `make fast-check &&
make workflow-check && make packaging-gates-test`.

## Goal

Close the last three failures on PR #64: `check-long-description.py` still
expected `Requires-Python >=3.13`; `setup-uv` ran after `chown -R root:root
/opt/hostedtoolcache` (so it could not `mkdir /opt/hostedtoolcache/uv`); and
`critical_handoff::a_binary_owned_by_another_uid_is_refused` keyed its
fatal-if-skipped guard on ambient `CI`, which GitHub sets on the unprivileged
`test` job too.

## Done

1. `scripts/check-long-description.py`
   - `Requires-Python` is no longer a constant. `required_fields()` reads
     `[project].requires-python` from `pyproject.toml` (`pathlib.Path(__file__)
     .resolve().parent.parent / "pyproject.toml"`, `tomllib`, stdlib) so the
     check and the build's metadata can never drift again; a missing file or key
     is a `Failure` naming the path. The module docstring says where the value
     comes from; the PEP 723 header is unchanged (`>=3.11`, `tomllib` is 3.11+).
2. `.github/workflows/ci.yml`
   - `smoke (root)`: order is now `setup-uv` → `uv python install 3.13 3.14`
     (new, `timeout-minutes: 5`) → `Make /opt and the tool cache
     non-world-writable (as root)` (still `timeout-minutes: 1`, unchanged three
     commands) → `Make the uv interpreters root-owned and put the minors on
     PATH` (new, `timeout-minutes: 2`: `chmod -R go-w` on the interpreter tree,
     `chown -R root:root` on `${UV_PYTHON_INSTALL_DIR:-$HOME/.local/share/uv/python}`,
     then the `/usr/local/bin/python3.13|3.14` symlinks) → `make python-runtime`
     → preflight. Every existing comment was carried over; the one that said
     "`/opt` got above" now reads "gets below"/states the `MB.51` ordering.
   - `pytest-plugin`: same reordering — `setup-uv` → `uv python install` → the
     `/opt` `chmod`/`chown` line → a new `Make the uv interpreters root-owned`
     step. Every existing step is kept.
   - `Run every test binary as root, one at a time`: added
     `"BLASTIT_REQUIRE_ROOT_TESTS=1"` to the `sudo --non-interactive env` list.
3. `crates/blastit/tests/critical_handoff.rs` (`a_binary_owned_by_another_uid_is_refused`,
   authorized by this node's `# Goal`): the skip is now fatal only when
   `BLASTIT_REQUIRE_ROOT_TESTS` is set; doc comment and message updated. No other
   assertion in the file changed.

## Evidence

Unprivileged (`uid 1000`) runner with `CI=true`, as the `test` job runs it —
no panic, all 16 pass:

```
$ CI=true cargo test -p blastit --test critical_handoff --locked --features python
running 16 tests
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

Unprivileged with `BLASTIT_REQUIRE_ROOT_TESTS=1` — panics (exit 101), as the
root gate requires:

```
$ BLASTIT_REQUIRE_ROOT_TESTS=1 cargo test -p blastit --test critical_handoff --locked \
    --features python -- a_binary_owned_by_another_uid_is_refused
SKIP a_binary_owned_by_another_uid_is_refused: creating another uid's file needs root
thread 'a_binary_owned_by_another_uid_is_refused' panicked at crates/blastit/tests/critical_handoff.rs:507:9:
BLASTIT_REQUIRE_ROOT_TESTS is set, so this safety rule must not go untested: ...
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out
exit=101
```

As root (`sudo --non-interactive`, the built binary directly, as `smoke (root)`
runs it) — passes:

```
$ sudo --non-interactive env ... BLASTIT_REQUIRE_ROOT_TESTS=1 "$binary" \
    a_binary_owned_by_another_uid_is_refused --test-threads=1 --nocapture
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 0.01s
exit=0
```

Long description against a real wheel:

```
$ uv run --with 'readme-renderer[md]' --no-project scripts/check-long-description.py dist/blastit-*.whl
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: ok

1 file(s), rendered the way PyPI renders them
```

(`UV_OFFLINE=1` cannot resolve `readme-renderer[md]`, so the run used the
index-enabled form; nothing was added to the repository.)

Workflow/packaging gates before the final combined run:

```
$ make workflow-check
workflow-check: ok — 715 checks over 9 workflows and 1 actions, 0 problem(s)

$ make packaging-gates-test
packaging-gates-test: ok — 131 cases passed (workflow=22, matrix=15, wheel-tag=4,
wheel-shims=6, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=16,
matrix-producer=2, package-workflow=11, wheel-floor=9, build-args=2, publish=19)
```

Final combined gate (run last, after this file): `make fast-check &&
make workflow-check && make packaging-gates-test`.

## Fix round 3

Four polish items, no behavioural redesign:

1. `declared_requires_python` splits `OSError` out of the TOML/value branch, so
   a missing or unreadable `pyproject.toml` now says
   `cannot read <path>: <errno>` instead of "is not valid project metadata".
2. Its `path` default is `None` and resolves the module's `PROJECT_FILE` at call
   time, so `main` can be redirected at a temporary file. New test
   `test_main_reports_a_broken_pyproject_once_and_exits_2` asserts exit 2, one
   non-empty stderr line, and no wheel-name prefix.
3. `check_description` wraps `import readme_renderer.markdown`, so a wrong
   invocation is a `Failure` line naming the `uv run --no-project` command, not a
   `ModuleNotFoundError` traceback. The import stays inside the function, so the
   stdlib-only test seam survives.
4. `ci.yml` `pytest-plugin`: the "Only the interpreter tree becomes root's"
   comment moved inside the `Make the uv interpreters root-owned` `run:` block,
   directly above `python_root=…` (as `smoke (root)` has it). No job-level
   `timeout-minutes` was added to `pytest-plugin`.

```
$ uv run --no-project -m pytest python/tests/test_release_scripts.py -q
........................                                                 [100%]
24 passed in 0.36s

$ uv run --with 'readme-renderer[md]' --no-project scripts/check-long-description.py dist/blastit-*.whl
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: ok

1 file(s), rendered the way PyPI renders them
rc=0

$ uv run --no-project python -c '…declared_requires_python(Path("/tmp/blastit/missing-pyproject.toml"))…'
Failure: cannot read /tmp/blastit/missing-pyproject.toml: [Errno 2] No such file or directory: '/tmp/blastit/missing-pyproject.toml'

$ uv run --no-project python scripts/check-long-description.py dist/blastit-*.whl  # renderer absent
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: FAILED

the description PyPI would show is not the one we want:

  blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: the renderer is missing: run me with `uv run --no-project scripts/check-long-description.py DIST...`
rc=1
```

## Follow-ups

- None. `scripts/test_packaging_gates.py` asserts no ci.yml step order (its
  ci.yml cases are fixture copies), so it needed no change and nothing was
  weakened by the reordering.
