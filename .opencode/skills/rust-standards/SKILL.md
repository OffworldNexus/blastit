---
name: Rust standards
description: Mandatory before writing or reviewing any Rust in Blastit (`crates/*`). Covers crate layering, traits/DI, zero globals, unsafe/error rules, MSRV 1.88, and workspace checks.
---

# Rust standards (Blastit)

Rust powers Blastit's supervisor, HTTP ingress, WASM validation gate, fair scheduler, view runners, and capability proxies (`crates/*`).

## 1. Design & architecture rules

1. **Pure library crates first, thin process/binary wiring last.**
   - Parsing, policy evaluation, wire codecs, WASM transcoding, and scheduling logic belong in pure, unprivileged library crates (`blastit-manifest`, `blastit-events`, `blastit-frame`, `blastit-sqlpolicy`, `blastit-env`, and future `blastit-ingress`, `blastit-wasm`, `blastit-fastcgi`, `blastit-proxy-*`) testable in milliseconds without root or Docker.
   - OS privilege operations (`fork`, `execve`, `setresuid`, `capset`, `seccomp`, `SCM_RIGHTS`, `SCM_CREDENTIALS`) stay isolated in `blastit-sys` and `blastit` (binary).
2. **Traits & dependency injection at seams.**
   - Inject clocks, random seeds, upstream connectors, WASM engines, and event sinks via constructors or generic trait bounds.
   - No `static mut`, no mutable `OnceLock` singletons hiding configuration, and no reading `std::env` outside the stage-0 / CLI configuration composition root.
3. **Parameter clumps -> config/context structs.** When functions share ≥ 3 parameters or exceed 5 arguments, group cohesive parameters into a typed struct.
4. **Error handling & zero panics on untrusted data.**
   - Library crates define typed `thiserror` error enums; binaries/glue map them cleanly with actionable user-facing messages (never exposing secret values!).
   - **Zero `unwrap()`, `expect()`, `panic!()`, `todo!()`, or unchecked `[i]` indexing** on any code path reachable from HTTP bytes, FastCGI/WSGI frames, WASM memory, manifest files, or wire-protocol peers. (`unwrap`/`expect` are permitted inside `#[cfg(test)]` only.)
   - `unsafe` requires an explicit `// SAFETY:` comment proving every precondition; `unsafe_op_in_unsafe_fn = "deny"` is enforced workspace-wide.
5. **Dependencies & licensing.**
   - Blastit is Apache-2.0 licensed. Every Rust crate and embedded WASM library must be permissively compatible (`Apache-2.0`, `MIT`, `BSD-*`, `ISC`, `Zlib`, `Unicode-3.0`; never GPL/AGPL).
   - Never add a dependency to `Cargo.toml` unless the node owns `Cargo.toml` / `Cargo.lock` (see `dispatch` parallelism rules).

## 2. Mandatory checks before reporting a Rust task done

Run all of these (or `make check`):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
```

If rustup toolchain `1.88.0` is installed locally, also run MSRV check:

```bash
cargo +1.88.0 check --workspace --all-targets --locked
```

Public structs, traits, enums, and functions must carry concise rustdoc comments (`///`) that compile cleanly under `RUSTDOCFLAGS="-D warnings"`.
