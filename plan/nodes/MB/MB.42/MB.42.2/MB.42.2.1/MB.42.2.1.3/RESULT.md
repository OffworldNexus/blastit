# MB.42.2.1.3 — Stage-0 discovery call site, fd handoff, `PyForkHooks`

## What moved

- **`crates/blastit/src/stage0/project_python.rs` is now a call site** (~330 LOC incl. tests, <200 prod): the activation gate `plan()` (`blastit_py_sapi::discovery::activates` over `manifest.can_boot_without_app()` + `config.app.is_some()`), the process environment read once into `DiscoveryEnv` (`dev`/`check` gets `BLASTIT_PYTHON`, `$VIRTUAL_ENV`, `BLASTIT_WHEEL_PYTHON`, `PATH`, `BLASTIT_PY_SHIM_DIR`, `target/py-sapi`; `serve` gets the absolute-only `ProductionEnv`), `SharedObject::open` on `libpython` + the shim, `gate_libpython` (`.dynsym`/`DT_SONAME`), `Provenance::check_objects`/`check_trees` in `Serve`, and `python_home()` for `stage1_env`. The venv-order / `pyvenv.cfg` / `site-packages` / `libpython`-search / shim-search code is deleted. Remaining call sites: none outside this file (`check.rs:1873` is a test name, not a caller).
- **`Config.python: Option<PythonHandoff>`** (`config/types.rs`, `settings.rs`) carries `py_minor`, `python_home`, `sys_path`, `lib: (RawFd,u64,u64)`, `shim: (RawFd,u64,u64)`. `document.rs` writes a `python` object (`null` when inactive) and `wire/{field,read,mod}.rs` read it back with the same field-name sentences; it never rides the environment.
- **Stage 0 → stage 1 `execve`:** `exec_stage1` and `exec_stage1_dev` append `--py-lib-fd=N --py-shim-fd=M` and clear `FD_CLOEXEC` on exactly those two descriptors (`stage0/fork_exec.rs`); `stage1_env` adds `PYTHONHOME` from `Config.python` only, and only when the app has not set one.
- **Stage 1 adoption** (`stage1/adopt.rs`): `Adoption.py_fds: Option<(RawFd, RawFd)>` read from `py_lib`/`py_shim` (both-or-neither, descriptors validated open). `adopt_py_fds()` runs **right after `test_swap_pause`**, prefers `Adoption.py_fds` over `--py-*`, re-`fstat`s both against `Config.python`, re-sets `FD_CLOEXEC` immediately, and refuses a moved `(st_dev, st_ino)` with the existing prose (`… was replaced after blastit inspected it (inode X is now Y); restore the file and restart blastit`).
- **`supervise::Adoption` is the single `OwnedFd` owner** for the pair: `Parts.py_fds` → `Adoption.py_fds`; `adoption_document` writes `py_lib`/`py_shim`; `exec_cycle` adds both to `keep` and clears `FD_CLOEXEC` for the re-exec. `stage1::Adoption` keeps numbers only (§1.2.1 item 6).
- **`forking.rs` names no `pyo3`:** `fork(hooks: Option<PyForkHooks<'_>>)` and `fork_for(protocol, hooks)`; the mismatch decision is a pure `agrees()` with the two preserved refusals ("…reports none, so the fork would skip `PyOS_BeforeFork`", "…must run `PyOS_BeforeFork` and its two matching halves"). Tests at the old `forking.rs:158` still pass (they call `agrees`).
- **`visibility::*` and `supervise.rs` fork paths take hooks:** `probe`/`unreadable`/`unexecutable`/`writable` take `Option<PyForkHooks<'_>>`; `Parts`/`Supervisor` carry `hooks` and `fork_view` derives both `fork_for(protocol, self.hooks)` and its `ForkProtocol`. `supervise.rs` keeps `app: Py<PyAny>` (cut-over is `.1.4`). `crate::py::runtime_hooks(live)` is the one bridge to the composition root (`PyForkHooks::hosted`, an explicit transitional constructor in `blastit-py-sapi/src/abi.rs`).
- **`crates/blastit/Cargo.toml`:** `python = ["dep:pyo3", "dep:blastit-view-python", "dep:blastit-py-sapi"]` (added, not replaced); `blastit-py-sapi` optional. `Cargo.lock` updated.
- **`host.rs` ELF dedup (deliverable 6):** the private `c_str`, `le_u32`, `le_u64`, `dynamic_strtab`, `dynamic_tag` and the `SHT_DYNAMIC` walk are deleted; `dynamic_search_paths` is now two `elf.dynamic_strings(DT_RPATH|DT_RUNPATH, MAX_DYNAMIC_STRTAB)` calls plus the "which paths matter" filter. `fn c_str|SHT_DYNAMIC` hits in `host.rs`: **7 → 4**, and the four remaining are a doc comment (`SHT_DYNAMIC array`), the test module's `use blastit_sys::elf::{SHT_DYNAMIC, SHT_STRTAB}` and two comments/fixture writes — no production walker. One ELF reader in the workspace (`blastit_sys::elf`).

## Tests (all new/changed legs named)

Stage-0 / stage-1 unit tests:
- `stage0::project_python::tests::a_php_only_or_static_boot_never_plans_a_search` — `entrypoint php` and static-only manifests are structurally inactive.
- `stage0::project_python::tests::python_home_is_added_only_when_python_was_discovered` — no handoff ⇒ no `PYTHONHOME`; handoff ⇒ it lands; the app's own `PYTHONHOME` always wins.
- `stage0::project_python::tests::an_unreadable_blastit_python_never_matters_when_python_is_inactive` — the explicit-bad-variable refusal is the discovery's own.
- `config::tests::the_json_document_round_trips` — now carries `python`, asserts `python.lib_fd`/`python.shim_ino`, and `Config::from_json(Config::to_json()) == config`.
- `config::tests::the_document_key_set_is_the_stage_0_contract` — key set includes `python`.
- `stage1::adopt::tests::a_replaced_libpython_descriptor_is_refused_by_identity` — real tempdir, a real second inode; asserts the prose sentence names both inodes and no internal stage.
- `stage1::adopt::tests::an_adoption_pair_is_reverified_and_cloexec_is_restored` — `Adoption.py_fds` wins over `--py-*`, both `FD_CLOEXEC` bits are back.
- `supervise::tests::the_adoption_document_round_trips_through_the_reader` — `exec_cycle`'s document now carries `py_lib`/`py_shim` and stage 1's reader returns `Some((lib, shim))` unchanged (the fd-survival leg; the actual `execve` is `.1.4`'s).

Real-process legs (no mocks):
- `crates/blastit/tests/python_activation_gate.rs::a_static_only_boot_never_opens_libpython` — real `blastit dev`, static manifest, `BLASTIT_PYTHON=/definitely/not/a/python` + bogus `VIRTUAL_ENV`, still `200`.
- `…::a_python_boot_with_the_same_unreadable_interpreter_is_refused` (`python` feature) — same env with a Python app exits `1` with the `BLASTIT_PYTHON … is not an executable file` sentence.
- `crates/blastit/tests/embed_venv_sys_path.rs` now creates the project venv's `bin/python3` (discovery resolves the venv from the interpreter) and skips cleanly when `PYO3_PYTHON`/`target/py-sapi` are absent.

## Review fixes (review-arch + review-ux)
- **`PYTHONHOME` is the linked interpreter's own again.** `project_python::python_home(install, app_env)` returns `option_env!("BLASTIT_PYTHON_HOME")` only for `Install::Binary` (never the discovered prefix, never a wheel), marked `// transitional (MB.42.2.1.4)`; `Config.python.python_home` stays in the document for the shim's `init_paths`.
- **Minor agreement restored:** `agree_minor` refuses a project venv built for another CPython, naming both and `uv venv --python {linked}`; the test `a_venv_for_another_minor_is_refused_naming_both`.
- **One refusal owner:** `logs::fail_sentence` and `dev::dev_refuse` print the discovery sentence verbatim (the `blastit {cmd}:` head lives only in `DiscoveryError`); used at the two Python call sites.
- **All nine refusals carry `blastit {cmd}:`:** `command` added to `Unresolvable`, `UnreadableMinor`, `NoLibpython`, `NoShim`; threaded through `minor.rs` and `libpython()`/`shim()`.
- **No swallowed `NoInterpreter`:** the arm is gone; a linked build with no candidate env now falls back to the build's recorded interpreter (`BLASTIT_PYTHON_HOME`) as the **last dev** candidate (`BLASTIT_WHEEL_PYTHON` position, after `<manifest>/.venv`), so §2.2's sentence is reachable and a project venv still wins.
- **Fail-closed handoff:** `adopt_py_fds` returns `Result`; a record without a descriptor and a descriptor without a record each refuse (tests `a_record_without_a_descriptor_is_refused`, `a_pinned_pair_without_a_record_is_refused`), and `boot.rs` does the single `fail`.
- **No production test scaffolding:** `virtual_env` deleted; the production guarantee is tested by driving `DiscoveryEnv::production` at a real tempdir `.venv` + shim.
- **`Boot { Dev, Serve }`** replaces the `&str` command comparison (the `Provenance` gate cannot be turned off by a typo).
- **Second `pyvenv.cfg` reader removed:** `DiscoveredPython` grew `venv_root` and `release`; `project_python` reads neither file itself.
- **ELF semantics restored:** `blastit_sys::elf::dynamic_strings` is strict UTF-8 (`ElfError::DynamicString`), `host.rs` now walks both tags in **file order** through `dynamic_strings_any`, maps the strict refusal to the old `DT_RPATH/DT_RUNPATH` sentence, and the stale "walk below stays here" paragraph is gone.
- **`adopt_py_fds`/`verify_py_fd` and `Options.py_*` are `#[cfg(feature = "python")]`;** the blanket `allow(unused_imports)` is gone. Handoff refusals now say "pinned …; restart blastit" with no stage names.
- **`runtime` keeps the release** (`3.14.10` from `pyvenv.cfg`, else `3.14`).
- **Coverage:** driven PHP-only leg (`a_php_only_boot_never_reaches_the_python_discovery`), `stage1_env` exercised through the real dev boots, the venv test's skips are loud (`SKIPPED … make py-sapi`), the nested `python` object has `deny_unknown_fields` and its nine keys pinned (`an_unknown_python_member_is_refused_like_the_top_level`).

## Verification (tails)

- `cargo clippy -p blastit --all-targets --locked -- -D warnings` → `Finished` (0 warnings).
- `cargo clippy -p blastit --all-targets --features python --locked -- -D warnings` → `Finished`.
- `cargo test -p blastit --lib --locked` → `284 passed; 0 failed`.
- `cargo test -p blastit --lib --features python --locked` → `305 passed; 0 failed`.
- `cargo test -p blastit --locked --test critical_dev_and_fail_closed --test critical_php_embed` → `29 passed` (php suite) + the dev suite green.
- `cargo test -p blastit-py-sapi -p blastit-sys` → all green (`174`/`25`/`6`/…).
- `make fast-check` and `tools/critical_guard.py` tails are appended by the final gate run.

## Follow-ups

- **`.1.4`:** `dlopen(libpython, RTLD_GLOBAL)` → `dlopen(shim, RTLD_LOCAL)` → `blastit_py_abi_v1` → `init_paths` → `hooks = loaded.hooks()` → `preload`; then delete `PyForkHooks::hosted` and `py.rs`'s `pyo3::ffi::PyOS_*Fork` wrappers (the loader owns every hook), and drop `pyo3`/`blastit-view-python` from `crates/blastit`.
- **Transitional fallback:** a `python`-feature build whose shell names *no* interpreter falls back to `Config.python = None` (stage 1 still embeds the linked CPython), which is what keeps an `env -c`-style dev shell booting until `.1.4`. Delete this fallback together with the embedded supervisor.
- **`Config.python` does not carry the source paths**, so the identity refusal names the descriptor's `/proc/self/fd` readlink; if a path is wanted in the sentence, add `lib_source`/`shim_source` in `.1.4` when the loader reports refusals anyway.
- `supervise.rs`/`visibility.rs` remain over 500 prod LOC (both were before this slice and are smaller now); splitting `supervise` further is a separate node.
