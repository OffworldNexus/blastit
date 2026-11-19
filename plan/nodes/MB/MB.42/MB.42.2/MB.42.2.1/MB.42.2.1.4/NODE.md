---
id: MB.42.2.1.4
title: "Stage-1 dlopen + PyRuntime supervisor cut-over + drop pyo3 (portable ELF)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2.1.3]
paths:
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "crates/blastit/**"
  - "crates/blastit-py-shim/**"
  - "crates/blastit-py-sapi/**"
  - "crates/blastit-view-python/tests/critical_loaded_files.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T17:21"
updated: "2026-11-19T20:06"
---

# Goal
Cut the binary over: Stage 1 `dlopen`s the pinned `libpython` and the pinned shim in the documented order, the supervisor drops its `Py<PyAny>` for the opaque zygote handle, reload syntax checks go through the vtable, and `blastit` stops depending on `pyo3`/`blastit-view-python` — so both `cargo build -p blastit` **and** `cargo build --workspace` yield a single binary with zero `libpython`, zero `libphp` and zero `RUNPATH` that still serves Python WSGI under CPython 3.13 and 3.14. Per `docs/design/universal-distribution.md` §1.1 (feature wiring, `build.rs` strip), §1.2 (vtable calls), §1.2.1 (ordering + `PyRuntime` handle), §7 R1–R5.

**Authorized protected-test edit (lead, 2026-11-19):** `crates/blastit-view-python/tests/critical_loaded_files.rs` may be **retargeted** in this node. Its `the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup` witness `include_str!`s `crates/blastit/src/stage1.rs` and counts `LoadedTracker::install` / `loaded.harvest(py)` / `CLOSE_AND_COLLECT` tokens that ADR-0017 §1.2.1 item 3 legitimately moved into the shim's `preload`. Retarget the `include_str!` and the token offsets at the new owner (`crates/blastit-py-shim/src/boot.rs`) and **keep the identical invariant**: exactly one install site, inside the dev-only gate, before the first application import and before the preload list; exactly one harvest after warmup and before the close-and-collect/freeze. Do **not** delete the test, `#[ignore]` it, weaken an assertion, or drop a leg.

# Exact Deliverables
1. **Stage-1 load order** (design §2.4 + §1.2.1, exactly this sequence): after the `(st_dev, st_ino)` re-`fstat` that `MB.42.2.1.3` added, and **after** `test_swap_pause`:
   `fork(http)` -> `fork(scheduler)` -> `dlopen("/proc/self/fd/<lib_fd>", RTLD_NOW | RTLD_GLOBAL)` -> `dlopen("/proc/self/fd/<shim_fd>", RTLD_NOW | RTLD_LOCAL)` -> `blastit_py_abi_v1()` -> `abi_version()`/`py_minor()` check -> `init_paths` -> `preload` (which installs `LoadedTracker` **pre-fork**, with `report_fd >= 0`) -> per-view `fork` through the `blastit-view` `spawn` seam. Both fds close right after `dlopen` unless `Adoption.py_fds` says the `dev` cycle owns them.
   The forking-before-load ordering is not cosmetic: it is what keeps `http` and `scheduler` from inheriting a half-initialised interpreter (`§1.2.1`).
2. **`PyRuntime` handle replaces every pyo3 type inside `crates/blastit`**: a small `Copy` value in `blastit_py_sapi::abi` carrying `(*mut PyZygoteOpaque, PyForkHooks, &'static BlastitPyVtableV1)`; `Supervisor`/`supervise.rs` and `visibility.rs` hold it, and **no pyo3 type may appear in any `crates/blastit` signature, struct field, or `cfg`** (`§1.2.1`). `supervise.rs:1679`'s `zygote::fork_view(py, ViewSpawn { app: &self.app, … })` becomes the opaque handle + view descriptor; the GIL discipline is the vtable's (`PyGILState_Ensure`/`Release` inside each entry), so `crates/blastit` never ensures the GIL.
3. **Reload syntax check through the vtable**: `supervise.rs:1076`'s direct `PyCode::compile` becomes `vtable.check_syntax(zygote, code, path, err)`, parent-side, no execution (`§1.2`), and the refusal text the `dev` reload path prints stays byte-identical (its suite is protected-adjacent — assert the sentence, not the impl).
4. **`crates/blastit/Cargo.toml`**: `default = ["python", "php", "static"]`, `python = ["dep:blastit-py-sapi"]`, **drop** `pyo3` and `blastit-view-python`. `crates/blastit/build.rs`: strip `python_home`/`rpath`/`PYO3_PYTHON` (keep `coverage` only) — a build-host `RUNPATH` is exactly the 0.2.0 defect (`LESSONS.md`, `MB.17`). `Cargo.lock` updated.
5. **`make` targets stop naming the old feature**: `rs-no-python`'s intent (`crates/blastit` links no interpreter) is now unconditional — keep the target, keep `scripts/check-portable-elf.sh` in it, and update its `cargo tree`/`grep` assertions so they prove `--no-default-features` pulls zero `blastit-py-sapi`, `blastit-view-php`, `blastit-view-static`, `pyo3` (design §1.1's `rs-no-default` promise), while the default build pulls `blastit-py-sapi` and **not** `pyo3`.
6. **New `crates/blastit/tests/critical_python_shim.rs`** (new critical file — adding is welcome, editing existing is not):
   - one binary serves a real WSGI app over a **real socket** under CPython **3.13** and under **3.14** (each leg asserts `200` + the app's body; zero `Mock*Server`/`StubPolicy`; per-leg `BLASTIT_PYTHON` and the matching `libblastit_py3<digits>-*.so` from `make py-sapi`);
   - a **PHP-only** and a **static-only** manifest boot and serve **without** `libpython` ever being opened (prove it by making the discoverable `libpython` path unreadable/absent for that leg, so an accidental open would fail loudly);
   - Stage 1 refuses an fd whose `(st_dev, st_ino)` was swapped after Stage 0 pinned it (real swap in a real tempdir, not a synthetic value);
   - a `dev` `exec_cycle` reload keeps serving (the `Adoption.py_fds` path) and a reload whose syntax is broken prints the same sentence as before.
7. **Portability proof**: `scripts/check-portable-elf.sh` must run on the artifact the build just produced, and must be satisfied by **both** builds. A workspace build inherits a member's feature request through Cargo feature unification (`LESSONS.md`: `blastit-py` enabling `blastit/python` re-added `NEEDED libpython3.X.so`), so this leg is the regression test for that exact bug — if `cargo build --workspace` is dirty, the remaining feature edge is `MB.42.2.2`'s to delete, and this slice must instead prove it via `crates/blastit-py` no longer requesting `blastit/python`.
8. **Retire the transitional `PYTHONHOME` mechanism** (carried out of `MB.42.2.1.3`): today stage 0 exports the *linked* interpreter's `BLASTIT_PYTHON_HOME` because the process still `Py_InitializeEx`es the CPython this binary links. Once the shim owns `Py_InitializeEx(0)`, `config.python.python_home` must reach CPython **only** through the shim's `init_paths` request, and stage 0 must stop exporting `PYTHONHOME` entirely (delete the `// transitional (MB.42.2.1.4)` path). The minor-agreement refusal added in `.1.3` stays: a venv on a different minor than the shim is still a named refusal, never a fatal `init_fs_encoding`. Note in `RESULT.md` that `docs/design/universal-distribution.md` §2's `PYTHONHOME` sentence now needs a one-line amendment (raise it for the lead to route to `architect`) — do **not** edit `docs/**` yourself.

# Acceptance
- `cargo build -p blastit --locked` then `scripts/check-portable-elf.sh target/debug/blastit` -> zero `libpython`, zero `libphp`, zero `RUNPATH`
- `cargo build --workspace --locked` then `scripts/check-portable-elf.sh target/debug/blastit` -> same result
- `cargo test -p blastit --locked --test critical_python_shim` (both minors; run after `make py-sapi`)
- `cargo tree -p blastit --locked -e normal | grep -c pyo3` -> `0`; `cargo tree -p blastit --no-default-features --locked -e normal | grep -cE 'blastit-py-sapi|blastit-view-php|pyo3'` -> `0`
- `cargo test -p blastit --locked --test critical_dev_reload --test critical_php_embed` (protected suites green and unedited)
- `make fast-check`
- `uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.1.4` -> clean

# Notes
- **1 Cargo writer**: this node is the only Cargo/`Cargo.lock`/`target/` writer while active; never run two cargo builds in parallel, and never build while `make check`'s harnesses are booting `target/debug/blastit` (`LESSONS.md`, `MB.25.2` — a relink mid-run makes a green suite red for a reason that is not your code).
- Never write `/home/remy/.local` or `/usr/bin/blastit`; never install or modify host PHP/Python packages — any package/runtime test runs in a container (`uv python install` into the uv cache is fine).
- Zero globals, zero `OnceLock`/`static Mutex`, zero `#[cfg(test)]` env seams, zero mock traits; the runtime arrives through `BootMode`'s composition root and constructors.
- `<500` prod LOC per file (you are shrinking `supervise.rs`/`visibility.rs`, never growing them), `<=6` args; search the workspace before writing a helper (the vtable is the helper).
- No `unwrap()`/`expect()` on untrusted input (fds, `fstat`, `dlopen`/`dlerror`, the shim's `err` buffer).
- Do **not** modify `crates/*/tests/critical_*.rs` or `python/tests/critical/`; out-of-scope finds go to `## Follow-ups` in `RESULT.md`.
- `crates/blastit-py` and `crates/blastit-dbproxy` must keep compiling until `MB.42.2.2` — its deletion, and the `--internal-json`/`_blastit.py` bridge, are that node's scope, not this one's.
