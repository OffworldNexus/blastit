# MB.42.2.1.2 — `blastit-py-shim` cdylib + `make py-sapi` (CPython 3.13/3.14)

## Files

**New crate `crates/blastit-py-shim/`**

| Path | What |
|---|---|
| `Cargo.toml` | `crate-type = ["cdylib", "rlib"]`; deps `blastit-py-sapi`, `blastit-sys`, `blastit-view`, `blastit-view-python`, `pyo3 = "0.30.1"`, `serde`, `serde_json`; non-default `extension-module = ["pyo3/extension-module"]` |
| `build.rs` | Records the build's `PYO3_PYTHON` minor into `BLASTIT_PY_MINOR` (`py_minor()` is never a literal) and, under `extension-module`, links every link target with `--unresolved-symbols=ignore-all` so the `cdylib` keeps its `UND Py*` |
| `src/lib.rs` | Crate docs (the "never unwind across FFI" rule, the opaque-zygote rule, the artifact contract) and the gated `surface` module: three `#[unsafe(no_mangle)]` exports (`blastit_py_abi_version`, `blastit_py_minor`, `blastit_py_abi_v1`), the ten-slot static `VTABLE`, and a test that maps every slot to its boundary |
| `src/buffer.rs` | `PyBufV1` allocation/reclaim (`free_buf`), JSON answers and errors, `guarded` (`catch_unwind` + `child::panic_message` → refusal) |
| `src/request.rs` | `#[derive(Deserialize)]` documents for `init_paths`/`preload`/`serve_view`, `deny_unknown_fields`, defaults, `Immortalize` → `immortal::Setting` |
| `src/env.rs` | `init_paths`: `Py_InitializeEx(0)`, `SITE_PACKAGES`, `sys.path` restore + cwd first, minor check, `SYS_PATH`/`VIEW_PATHS` → `{"version","prefix","view_paths","sys_path"}` |
| `src/boot.rs` | `preload`: `LoadedTracker` when `report_fd >= 0`, app import + preload + warmup, boot batch through `LoadedReporter`, `CLOSE_AND_COLLECT`, `immortal::apply`, `gc.freeze()`, opaque `Zygote` → `{"decision","objects","reason","elapsed_ns"}` |
| `src/serve.rs` | `serve_view`: `os.environ["HOME"/"TMPDIR"]`, adopts `view_fd` + optional `(loaded_fd, loaded_source)`, `serve_reporting` (returns `1` / `70`) |
| `src/syntax.rs` | `check_syntax`: `PyCode::compile(…, PyCodeInput::File)`, no execution, `<path>: <err>` |
| `tests/abi_load.rs` | `#[ignore]`d self-contained probe: `dlopen` libpython (`RTLD_GLOBAL`) then the shim (`RTLD_LAZY`), `blastit_py_abi_v1()` → `abi_version == 1`, `py_minor` == the filename's digits; skips with a printed reason when `BLASTIT_PY_SAPI_SHIM` is unset |

**Edited (inside `paths`)**

- `Makefile` — `py-sapi` target (+ `.PHONY`, `PY_MINORS ?= 3.14 3.13`, `PY_SAPI_*` variables): `PY_SAPI_PYTHON_<digits>` → manylinux `/opt/python` → `uv python find` → `python<minor>`; **refuses** an interpreter whose minor disagrees with the one being built and **skips** a free-threaded one (a WSGI view needs the GIL); per-minor `CARGO_TARGET_DIR=target/py-sapi/build-<digits>`, `PYO3_PYTHON`, `--features extension-module`, `-C link-arg=-Wl,-soname,…` (plus `-C target-feature=-crt-static` on musl); copies to `target/py-sapi/libblastit_py<digits>-<arch>-<libc>.so`; asserts `check-portable-elf.sh`, `DT_SONAME`, `UND Py_InitializeEx`, the three `T` exports, and the probe against the real libpython; `PY_SAPI_REQUIRE_ALL=1` fails closed naming each minor's reason. Not in `check`.
- `scripts/check-portable-elf.sh` — documents the distinction the gate already enforces (an `UND Py_*` in `.dynsym` is the shim's contract; `NEEDED libpython` is the refusal). Rules unchanged.
- `scripts/test_packaging_gates.py` — `portable-elf` case pair: a `cc`-built `.so` with `DT_NEEDED libpython3.13.so.1.0` is refused (`links an interpreter`), the same `Py_InitializeEx` name left `UND` with no `DT_NEEDED` is accepted.
- `Cargo.lock` — new workspace member.

`crates/blastit` is untouched; `crates/blastit-py` and `crates/blastit-dbproxy` still compile.

## Evidence

`make py-sapi` (host, x86_64-gnu) — both minors built **and** proved:

```
py-sapi: cc=cpython314 -> .../target/py-sapi/libblastit_py314-x86_64-gnu.so (.../.venv/bin/python)
ok   .../libblastit_py314-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
py-sapi: proving .../libblastit_py314-x86_64-gnu.so with the self-contained dlopen probe
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
py-sapi: cc=cpython313 -> .../target/py-sapi/libblastit_py313-x86_64-gnu.so (.../cpython-3.13-linux-x86_64-gnu/bin/python3.13)
ok   .../libblastit_py313-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
py-sapi: proving .../libblastit_py313-x86_64-gnu.so with the self-contained dlopen probe
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
py-sapi: 2 shim(s) for x86_64-gnu in .../target/py-sapi; skipped: none
```

Probe lines (the ABI proof itself):

```
ok   .../libblastit_py314-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 314
ok   .../libblastit_py313-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 313
```

`readelf -d` / `nm -uD` tails:

```
== target/py-sapi/libblastit_py313-x86_64-gnu.so
 0x0000000000000001 (NEEDED)   Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)   Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)   Shared library: [ld-linux-x86-64.so.2]
 0x000000000000000e (SONAME)   Library soname: [libblastit_py313-x86_64-gnu.so]
                 U _Py_Dealloc
                 U Py_InitializeEx
0000000000051680 T blastit_py_abi_v1
0000000000051688 T blastit_py_abi_version
0000000000051690 T blastit_py_minor

== target/py-sapi/libblastit_py314-x86_64-gnu.so
 0x0000000000000001 (NEEDED)   Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)   Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)   Shared library: [ld-linux-x86-64.so.2]
 0x000000000000000e (SONAME)   Library soname: [libblastit_py314-x86_64-gnu.so]
                 U _Py_Dealloc
                 U Py_InitializeEx
0000000000051f60 T blastit_py_abi_v1
0000000000051f68 T blastit_py_abi_version
0000000000051f70 T blastit_py_minor
```

`scripts/check-portable-elf.sh`:

```
ok   target/py-sapi/libblastit_py314-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   target/py-sapi/libblastit_py313-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
```

`UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py`:

```
packaging-gates-test: ok — 58 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2)
```

`cargo test -p blastit-py-shim` / `--features extension-module` (both non-zero):

```
test result: ok. 21 passed; 0 failed; 0 ignored
test result: ok. 10 passed; 0 failed; 0 ignored     # feature build: pure half
```

`make fast-check` (guard, critical-guard, fmt, clippy `-D warnings`, doc, the whole
workspace test run, py-lint, plan-validate) — zero compiler, manifest or clippy
warnings; tail:

```
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
```

## Notes / deviations

1. **A test build of an `extension-module` crate has nothing to link `Py*`
   against.** pyo3's `extension-module` feature suppresses `-lpython` for every
   link target of the package, and CPython's `_Py_Dealloc` arrives as a
   `GLOB_DAT` relocation that no `RTLD_LAZY` can defer — so a test executable
   built in that configuration cannot start. The interpreter-backed half
   (`surface` + `boot`/`env`/`serve`/`syntax`) is therefore compiled out of
   `cfg(all(test, feature = "extension-module"))` **only**, documented in the
   crate docs. The artifact never sees `cfg(test)`; the default
   `cargo test -p blastit-py-shim` runs all 21 tests with libpython linked, and
   the `--features extension-module` run still exercises the pure half (10
   tests).
2. **The probe loads `libpython` first**, exactly as stage 1 does
   (`RTLD_GLOBAL`), because pyo3's data relocations cannot be deferred. It never
   runs `Py_InitializeEx` and never boots `blastit`; with
   `BLASTIT_PY_SAPI_SHIM` unset it prints why and passes.
3. **`serve_view` returns `0`/`1`/`70`** per §1.2 and per the shipping child
   protocol (`zygote.rs` maps a served channel to `0`): `0` = the channel ended
   and every accepted request was served, `1` = a refusal (one JSON sentence in
   `err`), `70` = the `catch_unwind` arm. `init_paths`/`preload`/`check_syntax`
   use `0`/`-1`.
4. **The request JSON is this slice's wire format** (`deny_unknown_fields`), so
   `MB.42.2.1.3/.4` must send these exact keys — documented above in
   `src/request.rs`.
5. Zero globals, zero `OnceLock`/`static Mutex`, zero mock traits, zero
   `unwrap()`/`expect()` outside tests; the one `static` is the immutable
   vtable.
6. This host resolves CPython `3.14` through the project `.venv` (its `uv`
   base install) and `3.13` through the uv cache; `py-sapi` refuses an
   interpreter whose `sys.version_info[:2]` disagrees with the minor it is
   building for.

## Review fixes (second pass)

`review-arch` + `review-ux` findings, all applied:

1. **Exit-code contract.** `serve_view` now returns `0` (served and shut down
   cleanly) / `1` (refusal, one JSON sentence) / `70` (only the `catch_unwind`
   arm), matching `blastit-view-python/src/zygote.rs`'s `Ok(()) => child_exit("", 0)`.
   Pinned in `crates/blastit-py-sapi/src/abi.rs`'s `serve_view` doc.
2. **`report_fd` ownership.** `install_tracker` adopts the descriptor **before**
   anything can fail (`(req.report_fd >= 0).then(|| UnixDatagram::from_raw_fd(..))`)
   and returns it; `report_boot` consumes the owned socket, and a boot with no
   tracker drops it. No path can leak the write end into a forked view.
3. **`PyBufV1` ownership.** Documented in `abi.rs`: a slot arrives **zeroed**, a
   non-null `ptr` means "a buffer this shim returned", the shim allocates and the
   caller frees through `free_buf`. `store` overwrites instead of reinterpreting
   (no `Vec::from_raw_parts` on a caller's bytes), and `free` refuses
   `len > cap` rather than trusting a lying length.
4. **The artifact invariant is proven, not assumed.** With
   `BLASTIT_PY_SAPI_LIBPYTHON` named, the probe now `dlopen`s the shim
   `RTLD_NOW | RTLD_LOCAL` after a `RTLD_GLOBAL` libpython — every `Py*`
   reference must resolve against the real CPython — and it `dlsym`s all three
   exports, asserting `blastit_py_abi_version()`/`blastit_py_minor()` equal the
   vtable's first two slots. `make py-sapi` additionally asserts the three `T`
   exports on the artifact.
5. `zygote_state` is an `unsafe fn` with a `# Safety` section and rejects a
   misaligned pointer; `blastit_py_abi_v1` is wrapped in `catch_unwind` like
   every other boundary; a null `out`/`zygote` is refused with `-1` + sentence
   instead of silently discarding the answer; `check_syntax` validates its
   zygote slot (an identity check; the compile runs in the caller's process).
6. **Makefile.** `--locked` on the build and the probe test; `RUSTFLAGS` is
   appended to, not clobbered; the probe runs with
   `PYO3_PYTHON="$$interp"` (the interpreter the shim was built for) and the
   build's `LIBDIR` on `LD_LIBRARY_PATH`; a missing shared `libpython` fails
   with the package remedy instead of an `is not a file` lie; `readelf`/`nm`
   are checked up front with `binutils` named; `PY_SAPI_REQUIRE_ALL=1` prints
   the *reason* per minor (`no interpreter` / `free-threaded`) and the remedy;
   the help line and the build line are prose, not jargon.
7. `scripts/check-portable-elf.sh` branches on the artifact kind: a `.so`'s
   libpython remedy is `make py-sapi`, a binary's is the standalone build.
8. Display sentences in `env.rs`/`boot.rs`/`serve.rs` are prose one-liners
   (`the interpreter's sys.path could not be restored: …`), not label prefixes.

Second-pass evidence:

```
$ make py-sapi          # both minors, RTLD_NOW probe against the real libpython
py-sapi: cpython 3.14 -> .../libblastit_py314-x86_64-gnu.so (…/.venv/bin/python)
ok   .../libblastit_py314-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 314
py-sapi: cpython 3.13 -> .../libblastit_py313-x86_64-gnu.so (…/cpython-3.13-linux-x86_64-gnu/bin/python3.13)
ok   .../libblastit_py313-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 313
py-sapi: 2 shim(s) for x86_64-gnu in .../target/py-sapi; skipped: none

$ nm -D --defined-only .../libblastit_py313-x86_64-gnu.so | awk '{print $2" "$3}' | grep '^T blastit_py_'
T blastit_py_abi_v1
T blastit_py_abi_version
T blastit_py_minor
$ nm -D --defined-only .../libblastit_py314-x86_64-gnu.so | awk '{print $2" "$3}' | grep '^T blastit_py_'
T blastit_py_abi_v1
T blastit_py_abi_version
T blastit_py_minor

$ cargo test -p blastit-py-shim                  -> 23 passed
$ cargo test -p blastit-py-shim --features extension-module -> 12 passed
$ UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 58 cases passed (…, portable-elf=2)
```

## Follow-ups

- `crates/blastit/src/stage1.rs` still owns `SITE_PACKAGES`, `SYS_PATH`,
  `VIEW_PATHS`, `CLOSE_AND_COLLECT`, `FREEZE`, and
  `stage1/python_env.rs` owns `restore_sys_path`/`put_cwd_first`; this crate
  carries the same five snippets and two helpers so `crates/blastit` stays
  untouched. `MB.42.2.2` should delete the originals (the shim becomes the one
  owner) — a repo-wide grep for `blastit_sys_path` will find both copies until
  then.
- `make py-sapi` does not yet pass `--max-glibc 2_28`; `check-portable-elf.sh`
  has no `--max-glibc` flag yet (§6.1). Release/manylinux builds must add it
  together with `scripts/build-standalone.sh`.
- `scripts/platform_matrix.py` still lists PHP shim legs only; the Python
  `libblastit_py3{13,14}` legs (4 arch × libc) belong to the packaging slice.
