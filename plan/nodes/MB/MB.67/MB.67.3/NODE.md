---
id: MB.67.3
title: "Rust compile-time language tables (view-php, php-sapi, py-shim)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.67.2, MB.67.7]
paths:
  - "crates/blastit-view-php/**"
  - "crates/blastit-php-sapi/**"
  - "crates/blastit-py-shim/**"
  - "crates/blastit-py-sapi/**"
  - "crates/blastit-languages/**"
  - crates/blastit/src/stage0/project_python.rs
  - Cargo.toml
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:44"
updated: "2026-11-23T01:28"
---

# Goal

Delete the hardcoded PHP/CPython minor tables from the Rust crates: each `build.rs` reads `packaging/languages.tsv` at compile time (`cargo:rerun-if-changed`) and emits the table the crate uses, with a test that the generated value equals a fresh parse of the TSV.

# Scope

- `crates/blastit-view-php` — `build.rs` (new) parses the TSV and writes `OUT_DIR/languages.rs` with the supported minors (newest first, string order) and the `libphp<slug>.so` / `libphp-<version>.so` spellings; `src/discovery.rs` replaces the hardcoded `CANDIDATES`/`SUPPORTED_PHP`/name tables with `include!` of the generated file, and the refusal sentence and doc comments stop enumerating versions (they say "the versions in `packaging/languages.tsv`"). Search order stays: policy walk order derived from the same table.
- `crates/blastit-php-sapi` — `build.rs` derives `CANDIDATES` (`php-config`, `php-config<minor>` newest first) and `SUPPORTED` from the TSV; the "no toolchain" warning names the newest/minor list from the table instead of a literal; `src/toolchain.rs` consumes the generated list. `php_toolchain` cfg behaviour and the never-fail-the-build rule are unchanged.
- `crates/blastit-py-shim` — `build.rs` derives the CPython minors (`py313`/`py314` shim names and the supported-minor list) from the TSV; no literal `3.13`/`3.14` table remains.
- `Cargo.lock` only if a build-dependency is genuinely needed (prefer zero: `std` parsing of a TSV is ~20 lines; no `csv`/`serde` in `build.rs`).

# Out of scope

Python/bash loaders and packaging (`MB.67.1`/`MB.67.2`), docs and e2e (`MB.67.4`), `packaging/stage-dist.py`. Do not edit `crates/*/tests/critical_*.rs` except by adding tests.

# Acceptance

1. `cargo fmt --check`; `cargo test -p blastit-view-php`; `cargo test -p blastit-php-sapi`; `cargo test -p blastit-py-shim`.
2. Freshness tests: a unit/integration test in each crate parses `packaging/languages.tsv` at test time (or `include_str!`) and asserts it equals the generated constant; it must fail if `build.rs` is changed to emit a wrong minor (verify once by hand, revert).
3. `grep -rn '8\.[3-6]' crates/blastit-view-php/src crates/blastit-php-sapi/src crates/blastit-py-shim/src --include=*.rs` shows no version table — only prose naming one example (or none).
4. `make fast-check` (this is the only Cargo writer; do not run it concurrently with another slice).

# Proof (slice-specific)

`RESULT.md`: the three `cargo test` tails, the deliberately-wrong-build.rs failure tail, the grep output, and `make fast-check` tail.
