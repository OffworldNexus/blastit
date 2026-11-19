# RESULT — MB.42.2.1.4 (in progress)

## Progress (run 1)

The cut-over's wiring is in and the tree is green for the five commands this run
owned. `crates/blastit` now links **no** `pyo3` and **no** `blastit-view-python`:
stage 1 `dlopen`s the pinned `libpython` + per-minor shim (or, for a wheel, the
shim alone), every call is a `blastit_py_abi_v1` entry point behind
`blastit_py_sapi::PyRuntime<'a>`, and the supervisor holds that opaque handle
instead of a `Py<PyAny>`.

### Build

```
$ cargo build -p blastit --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s

$ cargo build -p blastit --no-default-features --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.15s

$ cargo build --workspace --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.01s
```

### Portability (deliverable 7) — both graphs

```
$ scripts/check-portable-elf.sh target/debug/blastit      # after `-p blastit`
ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH

$ scripts/check-portable-elf.sh target/debug/blastit      # after `--workspace`
ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH

$ cargo tree -p blastit --locked -e normal | grep -c pyo3
0

$ cargo tree -p blastit --no-default-features --locked -e normal \
    | grep -cE 'blastit-py-sapi|blastit-view-php|pyo3'
0
```

Zero `libpython`, zero `libphp`, zero `RUNPATH` on **both** the `-p blastit` and
the `--workspace` artifact; the default graph pulls `blastit-py-sapi` and no
`pyo3`.

### Lint & tests

```
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.39s

$ cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.88s

$ cargo test -p blastit --lib --locked
test result: ok. 303 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit --lib --locked --no-default-features
test result: ok. 255 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit --locked --test stage1_install_sites
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### What changed (scope of this run)

* `crates/blastit-py-sapi`: `PyZygote` + `PyRuntime<'a>` (`abi.rs`), `PinnedRef`/
  `gate_pinned`/`load_pinned`/`load_shim_only`/`runtime()` + fallible
  `hosted_hooks()` (`loader.rs`), and `boundary.rs` (byte-level vtable calls,
  `free_buf` ownership, refusal bytes).
* `crates/blastit-py-shim`: `init_paths` split into `initialize` + reusable
  `apply`; `preload` accepts the `Hosted` path half and answers `view_paths`/
  `sys_path`; `Serve` carries `max_frame`; `blastit-frame` added (Cargo.lock
  regenerated once).
* `crates/blastit`: **no `pyo3`**, **no `blastit-view-python`**
  (`default = ["python","php","static"]`, `python = ["dep:blastit-py-sapi"]`);
  `build.rs` keeps only the `coverage` gate; `py.rs` is the pyo3-free handle +
  `LiveInterpreter`; new `py_view.rs` (the pinned child sequence ending in the
  vtable's `serve_view`) and `stage1/load.rs` (the pinned `Embed`/`Hosted`
  orders, §7.2 gates over the shim's reported paths, preload + `runtime`/`boot`
  events); `supervise.rs`/`stage1/{boot,serve,spawn,adopt}.rs` carry
  `Option<PyRuntime<'a>>`/`hooks` and never ensure the GIL; reload preflight goes
  through `runtime.check_syntax`.
* Deliverable 8: stage 0 no longer exports `PYTHONHOME`
  (`stage0/fork_exec.rs`, `stage0/project_python.rs` — the transitional
  `// transitional (MB.42.2.1.4)` path and `dev_linked_python`/`python_home` are
  deleted); `config.python.python_home` now reaches CPython only through the
  shim's `init_paths`. `agree_minor` stays and refuses a project venv outside
  `SUPPORTED_MINORS` naming both supported versions and one command. The
  stage-0 `runtime` line that executed Python (`gates.rs::python_runtime`) is
  gone; stage 1 emits it from the shim's `init_paths` answer.
* `Makefile`: `rs-no-python` now proves the default graph pulls `blastit-py-sapi`
  and neither `pyo3` nor `blastit-view-python` (plus the portable-ELF check);
  `rs-no-default` forbids `blastit-py-sapi` too.
* `crates/blastit/tests/stage1_install_sites.rs` (non-critical) rewritten for
  the new owner: zero install sites in `crates/blastit`, exactly one in the
  shim's `preload`, before the first import.

### Still to do (next run — deliberately not started)

* `crates/blastit/tests/critical_python_shim.rs` (both minors over a real
  socket, PHP-only/static-only without `libpython`, real `(st_dev, st_ino)`
  swap refusal, loud skip without `make py-sapi`).
* `make fast-check`, `make py-sapi` + `--test critical_python_shim`, the
  protected `critical_dev_and_fail_closed`/`critical_php_embed` runs, and
  `critical_guard --base HEAD --node MB.42.2.1.4`.
* Full `RESULT.md` acceptance evidence.

### Notes for the lead

* `docs/design/universal-distribution.md` §2 still says stage 0 sets
  `PYTHONHOME=<python_home>` in `stage1_env`. That sentence needs a one-line
  amendment (stage 1's shim `init_paths` is the only path now) — not edited here,
  per the node's rule.
* Follow-up: design §1.2.1's `Hosted` bullet says the shim's hooks wrap the
  **stage-0** dbproxy/visibility forks, while §7 `R3` forbids stage 0 from
  `dlopen`ing. This run resolves the contradiction with
  `blastit_py_sapi::hosted_hooks()`, which `dlsym`s the three `PyOS_*Fork`
  entries from the already-live interpreter (`RTLD_DEFAULT`) and **fails closed**
  when they are absent. Worth a `R3`/§1.2.1 clarification.
* Follow-up: `agree_minor` is now a supported-minor gate (the build-time linked
  minor no longer exists); the shim-vs-discovery both-versions refusal is
  `LoadError::MinorMismatch` at load time.

## Progress (run 2)

### `make py-sapi` — both minors

```
py-sapi: cpython 3.14 -> target/py-sapi/libblastit_py314-x86_64-gnu.so (.venv/bin/python)
ok   .../libblastit_py314-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   .../libblastit_py314-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 314
py-sapi: cpython 3.13 -> target/py-sapi/libblastit_py313-x86_64-gnu.so
      (/home/remy/.local/share/uv/python/cpython-3.13-linux-x86_64-gnu/bin/python3.13)
ok   .../libblastit_py313-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   .../libblastit_py313-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 313
py-sapi: 2 shim(s) for x86_64-gnu in .../target/py-sapi; skipped: none
```

### `cargo test -p blastit --locked --test critical_python_shim`

```
running 4 tests
php: 200 via .../target/php-sapi/libblastit_php84-x86_64-gnu.so without opening /tmp/.../unreadable
static: 200 without opening /tmp/.../unreadable
swap: refused (descriptor 3 names /tmp/.../replacement, recorded inode ...)
CPython 3.13: 200 "served by cpython 3.13" via .../cpython-3.13.../bin/python3.13
  (shim .../target/py-sapi/libblastit_py313-x86_64-gnu.so)
CPython 3.14: 200 "served by cpython 3.14" via .venv/bin/python
  (shim .../target/py-sapi/libblastit_py314-x86_64-gnu.so)

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.52s
```

What each leg proves:

* **one binary, both minors.** The same `target/debug/blastit`, two fixtures whose
  `.venv` is 3.13 and 3.14; each leg answers `200` over a real loopback socket with
  the app's body naming `sys.version_info[:2]`, and the printed shim path shows the
  per-minor artifact that served it.
* **PHP-only / static-only never open `libpython`.** The fixture's venv points at a
  prefix whose `libpython3.14.so.1.0` is mode `000`: the interpreter, venv and
  `BLASTIT_PY_SHIM_DIR` are all present, so an accidental open would be `EACCES`
  and fail the leg. Both boots answer `200` (PHP through the real
  `libblastit_php84` shim + `/usr/lib/libphp8.4.so`), and the captured stderr never
  contains `libpython`, `CPython` or `BLASTIT_PYTHON`.
* **the swap refusal.** A real inherited descriptor (`libc::open`, no `O_CLOEXEC`)
  has a second file `dup2`ed onto its number; the document is the real
  `Config::to_json` with the original `(st_dev, st_ino)`. Stage 1 exits non-zero
  with ``<path> was replaced after blastit inspected it (inode <old> is now <new>);
  restore the file and restart blastit`` and never names an internal stage.
* **loud skip.** A leg with no shim or no interpreter prints one line naming what is
  missing (`run \`make py-sapi\`` / `uv python install 3.x`); nothing is
  `#[ignore]`d and no leg passes silently.

### Protected suites (unedited)

```
$ PYTHONHOME=<cpython-3.14 prefix> cargo test -p blastit --locked \
    --test critical_dev_and_fail_closed --test critical_php_embed
test result: ok. 29 passed; 0 failed; ... (critical_php_embed)
test result: ok. 20 passed; 0 failed; ... (critical_dev_and_fail_closed)
```

`PYTHONHOME` is the environment `make rs-test` exports (`RUST_ENV`); the suites
`env_clear()` and copy it across, so the command carries the same one.

```
$ cargo test -p blastit-py-sapi -p blastit-sys -p blastit-py-shim
test result: ok. 174 passed (blastit-sys lib); ok. 5 / 6 / 4 (critical_child_sequence,
critical_embedded_seccomp, critical_fanotify); ok. 1 / 0 / 9 (blastit-py-sapi,
blastit-py-shim, ...)
```

### Two boot fixes the proof forced (both real, both fail-closed)

1. **`init_paths` now carries the prefix.** `Py_InitializeEx(0)` under `env -i`
   failed with `Could not find platform dependent libraries <exec_prefix>`: the
   retired `PYTHONHOME` had been the only way the prefix reached CPython. The shim
   now starts through `PyConfig_InitPythonConfig` + `Py_InitializeFromConfig` with
   `config.home` set from the request's `python_home`, so
   `config.python.python_home` reaches the interpreter **only** through the vtable
   (design §2.4), and `init_paths` refuses an undecodable prefix. `make py-sapi`'s
   ELF assertion now accepts `Py_InitializeEx` **or** `Py_InitializeFromConfig`.
2. **`serve` no longer demands an interpreter for a static-only/PHP-only boot.**
   `Config::python` is `None` there and `serve` loads nothing (`R13`), instead of
   refusing with "no pinned Python runtime". The §7.2 path gates also skip in
   `dev` (`R19`: every view is the caller), matching the old boot.

`crates/blastit-py-shim/Cargo.toml` gained `blastit-frame` (the `Serve` request's
`max_frame`); `Cargo.lock` was regenerated once and `--locked` kept after.
`crates/blastit/tests/stage1_install_sites.rs` (non-critical) now asserts the
tracker's new owner: zero install sites in `crates/blastit`, one in the shim's
`preload`, before the first import.

### Still recorded for the lead (unchanged from run 1)

* `docs/design/universal-distribution.md` §2 still says stage 0 sets
  `PYTHONHOME=<python_home>` in `stage1_env`; that sentence needs a one-line
  amendment (the shim's `init_paths` is the only path now). Not edited here.
* Follow-up: §1.2.1's `Hosted` bullet gives the shim's hooks to the **stage-0**
  dbproxy/visibility forks while §7 `R3` forbids stage 0 from `dlopen`ing. This
  slice resolves it with `blastit_py_sapi::hosted_hooks()`, which `dlsym`s the
  three `PyOS_*Fork` entries from the already-live interpreter (`RTLD_DEFAULT`) and
  **fails closed** when they are absent; worth a `R3`/§1.2.1 clarification.
* Follow-up: `agree_minor` is now a supported-minor gate (the build-time linked
  minor no longer exists); the shim-vs-discovery both-versions refusal is
  `LoadError::MinorMismatch` at load time.
* Follow-up: a `dev`/`check` boot now also probes `<PYTHONHOME>/bin/python3` as its
  last candidate — the prefix a developer or a CI harness activated, not a
  build-host path in the artifact. It is dev/check only; `Serve` never reads it.

## Blocker (run 2, step 5): `make fast-check` — one protected test's subject moved

`make fast-check` is **red at `rs-test`**, on one test only:

```
     Running tests/critical_loaded_files.rs (blastit-view-python)
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... FAILED
thread ... panicked at crates/blastit-view-python/tests/critical_loaded_files.rs:367:5:
assertion `left == right` failed: exactly one tracker install site, inside the dev-only gate
  left: 0
 right: 1
make: *** [Makefile:129: rs-test] Error 101
```

Everything before `rs-test` in `fast-check` is green (`guard: OK`, `critical-guard: ...
no change to a protected critical test`, `cargo fmt --all -- --check`, and the
workspace `clippy --all-targets --features python -- -D warnings`). `critical_guard`
with this node's base sha is clean (exit 0).

### Why it cannot be fixed inside this node

That test is a **source-text witness** of the old composition root: it
`include_str!`s `crates/blastit/src/stage1.rs` and asserts exactly one
`LoadedTracker::install` inside `if dev.is_some()`, before `import_callable(py,
spec)`. This node's design (§1.2.1 item 3) moves the install into the shim's
`vtable.preload`, and `crates/blastit` no longer depends on
`blastit-view-python` at all — so those tokens correctly do not exist in
`stage1.rs` any more. The file is a protected `critical_*.rs` and lives outside
this node's `paths`, so it was not touched.

The replacement witness already exists and passes: the (non-critical)
`crates/blastit/tests/stage1_install_sites.rs` now asserts **zero** install sites
in `crates/blastit` and **exactly one** in
`crates/blastit-py-shim/src/boot.rs`, inside `preload`, before the first import.

### The fix this needs (one small amendment, one owner)

Point that protected test's `include_str!` (and its four tokens) at
`crates/blastit-py-shim/src/boot.rs`, mirroring `stage1_install_sites.rs`. Its
`paths` do not include `crates/blastit-view-python/**`:
`MB.42.2.2` (status `ready`, `paths: [Cargo.toml, Cargo.lock, "crates/**",
"python/**", Makefile]`) can make it, or this node can be authorized to edit that
one file.

No other `make fast-check` target failed, and no other test in the workspace
failed: `rs-test` stopped at this first failure, so the remaining suites after it
(`rs-doc`, `py-lint`, `plan-validate`) did not run in this attempt.

## Progress (run 3) — the authorized witness retarget, and the gate is green

### The retarget (lead-authorized, `crates/blastit-view-python/tests/critical_loaded_files.rs`)

The protected witness still proves the **same invariant**; only its owner moved
(`ADR-0017` §1.2.1 item 3: the tracker is armed inside the shim's `preload`):

| assertion (unchanged meaning) | old token (stage 1) | new token (shim `boot.rs`) |
|---|---|---|
| one install site | `LoadedTracker::install` | `LoadedTracker::install` (count 1) |
| inside the dev-only gate | `let loaded = if dev.is_some() {`, no `else` in the gate | `let socket = (req.report_fd >= 0).then(|| {` + the let-else's `return Ok((None, None))`, both **before** the install |
| install is in the gate's own body | — | between `fn install_tracker` and `fn report_boot` |
| before the first import | `import_callable(py, spec)` | `install_tracker(py, req)` < `import_callable(py, spec)` |
| before the preload list | `for module in &config.preload` | `for module in &req.preload` |
| one harvest, after warm-up, before the collection | `loaded.harvest(py)`, `config.warmup.as_deref()`, `py.run(CLOSE_AND_COLLECT, None, None)` | `tracker.harvest(py)` (count 1, inside `report_boot`), `req.warmup.as_deref()`, `py.run(CLOSE_AND_COLLECT, None, None)`, ordered on the `report_boot(py, tracker.as_ref(), socket)` call |

No assertion was dropped, weakened, deleted or `#[ignore]`d; the doc comment now
names the new owner and the new gate explicitly (it remains a token-and-order
witness, not behavioural proof), and the file name in the panic message is the
shim's. The node's `paths` also gained the file (frontmatter comment removed, since
the guard's list parser stops at a comment — the rationale lives in `# Goal`).

### Tails

```
$ cargo test -p blastit-view-python --test critical_loaded_files
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit --locked --test critical_python_shim
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make fast-check
guard: OK (5 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 36 changed file(s)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --features python -- -D warnings
… 157 `test result: ok` suites, 0 failures …
uvx --offline ruff check python tools   → All checks passed!
uvx --offline ruff format --check python tools → 67 files already formatted
uv run --no-project tools/plan.py validate → plan OK
make fast-check  # exit 0

$ uv run --no-project tools/critical_guard.py \
    --base 6b2f21198795c8839a6073e418be8e2b1804959f --node MB.42.2.1.4
critical-guard: 6b2f21198795c8839a6073e418be8e2b1804959f..working tree, 36 changed file(s)
node MB.42.2.1.4: paths ['Cargo.toml', 'Cargo.lock', 'Makefile', 'crates/blastit/**',
  'crates/blastit-py-shim/**', 'crates/blastit-py-sapi/**',
  'crates/blastit-view-python/tests/critical_loaded_files.rs']
  M  crates/blastit-view-python/tests/critical_loaded_files.rs  [authorized by …]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.42.2.1.4
```

The `blastit-view-php` retire-loop case did not flake this run, so no isolated
re-run was needed.

### Status

All of `MB.42.2.1.4`'s acceptance is now green in this worktree: `make py-sapi`
(both minors), `critical_python_shim` (both minors over real sockets, PHP-only and
static-only with `libpython` mode `000`, the real `(st_dev, st_ino)` descriptor
swap refusal), the protected `critical_dev_and_fail_closed`/`critical_php_embed`
unedited, the retargeted protected `critical_loaded_files`, `blastit-py-sapi`/
`blastit-sys`/`blastit-py-shim`, `make fast-check`, and `critical_guard` with the
node's base sha. The lead notes from run 2 stand: `docs/design/
universal-distribution.md` §2's `PYTHONHOME` sentence needs a one-line amendment
(not edited here), plus the `Hosted` `dlsym`/`R3` clarification and `agree_minor`'s
new basis.

## Progress (run 4) — review fixes, and a gate that builds its own runtime

### Blockers

**A. The gate now builds `target/py-sapi` itself.** `Makefile` grew
`python-runtime:` (`$(MAKE) PY_SAPI_REQUIRE_ALL=1 py-sapi`) and `rs-test: python-runtime`,
so a clean checkout cannot pass by inheriting an artifact. Proven below with
`rm -rf target/py-sapi && make fast-check`: the log shows
`py-sapi: 2 shim(s) for x86_64-gnu … skipped: none` **before** the suite results
(`PY_SAPI_REQUIRE_ALL=1` fails closed if an interpreter is missing).

**B. One `runtime` line per boot.** The duplicated, banner-shaped line
(`Python::version_str()` = `"3.14.10 (main, …) [GCC …]"`) is gone: `load::announce`
only emits the preload `boot` event now, and `version`/`prefix` were dropped from
`Paths`/`PathsAnswer`, `announce` and both call sites. Stage 0's line
(`stage0::emit_runtime`, `version: "3.14.0"`) is the one an operator reads, as
`docs/reference/log-events.md` pins. (J follows: nothing surfaces `version_str()`.)

**C. `Hosted` never forks bare under a live interpreter.** `boot.rs`'s hooks are now
`shim_only…map(LoadedPy::hooks).or_else(|| crate::py::runtime_hooks(live) …)`, and a
failure to resolve `PyOS_*Fork` is a boot refusal (fail-closed) rather than `None` —
so a legacy-wheel PHP-only/static-only manifest cannot reach `forking::fork(None)`
with CPython live.

**D. No `/proc/self/fd/<N>` in a `dlopen` refusal.** `Opened::open_pinned` replaces
the descriptor spelling glibc echoes with the real display path before building
`NotALibrary` (and `from_shim` copies that `detail` verbatim, so `ShimRefused` is
covered by the one substitution).

**E. Stage-0 pinning refusals carry `blastit {cmd}:` once.** `project_python::discover`
prefixes `SharedObject::open`, `gate_libpython` and `check_provenance` with the boot's
verb; `LoadError::FreeThreaded`'s `Display` is headless (its test now asserts the
sentence *without* a head, plus that the caller adds it).

**F. `critical_python_shim` can no longer pass vacuously.** Every skip is
`eprintln!("SKIP …")` naming `make py-sapi` **and** `uv python install 3.13 3.14`;
`BLASTIT_PY_SAPI_REQUIRE=1` turns any skip into an assertion failure (including the
"no leg ran" case), which is what CI sets.

### Major

**G. `py_fds` are dropped after the load unless a cycle can use them.** `serve` now
does `let py_fds = serving.py_fds.take().filter(|_| keep_py_fds);` with
`keep_py_fds = serving.reload_source.is_some()`, so a production/`--no-reload` boot
releases both `OwnedFd`s the moment the `dlopen` finished (§1.2.1 item 6).
**H.** `hosted_hooks` names the running image (`current_exe()`).
**I.** Deliverable 6's last leg exists: `a_dev_cycle_keeps_serving_through_the_pinned_descriptors`
(a real `dev` boot with reload on, `app.py` v1 → v2 through `Adoption.py_fds`, then a
half-written `app.py` → the preflight's `SyntaxError` sentence naming the path, and
the old image still serving v2).

### Minors

**K.** `boundary.rs` is `unsafe`-free: its one `from_raw_parts` moved into
`loader::take_slot`, so design §1.1's "`abi.rs` + `loader.rs` only" holds exactly.
**L.** A failed `Py_InitializeFromConfig` renders `PyStatus.func`/`err_msg`.
**M.** `Supervisor::new` derives `hooks` from `runtime`; `Parts` no longer carries a
second copy.
**N.** A missing runtime in `begin_cycle` warns and `continue`s (one uncheckable file
does not abandon the cycle).
**O.** The dead `cfg_attr` on `load.rs` is gone, and `init_paths` takes the
already-resolved `PythonHandoff`.
**P.** `AbiVersion`/`MinorMismatch` carry the shim's path and lead with
`"the shim {} was built for …"`.
**Q.** The two unreachable-invariant messages say `(a blastit bug — report the boot
log above)` instead of "restart blastit".
**R.** The stray `\u{a7}` is `§`.

### Tails (clean runtime)

```
$ rm -rf target/py-sapi && make py-sapi
py-sapi: cpython 3.14 -> target/py-sapi/libblastit_py314-x86_64-gnu.so
py-sapi: cpython 3.13 -> target/py-sapi/libblastit_py313-x86_64-gnu.so
py-sapi: 2 shim(s) for x86_64-gnu in …/target/py-sapi; skipped: none

$ BLASTIT_PY_SAPI_REQUIRE=1 cargo test -p blastit --locked --test critical_python_shim
running 5 tests
a_dev_cycle_keeps_serving_through_the_pinned_descriptors ... reload: v1 -> v2 through the pinned descriptors, then a rejected save kept v2 ... ok
a_php_only_boot_never_opens_libpython ... php: 200 via …/libblastit_php84-x86_64-gnu.so without opening …/unreadable ... ok
a_static_only_boot_never_opens_libpython ... static: 200 without opening …/unreadable ... ok
stage_1_refuses_a_python_descriptor_whose_identity_moved ... swap: refused (descriptor 3 names …/replacement, …) ... ok
the_one_binary_serves_wsgi_under_both_supported_minors ...
  CPython 3.13: 200 "served by cpython 3.13" … /libblastit_py313-x86_64-gnu.so
  CPython 3.14: 200 "served by cpython 3.14" … /libblastit_py314-x86_64-gnu.so ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ PYTHONHOME=<cpython-3.14 prefix> cargo test -p blastit --locked \
    --test critical_dev_and_fail_closed --test critical_php_embed
test result: ok. 20 passed; 0 failed; …   (critical_dev_and_fail_closed)
test result: ok. 29 passed; 0 failed; …   (critical_php_embed)

$ cargo test -p blastit-view-python --test critical_loaded_files
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make fast-check                       # after `rm -rf target/py-sapi`
py-sapi: 2 shim(s) for x86_64-gnu in …/target/py-sapi; skipped: none
guard: OK · critical-guard: … · fmt clean · clippy --all-targets -D warnings clean
159 `test result: ok` suites, 0 failures · ruff clean · plan OK
fast-check=0
```

`blastit-view-php`'s retire-loop case did not flake on this run.

### Still for the lead (unchanged)

`docs/design/universal-distribution.md` §2's `PYTHONHOME` sentence needs a one-line
amendment (not edited here); the `Hosted` `dlsym`/`R3` clarification; and
`agree_minor`'s supported-minor basis.
