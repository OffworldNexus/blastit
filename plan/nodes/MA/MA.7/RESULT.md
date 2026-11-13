# Result MA.7

## Summary

Both release-CI harness defects are fixed in `.github/workflows/ci.yml` and nothing else: no product
code, no fixture `sys.path` hack, no global `norecursedirs`/`collect_ignore`, no skipped job, no
weakened guard. The diff is +22/−2 lines, both hunks commented.

1. **`smoke (root)`** no longer relies on `sudo -E` (the runner's sudo-rs discards it, so root inherited
   neither `RUSTUP_HOME` nor `CARGO_HOME` and the rustup shim had no toolchain). Every variable root
   needs is now expanded by the *caller* shell and handed to `env` as `NAME=value` arguments, so the
   toolchain homes reach root explicitly. `cargo test -p blastit --locked -- --test-threads=1` is
   byte-identical and still runs **as root**.
2. **`pytest plugin (CPython 3.14)`** now collects `python/tests` with
   `--ignore=python/tests/runnable_docs/fixtures`, so the three runnable-docs fixture apps (which import
   `app` from their own directory via their own `pytest.ini`) are no longer collected from the repo root.

Local proof is complete for everything a subagent may run: the collection defect is reproduced and then
gone (0 errors, 246 passed locally), and `make check` is green on every target — 38 fences, 26 executed /
12 accounted / 0 failed, `zensical --strict`, `plan OK` in an isolated tree — **except** the final
`plan-validate` target, which fails on a plan-tree inconsistency the conductor created at 11:02
(`MA` is `done` with the still-`active` child `MA.7`). That is `plan/`, outside this node's `paths`, and
I may not mutate plan statuses: see "Acceptance item 2 and 4" below for the non-mutating proof that the
failure is not mine and for the one-line fix the conductor owns. Acceptance item 5 (real CI) needs the
next push, which is the conductor's.

## What changed

`.github/workflows/ci.yml` only.

**a. `smoke (root)`, lines 114–128 (15 lines, was 1):**

```yaml
      # sudo-rs (the runner's sudo) ignores `-E`, so root inherits neither
      # RUSTUP_HOME nor CARGO_HOME and the rustup shim finds no toolchain to run.
      # Pass what root needs explicitly; every `${...}` is expanded by *this*
      # shell before sudo, so the fallbacks see the runner user's homes.
      # PYO3_PYTHON is not just the build's interpreter: `tests/smoke.rs` derives
      # LD_LIBRARY_PATH for the binary it spawns from it, and the smoke tests read
      # the job's PostgreSQL service from BLASTIT_TEST_PG_URL.
      - run: |
          sudo env \
            "PATH=$HOME/.cargo/bin:$PATH" \
            "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" \
            "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
            "PYO3_PYTHON=$PYO3_PYTHON" \
            "BLASTIT_TEST_PG_URL=$BLASTIT_TEST_PG_URL" \
            cargo test -p blastit --locked -- --test-threads=1
```

**Why root now finds a toolchain:** sudo-rs resets the environment and ignores `-E`, so the old form gave
root only `PATH`; with `RUSTUP_HOME` unset the shim looked in root's home (`/root/.rustup`), found no
toolchain and refused to run. The new form passes `RUSTUP_HOME`/`CARGO_HOME` (the runner user's homes,
defaulted when the image does not export them) as literal `NAME=value` arguments to `env`, which `env`
sets *after* sudo's reset — so the shim resolves the toolchain `dtolnay/rust-toolchain` installed.
`PATH` keeps `$HOME/.cargo/bin` first, so `env` finds `cargo`; each `${...}` is expanded by the step shell
before `sudo` runs, which is why they are quoted as single arguments.

Two variables were added beyond the brief's minimum because dropping them would have silently changed
what the job proves (the old `-E` carried them):
- `PYO3_PYTHON` — `crates/blastit/tests/smoke.rs:699` (`library_path()`) derives the spawned binary's
  `LD_LIBRARY_PATH` from it, so with it missing the root smoke tests would lose `libpython`.
- `BLASTIT_TEST_PG_URL` — `crates/blastit/tests/smoke.rs:78` (`test_pg_url()`); without it the job's
  PostgreSQL service container is silently unused and the PG checks skip.

`LD_LIBRARY_PATH` was deliberately **not** added: `build.rs` puts the interpreter's library directory in
the binary's `RUNPATH` (see the `smoke.rs` module doc), and the job has never exported one.

**b. `pytest-plugin`, lines 235–242:**

```yaml
      - name: Run the plugin's tests
        # `python/tests/runnable_docs/fixtures/*` are runnable-docs fixture apps,
        # driven by the docs harness from their own directory (each has its own
        # pytest.ini). Collected from here they import `app`, which only exists on
        # the harness's path, so they error at collection. Ignore them here rather
        # than in a global `norecursedirs`, which would also change what the
        # harness collects inside those directories.
        run: /tmp/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures
```

`pytest python/tests -q` still means everything else: the ignore is on that one command line, so the
harness's own `pytest` invocations inside `fixtures/{hello,denial,findings,learn,shop}/` are untouched
(proved by `make docs-test` below, which drives those fixtures and stays 38/38 green).

## Acceptance evidence

### 1. `pytest python/tests -q --collect-only` from the repo root — before, then after

**BEFORE** (clean tree, HEAD `01c91cd`, my edit not yet applied):

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests -q --collect-only
_ ERROR collecting python/tests/runnable_docs/fixtures/findings/tests/test_app.py _
python/tests/runnable_docs/fixtures/findings/tests/test_app.py:3: in <module>
    from app import application
E   ModuleNotFoundError: No module named 'app'
_ ERROR collecting python/tests/runnable_docs/fixtures/learn/tests/test_app.py _
E   ModuleNotFoundError: No module named 'app'
_ ERROR collecting python/tests/runnable_docs/fixtures/shop/tests/test_app.py _
E   ModuleNotFoundError: No module named 'app'
=========================== short test summary info ============================
ERROR python/tests/runnable_docs/fixtures/findings/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/learn/tests/test_app.py
ERROR python/tests/runnable_docs/fixtures/shop/tests/test_app.py
!!!!!!!!!!!!!!!!!!! Interrupted: 3 errors during collection !!!!!!!!!!!!!!!!!!!!
301 tests collected, 3 errors in 0.17s
EXIT:2
```

**AFTER** — the job's command with the job's flag, exactly as CI will run it:

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests -q --collect-only --ignore=python/tests/runnable_docs/fixtures
python/tests/test_runnable_docs.py::RepositoryInventoryTest::test_the_real_inventory_is_green
301 tests collected in 0.09s
EXIT:0
```

Same 301 tests, 0 errors (the three fixture modules contributed 0 tests to that 301; they only errored).

**A real run** (not `--collect-only`), same command, this host (no PostgreSQL service, no `psycopg`):

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures
246 passed, 57 skipped, 57 subtests passed in 4.09s
EXIT:0
```

No failures. The 57 skips are environmental and are exactly the suites the CI job *does* provide for, via
`-rs`:

```console
$ ... -q -rs --ignore=python/tests/runnable_docs/fixtures
SKIPPED [1] python/tests/test_wire_capture.py:15: could not import 'psycopg': No module named 'psycopg'
SKIPPED [1] python/tests/test_pytest_plugin.py:833: needs a real PostgreSQL: set DATABASE_URL or BLASTIT_TEST_PG_URL
... (the rest of the 57 are the same two reasons)
```

The `pytest-plugin` job installs `psycopg[binary]` and sets `DATABASE_URL` from its service container, so
those run there; nothing was weakened to make this host green.

### 2. `make check` — every target green except the final `plan-validate` (see item 4)

```console
$ CARGO_NET_OFFLINE=true make check > /tmp/opencode/make-check.log 2>&1; echo "EXIT:$?"
EXIT:2
```

`make check` dependencies run in Makefile order (`guard critical-guard rs-fmt rs-lint rs-doc rs-test
test-critical py-lint docs-unit docs-check docs-test plan-validate`) and make stops at the first failure,
so an exit at the **last** prerequisite means every earlier one returned 0. The log confirms each:

```console
uv run --no-project tools/guard.py                      # 0
uv run --no-project tools/critical_guard.py --base HEAD  # "no change to a protected critical test"
cargo fmt --all -- --check                              # 0
cargo clippy --workspace --all-targets --locked -- -D warnings   # 0
cargo doc --workspace --no-deps --locked                # 0
cargo test --workspace --all-targets --locked           # ... "test result: ok" x N, 0 failed
cargo test --workspace --doc --locked                   # 0
cargo test -p <crate> --test critical_*                 # 10 critical suites, all ok
python -m unittest discover -s python/tests/critical    # Ran 50 tests ... OK
uvx --offline ruff check python tools                   # All checks passed!
uvx --offline ruff format --check python tools          # 61 files already formatted
uv run --no-project --with pytest==9.2.0 ... test_runnable_docs.py test_harness.py -q  # 46 passed, 57 subtests
uvx --offline zensical build --strict                   # No issues found / Build finished in 0.18s
scripts/build-wheel.sh                                  # blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl
uv run --no-project python python/tests/runnable_docs/run.py
...
38 fences: 26 executed, 12 accounted, 0 failed · 20.9s
uv run --no-project tools/plan.py validate
MA: done but has open children
make: *** [Makefile:54: plan-validate] Error 1
```

All 38 documented fences pass (26 executed / 12 accounted / 0 failed) — the docs harness is unchanged in
behaviour, which is the point of the narrow `--ignore` fix.

### 3. The `smoke (root)` step: sudo-rs-compatible, still root

New step lines are quoted in full under "What changed (a)" (15 lines: 7 comment + `- run: |` + 7). The
command itself is the old one verbatim — `cargo test -p blastit --locked -- --test-threads=1` — launched
through `sudo env …`, so it still runs as uid 0, which is the whole reason `tests/smoke.rs` uses
`/run/blastit` and `needs_root` (`crates/blastit/tests/smoke.rs:6-13`).

Loaded-layering proof on this host: sudo-rs resets the environment, so I reproduced that with `env -i`
(no `-E` equivalent) and root's home.

```console
# old form, as root would now see it: PATH only, HOME=/root, no RUSTUP_HOME
$ env -i HOME=/root "PATH=$PATH" cargo --version
error: could not create home directory: '/root/.rustup': Permission denied (os error 13)
EXIT:1

# new form: the step shell expands the homes, env sets them after the reset
$ env -i HOME=/root /usr/bin/env "PATH=$HOME/.cargo/bin:$PATH" "RUSTUP_HOME=$HOME/.rustup" \
    "CARGO_HOME=$HOME/.cargo" cargo --version
cargo 1.100.0 (3acc79c0c 2026-11-05)
EXIT:0
```

The same block, run through a `sudo()` shim that execs `env -i /usr/bin/env "$@"`, delivers all five
variables intact and resolves `cargo` from `$HOME/.cargo/bin`:

```console
PATH=/home/remy/.cargo/bin:…
RUSTUP_HOME=/home/remy/.rustup
CARGO_HOME=/home/remy/.cargo
PYO3_PYTHON=/nonexistent/pyo3
PG=postgresql://blastit:ci-not-secret@127.0.0.1:5432/blastit
/home/remy/.cargo/bin/cargo
cargo 1.100.0 (3acc79c0c 2026-11-05)
```

The workflow YAML parses (`pyyaml` was available offline) and both edited steps read back exactly as
intended:

```console
$ uv run --no-project --with pyyaml python -c 'import yaml; d=yaml.safe_load(open(".github/workflows/ci.yml")); print("YAML OK; jobs:", len(d["jobs"])); print(d["jobs"]["smoke"]["steps"][-1]["run"]); print(d["jobs"]["pytest-plugin"]["steps"][-1]["run"])'
YAML OK; jobs: 10
sudo env \
  "PATH=$HOME/.cargo/bin:$PATH" \
  "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" \
  "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
  "PYO3_PYTHON=$PYO3_PYTHON" \
  "BLASTIT_TEST_PG_URL=$BLASTIT_TEST_PG_URL" \
  cargo test -p blastit --locked -- --test-threads=1

/tmp/blastit-tests/bin/pytest python/tests -q --ignore=python/tests/runnable_docs/fixtures
```

### 4. `plan.py validate` — **fails, and not because of this change** (blocker for the conductor)

```console
$ uv run --no-project tools/plan.py validate
MA: done but has open children
EXIT:1
```

`MA` is `status: done` (`plan/nodes/MA/NODE.md`) and its only open child is `MA.7` — this node, created
by the conductor at 11:02 *after* the strategist's gate at 10:44 (`plan/STATE.md`: "MA status ready ->
done", gate PASSED with `make check` exit 0). The rule is `tools/plan.py:445`: a done node may not have
open children.

Non-mutating proof that my change is not involved (validate reads `plan/` only, and `MA.7` is still
untracked at HEAD):

```console
$ git diff --name-only            # my working tree vs HEAD, tracked files
.github/workflows/ci.yml
plan/AUTOPILOT
plan/BUDGET.md
plan/JOURNAL.md
plan/STATE.md

$ git status --short              # the first command of this session, before any edit of mine
 M plan/AUTOPILOT
 M plan/BUDGET.md
 M plan/JOURNAL.md
 M plan/STATE.md
?? plan/nodes/MA/MA.7/

$ rm -rf /tmp/opencode/validate-head && mkdir -p /tmp/opencode/validate-head
$ git archive HEAD | tar -x -C /tmp/opencode/validate-head
$ cd /tmp/opencode/validate-head && uv run --no-project tools/plan.py validate; echo "EXIT:$?"
plan OK
EXIT:0
```

So the tree at HEAD validates; the failure comes solely from the new `MA` → `MA.7` child under a done
milestone. The fix is one plan mutation the conductor owns (reopen `MA` to `active` for the release-CI
leaf, or reparent `MA.7`), and then `make check` exits 0 with the tail above. I did not edit `plan/`
(only this `RESULT.md`, which is the node's own folder) and did not run `plan.py set`/`bump`.

### 5. Real CI

Not provable locally: the two edited jobs only run on a push to `release/0.1.1`. The next CI run on PR
#62 must confirm `smoke (root)` and `pytest plugin (CPython 3.14, PostgreSQL)` go green — those two jobs
are exactly what needs the next run; nothing else in the workflow was touched. Everything the jobs do
before the failing step is unchanged, and the only claim they add is the flag/homes shown above.

## Files

- `.github/workflows/ci.yml` — the two hunks above (+22/−2).
- `plan/nodes/MA/MA.7/RESULT.md` — this report.

## Follow-ups

1. **Conductor, must land before `make check` can be green:** `MA` is `done` while `MA.7` is `active`
   under it (`tools/plan.py:445`), so `plan-validate` fails tree-wide. One of: reopen `MA` for this leaf
   (`uv run --no-project tools/plan.py set MA active`), or reparent `MA.7`. Until then every node in this
   tree fails `make check` on its last target, so the release gate cannot be re-verified.
2. **Conductor/lead:** confirm on the next CI run that `smoke (root)` and `pytest plugin` pass; the PR is
   the only place either can be proven.
3. **`docs/knowledge/LESSONS.md` (outside my `paths`, so recorded here for the lead to append):**
   *"sudo-rs ignores `-E`, so `sudo env "X=$X" …` (expanded by the caller shell, quoted as one argument)
   is the portable way to hand a variable to root — and remember that every job-level env var the step
   needs disappears too, not just the toolchain homes: `smoke (root)` loses `PYO3_PYTHON` and the
   `BLASTIT_TEST_PG_URL` service URL, which fail as 'no toolchain' and as silently-skipped PostgreSQL
   checks rather than as an obvious env error (MA.7)."*
4. **Harness-hygiene idea, not done here (out of scope):** the `pytest-plugin` job and `make docs-unit`
   now each spell out what to collect. If a future `python/tests/` directory is meant to run under the
   harness's own rootdir only, the least surprising place to say so is a `python/tests/conftest.py`
   `collect_ignore` **for that fixture tree** — but a root-level `norecursedirs` must stay off, because
   the docs harness runs pytest *inside* `fixtures/*/`.
