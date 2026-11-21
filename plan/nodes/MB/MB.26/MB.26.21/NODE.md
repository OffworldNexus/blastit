---
id: MB.26.21
title: "Triage the unread critical_dev_reload panic and remove the sleep-decided verdicts"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.23.6]
paths: [crates/blastit/tests/critical_dev_reload.rs, crates/blastit/src/supervise.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T21:11"
updated: "2026-11-19T00:36"
---

Re-filed from MB.23.13 (MB.23 audit, PASS WITH GAPS, 2026-11-17) so the deferred gap stays actionable; MB.23 closed for 0.2.0 scope.

# Goal

Triage the single unread `critical_dev_reload` panic from `MB.23.5.2.2` attempt 3, and make the next failure readable instead of attributed to load.

# Context

`MB.23.5.2.2`'s RESULT records one early failure whose panic was destroyed by `tail -2`; it did not reproduce in 90+ runs. This audit's own campaign (`docs/testing/AUDIT-MB.23.md`) also found six 750 ms `SETTLE` windows and one 90–500 ms ack bound that can turn host load into a false verdict.

# Acceptance

- Run `critical_dev_reload` 50× (idle and with all CPUs busy) with each run captured to a **full** log file (`> /tmp/... 2>&1`, never through `tail`); paste the pass count and any panic in full.
- If a panic appears, file it with the exact test, seed/load and the full backtrace; fix the sender or the test in a sibling node, never by weakening it.
- Replace the six `SETTLE`-decided negative assertions with a deterministic wait or a drain-to-EOF oracle where one exists; widen the ack-latency upper bound to ~1 s (or assert only against the deadline) and fix the stale docstring that names `the_ack_wait_is_bounded_by_the_design_s_deadline`.
- Drop the duplicated `products-1` assertion in `removing_a_store_is_one_warning_and_the_run_keeps_serving`.
- `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MINOR + one unknown). The ack bound and duplicate were inherited from `MB.23.5.3`'s log.
