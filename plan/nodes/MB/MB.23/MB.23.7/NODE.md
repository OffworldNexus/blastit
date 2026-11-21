---
id: MB.23.7
title: "Carry the out-of-step dev policy taint across the reload exec"
kind: task
status: superseded
owner: implementer
priority: 1
depends_on: [MB.23.6]
paths: [crates/blastit/src/supervise.rs, crates/blastit/tests/critical_dev_reload.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T20:59"
updated: "2026-11-17T21:09"
---

# Goal

A dev policy channel that an earlier save left **out of step** must still be out of step after the reload `execve`, so the first save after a `Blastfile`/preload cycle cannot read the timed-out document's late ack as its own.

# Context

`Supervisor::policy_out_of_step` is an in-memory `AtomicBool` (`crates/blastit/src/supervise.rs:514`) set on every `PolicyOutcome::Broken` and consulted by `refresh_policy` (`:924`). The reload cycle writes an adoption document (`adoption_document`, `:1953`) that carries the `policy` descriptor but not the taint, and the replacement image constructs `Supervisor::new` with `false` (`:644`). A save that times out, followed by a manifest/preload cycle, followed by another save therefore drains nothing and can attribute the old `0x01`/`0x00` to the new document: one report of a swap that did not happen (or a refusal that did).

# Acceptance

- The adoption document carries the taint (e.g. `"policy_out_of_step": true`) exactly when the old image set it; the replacement image adopts it and the existing one-warning sentence still fires on the next save without writing a document.
- Unit: a document round trip preserves both values; a supervisor built from a tainted document reaches `refresh_policy`'s out-of-step arm with no write.
- Live: with `BLASTIT_TEST_RELOAD_PAUSE`, taint the channel (peer that never acks), complete a cycle, then save a grant and assert exactly one `manifest.warning` with "out of step" and **zero** bytes on the policy socket.
- `cargo test -p blastit --locked --features python --lib --test critical_dev_reload`, `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (inherited from `MB.23.5.3`, MAJOR). The taint is a dev-only correctness property; do not widen `Cargo.toml`/`seccomp.rs`.
