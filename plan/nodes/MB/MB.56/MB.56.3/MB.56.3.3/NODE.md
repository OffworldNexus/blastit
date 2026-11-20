---
id: MB.56.3.3
title: "Create e2e_check_fmt.rs (§5 check/fmt/learn, exit table) and drop the audit_v1 shim"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.3.2.3]
paths: ["crates/blastit/tests/**", crates/blastit/src/lib.rs, "crates/blastit/src/check/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T18:45"
updated: "2026-11-20T18:54"
---

# Goal
Create `crates/blastit/tests/e2e_check_fmt.rs` (`TESTING_PLAN.md` §5 — CI completeness & formatting) and delete the suites it absorbs. Target ≲ 1,200 LOC.

1. `blastit check` / `check --json`: exit `0` on a full WSGI + Postgres suite, a DB-free suite and a static-only `Blastfile`.
2. Human + `--json` parity on failure (exit `1`): missing grants, dead grants (`sql`, `proxy`, `fix.to` (`view <name> postgres`)), unrouted requests, unexercised views, missing `body` policies — the exact `view`/`trait` nouns, singular/plural and `fix:` lines.
3. The documented **exit table**: `0` clean, `1` findings, `2` could-not-start, `3` suite failed (failing assertion, pytest usage error, signalled child, pytest `5` = nothing collected), stdout empty under `--json`.
4. `blastit check --learn`: proposes new views/grants without widening an existing view; the `learn:` line `learn: N routes, M queries added → /abs/Blastfile` and `proposed (dry-run) → …` under `--dry-run` (never `added`, never a relative path), on stderr under `--json`; JSON key `routes` never `route`.
5. Gate discovery for an installed binary: finds its embedded gate from the installed/repacked bundle and never a cargo tree, prefers the project venv's interpreter, prepends the caller's `PYTHONPATH`.
6. The pytest-django handshake: settings module/`manage.py`/`test_<database>`, owner-only, never reused, removed however the run ends; Django without a `postgres` proxy and a store with no input variables are exit-`2` start failures naming the block.
7. `blastit fmt` idempotent comment-preserving formatting; `fmt --check` prints a unified diff and fails on an unformatted file.
8. Absorb and delete: `critical_check_engine.rs`, `critical_blastfile_fmt.rs`, `critical_learn_executor_blocks.rs`, `audit_group_hint.rs`, `fmt_command.rs`, `blastfile_discovery.rs`.
9. Then remove the temporary `#[doc(hidden)] pub use check as audit_v1;` in `crates/blastit/src/lib.rs` and the now-unused `check::learn::plan_learn_json` (dead production code kept only for these suites) — `crates/blastit/src/lib.rs` and `crates/blastit/src/check/**` become in-scope for this slice.

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_check_fmt` green; `rg 'audit_v1|plan_learn_json' crates/` returns nothing.
