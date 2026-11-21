# MB.50 — the last 5 PR #64 CI items

STATUS: done

All five items are fixed at the root cause (no new `scripts/check_*.py` guards), and
one real bug the musl leg found while proving item 5 is fixed with them: `grep -q`
on a pipeline under `set -o pipefail` killed the producer with SIGPIPE (Alpine
scheduling), and the `exec bash` in the container entry dropped the EXIT trap that
restored `target/` to the host uid.

## 1. `scripts/wheel-smoke.sh` — `entry_points.txt` spacing (MB.50 item 1)

`maturin` writes `blastit=blastit.pytest_plugin`; the check was a substring match
for `blastit = blastit.pytest_plugin`. It is now a MULTILINE regex over the
installed metadata, whitespace-insensitive around `=` and at end of line, while the
name stays exact and `[pytest11]` is still required. The wheel and the entry-point
name are untouched.

```
$ bash scripts/wheel-smoke.sh --python "$(uv run --no-project python -c 'import sys;print(sys.executable)')" dist/blastit-*-py3-none-*.whl
wheel-smoke: CPython 3.14
wheel-smoke: installed dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
wheel-smoke: import blastit ok
wheel-smoke: console scripts on PATH: blastit, blastit-lsp, blastit-manifest
wheel-smoke: entry_points.txt carries [console_scripts] and [pytest11]
wheel-smoke: blastit 0.2.0
wheel-smoke: blastit --help ok
wheel-smoke: dbproxy executable at .../blastit/_bin/blastit-dbproxy
wheel-smoke: 200 /, 404 /no-such-path
wheel-smoke: 3.14 wheel ok
```

(The wheel was built for the run with `bash scripts/build-wheel.sh` →
`dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl`; the install the smoke made
in `.venv` was removed again, so the worktree is as it was.)

## 2. `scripts/build-wheel.sh` — retried `clang` install (item 2)

The generated manylinux Dockerfile now retries the install three times and still
fails the build when every attempt failed: the loop's own exit status is `sleep`'s,
so the post-condition is `command -v clang` (non-zero when nothing was installed)
behind `|| exit 1`.

```
FROM ${base_image}
RUN for attempt in 1 2 3; do dnf install -y --setopt=install_weak_deps=False clang && break; dnf clean all; sleep 2; done \
 && command -v clang && dnf clean all || exit 1
```

The heredoc is unquoted (`<<DOCKER`), and the line contains no `$`, so host
expansion cannot eat it; `bash -n scripts/build-wheel.sh` and the wheel build above
(both heredoc paths exercised by `scripts/build-wheel.sh`'s own checks) pass.

## 3. `crates/blastit/src/host.rs` — symlink-independent unit test (item 3)

`every_named_program_that_carries_capabilities_is_refused` now names synthetic
paths (`/nonexistent/blastit-test/python{,-,-3.13}`), so `placement()`'s real
`fs::read_link` can never follow a CI runner's `/usr/local/bin/python3.13 ->
.../uv-python-dir/...` symlink.

```
$ cargo test -p blastit --lib host::tests::every_named_program_that_carries_capabilities_is_refused
running 1 test
test host::tests::every_named_program_that_carries_capabilities_is_refused ... ok
test result: ok. 1 passed; 0 failed; ... 283 filtered out
```

## 4. `crates/blastit-py-sapi/src/discovery` — unset `PATH` fallback (item 4)

`dev.path == None` (`env_clear()` in `critical_dev_reload`) now searches `_CS_PATH`'s
defaults `/usr/local/bin:/usr/bin:/bin` instead of zero directories. The list is one
`POSIX_SEARCH_PATH` constant shared with `production_interpreter` (the deployment
path list is the same three directories, now written once), and `Some("")` /
`Some("a:b")` keep `std::env::split_paths` semantics exactly.

```
$ cargo test -p blastit-py-sapi --lib discovery::tests::an_unset_path_searches_the_posix_defaults
running 1 test
test discovery::tests::an_unset_path_searches_the_posix_defaults ... ok
test result: ok. 1 passed; 0 failed; ... 28 filtered out
```

The test asserts the fallback directories, then either the interpreter found under
one of them or `NoInterpreter { searched }` naming exactly those three, and that a
set `PATH` is still read verbatim.

## 5. GitHub Actions (item 5)

- `.github/workflows/py-sapi.yml` `legs`: `dtolnay/rust-toolchain@stable` and
  `Swatinem/rust-cache@v2` added between `astral-sh/setup-uv` and the
  `bash scripts/py_sapi_leg.sh` step (the leg runs `make py-sapi`, which needs
  cargo). `packages.yml`/`nightly.yml` `py_shims` untouched, so rule 21 has nothing
  to compare differently.
- `scripts/py_sapi_leg.sh` musl: `alpine:3.21` now enters through `sh -eu -c`
  (installs `bash`, then runs the same script `--inside`), with the leg values
  passed as `sh` positionals (`"$1" "$2" "$3"`) so nothing is re-parsed as shell,
  `HOST_UID`/`HOST_GID` exported for the `target/` chown trap. `bash` is *not*
  `exec`ed — `exec` replaced the shell and silently dropped the trap, leaving
  `/src/target` root-owned; the trap now runs (`target/py-sapi/*.so` are 1000:1000
  after the leg).
- `scripts/py_sapi_leg.sh` gnu: when `PY_SAPI_PYTHON_<digits>` is unset and
  `uv python find --no-project "$minor"` fails, it runs `uv python install "$minor"`
  and exports `PY_SAPI_PYTHON_<digits>` from the new find.
- `.github/workflows/ci.yml`: `smoke` and `pytest-plugin` now `chown -R root:root`
  `/opt/hostedtoolcache` (extracted as uid 1001 by `actions/setup-python`) next to
  the existing `chmod -R go-w`, and `sudo --non-interactive chown -R root:root
  "$interpreters"` inside the `[ -d "$interpreters" ]` guard in both jobs.

```
$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu --out /tmp/opencode/py-sapi-test
exported: T blastit_py_abi_version
exported: T blastit_py_minor
exported: T blastit_py_abi_v1
undefined: Py_Initialize* (resolved by the host interpreter)
NEEDED: libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2 (zero libpython, gnu libc)
portable-elf: ok (libblastit_py313-x86_64-gnu.so)
py-sapi-leg: ok — libblastit_py313-x86_64-gnu.so (x86_64-gnu, CPython 3.13)

$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc musl --out /tmp/opencode/py-sapi-test
exported: T blastit_py_abi_version
exported: T blastit_py_minor
exported: T blastit_py_abi_v1
undefined: Py_Initialize* (resolved by the host interpreter)
NEEDED: libgcc_s.so.1 libc.musl-x86_64.so.1 (zero libpython, musl libc)
portable-elf: ok (libblastit_py313-x86_64-musl.so)
py-sapi-leg: ok — libblastit_py313-x86_64-musl.so (x86_64-musl, CPython 3.13)
```

```
$ make workflow-check
workflow-check: ok — 713 checks over 9 workflows and 1 actions, 0 problem(s)

$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; ... 0 leg(s) dropped
```

## Gate

```
$ cargo fmt --all && cargo fmt --all -- --check && make fast-check
...(all suites green)...
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

`make fast-check` also runs `sh scripts/test_install.sh`; the Rust workspace,
`wheel-smoke`'s own artifacts and the packaging script checks are green.

## Follow-ups

- The musl leg writes its artifact to `/src/target/py-sapi` regardless of `--out`
  (the mount only covers the worktree). Pre-existing shape, left alone here; a
  caller wanting `--out` elsewhere needs a second `-v "$out:/out"`.
- `nm -uD … | grep -q …` under `pipefail` is a repository-wide shape worth a sweep
  (`Makefile` `py-sapi` uses it too); only the leg script was in these paths.
- `docs/design/universal-distribution.md:69-70` still states the last discovery step
  as `{python3,python}` on `PATH` / in `/usr/local/bin`, `/usr/bin`, `/bin`.
  `discovery::search()` now tries `python3.<minor>` from `SUPPORTED_MINORS` (newest
  first) before those generic names, and the *unset* `PATH` case is `_CS_PATH`'s
  defaults. `docs/**` is outside this node's paths — a one-line amendment belongs to
  the doc node that owns that file.

## Review fixes

### A. `discovery::search()` offers a bindable minor first

Per directory it now tries `python3.14`, `python3.13` (from `SUPPORTED_MINORS`,
newest first) before `python3`/`python`. Directory order (PATH / `_CS_PATH`) is
unchanged, and the generic names stay last so an old-only host still gets the honest
`UnsupportedMinor` rather than `NoInterpreter`: `/usr/local/bin/python3.13` exists on
the runner, `/usr/bin/python3` is 3.12, and the fallback must not hand that 3.12 to
`discover()` just to be refused one step later.

```
$ cargo test -p blastit-py-sapi --lib
test discovery::tests::a_supported_interpreter_beside_the_generic_one_wins ... ok
test discovery::tests::an_old_python3_alone_is_still_refused_by_its_own_minor ... ok
test discovery::tests::a_set_path_is_searched_in_its_own_order ... ok
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### B. `ci.yml` chowns only the interpreter tree

`interpreters` keeps its `chmod -R go-w` untouched; the chown moved to
`python_root="${UV_PYTHON_INSTALL_DIR:-$HOME/.local/share/uv/python}"` behind its own
`[ -d ]` guard, in both `smoke` and `pytest-plugin` — `~/.local` also holds the
runner's `uv`/pipx, and `pytest-plugin` needs `pipx install maturin` to keep working
after that step.

### C. The remaining SIGPIPE checks in `py_sapi_leg.sh`

`if readelf … | grep -q 'NEEDED.*libpython'` (the guard could silently skip and the
next line then claimed `zero libpython`) and both `printf … | grep -q` libc checks
(a false 141 turned a correct musl artifact into "claims musl but needs") are now
`grep -E … >/dev/null` / `grep -F … >/dev/null`. The comment states the fact — `grep
-q` closes the pipe on the first match, so stdout must be consumed — and the docker
bootstrap now names the failure:

```
    docker run … || {
        echo "error: py-sapi-leg: $soname could not run in $image (bash bootstrap failed)" >&2
        exit 1
    }
```

### D. `build-wheel.sh` cleanup no longer fails the image

```
RUN for attempt in 1 2 3; do dnf install -y --setopt=install_weak_deps=False clang && break; dnf clean all; sleep 2; done \
 && command -v clang || exit 1
RUN dnf clean all || true
```

All three failures still exit 1 (`command -v clang` is the post-condition), while a
`dnf clean` hiccup after a successful install is only a warning. Proved both ways
with a stub `dnf`: succeed-on-attempt-2 → `rc=0`; all-fail with no `clang` on `PATH`
→ `rc=1`.

### E. `wheel-smoke.sh` entry point is scoped to `[pytest11]`

The check now walks the file's sections and matches a line in the `[pytest11]` body
only (`[ \t]*=[ \t]*`, CRLF accepted via `splitlines()` + `strip()`), keeping the
"has no `[pytest11]`" problem and the full `entry_points.txt` dump.

```
$ bash scripts/wheel-smoke.sh --python "$(uv run --no-project python -c 'import sys;print(sys.executable)')"
wheel-smoke: entry_points.txt carries [console_scripts] and [pytest11]
wheel-smoke: 3.14 wheel ok
```

Negative/positive of the scoping itself:

```
console_scripts-only -> False | pytest11+CRLF -> True
```

### Re-proved legs and gates

```
$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu --out /tmp/opencode/py-sapi-test
undefined: Py_Initialize* (resolved by the host interpreter)
NEEDED: libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2 (zero libpython, gnu libc)
py-sapi-leg: ok — libblastit_py313-x86_64-gnu.so (x86_64-gnu, CPython 3.13)

$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc musl --out /tmp/opencode/py-sapi-test
undefined: Py_Initialize* (resolved by the host interpreter)
NEEDED: libgcc_s.so.1 libc.musl-x86_64.so.1 (zero libpython, musl libc)
py-sapi-leg: ok — libblastit_py313-x86_64-musl.so (x86_64-musl, CPython 3.13)
-rwxr-xr-x 1 1000 1000 target/py-sapi/libblastit_py313-x86_64-musl.so

$ make fast-check    # exit=0
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)

$ make workflow-check
workflow-check: ok — 713 checks over 9 workflows and 1 actions, 0 problem(s)
```

