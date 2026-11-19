---
id: MB.42.6
title: "Fix wrong remediation text for an unused proxy in blastit check human output (no 'proxy <name>' directive exists)"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.42.2.2]
paths: ["crates/blastit-audit/**", crates/blastit/tests/critical_check_engine.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T20:21"
updated: "2026-11-19T20:21"
---

# Goal
Fix a **wrong remediation instruction** in human `blastit check` output: for an unused proxy it prints `fix: remove \`proxy <name>\` or add the call that uses it`, but `Blastfile` has no `proxy <name>` directive — the declaration is a store (`postgres $DATABASE_URL`). The `--json` side is already correct (`proxy_unused` carries `{"action":"observe"}` and no `to`), so human and `--json` disagree about what to do.

Found by `review-ux` during `MB.42.2.2.1` and deliberately deferred out of that pure refactor (it is byte-identical to the pre-refactor code and pinned by a protected critical test).

# Exact Deliverables
1. `crates/blastit-audit/src/human.rs`'s `proxy_unused` fix line → `fix: remove the store, or add the call that uses it` (the noun is `store`, not `proxy`; keep the one-line, no-trailing-period house style).
2. Make the human line and `--json` agree: `proxy_unused` in `--json` must carry the same actionable shape as the other kinds — either a `fix.to` naming the store, or a `fix.action` that matches the human sentence. State which in `RESULT.md`; do **not** invent a `to` that no `Blastfile` line satisfies.
3. Update the protected `crates/blastit/tests/critical_check_engine.rs` assertions that pin the old string (lines ~905-906) — **explicitly authorized by this node's `paths` + `# Goal`**, since the assertion encodes the bug being fixed. Change only the pinned text, not the scenario or the rest of the expectation.
4. Grep the rest of the repo for the same wrong noun (`proxy <name>`, `remove proxy`, `proxy: declared`) in user-facing strings and docs-adjacent code, and fix them consistently. Report anything found in `docs/**` as a follow-up for `MB.42.4` (do not edit docs here).

# Notes
- No behaviour change beyond the message: same finding detection, same exit codes, same streams.
- `<500` prod LOC, no new globals; do not touch any other protected test.

# Acceptance
- `cargo test -p blastit --locked --test critical_check_engine`
- `cargo test -p blastit-audit --locked`
- `make fast-check`
