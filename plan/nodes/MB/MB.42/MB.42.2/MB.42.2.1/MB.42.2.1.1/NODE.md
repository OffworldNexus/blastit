---
id: MB.42.2.1.1
title: "blastit_sys::pin + blastit-py-sapi (abi/discovery/loader), rewired blastit-view-php"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.1]
paths:
  - Cargo.toml
  - Cargo.lock
  - "crates/blastit-sys/**"
  - "crates/blastit-py-sapi/**"
  - "crates/blastit-view-php/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T17:21"
updated: "2026-11-19T18:01"
---

# Goal
Create the two load-time foundations of the Python half of the PHP architecture — one shared fd-pinning owner in `blastit-sys` and one `pyo3`-free ABI/discovery/loader crate — **without touching `crates/blastit`**. Per `docs/design/universal-distribution.md` §1.1 (crate boundaries + shared pinning), §1.2 (frozen `blastit_py_abi_v1`) and §2 (discovery, refusals, `libpython` resolution, ELF gate).

This slice turns Stage-0 Python discovery into a library that a future `blastit` binary calls, so the discovery sentences and the pinned-descriptor primitive exist and are tested before any call site moves.

# Exact Deliverables
1. **`crates/blastit-sys/src/pin.rs` (<150 prod LOC, `pub mod pin;` from `lib.rs`)** — the single owner of "open a shared object and prove its identity", next to `blastit_sys::elf`:
   - `open_pinned(path: &Path, expected: Option<(u64, u64)>) -> Result<OwnedFd, PinError>`: `O_RDONLY | O_CLOEXEC | O_NOFOLLOW`, require a regular file, `fstat` and compare `(st_dev, st_ino)` when `expected` is `Some` (design §1.1: shared-object pins need **only** `(st_dev, st_ino)` — no `st_size`/`st_ctime`, no `O_PATH`).
   - `SharedObject { fd: OwnedFd }` with `path()` -> `/proc/self/fd/<fd>` (the only path `dlopen` ever sees), `identity()` -> `(st_dev, st_ino)` re-read from the live fd, `as_raw_fd()`, and `into_fd()` for the Stage-1 handoff.
   - `entry_identity(path: &Path) -> io::Result<(u64, u64)>` (lstat-based, no open) so a caller can record what Stage 0 proved.
   - Errors are typed (`PinError::{Open, NotRegular, IdentityChanged, Io}`), no `unwrap()`/`expect()` on anything derived from a path or an `fstat`; a refusal sentence names the path and both identities.
   - Unit tests over real files in `target/tmp`: accepted open, `IdentityChanged` after a rename/`cp` swap, symlink refused (`O_NOFOLLOW` -> `ELOOP`), FIFO/DIR refused as non-regular, and `/proc/self/fd/<fd>` still readable after the source path is unlinked.
2. **Rewire `crates/blastit-view-php`** onto the shared owner and **delete its private copy**: `pinning.rs:464 fn open_pinned` and its `fstat` identity helper go away; `boot_bindings` / `PhpBinding` (`libphp`, `shim`) call `blastit_sys::pin::open_pinned` + `SharedObject`. **`EntryPin` (`st_size` + `st_ctime` + `O_PATH` + `repin_entry`) stays in `pinning.rs` verbatim** (design §1.1 — PHP re-resolves `SCRIPT_FILENAME` by path per request), and `await_test_swap` stays where it is.
3. **New `crates/blastit-py-sapi`** (workspace member; `#![deny(unsafe_code)]`, with `#[allow(unsafe_code)]` on `abi.rs` + `loader.rs` only; `<500` prod LOC/file; deps `blastit-sys`, `libc`, `serde`, `serde_json`, `thiserror`-free typed errors as the workspace style dictates — **zero `pyo3`**):
   - **`abi.rs`** — the frozen surface of design §1.2 **verbatim**: `pub const ABI_VERSION: u32 = 1;`, `#[repr(C)] PyBufV1 { ptr, len, cap }`, `#[repr(C)] PyZygoteOpaque { _private: [u8; 0] }`, `#[repr(C)] BlastitPyVtableV1` with exactly the nine fields and doc comments of §1.2 (`abi_version`, `py_minor`, `before_fork`, `after_fork_parent`, `after_fork_child`, `init_paths`, `preload`, `check_syntax`, `serve_view`, `free_buf`), plus the `Copy` `PyForkHooks { before_fork, after_fork_parent, after_fork_child }` that `crates/blastit` will inject (design §1.2 "zero-global fork wiring"). Pin the C layout with a test asserting field offsets/size against `#[repr(C)]` ordering, and `py_minor` encoding (`313`/`314`).
   - **`discovery.rs`** — moved from `crates/blastit/src/stage0/project_python.rs` (469 lines today), same behaviour, new owner:
     - interpreter resolution order exactly per §2.2: `Dev`/`Check` = `BLASTIT_PYTHON` -> `$VIRTUAL_ENV/bin/{python3,python}` (`pyvenv.cfg` present) -> `<manifest_dir>/.venv/bin/{python3,python}` -> `BLASTIT_WHEEL_PYTHON` -> `{python3,python}` on `PATH`; `Production`/`Serve` = `BLASTIT_PYTHON` (absolute + executable) -> `<manifest_dir>/.venv/...` (**never** reads `$VIRTUAL_ENV`) -> `BLASTIT_WHEEL_PYTHON` (absolute) -> `/usr/local/bin`, `/usr/bin`, `/bin`. Filesystem-only: **zero Python execution in Stage 0**.
     - `pyvenv.cfg` `home = <dir>` -> `python_home = Path::new(home).parent()`, plus `<venv>/{lib,lib64}/python3.{13,14}/site-packages` into `sys_path`; non-venv `canonicalize` -> `python_home = exe.parent()?.parent()?`.
     - the **exact refusal sentences of §2.3**, byte-for-byte and tested: invalid `BLASTIT_PYTHON` (`blastit <cmd>: BLASTIT_PYTHON (<path>) is not an executable file; unset BLASTIT_PYTHON, or point it at <venv>/bin/python3.14`), free-threaded `3.13t`/`3.14t` (`CPython <found> is free-threaded and cannot serve WSGI views; reinstall without --free-threaded (e.g. uv python install cpython-3.14)`), unsupported minor `3.12`/`3.15` (`CPython <found> is not supported for WSGI views (CPython 3.13 and 3.14 are supported; Python 3.12+ runs blastit check, fmt, and fold): run uv python install 3.14 && uv venv --python 3.14, or set BLASTIT_PYTHON`).
     - `libpython` search: `libpython3.<m>.so.1.0` then `libpython3.<m>.so` across `<python_home>/lib/<triplet>`, `<python_home>/lib64`, `<python_home>/lib`, `/usr/lib/<triplet>`, `/usr/lib64`, `/usr/lib`, `/usr/local/lib`; missing -> the `apt install libpython3.<m>` / `apk add python3` / `dnf install python3-libs` / `uv python install 3.<m>` sentence.
     - shim search per §2.3: `Deployment` = `<exe_dir>` only; `Development` = `$BLASTIT_PY_SHIM_DIR` -> `<exe_dir>` -> `<cargo_target_ancestor>/py-sapi`.
     - the **activation gate** as a pure predicate `activates(manifest, config)` (`!runs_own_executors(&manifest) || config.app.is_some()`, after `static_app_refusal`) so a PHP-only/static-only boot structurally never reaches discovery (§2.1). Keep the predicate in this crate; the *call site* stays in `crates/blastit` for `MB.42.2.1.3`.
     - a `DiscoveredPython { interpreter, py_minor, python_home, sys_path, libpython, shim }` sum type (no globals, `<=6` args).
   - **`loader.rs`** — `LoadedPy`: `dlopen("/proc/self/fd/<lib_fd>", RTLD_NOW | RTLD_GLOBAL)` -> resolve `blastit_py_abi_v1` -> `abi_version() == ABI_VERSION` and `py_minor()` matches the discovered minor -> `dlopen("/proc/self/fd/<shim_fd>", RTLD_NOW | RTLD_LOCAL)`; expose `vtable()` and `hooks() -> PyForkHooks`. Refusals are typed and quoted; `dlopen` error text goes through `dlerror` once, never `unwrap`. Also the `.dynsym` gate helper (§2.4): `libpython` must export `Py_InitializeEx`, `PyGILState_Ensure`, `PyOS_BeforeFork` and its `DT_SONAME` must not match `*t.so*` — reuse `blastit_sys::elf`, do not write a second ELF reader.
   - Tests: discovery order with real tempdir fixtures (venv with `pyvenv.cfg`, no-venv, missing `BLASTIT_PYTHON`, a `3.13t` path, a `3.12` path), both directions of the activation predicate, `.dynsym` gate on a real `libpython3.1x.so` when the host has one (skip with a recorded reason otherwise), and a `loader` test that `dlopen`s a real `libpython` **only** when `BLASTIT_PY_SAPI_TEST_LIBPYTHON` names one — never a `Mock*` shim.
4. **No changes under `crates/blastit/`** in this slice: `stage0/project_python.rs` keeps compiling and keeps its tests green until `MB.42.2.1.3` moves the call site.

# Acceptance
- `cargo test -p blastit-sys -p blastit-py-sapi -p blastit-view-php`
- `cargo test -p blastit-py-sapi abi` (layout + `py_minor` encoding) and `cargo test -p blastit-py-sapi discovery` (both refusal sentences byte-for-byte)
- `cargo clippy -p blastit-sys -p blastit-py-sapi -p blastit-view-php --all-targets --locked -- -D warnings`
- `make fast-check`
- `uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.1.1` -> clean (no existing protected test edited)

# Notes
- **1 Cargo writer**: this node is the only Cargo/`Cargo.lock`/`target/` writer while active; never run two cargo builds in parallel, and do not let another session build while you edit `Cargo.toml`.
- Never write `/home/remy/.local` or `/usr/bin/blastit`; never install or modify host PHP/Python packages — any package/runtime test runs in a container (`uv python install` into the uv cache is fine).
- Zero globals, zero `OnceLock`/`static Mutex`, zero `#[cfg(test)]` env seams, zero mock traits; dependencies arrive through constructors and typed values.
- `<500` prod LOC per file, `<=6` args per function; search the workspace before writing a helper (extend `blastit_sys::elf`, `blastit_sys::fd`).
- No `unwrap()`/`expect()` on untrusted input (paths, `fstat` results, `dlerror` text); every refusal is a typed error with a sentence.
- Do **not** modify `crates/*/tests/critical_*.rs` or `python/tests/critical/`; out-of-scope finds go to `## Follow-ups` in `RESULT.md`.
- `crates/blastit-py` and `crates/blastit-dbproxy` must keep compiling until `MB.42.2.2` (their deletion is not this slice).
