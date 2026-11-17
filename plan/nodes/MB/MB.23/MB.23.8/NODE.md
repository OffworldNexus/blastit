---
id: MB.23.8
title: "Prove T31: a save during a reload cycle is not lost, and reconcile closes the gap"
kind: task
status: superseded
owner: implementer
priority: 1
depends_on: [MB.23.6]
paths: [crates/blastit/tests/critical_dev_reload.rs, crates/blastit/src/supervise.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T20:59"
updated: "2026-11-17T21:09"
---

# Goal

Prove design `T31`: a tracked save that lands **during** a reload cycle is not lost, and deleting `Policy::reconcile` turns a named test red.

# Context

`BLASTIT_TEST_RELOAD_PAUSE` exists in `supervise.rs` (`reload_pause`, `:2012`) and `Policy::reconcile` (`dev_reload.rs:1106`) is the one-shot mtime gap closure, but no test uses either. The `cargo-mutants` campaign in `docs/testing/AUDIT-MB.23.md` missed both `Policy::reconcile -> ()` and its `mtime <= upper` bound, which is the mechanical proof that the row is unwritten.

# Acceptance

- With `BLASTIT_TEST_RELOAD_PAUSE`, a preload save pauses the cycle after the scheduler stops; save a tracked file, release, and assert exactly one further `dev.reload` naming that file, the new body, and the listener still answering.
- Deleting the `reconcile` call (or replacing it with `()`) makes that test red; so does replacing `mtime <= upper` with `> upper` using a future-mtime fixture.
- `cargo test -p blastit --locked --features python --test critical_dev_reload`, `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MAJOR). The seam is debug-only and must stay in `scripts/check-no-test-seam.sh`.
