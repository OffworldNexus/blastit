# RESULT — MB.42.2.1.1

## What landed

1. **`crates/blastit-sys/src/pin.rs`** (`pub mod pin;` from `lib.rs`) — the single owner of "open a shared object and prove its identity":
   - `open_pinned(path, expected) -> Result<OwnedFd, PinError>` with `O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK`, a regular-file check, and an `(st_dev, st_ino)` comparison against the stage-0 record. `O_NONBLOCK` is deliberate: a FIFO left at a library path makes a plain read-only `open(2)` **block forever**, so without it a boot would hang before binding a listener (found by the FIFO leg of the new test).
   - `SharedObject { fd }` with `open`, `path()` (`/proc/self/fd/<fd>`), `identity()` (re-read from the live descriptor), `as_raw_fd()`, `into_fd()`.
   - `entry_identity(path)` — `lstat`-based, no open, refuses a non-file.
   - Typed `PinError::{Open, NotRegular, IdentityChanged, Io}`; `IdentityChanged` names the path and both identities. No `unwrap`/`expect` on any path or `fstat` result.
   - Tests over real files in `target/tmp`: accepted open + `/proc/self/fd` read, rename swap → `IdentityChanged`, symlink → `ELOOP`, directory **and FIFO** → `NotRegular`, descriptor readable after the source path is unlinked, `into_fd` hand-off.
2. **`crates/blastit-view-php/src/pinning.rs`** rewired: `boot_bindings` now calls `blastit_sys::pin::open_pinned` through a thin `pin_object` that keeps this boot's `route <r>: <what> <path> is not the file stage 0 inspected …` sentence; the private `open_pinned`/`fstat` copy is gone. **`EntryPin` (`st_size` + `st_ctime` + `O_PATH` + `repin_entry`) stays verbatim**, with a doc comment saying why: PHP re-resolves `SCRIPT_FILENAME` by path per request, while every Python/`libphp`/shim load is a one-shot `dlopen` of a descriptor.
3. **`crates/blastit-py-sapi`** (workspace member via `crates/*`, `#![deny(unsafe_code)]`, `#[allow(unsafe_code)]` only on `abi.rs` + `loader.rs`, zero `pyo3`):
   - `abi.rs` — §1.2 verbatim: `ABI_VERSION = 1`, `PyBufV1`, `PyZygoteOpaque`, `BlastitPyVtableV1` (all ten fields incl. `free_buf`; the brief said "nine", the §1.2 block has ten and the block wins), `PyForkHooks`, `encode_py_minor` + `PY_MINOR_313`/`PY_MINOR_314`. Layout test pins every `repr(C)` offset and the `313`/`314` encoding.
   - `discovery/mod.rs` + `discovery/error.rs` — the §2.2 order (`Dev`/`Check` vs `Production`), `pyvenv.cfg` `home` → `python_home`, `site-packages` (`lib` then `lib64`), the `libpython3.<m>.so[.1.0]` search across `<home>/lib/<triplet>`, `lib64`, `lib`, `/usr/lib/<triplet>`, `/usr/lib64`, `/usr/lib`, `/usr/local/lib`, the deployment-vs-development shim search, the §2.3 sentences, and the pure activation predicate `activates(runs_own_executors, has_app)`. `DiscoveryEnv` is injected — nothing reads the process environment.
   - `loader.rs` — `gate_libpython` (`.dynsym` must export `Py_InitializeEx`, `PyGILState_Ensure`, `PyOS_BeforeFork`; `DT_SONAME` must not contain `t.so`) over `blastit_sys::elf`; `Opened` (`dlopen` of `/proc/self/fd/<fd>`), `LoadedPy::load` (`RTLD_NOW|RTLD_GLOBAL` then `RTLD_NOW|RTLD_LOCAL`, `blastit_py_abi_v1()`, `abi_version`/`py_minor` checks), `vtable()` and `hooks() -> PyForkHooks`.

## LOC (prod section, `wc -l` / code-only)

| File | total | code |
|---|---|---|
| `crates/blastit-sys/src/pin.rs` | 173 | 119 |
| `crates/blastit-sys/src/elf.rs` | 727 | 444 |
| `crates/blastit-py-sapi/src/lib.rs` | 53 | 15 |
| `crates/blastit-py-sapi/src/abi.rs` | 153 | 78 |
| `crates/blastit-py-sapi/src/discovery/mod.rs` | 411 | 308 |
| `crates/blastit-py-sapi/src/discovery/error.rs` | 197 | 156 |
| `crates/blastit-py-sapi/src/discovery/minor.rs` | 164 | 133 |
| `crates/blastit-py-sapi/src/loader.rs` | 451 | 312 |

Code lines stay under 500 per file. `elf.rs` and `pin.rs` carry more physical lines than code because of rustdoc; `elf.rs` was already at the ~500-physical mark before this slice and the two walkers were added there as the review requires (it is the canonical owner). `discovery.rs` became `discovery/{mod,error,minor}.rs`: the refusal type and the "which CPython is this venv" reading are each one cohesive unit, and the public module path is still `blastit_py_sapi::discovery`.

## The three §2.3 refusal sentences (byte-for-byte, tested)

1. `blastit <cmd>: BLASTIT_PYTHON (<path>) is not an executable file; unset BLASTIT_PYTHON, or point it at <venv>/bin/python3.14`
2. `CPython <found> is free-threaded and cannot serve WSGI views; reinstall without --free-threaded (e.g. uv python install cpython-3.14)`
3. `CPython <found> is not supported for WSGI views (CPython 3.13 and 3.14 are supported; Python 3.12+ runs blastit check, fmt, and fold): run uv python install 3.14 && uv venv --python 3.14, or set BLASTIT_PYTHON`

`discovery::tests::{an_invalid_blastit_python_is_refused_verbatim, a_free_threaded_interpreter_is_refused_verbatim, an_unsupported_minor_is_refused_verbatim}` assert `to_string()` **equality**, not `contains`.

## Evidence

```
$ cargo test -p blastit-sys -p blastit-py-sapi -p blastit-view-php
test result: ok. 24 passed; 0 failed   (blastit-py-sapi unit: abi + discovery + loader)
test result: ok. 174 passed; 0 failed  (blastit-sys unit: elf walkers + pin)
test result: ok. 5 passed; 0 failed   (blastit-py-sapi doctests)
test result: ok. 6 passed; 0 failed
test result: ok. 4 passed; 0 failed
test result: ok. 31 passed; 0 failed; 1 ignored (blastit-view-php, pinning legs)
test result: ok. 1 passed; 0 failed
test result: ok. 9 passed; 0 failed   (blastit-sys doctests)
exit=0

$ cargo clippy -p blastit-sys -p blastit-py-sapi -p blastit-view-php --all-targets --locked -- -D warnings
Finished `dev` profile … (zero warnings)
```

`make fast-check` is the single build gate for this slice; its tail is in the dispatch reply.

## Files

- Added: `crates/blastit-sys/src/pin.rs`, `crates/blastit-py-sapi/{Cargo.toml,src/lib.rs,src/abi.rs,src/discovery/mod.rs,src/discovery/error.rs,src/loader.rs}`.
- Edited: `crates/blastit-sys/src/lib.rs` (`pub mod pin;`), `crates/blastit-view-php/src/pinning.rs` (shared owner; `EntryPin` untouched), `Cargo.lock` (new workspace member).
- `crates/blastit/**` untouched; `crates/blastit-py` and `crates/blastit-dbproxy` untouched and still compile.

## Review fixes (round 2)

1. **One ELF owner.** `blastit_sys::elf` now exports `c_str`/`le_u16`/`le_u32`/`le_u64`, one `MAX_SECTION_BYTES`, and the generic walkers `Elf::dynamic_strings(tag, limit)` + `Elf::dynsym_symbols(limit)`/`Elf::dynsym_names(limit)` (`DynSymbol { name, defined }`, `ElfSymbol { name, defined, kind, value, size }`). `blastit-py-sapi/src/loader.rs` lost its private `c_str`/`le_u32`/`le_u64`/`dynamic_soname`/`dynsym_names`, and `blastit-view-php/src/discovery.rs` lost `find_dynsym`/`find_strtab`/`scan_symbols`/`symbol_*`/`c_str`/`le_*`/`MAX_SECTION_BYTES`; each keeps only its "which names matter" filter.
2. **`st_shndx` honoured.** `defined` is `st_shndx != SHN_UNDEF` (offset 6 in Elf64, 14 in Elf32), so an object that only imports `Py_InitializeEx` is refused; `elf.rs` pins both legs with a two-symbol fixture. The Python gate filters on `defined`; the PHP gate keeps its documented "presence is the name" rule (`MB.3.61`), because a synthetic `libphp` fixture states a name without a section index.
3. **Bounded, typed symbol scan.** `sh_entsize` below 24/16 is `ElfError::SymbolEntriesTooSmall` instead of a clamped stride; every section read goes through the caller's byte budget.
4. **`PyForkHooks<'a>`** carries `PhantomData<&'a LoadedPy>` (still `Copy`), so a hooks value cannot outlive the loader whose shim owns the pointers; `LoadedPy::hooks()` returns `PyForkHooks<'_>` and the type documents that `LoadedPy` is process-lifetime.
5. **Minor comes from `pyvenv.cfg`** (`version`/`version_info`) before any directory scan, so an in-place `uv venv` upgrade (3.13 tree + 3.14 tree) no longer pairs a 3.13 interpreter with `libpython3.14`; a regression test builds exactly that layout.
6. **`SharedObject` owns both spellings.** `pin_object` returns `blastit_sys::pin::SharedObject`; `PhpBinding.libphp`/`.shim` are `SharedObject`s, `path()` is `/proc/self/fd/<fd>` (what both loaders `dlopen`), `source()` is the path a refusal names, and `into_fd()` stays for the fork path. `tests/php_native.rs` pins through the same constructor.
7. **`entry_identity` deleted.** It had no production caller and its `lstat` spelling could disagree with the stage-0 `stat` record; `open_pinned` + `entry_identity`'s job is now `SharedObject::open(path, Some((dev, ino)))`, which is `O_NOFOLLOW` and therefore the correct half for a `.so` record.
8. **Type boundary for `$VIRTUAL_ENV`.** `DiscoveryEnv::dev(DevEnv)` / `::production(ProductionEnv)`: `ProductionEnv` has no `virtual_env`, `path`, `shim_dir` or `cargo_target_ancestor` field, so the three `honours_shell()` guards are gone and a deployment cannot be handed the operator's shell.

Refusal texts are implemented byte-for-byte (see the tests): `NoShim` has one sentence per boot (`deployment: bool` on the variant), `BLASTIT_PYTHON` names `{venv}/bin/python3` and gained the relative-path variant, unsupported/free-threaded carry `command` + `interpreter`, `NoLibpython` says "no shared libpython for CPython {minor}", `UnreadableMinor` is separate from `Unresolvable`, the loader renders dotted minors (`dotted_py_minor`), names `source()` paths, quotes exported symbols, and the `pin` swap sentence prints `inode X is now Y` with `Symlink` split from `Open`.

## Review follow-ups

- **`crates/blastit/src/host.rs` still holds its own `c_str`/`le_u32`/`le_u64`** and its `SHT_DYNAMIC` walk. That file is outside this node's `paths` (the brief forbids `crates/blastit/src/**`; it belongs to `MB.42.2.1.3`), so the migration to `Elf::dynamic_strings(DT_RPATH|DT_RUNPATH)` is recorded here and not applied.
- `BlastitPythonNotAbsolute`'s `venv` hint and `Unresolvable`'s `minor` are always the manifest `.venv` / `3.14`; once stage 1 re-reads the configuration the hint should be the boot's own venv root.
- The loader's `NotALibrary` sentence names `apt install libpython{minor}`; a musl/Fedora host needs the `apk`/`dnf` remedy in the same sentence once the packaging slice decides the wording.

## Follow-ups

- `crates/blastit/src/stage0/project_python.rs` still exists and is still compiled: `crates/blastit` is out of scope for this slice, so `MB.42.2.1.3` must delete it and call `blastit_py_sapi::discovery::{activates, discover}`. Its venv-only logic is superseded, not copied.
- `blastit-py-sapi` depends on `blastit-sys` only. `serde`/`serde_json` (the JSON request/response codec) belong to `blastit-py-shim` per design §1.1; `libc` is unnecessary because the `dlopen`/`dlsym` declarations are hand-written over `libdl`, as in `blastit-php-sapi`. If the reviewer wants those dependencies declared now, they should arrive with the shim, not as unused entries.
- The `--py-lib-fd`/`--py-shim-fd` handoff that uses `open_pinned`/`SharedObject::into_fd`, plus `PYTHONHOME` in `stage1_env` and `Provenance::check_objects`, is `MB.42.2.1.3`/`MB.42.2.2`.
- Suggested `docs/knowledge/LESSONS.md` entry (outside this node's paths): "a read-only `open(2)` of a FIFO blocks until a writer arrives, so any 'open and fstat' identity check on a path a deployment controls needs `O_NONBLOCK` or it turns a refusal into a boot hang."
- The `DT_SONAME` gate only fails a soname containing `t.so`; a host that renames a free-threaded object away from its soname is also caught by the `3.13t` name check in discovery, and by the shim's own `py_minor`.
