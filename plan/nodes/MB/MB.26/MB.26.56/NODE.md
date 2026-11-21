---
id: MB.26.56
title: "Run cargo-mutants on blastit-php-sapi (ini.rs, request.rs, abi.rs, toolchain.rs) and close non-equivalent survivors"
kind: task
status: superseded
owner: "test-curator"
priority: 3
depends_on: []
paths: ["crates/blastit-php-sapi/tests/**", "docs/testing/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

Run `make mutants-rust PKG=blastit-php-sapi` across `ini.rs`, `request.rs`, `abi.rs`, and `toolchain.rs`, record the per-module kill rate in `docs/testing/`, and close any non-equivalent survivor below the 75 % policy bar.

# Context

Recommended for the milestone backlog by `docs/testing/AUDIT-MB.3.13-CLOSURE.md`: `blastit-php-sapi` holds 2 protected suites (`critical_file_sink.rs`, `critical_shim_contract.rs`) but had no standalone `cargo-mutants` run recorded in `0.2.0`.

# Acceptance

- [ ] `make mutants-rust PKG=blastit-php-sapi` completed and recorded in `docs/testing/` with >= 75 % non-equivalent kill rate.
- [ ] `make check` green.

# Provenance

Filed at the `v0.2.0` gate from `docs/testing/AUDIT-MB.3.13-CLOSURE.md`.
