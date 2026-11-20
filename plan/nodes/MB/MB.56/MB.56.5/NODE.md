---
id: MB.56.5
title: "Run make coverage to delete uncovered dead Rust code, strip historical diary comments & pass full E2E gates"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.4.4]
paths: ["crates/**", "python/**", "scripts/**", Makefile, ARCHITECTURE.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T21:07"
---

# Goal
Run `make coverage` across the consolidated `TESTING_PLAN.md` E2E & §7 suites, delete uncovered dead/speculative Rust production code, strip multi-paragraph historical diary comments (`MB.3.xx` essays) across `crates/*`, and verify all local gates!

1. **Coverage-Driven Dead-Code Pruning (`make coverage`):**
   - Run `make coverage` on `crates/*` using the consolidated `e2e_*` and §7 suites (`blastfile_language`, `sql_policy`, `http_framing`, `e2e_lsp`, `e2e_cli`, `e2e_dev`, `e2e_check_fmt`, `e2e_prod_refusals`).
   - Inspect low-coverage / `0%` modules and functions across `crates/*`:
     * Delete unreachable adapters, unused error variants, dead helpers, and left-over shims from retired features!
     * If any uncovered branch corresponds to a real user story in `TESTING_PLAN.md`, add the missing assertion to the owning E2E suite — NEVER add unit tests!

2. **Strip Historical Diary Comments & Tighten Code:**
   - Remove multi-paragraph plan-node history essays (`//! MB.3.xx: ...`, `// Why we used to do X in 0.1.0...`) across `crates/*/src/**/*.rs` while keeping crisp 1–3 line Rustdoc comments on public types/functions and mandatory `// SAFETY:` comments on every `unsafe` block!
   - Report the final before/after LOC reduction table in `RESULT.md`.

# Acceptance
- `make check` + `make zed-check` + `make docs-check` + local Docker E2E (`e2e/run_apps.py`, `e2e/run_prod.py`, `e2e/run_packaging.py`) all exit `0` with `0` warnings!
