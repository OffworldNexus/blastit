# RESULT — MA.3.13: docs-gate unit suites in `make check`, harness checks pinned in-repo

STATUS: done

## What changed

| Path | Change |
|---|---|
| `Makefile` | New `docs-unit` target: `UV_OFFLINE=1 uv run --no-project --with pytest==$(DOCS_PYTEST) python -m pytest python/tests/test_runnable_docs.py python/tests/runnable_docs/test_harness.py -q`, with `DOCS_PYTEST ?= 9.2.0` (the same pin the harness uses). `docs-test` depends on it, and `check` lists `docs-unit` explicitly before `docs-check`/`docs-test`. |
| `python/tests/runnable_docs/test_harness.py` | New 25-test suite (38 subtests) over the real harness functions: flag-table drift (invented `docker`/`doctl` flag and subcommand refused, audit refuses a flag the real tool rejects and accepts the ones it prints), nonexistent `-f`/`--file`/`--spec`/`--manifest` paths refused while a page-shown file and `blastit.toml` pass, an absolute path outside the tree counted not failed, the zero-command rule for every fence shape (executing refused, accounted needs its reason, `denial` prose allowed), and wheel/provenance mismatch (semver floor, Cargo/CHANGELOG drift, missing and duplicate wheels). No mocks of harness functions; only the `FLAG_AUDIT` tuple and `REPO`/`DIST` fixtures are swapped. |
| `python/tests/runnable_docs/harness.py` | `FLAG_AUDIT` now carries `DOCTL_GLOBAL` and every `DOCTL_COMMANDS` entry (non-gated: a host without `doctl` reports `not installed here` instead of reddening the gate). `Harness.__init__` keeps the `gate_only` audit summary and prints it to stderr, so a gate log says which tables it could referee. `assert_flag_tables`, `FlagAudit` and the module/`main()` docstrings no longer claim "every table" is checked on every run: the gate re-checks the `gate` tables (`psql`), `--audit-flags` reconciles `docker`/`docker compose`/`psql`/`doctl` and names the absent ones. |

## Acceptance

```console
$ grep -n 'test_runnable_docs\|test_harness\|docs-unit' Makefile
14:# The one pinned dependency the runnable-docs gate needs (`docs-unit`, and the
39:        py-format py-lint test-critical docs-check docs-unit docs-test e2e mutants-rust
118:docs-unit: ## Run the runnable-docs gate's own tests (offline, no wheel build)
120:		python/tests/test_runnable_docs.py \
121:		python/tests/runnable_docs/test_harness.py -q
123:docs-test: docs-unit ## Execute every console fence in README.md and the public docs pages
137:check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test plan-validate ## Full fast unprivileged check
```

### `make docs-test` — twice, both green (25 s and 32 s, well under 5 min)

The unit suites run first, then the wheel build and all 38 fences:

```console
$ time make docs-test        # run 1
...
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py -q
46 passed, 57 subtests passed in 0.08s
...
harness: flag tables checked against --help: psql
...
38 fences: 26 executed, 12 accounted, 0 failed · 21.7s

real	0m25.104s

$ time make docs-test        # run 2 (after the final docstring/attribute polish)
46 passed, 57 subtests passed in 0.10s
harness: flag tables checked against --help: psql
38 fences: 26 executed, 12 accounted, 0 failed · 23.8s

real	0m25.833s

$ time make docs-test        # run 3 (final tree)
46 passed, 57 subtests passed in 0.11s
38 fences: 26 executed, 12 accounted, 0 failed · 30.4s

real	0m32.208s
```

`UV_OFFLINE=1` is set on the `docs-unit` recipe; the pin is read from the uv
cache (the run fails closed rather than downloading).

### `make check` — green, and it now runs the unit suites (two runs: 1 m 29 s, 60 s)

```console
$ make check
...
Ran 50 tests in 5.692s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
61 files already formatted
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py -q
46 passed, 57 subtests passed in 0.09s
uvx --offline zensical build --strict
...
harness: flag tables checked against --help: psql
...
38 fences: 26 executed, 12 accounted, 0 failed · 19.6s
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

Second run tail: `38 fences: 26 executed, 12 accounted, 0 failed · 26.2s` / `plan OK`, exit 0.

### `harness.py --audit-flags` reports the absent doctl

```console
$ uv run --no-project python python/tests/runnable_docs/harness.py --audit-flags
harness: flag tables checked against --help: docker, docker run, docker build, docker push, docker pull, docker logs, docker exec, docker image ls, docker compose, docker compose up, docker compose down, docker compose logs, docker compose build, docker compose run, docker compose ps, docker compose pull, psql (not installed here: doctl)
exit=0
```

### Red probe — the gate is load-bearing (fresh /tmp copy of the tree, unit suites only)

`make docs-unit` in a pristine copy is green (46 passed). One guard is then
reverted per copy; each makes the same target red.

```console
$ make docs-unit              # pristine /tmp/opencode/ma313-red copy
46 passed, 57 subtests passed in 0.13s

=== shape probe: _OPEN reverted to three backticks, no tilde ===
SUBFAILED(shape='tilde') ... FenceShapeTest::test_every_shape_yields_its_command
SUBFAILED(shape='four backticks') ...
SUBFAILED(shape='tilde with an info suffix') ...
SUBFAILED(shape='indented tilde with an info suffix') ...
FAILED ... FenceShapeTest::test_mixed_shapes_are_numbered_in_reading_order
7 failed, 43 passed, 53 subtests passed in 0.17s
make: *** [Makefile:120: docs-unit] Error 1
make exit=2

=== audit probe: the raise in assert_flag_tables made unreachable ===
E   AssertionError: Failure not raised
FAILED test_harness.py::FlagTableDriftTest::test_the_audit_refuses_a_flag_the_tool_rejects
1 failed, 45 passed, 57 subtests passed in 0.17s
make: *** [Makefile:120: docs-unit] Error 1
make exit=2

=== zero-command probe: the empty-fence guard removed ===
E   AssertionError: Failure not raised
FAILED test_harness.py::ZeroCommandTest::test_a_fence_without_prompts_is_refused
1 failed, 45 passed, 57 subtests passed in 0.17s
make: *** [Makefile:120: docs-unit] Error 1
make exit=2
```

## Checks run

- `make docs-test` ×3 (incl. the twice the acceptance asks for), `make docs-unit` ×4, `make check` ×2 — all green.
- `uvx --offline ruff check python tools` and `ruff format --check` — green (the new file formatted).
- No skip/xfail: `grep -n 'skip\|xfail' python/tests/runnable_docs/test_harness.py` matches only the method name `test_gate_only_skips_a_tool_that_is_not_here`; there is no `skip`/`xfail` construct and no conditional pass.
- Nothing under `crates/`, `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `docs/**`, `plan/**` was touched.

## Follow-ups (out of scope)

- `docs/testing/README.md` line 20 still says `make docs-test` builds the wheel
  and runs the 38 fences; it now also runs the two unit suites first (0.1 s).
  Worth one clause in the README when docs prose is next in scope (this node's
  `paths` exclude it).
- `docs/testing/README.md` §"What the gate refuses" already names `doctl` as a
  table checked against the real flags; the harness now matches that claim via
  `--audit-flags` (it remains non-gated, so an absent `doctl` is reported, not
  gated). No prose change was made.
- The audit's stale `harness.unserved()` hint text (`MA.3.7 (a)`) is untouched;
  it belongs to whichever node next edits that function.
