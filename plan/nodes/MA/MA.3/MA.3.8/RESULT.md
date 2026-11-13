# RESULT MA.3.8 — Wire the docs gate and critical-test guards into CI (attempt 2)

- **Status:** `done`, with the same evidence gap attempt 1 declared: **no real GitHub Actions run
  exists until the owner pushes this branch** (an agent may not push, and `act` is not installed).
  Everything below is a YAML parse, structural assertions, and an execution of the exact `run:`
  bodies, byte for byte, on this host.
- **Attempt:** 2/3. Paths touched: `.github/workflows/ci.yml`, `.github/workflows/docs.yml`. Nothing else.
- **Review findings closed:** the four in `LOG.md` (2 × MAJOR, 2 × MINOR). Mapping is below.

## What the review asked for, and what the files now do

| Finding | Change |
|---|---|
| **[MAJOR]** `docs-gate` pins 3.13 but the harness sanitises `PATH`, so `uv venv --python 3.13` cannot see `/opt/hostedtoolcache` and the first fence aborts | Two new steps: link `setup-python`'s interpreter into `/usr/local/bin/python${PYTHON_VERSION}` (a directory the sanitised `PATH` keeps), then **probe** it by creating a venv in the harness's exact environment (`env -i`, `HOME=<temp>`, `PATH=dirname(uv):/usr/local/bin:/usr/bin:/bin`, `UV_PYTHON_DOWNLOADS=never`) and failing with an `::error::` naming the fix. `PYO3_PYTHON` is now an explicitly named step ("so the wheel matches the pinned CPython"). `docs-gate` proved green **at 3.13**, not 3.14. |
| **[MAJOR]** `guards` ran `critical_guard.py` without `--node`, so a policy-compliant protected-test edit could never pass | `guards` derives the changed plan-node ids from `git diff --name-only "$base" -- plan/nodes`, runs `critical_guard.py --base "$base" --node <id>` for each, and **passes as soon as one authorizes** the whole diff (`exit 0`). Only then does it fall back to the report-mode run plus the `[VIOLATION:` verdict, now with corrected guidance (which node to add the glob to). `issuer`/`epic`/`milestone` nodes are skipped — see [Why container nodes are skipped](#why-container-nodes-are-skipped) — and a node whose `kind` cannot be read is still handed to the guard so its exit 2 surfaces. |
| **[MINOR]** `docs.yml` compared the inventory with the nav only, so a brand-new page named by neither passed | The step now also asserts `docs_dir/**/*.md − exclude_docs ⊆ nav pages` (mkdocs `exclude_docs` syntax, read from `zensical.toml`), i.e. the orphan class itself, and reports the orphan by path. MA.3.9 keeps the remaining work (deriving `public_pages` inside `fences.py`); this check does not depend on that node. |
| **[MINOR]** `guards` had no `timeout-minutes` and kept no log | `timeout-minutes: 5` on `guards`, and `actions/upload-artifact@v4` (`if: failure()`) uploads the tee'd `$RUNNER_TEMP/critical-guard.log`. |

Exit codes are never swallowed: every `cmd | tee` pair is followed by `status=${PIPESTATUS[0]}` (or
runs under `set -o pipefail`), and every non-zero path ends in `exit "$status"`.

## Acceptance

### (1)+(2) the two jobs exist and run the gate and both guards

```console
$ grep -nE "^  (docs-gate|guards):|^    timeout-minutes:|make docs-test \||make critical-skip-check$|--node \"\\\$node\"" .github/workflows/ci.yml
228:  docs-gate:
231:    timeout-minutes: 45
319:          make docs-test 2>&1 | tee "$RUNNER_TEMP/docs-test.log"
330:  guards:
336:    timeout-minutes: 5
349:        run: make critical-skip-check
397:            uv run --no-project "$guard" --base "$base" --node "$node" 2>&1 | tee -a "$log"
```

### (3) both YAMLs parse and every structural requirement holds

`actionlint`/`act` are unavailable here, so a stdlib+PyYAML validator asserts the shape
(`/tmp/opencode/ma38/validate.py`, kept out of the repo — `tools/` is outside this node's paths):

```console
$ uv run --no-project --with pyyaml /tmp/opencode/ma38/validate.py
ok   .github/workflows/ci.yml parses as YAML (448 lines)
ok   .github/workflows/docs.yml parses as YAML (180 lines)
ok   ci.yml runs on pull_request and push to develop
ok   docs-gate runs on ubuntu-latest
ok   docs-gate has a timeout
ok   docs-gate pins UV_CACHE_DIR at a cacheable directory
ok   docs-gate links setup-python's interpreter into /usr/local/bin
ok   the link is named after the pinned minor (python3.13)
ok   docs-gate probes `uv venv` in the harness's sanitised environment
ok   the probe fails the job when the interpreter is wrong or invisible
ok   link -> probe -> fence, in that order
ok   docs-gate fills the uv cache with pytest==9.2.0 before the gate
ok   the cache-filling venv uses the job's PYTHON_VERSION (no shell-vs-expression drift)
ok   the fence step tees its log without swallowing the exit code
ok   docs-gate uploads the verdict table on failure
ok   guards has a timeout-minutes
ok   guards runs make critical-skip-check
ok   guards derives the changed plan nodes from the diff
ok   the node id is the NODE.md parent directory name
ok   guards runs critical_guard with --base and --node per changed node
ok   container nodes are skipped (their paths delegate, not authorize)
ok   the guard's own exit code survives the pipe to tee
ok   a guard that cannot inspect the tree fails the step with its own code
ok   the report is the fallback signal when no changed node authorizes the diff
ok   a guard that printed no report fails the step
ok   the whole guard output is tee'd to one log
ok   guards uploads the guard log on failure
ok   guards checks out with fetch-depth: 0
ok   docs.yml asserts docs/**/*.md minus exclude_docs is named by the nav
ok   the orphan problem is reported as such
ok   the inventory/orphan check runs before `zensical build --strict`
ok   docs.yml still builds with --strict
ok   every make target exists: ['check', 'critical-skip-check', 'docs-test', 'this']
ok   every file a workflow names exists (missing: [])

all workflow assertions hold
exit=0
```

Every `run:` body was extracted verbatim (`/tmp/opencode/ma38/extract.py`) and passes `bash -n`
before being executed. Re-extracting from the final files and diffing against the executed copies
prints nothing (`guards + docs-build bodies identical to the ones executed`).

## `docs-gate` at CPython 3.13 (the MAJOR)

### The interpreter the sanitised `PATH` cannot see (red), then the link (green)

Reproduced with the runner's shape. `/usr/bin/python3.13` is masked in a private mount namespace
(`sudo unshare -m` + `mount --bind` of a non-executable file, host untouched, nothing persisted) so
`/usr/bin` holds only 3.12 — exactly a GitHub `ubuntu-latest` image — and `uv` is reached through a
directory that holds no interpreter, as `astral-sh/setup-uv` installs it:

```console
$ cat /tmp/opencode/ma38/probe.log
### 1. negative: the pinned 3.13 is not on the sanitised PATH (runner shape: /usr/bin 3.12 only)
error: No interpreter found for Python 3.13 in managed installations or search path

hint: A managed Python download is available for Python 3.13, but Python downloads are set to 'never'
::error::the gate's sanitised PATH cannot resolve CPython 3.13 with UV_PYTHON_DOWNLOADS=never, so every fence would abort at 'uv venv'. /usr/local/bin/python3.13 must point at actions/setup-python's interpreter.
probe exit=1

### 2. the link step, then the same probe
lrwxrwxrwx 1 root root 31 Nov 13 01:58 /usr/local/bin/python3.13 -> /tmp/opencode/ma38/pybin/python
linked: 3.13.18 /usr/local/bin/python3.13
link step exit=0
Using CPython 3.13.18 interpreter at: /usr/local/bin/python3.13
Creating virtual environment at: /tmp/opencode/ma38/runner-ns/path-probe/venv
probe: 3.13.18 resolved from the sanitised PATH
probe exit=0
```

The failure in (1) is the one the review predicted (`uv venv` aborting inside the gate, before fence
1). Step (2) is the new check passing after the new link step.

### The fence step itself, green at 3.13

Executed as the job does, with the link in place and `PYO3_PYTHON` pointing at it:

```console
$ MATURIN="uvx --offline maturin" UV_CACHE_DIR="$HOME/.cache/uv" \
    PYO3_PYTHON=/usr/local/bin/python3.13 RUNNER_TEMP=/tmp/opencode/ma38/runner \
    bash /tmp/opencode/ma38/steps/docs-gate/12-execute-every-documented-console-fence.sh
docs-gate step exit=0
$ grep -n "Found CPython\|Built wheel\|harness: wheel" /tmp/opencode/ma38/docs-gate-313.log
5:🐍 Found CPython 3.13 at /usr/local/bin/python3.13
14:📦 Built wheel for CPython 3.13 to dist/blastit-0.1.0-cp313-cp313-manylinux_2_39_x86_64.whl
16:harness: wheel blastit-0.1.0-cp313-cp313-manylinux_2_39_x86_64.whl sha256 799a1bdee3d8 · per-run uv cache /tmp/opencode/brd-eaepgc5g/uv-cache · installed package verified against the built wheel
$ tail -3 /tmp/opencode/ma38/docs-gate-313.log
PASS  docs/reference/cli.md  fence 4  usage                 0.0s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked (…)

38 fences: 26 executed, 12 accounted, 0 failed · 17.3s
```

So the `install` fences pass on 3.13: the wheel is `cp313`, the harness resolved the interpreter
from the wheel tag (`uv python find ==3.13` → `…/cpython-3.13-…/bin/python3.13` → `"3.13"`), and the
fence venv came from the sanitised `PATH`. Nothing here claims the *recorded* attempt-1 run at 3.14;
it is superseded by the run above.

## `guards` (the second MAJOR)

### Authorised: a protected test edited by the node that owns it now passes

Base `refs/remotes/origin/ma38-pre` = `5596dc2` (`7440cd5^`), a real commit on this branch whose diff
contains `plan/nodes/MA/MA.3/MA.3.7/NODE.md` and where
`crates/blastit/tests/critical_config_and_presets.rs` already exists. The file is modified by two
harmless comment lines (restored immediately, `git status` clean afterwards):

```console
$ uv run --no-project tools/critical_guard.py --base origin/ma38-pre        # attempt-1 behaviour
critical-guard: origin/ma38-pre..working tree, 67 changed file(s)
  M  crates/blastit/tests/critical_config_and_presets.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [VIOLATION: not covered by any node path]
(report-mode exit=0)

$ uv run --no-project tools/critical_guard.py --base origin/ma38-pre --node MA.3.7
  M  crates/blastit/tests/critical_config_and_presets.rs  [authorized by crates/blastit/tests/critical_*.rs]
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [authorized by crates/blastit/tests/critical_*.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node MA.3.7
(exit=0)
```

The old step failed on that `[VIOLATION:` line even though node `MA.3.7` authorises it — the defect
in the review. The new step, same diff:

```console
$ RUNNER_TEMP=/tmp/opencode/ma38/runner GITHUB_BASE_REF=ma38-pre \
    bash /tmp/opencode/ma38/steps/guards/05-…-a-change.sh
guards: base origin/ma38-pre; plan nodes changed by this branch: MA.3 MA.3.1 MA.3.2 MA.3.3 MA.3.4 MA.3.5 MA.3.7
guards: node MA.3 is a 'epic' container, not a task: its paths delegate, they do not authorize
guards: node MA.3.1 (plan/nodes/MA/MA.3/MA.3.1/NODE.md) may authorize the diff; asking the guard
…
node MA.3.7: paths ['crates/blastit/src/**', 'crates/blastit-dbproxy/src/**', 'crates/blastit/tests/critical_*.rs', 'crates/blastit-dbproxy/tests/critical_*.rs', 'docs/knowledge/LESSONS.md']
  M  crates/blastit/tests/critical_config_and_presets.rs  [authorized by crates/blastit/tests/critical_*.rs]
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [authorized by crates/blastit/tests/critical_*.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node MA.3.7
guards: node MA.3.7 authorizes every protected critical test change since origin/ma38-pre
guards step exit=0
```

### Denied: a weakened test no changed task node owns

Same base, `python/tests/critical/test_stdlib_only.py` appended with a stray line (restored at once):

```console
$ RUNNER_TEMP=… GITHUB_BASE_REF=ma38-pre bash /tmp/opencode/ma38/steps/guards/05-…-a-change.sh
guards: node MA.3.1 does not cover a protected change (its report is above); trying the next node
…
32:  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [VIOLATION: not covered by any node path]
33:  M  python/tests/critical/test_stdlib_only.py  [VIOLATION: not covered by any node path]
::error::a protected critical test was modified or deleted since origin/ma38-pre and no task node this branch changes lists it in its paths. Fix it by adding the file's glob to the paths of the node whose task edits it (plan/nodes/**/NODE.md, kind: task), or report STATUS: blocked. Only a task node's paths authorize a protected change.
guards step exit=1
```

### Everyday green: a diff with no protected change at all

```console
$ (origin/develop temporarily at HEAD) bash …/ci.guards.05-…-a-change.sh
guards: base origin/develop; plan nodes changed by this branch: MA.3.5
guards: node MA.3.5 (plan/nodes/MA/MA.3/MA.3.5/NODE.md) may authorize the diff; asking the guard
critical-guard: origin/develop..working tree, 6 changed file(s)
node MA.3.5: paths ['docs/testing/AUDIT-MA.3.md']
  no change to a protected critical test
guards: node MA.3.5 authorizes every protected critical test change since origin/develop
guards step exit=0
```

(`origin/develop` was put back to `2c05139` in the same command; `git rev-parse origin/develop`
confirmed it.)

### Why container nodes are skipped

The review's instruction was "any changed node id". Taken literally that makes the gate vacuous: the
epic `MA.3` node has `paths: [crates/**, python/tests/**, …]` **and its `NODE.md` changes on every
branch** (`git diff --name-only origin/develop -- plan/nodes` lists `plan/nodes/MA/MA.3/NODE.md`,
`plan/nodes/MA/NODE.md`, `plan/nodes/MB/NODE.md`, …). The same weakened file, judged by the epic:

```console
$ uv run --no-project tools/critical_guard.py --base origin/ma38-pre --node MA.3
  M  python/tests/critical/test_stdlib_only.py  [authorized by python/tests/**]
critical-guard: OK: all 2 protected change(s) are authorized by node MA.3
(exit=0)
```

So the loop skips `kind: issuer|epic|milestone` with a logged reason and only a **task** node
authorises — "the plan node doing it" in `critical_guard.py`'s own docstring. A node whose `kind`
line is missing is *not* skipped: it goes to the guard, whose exit 2 then surfaces.

### Bad base, and a guard that cannot inspect the tree

```console
$ RUNNER_TEMP=… GITHUB_BASE_REF=no-such-branch bash …/05-…-a-change.sh
fatal: bad revision 'origin/no-such-branch'
guards step exit=128                       # the derivation fails before any guard runs: never green

$ uv run --no-project tools/critical_guard.py --base origin/no-such-branch --node MA.3.7
critical-guard: git diff origin/no-such-branch failed (128): fatal: bad revision 'origin/no-such-branch'
guard exit=2

$ uv run --no-project tools/critical_guard.py --base origin/ma38-pre --node MA.3.7   # stub uv, guard unusable
::error::critical_guard could not inspect origin/ma38-pre for node MA.3.7 (exit 2)
::error::critical_guard could not inspect origin/ma38-pre (exit 2)
guards step exit=2                         # every non-zero code is re-raised, never turned into a pass
```

### `make critical-skip-check` (R42), both directions

```console
$ bash …/ci.guards.04-…-r42.sh
protected critical tests hold no skip construct
exit=0

$ printf '\npytest.mark.skip\n' >> python/tests/critical/test_stdlib_only.py && bash …/04-…-r42.sh
error: a protected critical test may never skip (R42):
python/tests/critical/test_stdlib_only.py:111:pytest.mark.skip
make: *** [Makefile:86: critical-skip-check] Error 1
exit=2
```

## `docs.yml` — inventory, nav and the orphan class (the MINOR)

```console
$ bash /tmp/opencode/ma38/steps/docs-build/04-check-the-runnable-docs-inventory-and-the-page-lis.sh
inventory: 21 pages / 38 fences accounted, 20 nav pages, 0 orphan pages
exit=0

$ printf '# orphan probe\n' > docs/orphan-probe.md && bash …/04-…-page-lis.sh
inventory: docs/orphan-probe.md is published by the site but named by neither the nav nor the runnable-docs inventory
inventory: the runnable-docs gate and the site disagree; fix python/tests/runnable_docs/inventory.toml or zensical.toml's nav
exit=1

$ printf '# scratch\n' > docs/knowledge/scratch-probe.md && bash …/04-…-page-lis.sh
inventory: 21 pages / 38 fences accounted, 20 nav pages, 0 orphan pages
exit=0
```

A page named by neither the nav nor the inventory is now red; a path under `exclude_docs`
(`/knowledge/`) is not a false positive. Both probes were deleted immediately (`git status` clean for
`docs/` apart from MA.3.5's own untracked `docs/testing/AUDIT-MA.3.md`).

The `docs.yml` header comment was rewritten accordingly: it no longer says the inventory alone covers
`site/orphan`, and it does not defer the class to MA.3.9 — the assertion is in the step.

## `make check`

```console
$ make check > /tmp/opencode/ma38/make-check.log 2>&1; echo "make check exit=$?"
make check exit=0
$ head -5 /tmp/opencode/ma38/make-check.log
uv run --no-project tools/guard.py
guard: OK (13 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD 
critical-guard: HEAD..working tree, 6 changed file(s)
  no change to a protected critical test
$ grep -n "hold no skip construct" /tmp/opencode/ma38/make-check.log
2157:protected critical tests hold no skip construct
$ tail -4 /tmp/opencode/ma38/make-check.log
PASS  docs/security.md  fence 1  static                0.0s  statically checked (…)

38 fences: 26 executed, 12 accounted, 0 failed · 18.6s
uv run --no-project tools/plan.py validate
plan OK
```

2 510 lines, exit 0: `guard`, `critical-guard`, `rs-fmt`, `rs-lint`, `rs-doc`, `rs-test`,
`test-critical` (incl. `critical-skip-check`), `py-lint`, `docs-check`, `docs-test`, `plan-validate`.
Nothing is committed; `git status` shows only ` M .github/workflows/ci.yml`,
` M .github/workflows/docs.yml` from this node.

## Host substitutions (named, not hidden)

`extract.py` writes each `run:` body to a file and the file is executed unchanged. Four environment
facts of this host had to be supplied, and no `run:` line, flag or condition was altered:

1. `MATURIN="uvx --offline maturin"` — this host has no `pipx`, so the job's `pipx install maturin`
   has no equivalent; `build-wheel.sh` accepts the override.
2. `uv` was reached through `/tmp/opencode/ma38/uvbin/uv` (a directory holding only `uv`/`uvx`) so
   `dirname($(command -v uv))` is interpreter-free, as `astral-sh/setup-uv`'s directory is. On this
   host `uv` shares `~/.local/bin` with uv-managed `python3.13`/`python3.14` shims, which would
   otherwise leak an interpreter into the harness's sanitised `PATH` and hide the bug.
3. `setup-python`'s `python` was emulated by `/tmp/opencode/ma38/pybin/python` → CPython 3.13.18,
   and `/usr/bin/python3.13` was masked in a private mount namespace (`sudo unshare -m`, nothing
   persisted) so `/usr/bin` looks like the runner's 3.12-only one.
4. `RUNNER_TEMP=/tmp/opencode/ma38/runner`; `PYO3_PYTHON`/`GITHUB_ENV` were exported instead of
   written to `$GITHUB_ENV` (the only GitHub-only indirection in those steps).
   `UV_CACHE_DIR="$HOME/.cache/uv"` for the gate run (this host is offline, so the cache was already
   filled; the job's fresh-cache path was proven in attempt 1 and is unchanged).

The `/usr/local/bin/python3.13` link created for evidence was removed afterwards
(`ls /usr/local/bin/python3.13` → no such file), which is the state `make check` was measured in.

## What is not proven

- **No GitHub Actions run exists.** Neither job has executed on a runner; the owner's push (or a
  `workflow_dispatch` run) is the first real signal.
- **Not verified on the runner image:** that `sudo ln -sfn` into `/usr/local/bin` is permitted (it is
  the standard mechanism `setup-*` actions use, and `ubuntu-latest` runs steps as a passwordless
  sudoer), that `pipx install maturin` and `actions/cache@v4` behave, and the cold-cache wall time of
  `make docs-test` (the local run above is warm at 41 s wall, 17 s in fences).
- **`uv python find ==3.13` reading the wheel tag** is exercised locally against uv-managed 3.13; on
  the runner it will resolve `setup-python`'s (or the new link) and yield the same `"3.13"` suffix,
  but that exact resolution is inferred, not observed.

## Follow-ups (outside this node's paths)

1. **`tools/critical_guard.py` has no union/`--enforce` mode.** The per-node loop accepts a single
   node that covers *every* protected change in the diff; a PR touching two protected tests owned by
   two different task nodes would need the union of their paths and fails today. An
   `--enforce-nodes <id>…` flag that unions `paths` and, on failure, names the offending files is the
   right fix; `tools/` is not in MA.3.8's paths.
2. **Container-node authorisation should be impossible in the tool**, not in the workflow: `--node
   MA`/`MA.3` authorises anything under `crates/**`/`python/tests/**`, and those `NODE.md` files
   change on every branch. If `--enforce` lands, it should refuse a `kind` that is not `task` itself.
3. **`tools/critical_guard.py` is not protected by any guard** (unchanged from attempt 1): a PR may
   weaken the checker in the same commit that weakens a test.
4. **On a push to `develop` the base is the pushed commit itself**, so the diff is empty and `guards`
   passes by construction; the pull-request run is the enforcing one. Using `github.event.before`
   (with the all-zeros branch-creation guard) would close that, but it deviates from the node's
   literal command, so it is not taken here.
5. **`docs.yml` duplication:** the orphan assertion reads `zensical.toml` inline (fourteen lines) until
   MA.3.9 moves `public_pages` off the nav and into `fences.py`; then this step should call the shared
   helper instead of re-deriving `exclude_docs`.
6. **LESSONS.md candidate (could not be appended: `docs/knowledge/` is not in this node's paths):**
   creating `/usr/local/bin/python3.N` while testing an interpreter-resolution fix breaks
   `cargo test -p blastit --lib` — `host::tests::every_name_of_a_capable_interpreter_is_refused`
   treats any `<name>`/`<name>-init` pair on `PATH` as an init candidate and fails, so the link must be
   removed before `make check`.
