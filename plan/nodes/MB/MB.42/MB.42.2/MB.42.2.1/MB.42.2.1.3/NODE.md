---
id: MB.42.2.1.3
title: "Stage-0 discovery call site + fd handoff + PyForkHooks (no stage-1 dlopen)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.2.1.2, MB.42.2.0]
paths: [Cargo.toml, Cargo.lock, "crates/blastit/**", "crates/blastit-py-sapi/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T17:21"
updated: "2026-11-19T17:21"
---

# Goal
Move the Stage-0 Python call site onto `blastit-py-sapi::discovery`, pin `libpython` + the shim as descriptors, hand both fds to Stage 1 across `execve`, and remove every `pyo3::ffi::PyOS_*Fork` call from `crates/blastit` — so the binary stops carrying an interpreter *object* and starts carrying an *fd pair* plus three fn pointers. **No stage-1 `dlopen` yet**: Stage 1 adopts, re-sets `FD_CLOEXEC`, verifies, and stops. Per `docs/design/universal-distribution.md` §2.1–§2.4, §1.2 (`PyForkHooks`, zero-global fork wiring) and §1.2.1 (the fork/zygote/GIL ordering pinned by `MB.42.2.0`).

# Exact Deliverables
1. **`crates/blastit/src/stage0/project_python.rs` becomes a call site, not an owner** (<200 prod LOC): delete the interpreter-order/`pyvenv.cfg`/`site-packages`/`libpython`-search logic moved by `MB.42.2.1.1`; keep only
   - the **activation gate** (§2.1): discovery runs **only** when `!runs_own_executors(&manifest) || config.app.is_some()`, evaluated after `static_app_refusal`; a PHP-only or static-only boot returns before any search, open, or `dlopen` — structurally, not by a flag;
   - `blastit_sys::pin::open_pinned` on `libpython` and the shim (record the `(st_dev, st_ino)` pair in the config document Stage 0 writes), the `.dynsym`/`DT_SONAME` gate, and in `Serve` `Provenance::check_objects((libpython, shim))` + `check_trees((python_home, venv_root?, manifest_dir))`;
   - `PYTHONHOME=<python_home>` in `stage1_env` unless the app already set it;
   - clearing `FD_CLOEXEC` on exactly `(libpython_fd, shim_fd)` **only** for the Stage-1 `execve`, and passing `--py-lib-fd=<N> --py-shim-fd=<M>`.
2. **`Config.python` grows the handoff record**: `Some(PythonHandoff { py_minor, python_home, sys_path, lib: (RawFd, u64, u64), shim: (RawFd, u64, u64) })`, serialized into the Stage-1 configuration document. Read it back in stage 1 the same way `php_probe_timeout_ms` is read (`LESSONS.md`: a new stage-1 knob **cannot** come from the environment, because the stage-0→stage-1 `execve` strips every `BLASTIT_*`).
3. **`crates/blastit/src/stage1/` adoption**: `Adoption` (`stage1/adopt.rs:135`) gains `py_fds: Option<(RawFd, RawFd)>`; the `--py-lib-fd`/`--py-shim-fd` values are validated as owned descriptors, `fstat` re-checked against `Config.python`, and `FD_CLOEXEC` **re-set** immediately (they must not survive a further `execve`). The `test_swap_pause` re-`fstat` point lives here so `MB.42.2.1.4` only adds the `dlopen`; a mismatched `(st_dev, st_ino)` is a hard refusal with a sentence naming both identities.
4. **`crates/blastit/src/forking.rs` loses `pyo3` entirely** (`forking.rs:104/124/132`): it takes an injected `Option<PyForkHooks>` (`Copy`, from `blastit_py_sapi::abi`) — `None` for a PHP/static-only image, `Some(hooks)` once a Python runtime exists. `fork(interpreter: bool) -> io::Result<Fork>` becomes `fork(hooks: Option<PyForkHooks>)`; the existing refusal text about "an image that reports an interpreter it does not have" and "a `PyOS_BeforeFork` with no interpreter" is preserved (its tests at `forking.rs:158` stay green)
5. **`crates/blastit/src/visibility.rs` and `supervise.rs` stop naming pyo3**: `visibility::*` and `Supervisor` receive the hooks (and nothing else Python-shaped) from a single `PyRuntime`-free holder; `visibility.rs` currently takes `Python<'_>` parameters — replace them with the hooks/handoff values it actually uses. `supervise.rs` keeps `app: Py<PyAny>` **until `MB.42.2.1.4`**; this slice only removes the fork-call plumbing and the `Python<'_>` threading that the fork path needed.
6. **`crates/blastit/Cargo.toml`**: `python = ["dep:blastit-py-sapi"]` added while `pyo3` + `blastit-view-python` remain (the cut-over is `MB.42.2.1.4`, so this slice must not break the current supervisor). `Cargo.lock` updated.
7. **Finish the `blastit_sys::elf` dedup in `crates/blastit/src/host.rs`** (deferred out of `MB.42.2.1.1`, which may not touch `crates/blastit/src/**`): `host.rs` still carries its own `c_str`/`le_u32`/`le_u64` and a private `SHT_DYNAMIC` walk (`dynamic_search_paths`, `host.rs:683-780,828-849`). Delete them and use `blastit_sys::elf::{dynamic_strings, dynsym_names, c_str, le_u*}` so there is exactly **one** ELF reader in the workspace; keep only host's "which names/paths matter" filter at the call site. `host.rs` is 1,999 lines — take the opportunity to move the removed walkers' neighbours into a cohesive submodule if it keeps it under the `<500` prod-LOC rule.
7. **Tests (Stage-0 owned, no `Mock*Server`)**: `entrypoint php` and a static-only manifest never open `libpython` (assert by driving Stage 0 with an unreadable/absent `libpython` and a normal boot — it must succeed); the hash/identity refusals fire (`st_dev`/`st_ino` mismatch on `--py-lib-fd` after a real swap under a real tempdir); `PYTHONHOME` lands in `stage1_env` only when Python is active; `CFG` round-trip of `Config.python`. `Adoption.py_fds` survives a `dev` `exec_cycle` re-exec (which fds survive, which are re-pinned — per §1.2.1).

# Acceptance
- `cargo test -p blastit --lib --locked` (stage0 + stage1 adoption + forking + visibility units)
- `cargo test -p blastit --locked --test critical_dev_and_fail_closed --test critical_php_embed` (protected suites stay green and **unedited**)
- `cargo test -p blastit-py-sapi -p blastit-sys`
- `cargo clippy -p blastit --all-targets --locked -- -D warnings`
- `make fast-check`
- `uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.1.3` -> clean

# Notes
- **1 Cargo writer**: this node is the only Cargo/`Cargo.lock`/`target/` writer while active; never run two cargo builds in parallel, and never build while `make check`'s harnesses are booting `target/debug/blastit` (`LESSONS.md`, `MB.25.2`).
- Never write `/home/remy/.local` or `/usr/bin/blastit`; never install or modify host PHP/Python packages — any package/runtime test runs in a container (`uv python install` into the uv cache is fine).
- Zero globals, zero `OnceLock`/`static Mutex`, zero `#[cfg(test)]` env seams, zero mock traits; the three hooks arrive through `forking`/`Supervisor` constructors, never through `env::var` or a `static`.
- `<500` prod LOC per file (note `supervise.rs`/`visibility.rs` are already over — you are making them smaller, not larger), `<=6` args; search the workspace before writing a helper.
- No `unwrap()`/`expect()` on untrusted input (fds, `fstat`, the config document read back in stage 1).
- Do **not** modify `crates/*/tests/critical_*.rs` or `python/tests/critical/`; out-of-scope finds go to `## Follow-ups` in `RESULT.md`.
- `crates/blastit-py` and `crates/blastit-dbproxy` must keep compiling until `MB.42.2.2`.
