---
id: MB.1
title: "MA Gate Follow-ups: Mutation Coverage, Startup Diagnostics & Harness Hygiene"
kind: epic
status: done
priority: 1
depends_on: []
paths: ["docs/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-13T19:56"
---

# Goal

Close every non-blocking follow-up deferred from the `MA` (`0.1.1`) milestone gate before `0.2.0` backend work begins: lift whole-crate `blastit-manifest` mutation kill rate past 75 %, complete the remaining 511/1019 `blastit-sqlpolicy` mutants and eliminate the 20 cursor-loop timeouts, fix the missing-`./blastit.toml` startup message, and land the remaining build/e2e/scheduler hygiene items.

# Context

- Source of truth: `plan/nodes/MA/RESULT.md` (§2–§5) and `plan/nodes/MB/NODE.md` ("Starting points").
- Multi-node `tools/critical_guard.py` and the `.github/workflows/ci.yml` `guards` union check already landed in `70a4357` — do not duplicate them.
- Keep `Cargo.toml` / `Cargo.lock` edits (`blastit-events` unused `serde` removal) in a dedicated sequential leaf task so parallel Rust tasks never contend on workspace lockfiles.

# Acceptance

- [ ] `make mutants-rust PKG=blastit-manifest` achieves >= 75 % whole-crate killed (covering `edit.rs:961` `verify()` and TOML comment/bracket patcher branches) with zero 90 s cursor-loop timeouts (`parse.rs:252`, `edit.rs:738,1116..1369`).
- [ ] All 1019 mutants in `blastit-sqlpolicy` are measured (including `lexer.rs` tail, `literals.rs`, `lru.rs`, `normalize.rs`, `statements.rs`, `tables.rs`, sharded by file `-f` if needed to stay fast) at >= 75 % killed per policy module, with the 9 cursor-loop timeouts in `learn.rs:661` / `lexer.rs:179..230` bounded or eliminated.
- [ ] Running `blastit dev` or `blastit` with no `./blastit.toml` and unset `BLASTIT_APP` reports the missing `./blastit.toml` before complaining about `BLASTIT_APP` (`crates/blastit/src/stage0.rs:444,666`), pinned in `crates/blastit/tests/critical_config_and_presets.rs` and reconciled in `docs/troubleshooting.md`.
- [ ] `scripts/build-wheel.sh` rewrites relative `README.md` links via `scripts/readme-absolute-links.py` (restoring `README.md` on exit), `CARGO_NET_OFFLINE=true` replaces `1` anywhere `maturin >= 1.16` can inherit it, `crates/blastit-events/Cargo.toml` drops its unused `serde` dependency, and `Scheduler::plan_request_body` returns `Ok(BodyPlan::AllowEmptyOnly)` immediately when `policy.is_none()`.
- [ ] `e2e/` includes permanent assertions for a `queries = []` denying listener (`42501`) and an unprivileged negative-boot run (`--cap-drop ALL` exits `1` naming missing capabilities); `python/tests/critical/test_check_contract.py` learn-refusal statement sync is deterministic; `docs/brand/logo.svg` light-mode header contrast (`VISUAL-MA.3.md`) is resolved; `audit_v1` `fix` guidance suggests group-scoped grants (`add_group` / `[group.<name>.db]`) for connection-setup SQL (`MA.8`); `make check` and `make e2e` pass green.
