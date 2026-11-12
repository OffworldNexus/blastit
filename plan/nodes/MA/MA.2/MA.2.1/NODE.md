---
id: MA.2.1
title: Canonical wire codecs and workspace seams (ingress + sqlpolicy)
kind: epic
status: ready
priority: 2
depends_on: [MA.5]
paths:
  - Cargo.toml
  - Cargo.lock
  - "crates/*/Cargo.toml"
  - "crates/blastit-ingress/**"
  - "crates/blastit-env/**"
  - "crates/blastit-sqlpolicy/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Container for the sequential foundation and the two canonical byte codecs of `0.1.1`: the workspace `Cargo` wiring + crate shims, the `blastit-ingress` body matrix, and `blastit-sqlpolicy` literal redaction. Closing this epic means every later leaf compiles offline and both codec crates are proven by their own protected critical suites.

# Context

- Technical design: `docs/design/ma-lean-dx.md` §4.1, §4.2 A/C, §8 leaves 1–3; `ADR-0003`, `ADR-0006`.
- Children: `MA.2.1.1` (sequential, owns root `Cargo.toml`/`Cargo.lock`), then `MA.2.1.2` and `MA.2.1.3` in parallel with 100% disjoint paths.
- DX status: user docs for Local / CI-CD / Deployments are already accepted (`MA.1`); no child may rewrite them.

# Acceptance

- [ ] `CARGO_NET_OFFLINE=1 cargo test --workspace --all-targets --locked` and `make check` pass on the epic tip.
- [ ] `crates/blastit-ingress/tests/critical_ingress.rs` and `crates/blastit-sqlpolicy/tests/critical_literals.rs` exist and assert both allow and deny twins for `R14..R16`, `R24..R25`.
- [ ] Root `Cargo.toml`/`Cargo.lock` were touched by exactly one child (`MA.2.1.1`).
- [ ] No leaf bumped a crate version or edited `docs/**`.

# Notes

- Leaf 1 is the only node allowed to add workspace dependencies; everything downstream must `depends_on` it.
