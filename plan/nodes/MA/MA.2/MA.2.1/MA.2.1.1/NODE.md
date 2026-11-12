---
id: MA.2.1.1
title: "Leaf 1: workspace seams, blastit-ingress stub and Cargo wiring"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - Cargo.toml
  - Cargo.lock
  - "crates/*/Cargo.toml"
  - "crates/blastit-ingress/src/lib.rs"
  - "crates/blastit-env/src/**"
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-dbproxy/src/config.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:25"
---

# Goal

Land the sequential foundation that lets every other `MA.2` leaf compile in parallel: create the `blastit-ingress` crate skeleton (`BodyCodec`, `IngressPolicy`, constants), apply the exact `Cargo.toml`/`Cargo.lock` matrix from design §4.1, and add the constructor/struct shims (`blastit_env::build_with_socket_dir`, `ListenerSpec::enforcing`, `DbproxyConfig::enforcing`, `RouteInfo::with_ingress`, `..SchedulerConfig::default()`) so later leaves only add behaviour.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.1 (Cargo matrix), §4.2 A (types), §4.2 D.2/D.3 (shims), §8 Leaf 1; `ADR-0001`, `ADR-0006`.
- Depends on nothing (first leaf). **This is the only leaf allowed to touch root `Cargo.toml` / `Cargo.lock`.**
- Seams required now (stubs only, no codecs): `crates/blastit-ingress/src/lib.rs` declares `MAX_JSON_DEPTH`, `MAX_MIME_REPORT_BYTES`, `MAX_CHECK_SAMPLE_BYTES`, `BodyCodec` (serde lowercase), `IngressPolicy`.
- Add the exact dependency versions from §4.1 to `blastit-ingress`, `blastit-manifest`, `blastit-scheduler`, `blastit`, `blastit-py`; keep the graph acyclic (`sqlpolicy` + `ingress` -> `manifest`).
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=1 cargo metadata --locked --format-version 1 >/dev/null
$ CARGO_NET_OFFLINE=1 cargo build --workspace --all-targets --locked
$ CARGO_NET_OFFLINE=1 cargo test --workspace --all-targets --locked
$ make check
```

- `cargo tree -p blastit-manifest` shows `blastit-ingress` and `blastit-sqlpolicy` and **no** dependency edge back to `blastit-manifest`.
- `blastit_ingress::{BodyCodec, IngressPolicy}` are importable from `blastit-scheduler`, `blastit`, and `blastit-py` (a compile-time `use` in each is enough for this leaf).
- `make check` passes (critical tier reports zero protected tests so far; that is expected).

# Notes

- ~180 changed lines. No behaviour beyond wiring; the codec matrix is Leaf `MA.2.1.2`.
- If an exact version from §4.1 is absent from the local cargo cache, do **not** change versions or reach the network: record the evidence in `RESULT.md` and report `STATUS: blocked`.
- Never edit `docs/**`, `README.md`, or accepted DX/ADR decisions.
