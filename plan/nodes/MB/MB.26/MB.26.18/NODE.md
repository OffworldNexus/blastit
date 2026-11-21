---
id: MB.26.18
title: "Close accepted rows T17, T20 and T23: production/check hold no hook, and keep-alive survives an all-views and a preload reload"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: [MB.23.6]
paths: [crates/blastit/tests/critical_dev_reload.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T21:11"
updated: "2026-11-19T00:36"
---

Re-filed from MB.23.10 (MB.23 audit, PASS WITH GAPS, 2026-11-17) so the deferred gap stays actionable; MB.23 closed for 0.2.0 scope.

# Goal

Close the remaining accepted design rows that have no protected test: `T17` (production/`check` hold no fanotify fd, no report socket, no loader hook), `T20` (a keep-alive connection opened before an all-views `loaded` reload answers after it), and `T23` (the same keep-alive across a `preload` cycle).

# Context

`critical_dev_reload.rs` proves the `--no-reload` side and the `Blastfile` cycle's keep-alive, but it never runs `blastit check`/production and never holds a connection across an all-views reload or a preload cycle. `docs/testing/AUDIT-MB.23.md` records the strace proof of "no fanotify syscall", which does not cover the fd table, the loader scope or the audit hook.

# Acceptance

- `blastit check` and a production-mode fail-closed boot: no `anon_inode:fanotify`, no report socket, no `blastit_loaded_fd` in the loader scope, no audit hook.
- Editing a view file: a keep-alive connection opened before the save gets its next response after the `dev.reload`, and a `preload` module save does the same.
- `cargo test -p blastit --locked --features python --test critical_dev_reload`, `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MAJOR). The production boot cannot reach the app unprivileged; assert before the fail-closed exit, as `critical_dev_and_fail_closed.rs` does.
