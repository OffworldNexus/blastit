---
id: MB.36.5
title: "Extract crates/blastit-view-php (worker, SAPI, discovery, pinning, probe), delete WorkerRuntime/runtime.rs, and shrink stage0.rs by the PHP half"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.36.4]
paths:
  - "crates/blastit-view-php/**"
  - "crates/blastit-view/src/**"
  - "crates/blastit-view/tests/**"
  - "crates/blastit-view/Cargo.toml"
  - crates/blastit/src/php_view.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/tests/critical_php_backend.rs
  - crates/blastit/tests/critical_php_embed.rs
  - crates/blastit/tests/critical_loaded_files.rs
  - "crates/blastit-scheduler/src/scheduler.rs"
  - crates/blastit/Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-17T14:07"
---

# Goal

Create `crates/blastit-view-php`, move every PHP internal into it, delete the
`WorkerRuntime` trait, and reduce `crates/blastit-view` to transport only.

After this leaf the PHP crate is the only place that knows about `libphp`, the SAPI shim, the
per-minor ELF probes, `(st_dev, st_ino)` pinning, the probe child — and `crates/blastit` is
back to being an orchestrator.

# Context

- Design of record: `MB.36.1`'s `view-crates-architecture.md`, seam decision 5 (discovery in
  init vs worker in the view) and the transport split of `runtime.rs`, plus `ADR-0016`.
- Three moves, each mechanical:
  1. `crates/blastit-view/src/php.rs` (2558) → `blastit-view-php/src/worker.rs`
     (`PhpEmbedRuntime`, `PhpRuntimeConfig`, `PhpLoadedReport`, `serve_forever`,
     `is_serveable_entry`).
  2. `crates/blastit/src/php_view.rs` (1726) → `blastit-view-php/src/{fork,pinning,probe}.rs`
     (`fork_php_view`, `child_main`, `runtime_config`, `PinnedEntryRuntime`,
     `boot_bindings`/`open_pinned`/`pin_entry`/`verify_entry`,
     `probe_bindings`/`probe_route`/`probe_child`/`await_probe`/`kill_probe`/`own_process_group`,
     `load_and_shut_down`).
  3. The PHP half of `crates/blastit/src/stage0.rs` → `blastit-view-php/src/discovery.rs`:
     `php_boot_gate`, `php_boot`, `php_boot_in`, `discover_php`, `resolve`, `first_existing`,
     `installed_unsupported`, `version_of`, `LIBPHP_PROBES`, `PhpSearch`, `LibphpProbe`,
     `shim_name`, `abi_tuple`, `php_probe_refusal`, `php_refusal_message`, `present_refusal`,
     `php_version_string`, `version_prefix`, `php_runtime_line`. Consequences:
     `stage0.rs` loses roughly 1500 lines, and **init** now calls
     `blastit_view_php::discovery::…` (a pure-`std`/`rustix` module: no `dlopen`, no worker,
     no `libphp` symbol reachable from init).
- Delete `crates/blastit-view/src/runtime.rs` (1088). Split it by owner, per the design:
  `OutputStream`, `RequestIo`, `RequestEnd`, `OUTPUT_LINE_CAP` → `flow.rs`/`wire.rs`;
  `LoadedReport`, `LoadedReporter`, `LoadedTracker` → `blastit-view-python` (they install a
  Python audit hook); the `WorkerRuntime` trait itself **is deleted**, and
  `PhpEmbedRuntime::serve_forever` / the WSGI pool each own their loop. Update the stale
  `WorkerRuntime::recycle` mention in `crates/blastit-scheduler/src/scheduler.rs`.
- End state of `crates/blastit-view/Cargo.toml`: `blastit-frame`, `thiserror` and nothing
  else; **no `pyo3`** (also not in `[dev-dependencies]`), **no `blastit-php-sapi`**, no
  `python`/`php` features. `blastit`'s features become
  `python = ["dep:pyo3", "dep:blastit-view-python"]`, `php = ["dep:blastit-view-php"]`,
  `static = ["dep:blastit-view-static"]`.
- Protected files this leaf may relocate (its `paths` must name the **old** globs):
  `crates/blastit/tests/critical_php_backend.rs`, `critical_php_embed.rs`,
  `critical_loaded_files.rs`, `crates/blastit-view/tests/php_native.rs`. Every case moves or
  stays; none is dropped.

# Acceptance

```console
$ cargo test -p blastit-view --locked
$ cargo test -p blastit-view-php --locked
$ cargo test -p blastit-view-python --locked
$ make critical-guard BASE=develop NODE=MB.36.5
$ test ! -e crates/blastit-view/src/runtime.rs && echo "trait file deleted"
$ test -z "$(grep -rn 'WorkerRuntime' --include=*.rs crates/)" && echo "no WorkerRuntime anywhere"
$ test -z "$(grep -n 'pyo3\|blastit-php-sapi' crates/blastit-view/Cargo.toml)" && echo "transport crate is language-free"
$ test ! -e crates/blastit-view/src/php.rs && test ! -e crates/blastit/src/php_view.rs && echo "PHP left"
$ test -z "$(grep -rn 'libphp8\|LibphpProbe' crates/blastit/src/stage0.rs)" && echo "discovery left stage0"
$ test -z "$(grep -rn 'blastit_view::php' crates/)" && echo "no old module path"
$ make rs-no-python
$ make check
```
- `cargo tree -p blastit-view` is `blastit-frame` + `thiserror` only; `cargo tree -p
  blastit-view` and `-p blastit-view-php` show no `pyo3`, and `blastit-view-php` carries
  `blastit-php-sapi` with **no** link-time `libphp`.
- `make rs-no-python` proves the standalone binary still carries no `libpython`/`libphp`
  `DT_NEEDED` and no `RUNPATH` after the moves.
- `RESULT.md` pastes `make check`, `make rs-no-python`, `cargo tree` for both view crates,
  and `wc -l crates/blastit/src/stage0.rs` (expect a four-digit drop, not a cosmetic one).

# Notes

- `boot_bindings` re-opens exactly what stage 0 recorded; `pin_entry`/`verify_entry` are the
  `(st_dev, st_ino)` gate for `entry`. Neither the recording nor the verification may move to
  a crate that init does not link, and neither may lose its refusal message.
- `PinnedEntryRuntime` exists to re-verify the pin between requests: keep it, even though the
  trait it implemented is gone — it becomes a plain adapter the worker loop owns.
- Do not touch `SeccompRole`, `harden_child`, the privilege drop, or the probe's process-group
  handling in this leaf beyond the `use` paths the move forces.
