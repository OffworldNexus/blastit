---
id: MB.56.1.2.1
title: "Inline live check/dev_learn grant-diff into crates/blastit/src/check/ and delete the blastit-audit crate"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - Cargo.toml
  - Cargo.lock
  - "crates/blastit-audit/**"
  - crates/blastit/Cargo.toml
  - "crates/blastit-dbproxy/Cargo.toml"
  - crates/blastit/src/lib.rs
  - crates/blastit/src/check.rs
  - "crates/blastit/src/check/**"
  - crates/blastit/src/dev_learn.rs
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T16:25"
updated: "2026-11-20T16:40"
---

# Goal
Inline the *live* `blastit check` / `blastit dev --learn` grant-diff & report code into `crates/blastit/src/check/`, then delete the `blastit-audit` crate (6,814 LOC) entirely. **Do not touch the `blastit-dbproxy` `[[bin]]` here — that is `MB.56.1.2b`.**

1. **Move the live modules.** Every `blastit_audit` symbol reachable from the shipped binary, and nothing else, moves under `crates/blastit/src/check/` as a cohesive submodule tree:
   - Consumers today: `crates/blastit/src/check.rs` (`evaluate`, `human`, `parse_manifest`, `plan_learn`, `plural`, `Report::all_static`, `unrequested_static_root(s)`, `LearnPlan`, `CheckObsV1`, `HttpObsLine`, `SCHEMA_VERSION`, `ObsError`) and `crates/blastit/src/dev_learn.rs` (`plural`, `pattern_is_writable`, `crate::infer_routes::is_learnable_path`).
   - Modules to carry over (extend this list only if a live caller needs one): `analyze` (only the parts `evaluate`/`plan_learn` reach), `assemble`, `dead`, `diff`, `error`, `evaluate`, `findings`, `human`, `infer_routes`, `ingress` (only if `check` still calls it), `learn`, `ledger`, `model`, `normalize`, `obs`, `patterns`, `sections`, `static_roots`.
   - **Delete, do not move**, everything only the retired `fold`/`pytest_plugin`/`--internal-json` bridge used: `pytest.rs` (`audit`), `analyze::{analyze, classify, covering}` (`covering`/`analyze` had exactly one caller, the deleted RPC), `learn::plan_learn_json`, `model::{blastfile_apply_edits, blastfile_model}` if nothing live calls them, `normalize::{has_quoted_literal, redact_literals}` if nothing live calls them, plus the crate's own `tests/`, `src/tests/`, `src/check_tests.rs`.
2. **Slim `crates/blastit/src/check.rs`** so it stays `< 500` prod LOC of cohesive units (split into `check/mod.rs` + focused files such as `check/gate.rs`, `check/report.rs`); its inline `#[cfg(test)]` blocks may stay for now (`MB.56.2`/`MB.56.3` own them) but every `crate::audit_v1::` path must become `crate::check::…` (or a nested `mod`) and `crates/blastit/src/lib.rs` must drop `pub use blastit_audit as audit_v1;` / `pub use blastit_audit::infer_routes;`.
3. **Cargo/workspace:** remove `crates/blastit-audit/` from the tree and `Cargo.toml` `members`/dependencies (`crates/blastit/Cargo.toml`, `crates/blastit-dbproxy/Cargo.toml` if it depended on it), regenerate `Cargo.lock`. `pyproject.toml`'s `crates/**/*` sdist glob keeps working untouched.
4. **Zero behaviour change:** `blastit check`, `blastit check --json`, `blastit check --learn`, `blastit dev --learn` print byte-identical output. No new globals, no mock traits, no `unwrap()`/`expect()` on untrusted input, no diary comments — keep concise Rustdoc with the `// SAFETY:` invariants.

# Acceptance
- `make fast-check` green, zero warnings; `rg 'blastit_audit|blastit-audit|audit_v1' crates/` returns nothing.
- `cargo test -p blastit --test critical_check_engine --test critical_learn_executor_blocks` green.
