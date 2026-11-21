# MB.3.11 — RESULT

**Status:** done (attempt 1)

Turned `python` into a cargo feature (default on) on `blastit` and `blastit-view`, added an
(empty) `php` feature, and gave the interpreter-free build a real, tested shape: it compiles,
boots, serves `[route.static]`, refuses a WSGI manifest in **stage 0** with the sentence
ADR-0009 §5 asks for, and links **no `libpython`**. The `PyOS_*Fork` hooks are now behind one
`cfg` at the crate's only `fork` call site, and the `view.chan` framing (`wire`/`flow`) plus
the supervisor loop compile and are unit-tested with no interpreter.

## What changed (10 files + 1 new, 419 insertions / 14 deletions)

| File | Change |
|---|---|
| `crates/blastit/Cargo.toml` | `[features] default = ["python", "php"]`, `python = ["dep:pyo3", "blastit-view/python"]`, `php = []`; `pyo3` optional; `blastit-view` with `default-features = false`. No `libphp` linking. |
| `crates/blastit-view/Cargo.toml` | `default = ["python"]`, `python = ["dep:pyo3"]`, `pyo3` optional. |
| `crates/blastit-view/src/lib.rs` | `environ`/`input`/`request`/`response` and the whole `serve`/pool/worker machinery behind `python`; `config`/`error`/`flow`/`sync`/`wire` stay in both builds (the transport), each with its unit tests. |
| `crates/blastit/src/py.rs` (new, 35 ln) | One interpreter token for both builds: `pyo3::Python` with the feature, a `PhantomData` token without, so `run(py)`/`spawn_now(py, …)` keep one shape. |
| `crates/blastit/src/lib.rs` | `immortal` and `zygote` behind `python`; `py` module; the feature documented in the crate docs; `allow(dead_code, unused_imports)` **only** for the interpreter-free build (interpreter-only helpers are genuinely unreachable there). |
| `crates/blastit/src/forking.rs` | The `PyOS_BeforeFork` / `PyOS_AfterFork_{Child,Parent}` trio behind `#[cfg(feature = "python")]` at the single call site, with the comment proving the pair still matches; the no-interpreter build is plain `fork(2)` and asserts `interpreter` is unused. |
| `crates/blastit/src/cli.rs` | `use crate::py::Python` (same signature; `None` in the standalone binary). |
| `crates/blastit/src/stage0.rs` | `no_python_support(config, manifest, learn)` — the stage-0 capability gate (§9.4), called before `app_rule` in `run` and `run_dev`; 5 unit tests (allowed static boot, app, view route, `--learn`, and the all-static + `BLASTIT_APP` case that stays `static_app_refusal`'s). |
| `crates/blastit/src/stage1.rs` | Python-only consts/fns gated (`serve`, `load`, `check_view_paths`, `put_cwd_first`, `restore_sys_path`, `immortalize`, `emit_preload`, `preload_lifecycle`, `import_callable`, `python_fail`, the C snippets); `Serving.hosted` gated; `serve_without_interpreter` supervises the forked roles and repeats the stage-0 gate (`--config-fd` is not trusted). |
| `crates/blastit/src/supervise.rs` | `Parts.app`/`Supervisor.app` and `zygote` gated; the no-interpreter `spawn_now` **fails the boot** on a `ctl.chan` SPAWN instead of dropping it (fail-closed). |
| `Makefile` | New `rs-no-python` target (check ×2, unit tests ×2, clippy ×2, `cargo tree` pyo3 count), added to `make check`. |

## Acceptance

```console
$ cargo check -p blastit --no-default-features --features php --locked
    Checking blastit v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.09s

$ cargo check -p blastit-view --no-default-features --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s

$ cargo tree -p blastit --no-default-features --features php -e normal | grep -c pyo3
0

$ cargo tree -p blastit -e normal | grep -c pyo3          # python build, for contrast
5

$ cargo test -p blastit --locked
test result: ok. 183 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out   # lib
test result: ok. 4 / 19 / 12 / 19 / 12 / 1 / 31 passed ... (integration + doc suites)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out     # doc
   # the 1 ignored is `immortal::tests::...` (re-runs the binary with a child env), unchanged

$ cargo test -p blastit-view --locked
test result: ok. 4 / 12 / 7 / 1 / 30 passed ... ; Doc-tests blastit_view: ok

$ make check
…
clippy      -p blastit{,-view} --no-default-features
cargo clippy -p blastit --no-default-features --features php --all-targets --locked -- -D warnings
    Finished `dev` profile … in 0.19s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile … in 0.07s
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
# exit 0 (/tmp/opencode/make-check.log)
```

`uv run --no-project tools/critical_guard.py --base 4cd727b --node MB.3.11`:

```console
critical-guard: 4cd727b..working tree, 72 changed file(s)
node MB.3.11: paths ['Cargo.toml', 'Cargo.lock', 'crates/blastit/Cargo.toml', 'crates/blastit/src/**',
  'crates/blastit-view/Cargo.toml', 'crates/blastit-view/src/**', 'Makefile']
  no change to a protected critical test
```

No `critical_*.rs` was touched, no protected signature changed (the `cli::main` and
`stage1::run` signatures are identical in the python build), and no plan status was edited.

## Extra evidence

**No `libpython` in the produced binary** (the node's `ldd` smoke step):

```console
$ cargo build -p blastit --no-default-features --features php --locked   # Finished … 2.95s
$ ldd target/debug/blastit | grep libpython ; echo "exit=$?"
exit=1          # no match: the python-less binary links no libpython
```

**The interpreter-free build boots and serves** the real five-role tree (unprivileged
`blastit dev`, static-only manifest, port 18099):

```console
$ printf 'GET / HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' | nc 127.0.0.1 18099
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 13
ETag: "d-18e7ed6123324ec5"
Last-Modified: Sat, 14 Nov 2026 13:46:17 GMT
$ # SIGTERM to the same pid
INFO  process.exited -  pid=2369684 uid=0  signal=15
INFO  shutdown    stopped
```

**The refusals, from the same binary** (exit 1, nothing on stdout for `dev`):

```console
$ blastit dev --port 18099                     # route with no [route.static]
blastit dev: route "web": this build has no Python support (built without feature "python"), and
the route declares no [route.static] block, so it needs a runtime this build does not carry;
install the Python package: uv add blastit, or add a [route.static] block

$ BLASTIT_APP=web.wsgi:app blastit dev --port 18099    # static tree + app
blastit dev: BLASTIT_APP: every route is static, so no route can reach an app: unset
BLASTIT_APP, or declare a route without [route.static]     # unchanged pre-existing rule

$ blastit dev --learn --port 18099
blastit dev: dev --learn: this build has no Python support (built without feature "python")
and --learn needs the interpreter for its gate; install the Python package: uv add blastit, or
run a build with --features python
```

## Security invariants (no wall behind the gate)

- **Stage-0 env strip** (`blastit_env::build_from_process`), `seccomp` roles and the view-uid
  layout were not gated: `git diff -U0 | grep '^\+.*#\[cfg'` lists only
  `#[cfg(feature = "python")]` on interpreter items, `#[cfg(not(feature = "python"))]` on the
  new gate/fail-closed paths, and `#[cfg(all(test, feature = "python"))]` on the WSGI tests.
  The static 200 above is the whole tree — init → dbproxy → http → scheduler — forked with
  `seccomp` on (`boot` event: `"seccomp":true`) in the interpreter-free build.
- **Fail closed where a worker runtime is missing:** a `ctl.chan` `SPAWN` in the python-less
  build fails the boot (`route "…" (slot N): ctl.chan asked for a view and this blastit build
  has no Python worker runtime`) instead of being ignored.
- **No behaviour change on the `python` path:** every `#[cfg(feature = "python")]` line gates
  code that was already there; `cli::main`, `stage1::run` and `Supervisor::run` keep their
  signatures, and the full default-feature suite (183 + 98 tests) passes unmodified.

## Follow-ups

1. **MB.3.13** must make `blastit-view`'s framing a public, python-free surface (`wire::Writer`,
   `flow::Flow`, `ViewConfig`) before `PhpEmbedRuntime` consumes it — today they are
   `pub(crate)`, reachable only by the crate's own unit tests in the python-less build, and
   `blastit` keeps `use blastit_view as _;` so the dependency does not read as forgotten.
2. **`critical_backend_capability.rs`** (design `backend-runtimes.md` R12) does not exist yet:
   this node proves the refusals with unit tests (`stage0::no_python_capability`) and a manual
   boot. Whoever owns that protected suite should assert the two sentences above verbatim
   against the `--no-default-features --features php` binary.
3. **`blastit check` in a python-less build** is not refused: an all-static manifest reaches
   the pytest gate. It needs the ADR-0009 "external suite" path (not this node's scope).
4. `make docs-unit`/`docs-test` failed once on a `dist/*.whl` being written concurrently (not a
   zip) and once on the known uv wheel cache; a clean `uv cache clean blastit --force` +
   re-run is green. Nothing to fix in code.

## Review round 1 — two MINORs cleared

**(a) `[route.fastcgi]` advice.** `no_python_support` now branches on
`route.fastcgi()` **before** the "no executor" advice: an external FastCGI route is refused
on its own terms — the external fallback is `MB.3.13`'s wiring, `BLASTIT_ALLOW_EXTERNAL_FASTCGI`
is named as the gate it will arrive behind (not as a cure), and the sentence deliberately does
**not** offer `uv add blastit`, which cannot reach an operator-managed pool. `uv add blastit`
is kept for a manifest whose routes have no executor at all. Asserted exactly, twice:

- unit: `stage0::no_python_capability::an_external_fastcgi_route_is_refused_on_its_own_terms`
  (`assert_eq!` on the whole sentence + `!contains("uv add blastit")`);
- process: `crates/blastit/tests/no_python_build.rs::an_external_fastcgi_manifest_is_refused_on_its_own_terms`.

**(b) The wiring is now proved, and orphans warn again.**

- The crate-wide `#![cfg_attr(not(feature = "python"), allow(dead_code, unused_imports))]` is
  gone. The 12 items that only an interpreter reaches carry
  `#[cfg_attr(not(feature = "python"), allow(dead_code))]` where they live
  (`Supervisor::egress`/`mask`/`send_spawned`, `Serving::egress`/`memory`, `Dev::view_dir`,
  `visibility::{Writable, Writable::message, Probe::Write, writable, writable_by_mode,
  targets_above, sticky_protects_for_all}`), and the two interpreter-only imports
  (`OsStringExt`, `derived_view_cap`) are `#[cfg(feature = "python")]`. Every one of these
  stays **compiled** in the gated build — the walls do not disappear — and
  `cargo check -p blastit --no-default-features --features php` is warning-free.
  Removing all three gate call sites now reports
  `warning: function no_python_support is never used` (verified, then restored).
- `crates/blastit/tests/no_python_build.rs` (new, `#![cfg(not(feature = "python"))]`, so it is
  an empty test binary in the wheel build) runs the **artifact**: `CARGO_BIN_EXE_blastit` is
  the python-less binary in that configuration, started as `blastit dev` in a scratch dir with
  a port the test holds open (so a boot that got past the gate cannot serve, and a bounded
  15 s wait means the suite never hangs). Each case asserts exit `1`, empty stdout, the gate's
  own sentence, **and the absence of `app_rule`'s** `BLASTIT_APP: the WSGI callable …`
  sentence — the exact message the boot falls back to without the gate.
- `make rs-no-python` runs it, and it fails when the wiring is removed (verified by commenting
  the stage-0 dev call out: `test result: FAILED. 0 passed; 2 failed`, with
  `blastit dev: BLASTIT_APP: the WSGI callable (`module:attribute`) is required` in the diff).

```console
$ make rs-no-python
cargo test -p blastit --no-default-features --features php --locked --test no_python_build
     Running tests/no_python_build.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
cargo clippy -p blastit --no-default-features --features php --all-targets --locked -- -D warnings
    Finished `dev` profile … in 0.12s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile … in 0.04s
interpreter-free build: 0 pyo3 dependencies (no libpython linked)

$ make check
… rs-test … test-critical … docs-unit … docs-check … docs-test … rs-no-python …
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
# exit 0

$ cargo test -p blastit --locked
test result: ok. 184 passed; 0 failed; 1 ignored   # lib (+ the new fastcgi unit test)
tests/no_python_build.rs: test result: ok. 0 passed   # empty in the wheel build, on purpose
… every integration suite ok …

$ uv run --no-project tools/critical_guard.py --base 4cd727b --node MB.3.11
critical-guard: 4cd727b..working tree, 74 changed file(s)
  no change to a protected critical test
```

Follow-up 2 above is now satisfied for the two sentences this node owns; the `critical_*`
suite from design R12 (`critical_backend_capability.rs`) is still the PHP/provider node's to
add — `tests/no_python_build.rs` is a standard test, not a protected one.
