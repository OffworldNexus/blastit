---
id: MB.26.22
title: "Repair MB.23.6's acceptance evidence commands (scripts/noop and the wheel .so path)"
kind: task
status: superseded
owner: planner
priority: 4
depends_on: [MB.23.6]
paths: ["scripts/**", plan/nodes/MB/MB.23/MB.23.6/NODE.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T21:11"
updated: "2026-11-19T00:36"
---

Re-filed from MB.23.14 (MB.23 audit, PASS WITH GAPS, 2026-11-17) so the deferred gap stays actionable; MB.23 closed for 0.2.0 scope.

# Goal

Make `MB.23.6`'s own acceptance commands runnable as written.

# Context

`crates/blastit/tests/...` has no `scripts/noop` (the `strace ... blastit check -- ./scripts/noop` line fails at both the missing `Blastfile` in the repo root and the missing file), and `make docs-test` leaves only `target/docs-wheel/blastit-<version>-*.whl` — the `.so` is inside the zip, so `ls target/docs-wheel/*/blastit/_blastit*.so` finds nothing until the wheel is unpacked. This audit had to create a fixture and unzip to `target/docs-wheel/unpacked/` to run the guard.

# Acceptance

- Either add the intended `scripts/noop` fixture (and a `Blastfile`/working directory the command can run in) or correct the node's command to a real, documented fixture path.
- The wheel step names the unzip (or the harness gains an unpack step) so a fresh `make docs-test` output supports the guard command.
- `docs/testing/AUDIT-MB.23.md` is not edited by this node (it is the record of the workaround); a one-line note in `MB.23.6`'s LOG points at the repair.
- `uv run --no-project tools/plan.py validate`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MINOR, evidence hygiene). No code behaviour changes.
