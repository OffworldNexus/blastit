---
id: MB.42.2.1.2
title: "blastit-py-shim cdylib (9 C exports) + make py-sapi for CPython 3.13/3.14"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.2.1.1]
paths:
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "scripts/**"
  - "crates/blastit-py-shim/**"
  - "crates/blastit-py-sapi/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T17:21"
updated: "2026-11-19T17:21"
---

# Goal
Produce the artifact the universal binary will `dlopen`: a per-minor `cdylib` shim exporting the frozen `blastit_py_abi_v1` surface, built by `make py-sapi` for CPython 3.13 **and** 3.14, proven by `readelf`/`nm` and by a self-contained `dlopen` -> `blastit_py_abi_v1()` -> `abi_version`/`py_minor` probe that boots **no** `blastit`. Per `docs/design/universal-distribution.md` §1.1 (`blastit-py-shim` crate boundary), §1.2 (the nine exports + `make py-sapi`), §6.1 (shim filenames `libblastit_py3{13,14}-<arch>-<libc>.so`).

# Exact Deliverables
1. **New `crates/blastit-py-shim`** (workspace member; `crate-type = ["cdylib", "rlib"]` so the same crate is unit-testable as a lib): deps `blastit-py-sapi`, `blastit-sys`, `blastit-view`, `blastit-view-python`, `pyo3 = "0.30.1"`, `serde`, `serde_json`; non-default feature `extension-module = ["pyo3/extension-module"]` (design §1.1).
2. **All nine C exports of §1.2**, each a `#[unsafe(no_mangle)] pub extern "C" fn` wrapped in `std::panic::catch_unwind` (`AssertUnwindSafe`) with a `-1`/`70` refusal instead of an unwind across FFI:
   - `blastit_py_abi_version() -> u32` = `1`, `blastit_py_minor() -> u32` = `313`/`314` (from the build's `PYO3_PYTHON` minor, not a literal),
   - `blastit_py_abi_v1() -> *const BlastitPyVtableV1` returning a `static` vtable whose nine slots are exactly `abi.rs`'s `BlastitPyVtableV1` fields (the shim and `blastit-py-sapi` must not disagree: add a test that assigns each field to the matching export),
   - `before_fork` / `after_fork_parent` / `after_fork_child` -> `pyo3::ffi::PyOS_BeforeFork` / `PyOS_AfterFork_Parent` / `PyOS_AfterFork_Child` (caller holds the GIL, per §1.2.1),
   - `init_paths` -> `Py_InitializeEx(0)`, set `SITE_PACKAGES`, restore `sys_path` (`Vec<Vec<u8>>`) + cwd, verify the running minor, evaluate `VIEW_PATHS`/`SYS_PATH` -> `{"version","prefix","view_paths","sys_path"}` into `out`, error text into `err` (JSON both ways),
   - `preload` -> `LoadedTracker` when `report_fd >= 0`, app import + preload + warmup, flush the boot batch, `CLOSE_AND_COLLECT`, `blastit_view_python::immortal::apply`, `gc.freeze()`, and write the opaque `PyZygoteOpaque` pointer through `zygote` -> `{"decision","objects","reason","elapsed_ns"}`,
   - `check_syntax` -> `PyCode::compile` with `PyCodeInput::File`, **no execution** (`dev` reload preflight),
   - `serve_view` -> post-`blastit_sys::child::enter(plan)` child entry: set `os.environ["HOME"/"TMPDIR"]`, adopt `view_fd` + optional `(loaded_fd, loaded_source)`, then `blastit_view_python::serve_reporting` (returns `1` or `70`),
   - `free_buf` -> reclaims a `PyBufV1` the shim allocated (never one the caller built).
3. **`make py-sapi` in `Makefile`** (mirror the shape and idiom of the existing `php-sapi` target, which is the worked example at `Makefile:433`):
   - `PY_MINORS ?= 3.14 3.13`; interp resolution per §1.2: `PY_SAPI_PYTHON_<digits>` -> `/opt/python/cp<digits>-cp<digits>/bin/python3` -> `uv python find --no-project <minor>` -> `command -v python<minor>`; a minor with no interpreter is `skipped` **unless** `PY_SAPI_REQUIRE_ALL=1`, which fails closed;
   - `arch`/`libc` detected like `php-sapi` does (supported: `x86_64`/`aarch64` × `gnu`/`musl`), overridable via `PY_SAPI_ARCH`/`PY_SAPI_LIBC`;
   - per-minor `CARGO_TARGET_DIR=target/py-sapi/build-<digits>` with `PYO3_PYTHON=<interp>`, `--features extension-module`, `-C link-arg=-Wl,-soname,libblastit_py<digits>-<arch>-<libc>.so`, and `-C target-feature=-crt-static` on `musl` (a static musl object cannot `dlopen` **or** be `dlopen`ed — see `LESSONS.md`);
   - output `target/py-sapi/libblastit_py<digits>-<arch>-<libc>.so`, then the assertions of §1.2: `scripts/check-portable-elf.sh` on the artifact, `readelf -d … | grep SONAME` equals the filename, and `nm -uD … | grep -q Py_InitializeEx` (undefined-and-dynamic is the point: the symbols come from `libpython` at Stage-1 `dlopen`);
   - add `py-sapi` to `.PHONY` and the `help` surface; keep it out of `check` (it needs interpreters), while `rs-no-python` keeps proving the *binary* stays clean.
4. **`scripts/check-portable-elf.sh`**: accept a `.so` input for the new call site without weakening its binary rules (zero `libpython`/`libphp` `DT_NEEDED`); the shim legitimately carries `UND Py*` symbols, so the gate must distinguish "undefined, to be resolved by a later `dlopen`" from "`DT_NEEDED libpython3.x.so`". Cover that distinction with a case in the script's own self-test path.
5. **Self-contained load probe** (`crates/blastit-py-shim/tests/abi_load.rs`, `#[ignore]`d unless a shim is named — it is a lab test, not a `check` step): `dlopen` the built `.so` from an explicit path, resolve `blastit_py_abi_v1`, assert `abi_version() == 1` and `py_minor()` in `{313, 314}` matching the filename digits. It must **not** boot `blastit`, must not need a `Blastfile`, and must exit cleanly without `Py_InitializeEx`. Run it from `make py-sapi`'s tail for each minor so a broken vtable fails the build, not a later slice.

# Acceptance
- `make py-sapi` on the host for both minors (installing interpreters with `uv python install 3.13 3.14` into the uv cache is fine)
- `readelf -d target/py-sapi/libblastit_py313-*.so | grep SONAME` and `nm -uD target/py-sapi/libblastit_py313-*.so | grep Py_InitializeEx` (and the same for `py314`)
- `scripts/check-portable-elf.sh target/py-sapi/libblastit_py314-*.so` -> zero `libpython`/`libphp` `DT_NEEDED`
- `cargo test -p blastit-py-shim --features extension-module` (vtable/export assignment test + `PyBufV1` round trip)
- `cargo test -p blastit-py-shim --features extension-module --test abi_load -- --ignored` with `BLASTIT_PY_SAPI_SHIM=<path>` for each minor
- `make fast-check`

# Notes
- **1 Cargo writer**: this node is the only Cargo/`Cargo.lock`/`target/` writer while active; `make py-sapi` uses per-minor `CARGO_TARGET_DIR`s, but the `blastit-py-shim` crate itself still shares the workspace `target/` — never run two cargo builds in parallel.
- Never write `/home/remy/.local` or `/usr/bin/blastit`; never install or modify host PHP/Python packages — manylinux/shim matrix work runs in a container (`uv python install` into the uv cache is fine).
- Zero globals, zero `OnceLock`/`static Mutex`, zero `#[cfg(test)]` env seams, zero mock traits. The one `static` allowed is the immutable `BlastitPyVtableV1` const table, which carries no interior mutability.
- `<500` prod LOC per file, `<=6` args; a shim boundary function takes `(req, req_len, out, err)` — do not grow it.
- No `unwrap()`/`expect()` on untrusted input; a panic inside the shim must become a refusal via `catch_unwind`, never a process abort.
- Do **not** modify `crates/*/tests/critical_*.rs` or `python/tests/critical/`; out-of-scope finds go to `## Follow-ups` in `RESULT.md`.
- `crates/blastit-py` and `crates/blastit-dbproxy` must keep compiling until `MB.42.2.2`.
