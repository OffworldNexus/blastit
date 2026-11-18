---
name: Rust standards
description: Mandatory standards for Rust in Blastit (`crates/*`). Covers True Constructor IoC, <500 LOC cohesion, protocol crate boundaries, zero globals/mock-traits, unsafe/error rules, MSRV 1.88, and `make fast-check` vs `make check`.
---

# Rust standards (Blastit)

Rust powers Blastit's supervisor, HTTP ingress, WASM validation gate, fair scheduler, language view workers, LSP, and capability proxies (`crates/*`).

## 1. Design & architecture rules

1. **Pure library & protocol crates first, thin composition root last.**
   - Parsing, policy evaluation, wire codecs, LSP, and scheduling belong in pure unprivileged crates (`blastit-manifest`, `blastit-lsp`, `blastit-events`, `blastit-frame`, `blastit-sqlpolicy`, `blastit-env`, and upcoming `blastit-ingress`, `blastit-wasm`, `blastit-proxy-*`) testable in milliseconds without root or Docker.
   - Isolate language runtimes in dedicated `blastit-view-*` crates (`blastit-view`, `blastit-view-php`, `blastit-view-static`, `blastit-php-sapi`) speaking length-prefixed `view.chan` wire frames with per-flow CPU time reporting — never cross-language `WorkerRuntime` mega-traits.
   - OS privilege operations (`fork`, `execve`, `setresuid`, `capset`, `seccomp`, `fanotify`, `SCM_RIGHTS`, `SCM_CREDENTIALS`) stay isolated in `blastit-sys` and `blastit`.
2. **True Constructor IoC & Zero Globals.**
   - Read `std::env` **only** at the top-level CLI / stage-0 composition root (`BootMode::{Production, Dev, Check}`), then pass typed dependencies through constructors.
   - **Zero** `static mut`, **zero** `OnceLock<Mutex<...>>` singletons, **zero** `#[cfg(any(test, debug_assertions))]` env-var hooks in new code, and **zero** single-implementation mock-ceremony traits (`Acceptor`, `RequestIo`, `Clock`, `Editor`). Inject concrete collaborators, channels, or closures.
3. **Cohesion & Workspace Reuse.**
   - Keep production files `<500` LOC, functions `<=6` args, and configs structured via nested `#[derive(Deserialize)]` structs (never 30+-field flat `Config` bags or hand-rolled `json_*` accessors).
   - Search sibling crates before writing helpers: extend the canonical owner (`blastit-sqlpolicy`, `elf`, `blastit-frame`) rather than duplicating enums (`ObsVerdict`), SQL lexers, or `ProxySpec` builders.
4. **Error handling & zero panics on untrusted data.**
   - Library crates define typed `thiserror` error enums; CLI/glue maps them to actionable user-facing diagnostics without ever interpolating secret values (`DATABASE_URL`, passwords, tokens).
   - **Zero `unwrap()`, `expect()`, `panic!()`, `todo!()`, or unchecked `[i]` indexing** on any code path reachable from HTTP bytes, `view.chan` frames, WASM memory, `Blastfile` text, or wire-protocol peers (`unwrap`/`expect` allowed inside `#[cfg(test)]` only).
   - `unsafe` requires an explicit `// SAFETY:` comment proving every precondition; `unsafe_op_in_unsafe_fn = "deny"` is enforced workspace-wide.
5. **Dependencies, Standalone Purity & Licensing.**
   - Apache-2.0 compatible licenses only (`Apache-2.0`, `MIT`, `BSD-*`, `ISC`, `Zlib`, `Unicode-3.0`; never GPL/AGPL).
   - Standalone `blastit` (`cargo build -p blastit --locked`) and `blastit-lsp` must never link `libpython` or carry a build-host `RUNPATH` (`make rs-no-python` checks ELF portability).

## 2. Verification

- **Leaf slice verification (single combined run):** `cargo fmt --all && make fast-check` (runs `guard`, `critical-guard`, `rs-fmt`, `rs-lint`, `rs-test`, `py-lint`, `plan-validate` with `PYO3_PYTHON` and `PYTHONHOME` configured automatically).
- **Epic / release gate verification:** `make check` (adds `rs-doc`, Python `test-critical`, `docs-check`, wheel + `docs-test`, `rs-no-python`, `rs-no-default`, `seam-check`, and packaging/workflow/Zed checks).
