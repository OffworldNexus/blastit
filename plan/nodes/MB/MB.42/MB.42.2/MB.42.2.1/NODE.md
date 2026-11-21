---
id: MB.42.2.1
title: "Shared fd pinning + blastit-py-sapi/-py-shim + Stage-0/1 dlopen + make py-sapi (one libpython-free binary)"
kind: epic
status: done
priority: 1
depends_on: []
paths: [Cargo.toml, Cargo.lock, "crates/**", Makefile, "scripts/**", "e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T17:18"
updated: "2026-11-19T20:06"
---

# Goal
Make one `libpython`-free `blastit` binary serve Python WSGI views by `dlopen`ing fd-pinned per-minor shims in Stage 1 — the Python half of the PHP architecture — per `docs/design/universal-distribution.md` §1, §2 and `ADR-0017` Decisions 1–2.

# Exact Deliverables
1. **`crates/blastit-sys/src/pin.rs` (<150 LOC)**: lift the `(path, st_dev, st_ino)` verified descriptor open (`open_pinned`, `O_RDONLY | O_CLOEXEC | O_NOFOLLOW`) and `SharedObject` (`/proc/self/fd/<fd>`) out of `crates/blastit-view-php/src/pinning.rs`, so PHP and Python share one owner next to `blastit_sys::elf`. `EntryPin` (`st_size` + `st_ctime` + `O_PATH` + `repin_entry`) **stays** in `blastit-view-php/src/pinning.rs` (§1.1); `blastit-view-php` keeps passing its own suites after the move.
2. **New `crates/blastit-py-sapi`** (`#![deny(unsafe_code)]`, `#[allow(unsafe_code)]` only in `abi.rs` + `loader.rs`, `<500` LOC/file): `abi` (the frozen `blastit_py_abi_v1` types of §1.2 verbatim — `PyBufV1`, `PyZygoteOpaque`, `BlastitPyVtableV1`, `ABI_VERSION = 1`), `discovery` (moved from `crates/blastit/src/stage0/project_python.rs`: interpreter order, `pyvenv.cfg` `home` -> `python_home`, `site-packages`, free-threaded / unsupported-minor refusals with the exact sentences of §2.3, `libpython3.<m>.so[.1.0]` search, deployment-vs-development shim search, `.dynsym` gate on `Py_InitializeEx`/`PyGILState_Ensure`/`PyOS_BeforeFork`), and `loader` (`dlopen("/proc/self/fd/<fd>")` -> `blastit_py_abi_v1()`, `abi_version`/`py_minor` check, `LoadedPy`).
3. **New `crates/blastit-py-shim`** (`crate-type = ["cdylib", "rlib"]`; deps `blastit-py-sapi`, `blastit-sys`, `blastit-view`, `blastit-view-python`, `pyo3 = "0.30.1"`, `serde`, `serde_json`; non-default `extension-module = ["pyo3/extension-module"]`): exports every `blastit_py_abi_v1` symbol of §1.2 (`before_fork`, `after_fork_parent`, `after_fork_child`, `init_paths`, `preload`, `check_syntax`, `serve_view`, `free_buf`) with `std::panic::catch_unwind` on each boundary, wrapping `blastit-view-python`'s `serve_reporting`, `immortal`, `LoadedTracker` and `PyCode::compile`.
4. **Zero-global fork wiring**: `crates/blastit/src/forking.rs` takes an injected `Option<PyForkHooks>` (`Copy` struct of the three `extern "C"` hooks) from `LoadedPy` into `visibility::*` and `Supervisor`; remove all three `pyo3::ffi::PyOS_*Fork` calls.
5. **Stage 0 / Stage 1**: Stage 0 (`stage0/project_python.rs` call site) runs only under the §2.1 activation gate, opens `libpython` + shim through `blastit_sys::pin`, keeps `Provenance` in `Serve`, sets `PYTHONHOME`, clears `FD_CLOEXEC` for the Stage-1 `execve` only and passes `--py-lib-fd=<N> --py-shim-fd=<M>`; Stage 1 re-`fstat`s `(st_dev, st_ino)` after `test_swap_pause`, re-sets `FD_CLOEXEC`, forks `http`/`scheduler` first, then `dlopen`s lib (`RTLD_NOW|RTLD_GLOBAL`) then shim (`RTLD_NOW|RTLD_LOCAL`); `Adoption` carries `py_fds` across `dev` `exec_cycle`.
6. **Crate wiring**: `crates/blastit/Cargo.toml` `default = ["python", "php", "static"]`, `python = ["dep:blastit-py-sapi"]`, drop `pyo3` + `blastit-view-python`; strip `python_home`/`rpath`/`PYO3_PYTHON` from `crates/blastit/build.rs` (keep `coverage`); `Cargo.lock` updated. **Do not delete `crates/blastit-py`** — it must keep compiling (its deletion is `MB.42.2.2`).
7. **`make py-sapi`** in `Makefile`: `PY_MINORS ?= 3.14 3.13`; `PY_SAPI_PYTHON_<digits>` -> `/opt/python/cp<digits>-cp<digits>/bin/python3` -> `uv python find --no-project <minor>` -> `command -v python<minor>`; build in `CARGO_TARGET_DIR=target/py-sapi/build-<digits>` with `PYO3_PYTHON=<interp> --features extension-module -C link-arg=-Wl,-soname,libblastit_py<digits>-<arch>-<libc>.so` (`-C target-feature=-crt-static` on musl); output `target/py-sapi/libblastit_py<digits>-<arch>-<libc>.so`; then `scripts/check-portable-elf.sh` + `DT_SONAME` + `UND Py_InitializeEx` assertions; `PY_SAPI_REQUIRE_ALL=1` fails closed.

# Acceptance
- `make fast-check`
- `cargo test -p blastit-sys -p blastit-py-sapi -p blastit-view-php`
- `make py-sapi` then `readelf -d target/py-sapi/libblastit_py313-*.so | grep SONAME` and `nm -uD … | grep Py_InitializeEx` (both shims, 3.13 + 3.14).
- `cargo build -p blastit` **and** `cargo build --workspace` then `scripts/check-portable-elf.sh target/debug/blastit` -> zero `libpython`, zero `libphp`, zero `RUNPATH`.
- New `crates/blastit/tests/critical_python_shim.rs`: the one binary serves a real WSGI app under CPython 3.13 **and** 3.14 (each leg asserts 200 + body over a real socket, zero `Mock*Server`), refuses a PHP-only and a static-only manifest without opening `libpython`, and refuses an fd whose `(st_dev, st_ino)` was swapped in Stage 1.

# Notes
- **1 Cargo writer**: this node is the only Cargo/`target/` writer while active; never run two cargo builds in parallel.
- Never write `/home/remy/.local`, `/usr/bin/blastit`, or the host's PHP/Python packages; any package/runtime test runs in a container.
- `scripts/check-portable-elf.sh` must run on the artifact the build just produced (mind `CARGO_TARGET_DIR`).
- No `Mock*Server`/`StubPolicy` fixtures, no `scripts/check_*.py` regex guards; enforce invariants with types and `blastit_sys::pin`.
- Keep every file `<500` prod LOC and functions `<=6` args; search the workspace before writing a helper.
- Do not modify `crates/*/tests/critical_*.rs` without it being listed here; record out-of-scope finds under `## Follow-ups` in `RESULT.md`.

## Slices
1. `MB.42.2.1.1` — `blastit_sys::pin` (`open_pinned` + `SharedObject`) + `blastit-py-sapi` (`abi` / `discovery` / `loader`), `blastit-view-php` rewired onto the shared owner; `crates/blastit` untouched. `depends_on: MB.42.1`.
2. `MB.42.2.1.2` — `blastit-py-shim` `cdylib` (all nine §1.2 C exports, `catch_unwind`) + `make py-sapi` for CPython 3.14/3.13 with `DT_SONAME`/`UND Py_InitializeEx` assertions. `depends_on: MB.42.2.1.1`.
3. `MB.42.2.1.3` — Stage-0 `discovery` call site under the §2.1 activation gate, `blastit_sys::pin` fd handoff (`--py-lib-fd`/`--py-shim-fd`, `Adoption.py_fds`), injected `Option<PyForkHooks>`, three `PyOS_*Fork` calls deleted; no stage-1 `dlopen`. `depends_on: MB.42.2.1.2, MB.42.2.0`.
4. `MB.42.2.1.4` — Stage-1 `dlopen` (lib `RTLD_GLOBAL`, shim `RTLD_LOCAL`), `PyRuntime` supervisor cut-over, `check_syntax` via the vtable, `pyo3` + `blastit-view-python` dropped, portable-ELF proof for `-p blastit` and `--workspace`. `depends_on: MB.42.2.1.3`.
