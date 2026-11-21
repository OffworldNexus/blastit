---
id: MB.56.5.2
title: Strip diary comments and stale tool references; land the final gates
kind: epic
status: done
priority: 1
depends_on: [MB.56.5.1]
paths:
  - "crates/**"
  - "python/**"
  - "scripts/**"
  - "e2e/**"
  - ARCHITECTURE.md
  - docs/knowledge/LESSONS.md
  - Makefile
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T22:57"
updated: "2026-11-20T23:17"
---

# Goal
Strip the historical diary comments across `crates/*/src/**/*.rs` and land the final gates.

1. Remove multi-paragraph plan-node history (`//! MB.3.xx: …`, "why we used to …", "before 0.2 we …") from every crate's source and its doc modules, keeping: concise 1-3 line Rustdoc on public items, every `// SAFETY:` comment on an `unsafe` block, and any note a reader needs to avoid a real trap.
2. Do the same for stale cross-references to deleted tools and suites (`scripts/check-no-test-seam.sh`, `critical_guard.py`, `critical_*` names, `e2e/run.py`, `_bin/blastit-dbproxy`, `e2e/php/fixture`) in `crates/**`, `e2e/**`, `python/**`, `docs/knowledge/LESSONS.md` and `scripts/**`.
3. Report the final before/after prod and test LOC table in `RESULT.md` (crate | prod LOC before → after | why).

# Acceptance
- `make check` + `make zed-check` + `make docs-check` green; local Docker E2E (`e2e/run_apps.py`, `e2e/run_prod.py`, `e2e/run_packaging.py`) green or loudly skipped; `rg 'MB\.[0-9]+\.[0-9]+' crates/*/src | wc -l` is 0.
