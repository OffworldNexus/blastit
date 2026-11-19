---
id: MB.42.2.2.1
title: "Extract crates/blastit-audit: one grant-diff owner (<500 LOC modules), audit.rs + audit_v1 collapsed"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [Cargo.toml, Cargo.lock, "crates/**", Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T20:07"
updated: "2026-11-19T20:30"
---

# Goal
Give the workspace **one** owner of the audit/grant-diff logic by extracting `crates/blastit-py/src/audit.rs` (3,683 LOC) and `crates/blastit/src/audit_v1.rs` (2,156 LOC) into a new `crates/blastit-audit`, split into cohesive `<500`-prod-LOC modules, and point `crates/blastit` at it.

`review-arch` found these are **two implementations of the same computation** ("executed but not granted / dead grant"): `blastit-py`'s `audit()` and `audit_v1`'s `evaluate()`. `docs/design/universal-distribution.md` §1.1 requires them to collapse into one crate so the CLI, `check`, and the Python bridge all read one grant diff.

# Exact Deliverables
1. **New `crates/blastit-audit`** (zero `pyo3`; deps stay minimal — `blastit-manifest`, `blastit-sqlpolicy`, `blastit-scheduler`/`blastit-http` only if the moved code already needs them; **no** `serde_json` in `crates/blastit`'s public surface unless the moved code already used it):
   - Cohesive modules, each `<500` prod LOC: `normalize` (`normalize`, `has_quoted_literal`, `redact_literals`), `analyze` (`analyze`, `classify`), `covering` (`covering`), `model` (`blastfile_model`, `blastfile_apply_edits`), `evaluate` (`check_audit`, `plan_learn`, the route-trace/grant-diff), `report` (the JSON shapes) — adjust the cut to what the code actually is, but **one** implementation of each behaviour.
   - Public API mirrors the existing symbol set so both current callers move mechanically: `normalize`, `has_quoted_literal`, `redact_literals`, `audit`, `analyze`, `covering`, `classify`, `check_audit`, `plan_learn`, `blastfile_apply_edits`, `blastfile_model`.
2. **One grant-diff:** the moved `audit()` and `evaluate()` must share a single route-trace/grant-diff implementation. If they genuinely compute different reports, share the underlying "what did this view ask for vs what was granted" pass and keep two thin presenters — say which in `RESULT.md`.
3. **`crates/blastit`:** delete `src/audit_v1.rs`; `check.rs`, `dev_learn.rs`, `cli/**` and any other caller go through `blastit-audit`. `crates/blastit/src/cli/internal_json.rs` must stay a thin dispatcher (`<200` LOC) — it may keep a stub for now; `MB.42.2.2` fills it.
4. **`crates/blastit-py` must still compile and still export its current `audit`/`covering`/... functions**, now re-exported from `blastit-audit` (a few lines, not a copy). **Do not delete `crates/blastit-py`, `Host::wheel` or `Install::Wheel` in this slice** — `python/blastit/__main__.py` still imports `blastit._blastit`, so deleting them now would turn the Python side red before `MB.42.2.2` ships the replacement. Their deletion is `MB.42.2.2`'s first step.
5. **Invariants that must survive the move verbatim** (each is a security property, not a style choice):
   - **secret redaction** on `stderr`/report text (no secret, password, or literal value ever in a message),
   - backslash-aware literal handling for `E'..'`/`U&'..'` spans (`MB.2.3.1`'s `expert` fix),
   - atomic write with mode preservation on `blastfile_apply_edits`,
   - `fold.py`'s exact JSON shapes (`audit()` returns **JSON text**, `covering()` returns **JSON text**),
   - zero `unwrap()`/`expect()` on untrusted manifest/SQL text.
6. **Tests move with the code** and stay green: the existing unit tests for both modules, plus a lab-first test per module boundary. Add at least one test proving the shared grant-diff gives the **same** answer through the `audit` and `check_audit` entry points for the same fixture (that is the whole point of the merge).

# Notes
- The only Cargo writer. Never write `/home/remy/.local` or `/usr/bin/blastit`; container-only for any distro package need.
- `<500` prod LOC per file, `<=6` args, no globals/`OnceLock`/`static Mutex`/mock traits, no `#[cfg(test)]` env seams.
- Do not touch `crates/*/tests/critical_*.rs` or `python/tests/critical/`; if a protected test must move, stop and report instead.
- Never touch `docs/**`.
- `blastit-py`'s `3683`-LOC file is the thing being dissolved — do not simply rename it.

# Acceptance
- `cargo test -p blastit-audit -p blastit -p blastit-py --locked`
- `cargo clippy --workspace --all-targets --locked --features python -- -D warnings`
- `cargo tree -p blastit-audit --locked -e normal | grep -c pyo3` -> `0`
- every new `crates/blastit-audit/src/**` file `<500` prod LOC (state the table in `RESULT.md`)
- `make fast-check`
