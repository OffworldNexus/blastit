# MB.42.2.2.2 — `--internal-json` (12-row contract) + pure-stdlib `_blastit.py`; `crates/blastit-py` retired

## What landed

1. **`crates/blastit/src/cli/internal_json.rs`** (new, **199** comment-free prod LOC): the
   hidden `blastit --internal-json` NDJSON loop. Design §3's twelve ops with the exact `args`
   keys and result types (`normalize`/`redact_literals`/`blastfile_apply_edits` → `str`,
   `has_quoted_literal` → `bool`, the other eight → JSON text). `probe_ingress` takes
   `prefix_b64` and decodes the standard alphabet in place (no new dependency). One
   `{"ok":true,"result":…}` / `{"ok":false,"error":…}` line per request; a frame past 8 MiB is
   drained and refused once, and the session continues.
2. **`crates/blastit/src/cli.rs` + `cli/parse.rs`**: `Command::InternalJson`, absent from
   `--help`, `parse_internal_json` refuses any second word, and `parse` refuses
   `--internal-json` on a `--stage=1`/`--role=*` line (either order is `exit 2`).
3. **`crates/blastit/tests/critical_internal_json.rs`** (new critical suite, 4 rows): all twelve
   ops over real pipes with their own result type; an unknown op, a wrong argument type, a
   missing argument, a URL-safe base64 alphabet, a non-JSON frame and a 9 MiB frame are refused
   while the session survives; two refusals carry a planted secret and neither repeats it;
   `--help` does not name the flag and it never shares a command line with `--stage=1`/`--role=*`.
4. **`python/blastit/_blastit.py`** (new, 100% stdlib, `>=3.12`, no compiled import): lazy
   `blastit --internal-json` child owned by `(owner_pid == os.getpid(), threading.Lock)`,
   unbuffered pipes (`bufsize=0`, so `close()` never flushes the parent's in-flight request),
   `os.register_at_fork(after_in_child=…)` + `atexit`, `bin_path()` =
   `<pkg>/_bin/blastit` → `<pkg>/../../lib/blastit` → `BLASTIT_BIN` → `PATH`, the twelve typed
   functions, `{"ok":false}` → `ValueError(msg)`, and `main(argv, executable, dbproxy="")` as an
   `execvpe` wrapper.
5. **`python/blastit/__main__.py`**: `BLASTIT_WHEEL_PYTHON` (`setdefault`) +
   `BLASTIT_HANDOFF_DONE=1`, then `_blastit.main(...)`; `main_lsp()` execs `lsp_bin_path()`.
6. **`python/blastit/_check_pytest.py`**: `_probe` is `_probe_stdlib`, unconditionally (the
   `from blastit import _blastit` ingress path is gone).
7. **`Makefile` `test-critical`** stages `target/debug/blastit` at
   `$(CRITICAL_STAGE)/blastit/_bin/blastit` plus the `blastit-dbproxy -> blastit` symlink, and
   `install:` stages `_bin/{blastit,blastit-dbproxy}` symlinks instead of the extension.
8. **`crates/blastit-py` deleted** (`Host::wheel`, `Install::Wheel` and its unit tests removed;
   `Install::Binary` became `host::FILE_NOUN`), and **`check.rs`** now reads
   `BLASTIT_WHEEL_PYTHON` (design §2's Dev/Check order) and resolves `bundled_python` from
   `<exe_dir>/../python` **or** `<exe_dir>/../../..` (the wheel/critical-stage layout).

## Item 7 — declined by the lead (re-exports kept)

Retiring `pub use blastit_audit as audit_v1;` and `pub use blastit_audit::infer_routes;` would
force edits to two **protected** suites:

- `crates/blastit/tests/critical_static_cli.rs:33` — `use blastit::audit_v1::{self, CheckObsV1, HttpObsLine};`
- `crates/blastit/tests/critical_check_engine.rs:16,19` — `use blastit::audit_v1::{…}` and
  `use blastit::infer_routes::infer;`

Both re-exports are therefore **kept** (lead decision, recorded below); nothing in item 7 was
changed, and `cargo test -p blastit` stays green.

## Evidence (tails)

- `cargo test -p blastit --locked` → every suite `ok` (incl. 285 lib tests, 4 `critical_internal_json`,
  no failures).
- `cargo test -p blastit --locked --test critical_internal_json` → `test result: ok. 4 passed`.
- `make critical-skip-check` → `protected critical tests hold no skip construct` (green).
- Protected Python tier, **unedited**, through the real staging (not hand-copied): `make test-critical`
  → `critical-skip-check` green, `cargo build --locked -p blastit`, then
  `Ran 53 tests in 11.887s` / `OK` (`test_check_contract.py` and `test_lsp_stdio.py` included);
  `make test-critical exit=0`.
- `python/tests/test_edits_binding.py` (pure-Python module, stale `python/blastit/_blastit*.so` removed)
  → `4 passed in 0.02s`.
- CPython **3.12.16** venv (`uv venv --python 3.12 /tmp/opencode/py312`, no per-minor artifact;
  `python/blastit` has no `.so`):
  - `_blastit.normalize('SELECT  1 ;')` → `'SELECT 1'`, `has_quoted_literal("SELECT 'x'")` → `True`
  - `python -m blastit check --json` → exit `0`, `{"schema_version":1,"ok":true,"requests":1,"findings":[]}`
  - `python -m blastit.fold --manifest Blastfile /dev/null` → `Blastfile: nothing to add`, exit `0`
- `cargo build --workspace --locked` → exit `0`; `scripts/check-portable-elf.sh target/debug/blastit`
  → `NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH`.
- `cargo tree -p blastit --locked -e normal | grep -c pyo3` → `0`.
- `make py-lint` → `All checks passed!`; `cargo fmt --all -- --check` → clean; clippy `-D warnings` on
  `-p blastit --all-targets --features python` → clean.

## Follow-ups (out of this node's paths)

1. `MB.42.2.2.2` item 7 declined by lead: re-exports kept (alias, single owner; retiring would edit protected tests for no gain).
2. **Wheel packaging still points at the deleted crate** (`pyproject.toml` `[tool.maturin]
   manifest-path`, `blastit_build.py`, `scripts/build-wheel.sh`, `scripts/wheel-smoke.sh`,
   `scripts/check-portable-elf.sh`'s hint). MB.42.3's `repack_universal_wheel.py` + `maturin
   --bindings bin` rework owns that; until then `make docs-test`/`make e2e` cannot build a wheel.
3. **`crates/blastit-py/tests/critical_check_schema.rs` went with the crate** (item 6 deletes it).
   It was the only suite pinning `check_audit`'s on-disk schema through the Python surface; the same
   assertions live in `blastit-audit`'s unit tests, but a `test-curator` may want a `critical_*`
   twin in `crates/blastit-audit/tests/`.
4. `crates/blastit/src/stage1/boot.rs:243` still carries a comment naming `crates/blastit-py`
   "until MB.42.2.2"; it is now stale prose only.
5. `make test-critical`'s `critical-skip-check` now passes: the two prose lines in
   `crates/blastit/tests/critical_python_shim.rs` that spelled the skip attribute literally were
   reworded ("no skip attribute", "never a skipped test"); the loud-skip behaviour is unchanged.
6. **Pre-existing, not this node:** `cargo clippy -p blastit --no-default-features --all-targets`
   fails (`Parts<'a>`'s lifetime and `parts.runtime` at `supervise.rs:643` are not `#[cfg]`-guarded
   for a build without `python`; `git status` shows `supervise.rs` untouched by this node). `make
   check`'s `rs-no-default` leg will hit it; it needs one `#[cfg(feature = "python")]` on the
   `hooks:` initialiser (and a `PhantomData` or a removed lifetime on `Parts`).

## Progress (run 2) — the consolidated review fixes

### MAJOR 1 — the wheel builds again

`pyproject.toml` now names the crate that exists and a binding that needs no interpreter:

```toml
[tool.maturin]
manifest-path = "crates/blastit/Cargo.toml"
bindings = "bin"
python-source = "python"
include = [ … dbproxy … ]
```

`module-name` is gone (extension-only), and **`[project.scripts]` is gone too**: maturin refuses
`bindings = "bin"` beside scripts (`Defining scripts and working with a binary doesn't mix well`),
so the interim wheel's `blastit` is the Rust binary itself, and `python -m blastit` /
`python -m blastit.fold` remain the Python entry points. `MB.42.3`'s `py3-none` repack relocates
the binary into `blastit/_bin/` and restores the three console scripts (design §6.1).
`_blastit.bin_path()` gained the interpreter's own `bin/` as a candidate, so an installed wheel's
`python -m blastit` finds the binary `pip` put there.

Also reworded/fixed: `scripts/build-wheel.sh` (header, `--features blastit/python` instead of the
deleted `blastit-py/extension-module`, a bin wheel now *notes* that it carries no
`blastit/_blastit*.so` instead of failing), `scripts/check-portable-elf.sh:84`'s hint, and
`pyproject.toml`'s header.

```
$ uv build --wheel --out-dir /tmp/opencode/wheel
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel …/blastit-0.2.0-py3-none-linux_x86_64.whl
$ unzip -l … | grep data/scripts
17726152 … blastit-0.2.0.data/scripts/blastit
$ uv pip install --python /tmp/opencode/wheel-venv/bin/python --no-deps <wheel>
$ /tmp/opencode/wheel-venv/bin/blastit --version      -> blastit 0.2.0   (exit 0)
$ /tmp/opencode/wheel-venv/bin/python -m blastit --version -> blastit 0.2.0 (exit 0)
```

### MAJOR 2 — the `Hosted` seam is gone

Deleted: `cli::main`'s `LiveInterpreter` generic (`cli::main(host, argv)`), `py::runtime_hooks`
(both `cfg` halves), `py::LiveInterpreter` + its `Option<T>` impl, `pub struct NoInterpreter` and
the `pub use crate::py::{LiveInterpreter, NoInterpreter}` re-export, `role_is_allowed` (always
`Ok`) and its two unit tests, `blastit_py_sapi::hosted_hooks`, `PyForkHooks`'s "already live"
constructor doc, `LoadedPy::load_shim_only`, `load::hosted`, the `shim_only` branch in
`stage1/boot.rs`, `Serving.live`, `stage1/serve.rs`'s `(live, shim)` match, and the `interpreter`
flag threaded through `stage0::run`/`run_dev`/`emit_runtime`/`runtime_shape`/
`static_root_permission`/`fork_dbproxy{,_dev}`/`fork_ok`/`check::run` — every stage-0 fork is now
a plain `fork(None)` and stage 1's per-view `Forker` still takes the loaded runtime's hooks.
`make test-critical` (53 tests, real `dev` boots) and the whole workspace suite stay green.

### BLOCKERs 3–5, 7–8 — the bridge's failure modes

```
A  no binary:            ValueError: this blastit package is incomplete: it ships no `blastit`
                         (looked in …/blastit/_bin/blastit, …/lib/blastit, BLASTIT_BIN, PATH);
                         reinstall it with `pip install --force-reinstall blastit`
A2 python -m blastit:    blastit: <the same sentence>            (SystemExit, no traceback)
B  child exits 3 first:  ValueError: `blastit --internal-json` exited before answering `normalize`
                         (status 3): boom: the shim is gone; reinstall this package with
                         `pip install --force-reinstall blastit`
C  broken pipe:          ValueError: normalize: the blastit bridge closed its input
```

`_spawn` keeps the child's stderr in a `tempfile.TemporaryFile` and `_stderr_tail()` quotes its
last line; an empty reply is the "exited before answering `<op>` (status N)" sentence, only a
*non-empty* reply without a newline is the size sentence; `BLASTIT_BIN` is validated
(`… is not an executable file; unset BLASTIT_BIN, or point it at the blastit binary`);
`Popen`'s `OSError` is a `ValueError`; invalid JSON names the resolved program
(`the blastit at <program> answered `<op>` with invalid JSON (…); this package needs the blastit
it ships — run `pip install --force-reinstall blastit``); and a refused op reads
`<op>: <message>`.

### MAJOR 6–7, MINOR 9–12

* `--internal-json` refuses a terminal: `blastit --internal-json` under `script(1)` prints one
  sentence and exits `2`.
* The frame reader checks `line.len() + index > MAX_FRAME` *before* extending, so a frame can no
  longer overshoot by one `BufRead` chunk.
* The five decoders moved to `crates/blastit/src/cli/internal_json/args.rs` (60 prod LOC);
  `internal_json.rs` is now **152** comment-free prod LOC (was 199) — `run`/`call` plus framing.
* Stale "pyo3 extension" prose reworded in `lib.rs`, `cli.rs`, `main.rs`, `stage1/boot.rs`,
  `pyproject.toml` and `pytest_plugin.py` (`in the binary the package ships`).
* `CRITICAL_SKIP_PATTERN` no longer sees comments: `critical-skip-check` strips `//` lines from
  the Rust files and `#` lines from the Python ones before matching, so the honest prose is back
  in `critical_python_shim.rs` ("no `#[ignore]`", "never an `#[ignore]`"). Proved both ways
  with a temporary `crates/blastit/tests/critical_zz_scratch.rs` (deleted afterwards):
  real `#[ignore]` → `critical_zz_scratch.rs:2:#[ignore]` + exit 1; prose mention only → green.

### Run-2 tails

```
$ make critical-skip-check      -> protected critical tests hold no skip construct
$ make test-critical            -> exit 0; Ran 53 tests in 10.115s / OK
$ cargo test -p blastit --locked --test critical_internal_json
                                -> test result: ok. 4 passed
$ (CPython 3.12.16, python/blastit has no .so)
  _blastit.normalize('SELECT  1 ;')                    -> 'SELECT 1'
  python -m blastit check --json                       -> exit 0, ok:true
  python -m blastit.fold --manifest Blastfile /dev/null -> "Blastfile: nothing to add", exit 0
$ uv build --wheel              -> blastit-0.2.0-py3-none-linux_x86_64.whl; install + --version OK
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings -> clean
$ make fast-check               -> exit 0; guard OK; critical-guard report-only; ruff clean;
                                  68 files already formatted; plan OK
```

## Progress (run 3) — the `rs-no-default` gate

`make check`'s `rs-no-default` leg was red on `HEAD`, and the cause was structural: in a build
without `python`, `Parts<'a>`'s lifetime had no user (`runtime: Option<PyRuntime<'a>>` is
`#[cfg(feature = "python")]`) and `Supervisor::new` still read `parts.runtime` unguarded.

Fixed in `crates/blastit/src/supervise.rs`, without changing the `python` build's shape:

* `Parts<'a>` keeps `#[cfg(feature = "python")] pub runtime: Option<PyRuntime<'a>>` and gains
  `#[cfg(not(feature = "python"))] pub _runtime: std::marker::PhantomData<&'a ()>` — the one
  lifetime stays legal in both shapes (no `#[allow(dead_code)]` anywhere: the marker is `pub`, and
  no warning is silenced).
* `Supervisor::new` initialises `hooks` per shape: `parts.runtime.map(|runtime| runtime.hooks)`
  under `python`, and a plain `None` without it (a build that loaded no interpreter has no
  hooks). Every other `runtime`/`hooks`/`'a` use in the file was already behind a `cfg`; the
  existing `#[cfg_attr(not(feature = "python"), allow(dead_code))]`s are untouched.
* The five `Parts { … }` literals were updated where the no-python shape compiles them
  (`serve_without_interpreter` and the three supervisor test fixtures) with the same
  `#[cfg(not(feature = "python"))] _runtime: std::marker::PhantomData` line.
* With the library legal, the leg then hit a second, unrelated blocker: the protected
  `crates/blastit/tests/critical_python_shim.rs` target compiles its helpers unused without
  `python` (`unused import` / 7 `never used` under `-D warnings`). The file is protected and
  unchanged; `crates/blastit/Cargo.toml` now declares
  `[[test]] name = "critical_python_shim"` with `required-features = ["python"]`, so the target is
  built exactly where it can observe a shim. Nothing is hidden in the `python` build, where the
  suite still runs in full.

### Run-3 tails (all six shapes)

```
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
                                                            -> clean
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
                                                            -> clean
$ cargo clippy -p blastit --no-default-features --all-targets --locked -- -D warnings
                                                            -> clean (was exit 101)
$ cargo test -p blastit --no-default-features --locked --lib
                                                            -> test result: ok. 234 passed
$ cargo build -p blastit --no-default-features --locked && scripts/check-portable-elf.sh …
   ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2;
        no RPATH/RUNPATH
$ cargo build --workspace --locked && scripts/check-portable-elf.sh target/debug/blastit
   ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2;
        no RPATH/RUNPATH          (readelf: 0 NEEDED libpython/libphp, 0 RPATH/RUNPATH)
$ make fast-check                                            -> exit 0; guard OK;
        All checks passed!; 68 files already formatted; plan OK
```

Follow-up #6 above is therefore closed; `crates/blastit/tests/critical_python_shim.rs` is still
byte-identical to `HEAD`.
