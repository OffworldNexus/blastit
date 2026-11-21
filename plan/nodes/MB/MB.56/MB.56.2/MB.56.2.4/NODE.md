---
id: MB.56.2.4
title: "Consolidate blastit-sqlpolicy (<=1.5k) and blastit-http (<=800) §7 suites"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.2.3]
paths: ["crates/blastit-sqlpolicy/**", "crates/blastit-http/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T17:15"
updated: "2026-11-20T18:27"
---

# Goal
Collapse the two remaining §7 suites: `blastit-sqlpolicy` (**5,080 test LOC → ≤ 1,500**) and `blastit-http` (**3,854 → ≤ 800**).

1. **`crates/blastit-sqlpolicy/tests/sql_policy.rs`** (1–2 files): table/column/function allowlists, `$?` / `IN ($?, …)` / multi-row `VALUES` template matching, prepared statements, session-setup queries, and strict rejection of SQL injection, multi-statement batches, unparameterized literals, ungranted columns, and `E'…'`/`U&'…'` escaping (the policy is the security boundary: every allowed case needs its denied twin).
2. **`crates/blastit-http/tests/http_framing.rs`** (1 file): HTTP/1.1 request framing, `body` policies (`none`, `json`, `form`, `raw unvalidated`), payload size limits, client disconnects, and rejection of `Content-Length`/`Transfer-Encoding` smuggling and protocol upgrades.
3. Delete the superseded files and every inline `#[cfg(test)]` in both crates' `src/**`; no `#[ignore]`, no production logic change.

# Acceptance
- `make fast-check` green, zero warnings; `blastit-sqlpolicy/tests` ≤ 1,500 LOC, `blastit-http/tests` ≤ 800 LOC; no `cfg(test)` left in either crate.
