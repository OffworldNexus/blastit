---
id: MB.56
title: "Execute TESTING_PLAN.md test & codebase cleanup: E2E-first consolidation, multi-platform GHA stories & dead-code removal"
kind: epic
status: ready
priority: 1
depends_on: []
paths:
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - pyproject.toml
  - AGENTS.md
  - ARCHITECTURE.md
  - README.md
  - CHANGELOG.md
  - RELEASING.md
  - "crates/**"
  - "python/**"
  - "scripts/**"
  - "tools/**"
  - "e2e/**"
  - "docs/**"
  - ".github/**"
  - ".opencode/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T16:02"
---

# Goal
Execute the drastic test and codebase cleanup aligned strictly to the owner-approved `TESTING_PLAN.md` (do NOT alter `TESTING_PLAN.md`):
1. Delete retired `0.1.0` Python code (`pytest_plugin.py`, `fold.py`, `logs.py`), the `--internal-json` RPC bridge (`internal_json.rs`), the separate `blastit-audit` crate, the duplicate `blastit-dbproxy` `[[bin]]` binary (`blastit` runs `--role=dbproxy` directly), `tools/critical_guard.py`, `mutants-*.sh`, `runnable_docs/`, and ~20 meta-linter scripts in `scripts/`.
2. Delete all non-§7 inline `#[cfg(test)]` & component unit/micro-tests, keeping only tight focused suites for `TESTING_PLAN.md` §7 (`Blastfile` grammar/formatting/edits, SQL grant matching, HTTP/1.1 framing & body enforcement, `blastit-lsp` + Zed).
3. Consolidate `crates/blastit/tests/` (31 files -> 4 crisp end-to-end suites: `e2e_cli.rs`, `e2e_dev.rs`, `e2e_check_fmt.rs`, `e2e_prod_refusals.rs`).
4. Consolidate `e2e/` and GitHub Actions workflows so every story in `TESTING_PLAN.md` §2–§6 is tested end-to-end (including a multi-view Laravel/Symfony-style PHP app with distinct PostgreSQL table/column grants + public static files across PHP 8.3–8.6 in GHA, Django & Flask/SQLAlchemy across Python 3.13–3.14, interpreter-free Static/SPA, `.deb`/`.rpm`/`install.sh`/`py3-none` wheel/Composer across `x86_64`+`aarch64` `gnu`+`musl`, and root + rootless production presets).
5. Run `make coverage` against the new E2E + §7 suites to delete uncovered dead Rust code and strip historical diary-essay comments across `crates/*`.

# Acceptance
- `make check` green + all local Docker E2E suites green + all GitHub Actions checks on PR #64 green
