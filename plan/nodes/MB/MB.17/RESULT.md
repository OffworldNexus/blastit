# Result MB.17

## Summary

A plain `cargo build -p blastit --locked` now produces the **standalone** artifact: `blastit`'s
default features are `php` only, so the binary links `libc`/`libm`/`libgcc_s`/`ld-linux` and carries
no `RPATH`/`RUNPATH` at all. The wheel opts in to the interpreter in the one place that *has* one
(`crates/blastit-py`: `blastit = { …, features = ["python"] }`), and every `make` target that needs
CPython passes the feature explicitly, so `make check` still exercises both configurations.

Three guards were added, and each was proven to fail on the 0.2.0 defect before being accepted:

* `scripts/check-portable-elf.sh` — `readelf -d`: no `NEEDED … libpython`, no `RPATH`/`RUNPATH`
  (a build-host path can only be a search path, so "none" is the checkable assertion). Wired into
  `make rs-no-python`, `make docs-serve-stage` (MB.14 dogfood target) and `scripts/build-wheel.sh`,
  which now also extracts `blastit/_blastit*.so` from the built wheel and runs the same check on it.
* `scripts/standalone-runtime-check.sh` — boots `blastit dev` against a static fixture and asserts
  `200` + `Cache-Control: no-cache` and `404`, with `--require-no-python` refusing to run if a
  CPython is on `PATH`, so the CI job is an execution proof and not an inspection.
* `.github/workflows/ci.yml` — new `standalone` job: `cargo build -p blastit --locked`, the `readelf`
  check, `docker run debian:trixie-slim` (asserted to have no Python) running that script. Both
  container steps were executed locally against the final binary and against the 0.2.0-style
  Python-linked binary (see below).

The guard immediately paid for itself: `make docs-serve-check` failed on its first run because
`crates/blastit/build.rs` emitted `-Wl,-rpath,<PYO3_PYTHON prefix>/lib` whenever that **variable** was
set — even with the `python` feature off, where nothing links `libpython`. `build.rs` now gates the
link arg on `CARGO_FEATURE_PYTHON` (the only signal that answers "does this artifact link
`libpython`?"), and the Makefile still builds the artifact under `STANDALONE_ENV` as belt and braces.

## The defect, verbatim

### BEFORE (`default = ["python", "php"]`, `cargo build -p blastit --locked`)

```console
$ readelf -d target/debug/blastit | grep -E 'NEEDED|RUNPATH|RPATH'
 0x000000000000001d (RUNPATH)            Library runpath: [/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/lib]
 0x0000000000000001 (NEEDED)             Shared library: [libpython3.14.so.1.0]
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
```

The same build, run where that one developer's uv cache is not mounted (the CI image), reproduces the
owner's error exactly:

```console
$ docker run --rm -v /tmp/…/python-linked/blastit:/usr/local/bin/blastit:ro \
    -v "$PWD/scripts/standalone-runtime-check.sh:/usr/local/bin/…:ro" \
    -e BLASTIT_BIN=/usr/local/bin/blastit \
    debian:trixie-slim bash /usr/local/bin/standalone-runtime-check.sh --require-no-python
host: no CPython on PATH
standalone-runtime-check: /usr/local/bin/blastit dev --port 18400
error: blastit dev exited before 127.0.0.1:18400 answered
/usr/local/bin/blastit: error while loading shared libraries: libpython3.14.so.1.0: cannot open shared object file: No such file or directory
exit=1
```

`scripts/check-portable-elf.sh` on that same file exits `1` and names both defects.

### AFTER (`default = ["php"]`, `cargo build -p blastit --locked`)

```console
$ cargo build -p blastit --locked && readelf -d target/debug/blastit | grep -E 'NEEDED|RUNPATH|RPATH'
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
```

## The build-script gate, both ways (follow-up pass)

```
$ PYO3_PYTHON=/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/bin/python3.14 \
    cargo build -p blastit --locked --no-default-features --features php
$ readelf -d target/debug/blastit | grep -E 'NEEDED|RUNPATH|RPATH'
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
# no RPATH/RUNPATH line at all (grep -E '\((RUNPATH|RPATH)\)' exits 1)
```

With the capability on, the rpath is expected in a dev build and only there:

```
$ PYO3_PYTHON=… LD_LIBRARY_PATH=… cargo build -p blastit --locked --features python
$ readelf -d target/debug/blastit | grep -E 'NEEDED.*libpython|RUNPATH|RPATH'
 0x000000000000001d (RUNPATH)            Library runpath: [/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/lib]
 0x0000000000000001 (NEEDED)             Shared library: [libpython3.14.so.1.0]
```

The shipped wheel, built with `PYO3_PYTHON` set and `blastit/python` on, still carries neither:

```
$ PYO3_PYTHON=… scripts/build-wheel.sh | tail -2
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   /var/tmp/blastit-wheel.pqaHA4/_blastit0.so: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
```

## Acceptance

### 1. Default build is standalone, and runs without Python

```console
$ env -i PATH=/usr/bin:/bin BLASTIT_MANIFEST=/tmp/opencode/mb17-port8080/blastit.toml \
    ./target/debug/blastit dev --port 8080
GET / -> 200
GET /nope -> 404
--- boot log ---
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
16:21:37.185 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
16:21:37.193 INFO  ready       serving  routes=1  workers=16
16:21:37.437 INFO  request     GET / → site  200  0.3ms
16:21:37.443 INFO  request     GET /nope → site  404 unmatched  0.1ms
```

`/usr/bin/python3` (3.10, no shared `libpython`) is on that `PATH` but is never executed; the CI image
proof below has no Python at all.

### 2. WSGI opts in explicitly

`crates/blastit-py/Cargo.toml` → `blastit = { …, features = ["python"] }`; `make check` stays green.

Make targets that got `--features python` added (the owner asked for this list):

| Target | Change |
|---|---|
| `rs-lint` | `cargo clippy --workspace --all-targets --locked --features python` |
| `rs-doc` | `cargo doc --workspace --no-deps --locked --features python` |
| `rs-test` | `cargo test --workspace {--all-targets,--doc} --locked --features python` |
| `rs-test-critical` | `--features python` for the `blastit` crate (`crates/blastit/tests/critical_*.rs`); other crates unchanged |
| `test-critical` | `cargo build -p blastit-py -p blastit-dbproxy --features blastit/python` |
| `rs-no-python` (rewritten) | now the **default build's** guard: `cargo build -p blastit --locked` under `STANDALONE_ENV` + `scripts/check-portable-elf.sh` + `cargo tree -e normal` shows `0 pyo3`, plus the interpreter-free unit/`no_python_build` suites |
| `docs-test` / `e2e` (via `scripts/build-wheel.sh`) | maturin `--features blastit-py/extension-module,blastit/python` |
| `docs-serve-stage` (MB.14) | plain `cargo build -p blastit --locked` under `STANDALONE_ENV` + `scripts/check-portable-elf.sh` on the result |

So `make check` runs **both** configurations: `rs-test`/`rs-test-critical`/`test-critical`/`docs-test`
with the capability on, `rs-no-python` on the default artifact.

```console
$ make rs-no-python
cargo build -p blastit --locked            # the default build is the standalone artifact
cargo build -p blastit --locked
ok   /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
cargo check -p blastit-view --no-default-features
cargo test  -p blastit --lib --locked      # the default configuration
cargo test  -p blastit --test no_python_build
cargo test -p blastit-view --no-default-features --locked --lib
clippy      -p blastit{,-view} (standalone)
default build: 0 pyo3 dependencies (no libpython linked)
exit=0
```

### 3. No RUNPATH on shipped artifacts (binary **and** wheel `.so`)

```console
$ scripts/build-wheel.sh 2>&1 | tail -3
📦 Built wheel for CPython 3.14 to dist/blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   /var/tmp/blastit-wheel.DulUcM/_blastit0.so: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
```

The wheel's WSGI path is exercised in the same `make check`: `docs/frameworks/flask.md` fence 2
(`dev`, "served on 127.0.0.1:8080") and `django.md` fence 3 (`learn` then clean `check`) install and
boot the wheel this script built.

### 4. CI guard + execution on an image with no Python

`.github/workflows/ci.yml` gains the `standalone` job (7 steps, YAML parsed with PyYAML: 11 jobs).
Both container steps were run here, verbatim, against the final binary:

```console
$ docker run --rm debian:trixie-slim sh -eu -c '…no python on PATH…'
debian:trixie-slim: no CPython on PATH

$ docker run --rm -v "$PWD/target/debug/blastit:/usr/local/bin/blastit:ro" … debian:trixie-slim \
    bash /usr/local/bin/standalone-runtime-check.sh --require-no-python
host: no CPython on PATH
standalone-runtime-check: /usr/local/bin/blastit dev --port 18400
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
standalone-runtime-check: 2/2 passed without touching an interpreter
exit=0
```

Also in `ci.yml`: `--features python` on the `lint` (clippy/doc), `test` and smoke-job `cargo test -p
blastit` steps, because those either select only `blastit` or should say what they mean now that the
default is the standalone build.

### 5. Docs (one or two lines each, no tutorial)

* `docs/reference/cli.md` under `## blastit`: "There is no `--port` here: the listener is a
  [setting](settings.md) (`PORT`, or `BLASTIT_*`), and `blastit --port 8080` exits `1` with the
  usage. `--port` is `blastit dev`'s flag." (verified: `blastit --port 4242` → ``blastit: `--port`
  is not `--key=value` `` + usage, exit 1)
* `docs/getting-started.md` §1: "Without Python at all, build the standalone binary from this
  repository: `cargo build -p blastit --locked` writes `target/debug/blastit`, a plain Rust binary
  with no `libpython` to install and no `PATH` to configure."; §3: "`blastit dev --port N` moves it
  off `8080`."

`make docs-check` (Zensical `--strict`) and the runnable-docs fences `docs/reference/cli.md` 1–4 pass.

### 6. `make docs-serve` builds the standalone variant

`docs-serve-stage` builds `cargo build -p blastit --locked` — now the standalone build — under
`STANDALONE_ENV` (which unsets `PYO3_PYTHON`, see the note below), then runs
`scripts/check-portable-elf.sh "$(DOCS_SERVE_BIN)"`, so the dogfood target fails closed if the
dependency ever returns.

```console
$ make docs-serve-check
portable check   -> …/target/debug/blastit
ok   …/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
docs-serve-check: serving …/target/docs-serve/site on 127.0.0.1:8765
ok   / (index)                      200  text/html; charset=utf-8  no-cache
ok   /reference/cli/                200  text/html; charset=utf-8  no-cache
ok   missing path (not the index)   404  text/plain; charset=utf-8  no-store
ok   asset /assets/javascripts/bundle.3cb411a8.min.js 200  text/javascript  no-cache
docs-serve-check: 4/4 checks passed
exit=0
```

### Gate

```console
$ make check
…
clippy      -p blastit{,-view} (standalone)
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0        # 0 FAIL lines in the fence table; 58 fences: 28 executed, 30 accounted, 0 failed
```

## Notes for the owner

* **`cargo build` at the workspace root is not the standalone build.** Cargo unifies features across
  the graph, so `--workspace` inherits `blastit/python` from `blastit-py` and relinks `target/debug/
  blastit` against `libpython` (measured: `NEEDED libpython3.13.so.1.0`). That is inherent to keeping
  the wheel correct, including a from-source `pip install .` that passes no `--features` at all.
  Build/ship with `-p blastit` (docs, `make rs-no-python`, `ci.yml`'s `standalone` job all do), and
  `docs/knowledge/LESSONS.md` now records the trap plus a comment in `crates/blastit-py/Cargo.toml`.
* One `make check` run in this session failed `critical_dev_and_fail_closed::
  dev_boots_five_unprivileged_roles_and_strips_secrets` (POST `/env` observed `200`, server logged
  `413`) under full-gate load; the test then passed 3/3 consecutively standalone and in two later
  full `make check` runs, and no code on that path was touched. Recorded as a follow-up rather than
  changed (it is a protected critical test).

## Final pass — two review findings

### Mutation campaigns kept the interpreter roles

`make mutants-rust` now passes the capability explicitly. `cargo mutants --list` is textual, so the
two counts are identical — what changed is whether the campaign can run and catch anything:

```console
$ cargo mutants -p blastit --list | wc -l                          # standalone
1561
$ cargo mutants -p blastit --features python --list | wc -l        # the old `python, php` set
1561
$ scripts/mutants-run.sh -p blastit --features python --list | head -2
mutants: command=cargo mutants -p blastit --features python --list -j 2 --timeout 90
$ cargo mutants -p blastit --file crates/blastit/src/immortal.rs --regex 'replace \|\| with && in marker'
… ERROR cargo test failed in an unmutated tree, so no mutants were tested      # standalone
$ cargo mutants -p blastit --features python --file crates/blastit/src/immortal.rs --regex '…'
ok       Unmutated baseline in 41s build + 1s test
1 mutant tested in 46s: 1 caught
```

Why `--features python` and not `--all-features`: the latter also enables
`blastit-py/extension-module`, whose test binaries cannot link. Not `blastit/python` either — cargo
refuses a package-qualified feature for a graph that does not contain the package, so the flag
follows `PKG`, and a campaign with no `PKG` runs the other thirteen members and then
`-p blastit --features python` (computed from `cargo metadata`, no hardcoded list).

### `blastit --port 4242` names where the flag lives

```console
$ ./target/debug/blastit --port 4242
blastit: `--port` is a `blastit dev` flag; production takes `PORT=<n>` or `--preset`
usage: …
```

`--port`, `--port=N`, `-p`, `-p=N` all answer with it; a genuine key/value mistake keeps
`` `--stage` is not `--key=value` ``. Both are asserted in
`cli::tests::the_dev_only_port_flag_is_answered_with_the_two_places_it_belongs`.

### Found while proving it (needs a protected-test paths grant)

`cargo test -p blastit` (the new default, standalone) is red in three suites, all for assertions the
capability gate legitimately preempts: `critical_config_and_presets` (1),
`critical_dev_and_fail_closed` (16), `critical_static_cli` (1). They are WSGI-bound and need
`#![cfg(feature = "python")]` (the mirror of `no_python_build.rs`'s `#![cfg(not(feature = "python"))]`),
which `crates/blastit/tests/**` does not authorize here. `make check` is green because
`rs-no-python` runs the interpreter-free subset by design; this is the same class as the mutation
gap: a python-only assertion with no cfg gate.

## Follow-ups

1. **Needs a protected-test paths grant:** `crates/blastit/tests/critical_config_and_presets.rs`,
   `critical_dev_and_fail_closed.rs` and `critical_static_cli.rs` need `#![cfg(feature = "python")]`
   for their WSGI-bound tests (3 binaries, 18 failures under the new default `cargo test -p blastit`).
2. `.github/actions/build-wheel/action.yml` now passes `--features blastit-py/extension-module,blastit/python`
   explicitly (it previously worked only because `blastit-py`'s dependency carries `python`), so a
   feature reshuffle cannot produce a wheel whose extension loads no interpreter. [fixed in this pass]
2. The `critical_dev_and_fail_closed` 200/413 read looks like response/connection reuse in the test
   harness (`Dev::request`) rather than the server; worth a dedicated flake task.
3. `crates/blastit-view` still defaults to `python`; harmless to the binary (`blastit` depends on it
   with `default-features = false`) but it is why the workspace graph has pyo3 at all.
