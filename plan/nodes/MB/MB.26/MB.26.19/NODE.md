---
id: MB.26.19
title: "Prove epic item 4 in a container: Docker's default seccomp gives one dev.reload.unavailable and 200s"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: [MB.23.6]
paths: ["e2e/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T21:11"
updated: "2026-11-19T00:36"
---

Re-filed from MB.23.11 (MB.23 audit, PASS WITH GAPS, 2026-11-17) so the deferred gap stays actionable; MB.23 closed for 0.2.0 scope.

# Goal

Prove epic `MB.23` acceptance item 4 in a real container: under Docker's default seccomp profile, `blastit dev` prints exactly one `dev.reload.unavailable` and answers `200` for the whole run, never crashes and never opens a second watcher.

# Context

The `EPERM` row in `critical_dev_reload.rs` drives the ladder through the debug-only `BLASTIT_TEST_RELOAD_FAULT=open` seam; no `e2e/` harness starts `blastit dev` at all, so the real default-profile refusal is unproven.

# Acceptance

- A new `e2e/` harness (or an addition to an existing one) runs `blastit dev` in a container with Docker's default seccomp profile, makes several requests, asserts exactly one `dev.reload.unavailable` with `errno: EPERM`, no `dev.reload`, no crash, and `200` throughout.
- `make e2e` (or the specific target) is green on a host with Docker; the harness names its skip when Docker is absent, never a silent pass.
- The docs sentence in `docs/troubleshooting.md` is checked against the observed event, not a hand-written string.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MAJOR). Reuse `e2e/` conventions; do not require root.
