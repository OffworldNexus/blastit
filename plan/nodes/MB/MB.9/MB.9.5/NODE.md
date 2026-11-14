---
id: MB.9.5
title: "PHP conformance row: the shared table against EmbeddedView/PhpEmbedRuntime"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.9.4, MB.3.13]
paths: ["crates/blastit-backend/src/**", "crates/blastit/tests/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T17:04"
updated: "2026-11-14T17:04"
---
# Goal

Add the PHP row to the shared conformance table: the same `cases()` exercised against
`ViewUpstream` driving a `PhpEmbedRuntime`, so static, WSGI, embedded PHP and external
behaviour cannot drift.

# Context

- Design of record: `docs/design/backend-runtimes.md` §3/§5 (the per-backend matrix and
  what "exercised" means) and `ADR-0010`; the table itself is `MB.9.4`'s.
- The PHP driver reports capability-filtered skips like the others; classes are `refusal`,
  `body_cap`, `observation`, `error_mapping`. A row that cannot run on a host without
  `libphp` is **skipped and counted**, never silently passed.
- No dispatch branch on backend kind: the driver iterates `cases()` exactly as WSGI's does.

# Acceptance

```console
$ cargo test -p blastit --locked
$ make check
```
- The PHP row runs in `make check` where `libphp` exists, and reports skips (not passes)
  where it does not; the count is visible in the test output.
- Every class has at least one PHP case with a denied and an accepted side.
- Adding the row requires no edit to `cases()` beyond the capability flag — a new `match`
  on backend kind in the driver is an auto-FAIL.
