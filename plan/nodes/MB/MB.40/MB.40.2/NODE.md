---
id: MB.40.2
title: "Fix the 4 live P0/High bugs: store-less check crash, table-grant $? check false-positive, --learn reload/probe bugs & coverage flush"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.40.1]
paths:
  - "crates/blastit/**"
  - "crates/blastit-sqlpolicy/**"
  - "crates/blastit-manifest/**"
  - "crates/blastit-py/**"
  - "python/**"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T00:16"
---

# Goal
Fix all 4 live runtime `P0`/`High` bugs discovered by Forensic Audits A & C, with real lab tests covering both allowed and adversarial paths.

# Exact Targets (`file:line` pre-diagnosed)
1. **P0 #1 — Store-less `blastit check` `BrokenPipeError` (`python/blastit/_check_pytest.py:173`, `crates/blastit/src/check.rs:556-920`):**
   - Repro: running `blastit check` on `README.md`'s store-less hello-world (0 `postgres` proxies declared) crashes in `_check_pytest.py:173` with `BrokenPipeError` (exit 3) when pushing route observations over the control channel.
   - Fix the control-channel / proxy lifecycle in `check.rs` and `_check_pytest.py` so 0-proxy apps complete `blastit check` and `blastit check --json` cleanly (`exit 0` when all views exercised, `exit 1` when unexercised/unrouted). Add a pytest test file to `python/tests/runnable_docs/fixtures/hello/tests/test_hello.py` (and/or critical test in `crates/blastit/tests/` + `python/tests/critical/`) so store-less `blastit check` is permanently tested!
2. **P0 #2 — `blastit check` false `missing_grant` on table/column-granted parameterized SQL (`crates/blastit/src/audit_v1.rs:707` -> `crates/blastit-sqlpolicy/src/lib.rs:262`, `crates/blastit-py/src/audit.rs`):**
   - Root cause: `Allowlist::check` / `analyze_cached` is called on `$?`-normalized SQL text (`SELECT id FROM items WHERE id = $?`), which `libpg_query` fails to parse because PostgreSQL syntax requires `$1, $2, ...` (whereas `blastit dev` checked the real wire query before `$?` normalization!).
   - Fix: translate `$?` placeholders back to `$1, $2, ...` (respecting quoted strings/dollar-quotes/comments via `blastit_sqlpolicy`'s lexer) before AST table/column analysis, or carry the original parameterized query / proxy allow verdict, so `select items(id)` passes `blastit check` on `SELECT id FROM items WHERE id = $1` while still denying ungranted columns (`SELECT secret FROM items WHERE id = $1`)!
3. **High #3 — `check --learn` vs `check` disagreement & probe-learning / `dev --learn` reload kill (`crates/blastit/src/{check,dev_learn,infer_routes,supervise,dev_reload}.rs`, `crates/blastit-manifest/src/edit.rs`):**
   - Ensure `check --learn` proposes missing queries for **existing declared views** as well as new views (never exiting 0 with `0 proposed` when `check` fails on learnable missing SQL grants!).
   - Refuse learning adversarial/scanner paths: paths containing `..`, dotfile segments (`/.env`, `/static/.env`), or paths already covered/refused by a `static` mount.
   - In `dev --learn`, track/ignore self-writes to `Blastfile` so rewriting `Blastfile` during `--learn` does not disable subsequent `fanotify` source-code live reloads!
4. **High #4 — Role coverage flush on `_exit` (`crates/blastit/src/{supervise,stage1,visibility}.rs`):**
   - Ensure clean role shutdown flushes coverage profiles when built with `-C instrument-coverage` (`#[cfg(coverage)]` `unsafe extern "C" { fn __llvm_profile_write_file() -> c_int; }` before `libc::_exit`, plus preserving `LLVM_PROFILE_FILE` across `dev`/`check` child spawns ONLY when `LLVM_PROFILE_FILE` is set in non-prod modes).

# Acceptance
- `make fast-check`
- `cargo test -p blastit -p blastit-sqlpolicy -p blastit-manifest -p blastit-py --features blastit/python --locked`
