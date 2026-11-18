---
id: MB.26.23
title: "Prove the reload transaction is idempotent and a payload-carrying 0x45 is refused by the streaming Decoder"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.23.6]
paths:
  - "crates/blastit-scheduler/tests/pool.rs"
  - "crates/blastit-frame/src/decoder.rs"
  - "crates/blastit-frame/src/frame.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T21:11"
updated: "2026-11-19T00:36"
---

Re-filed from MB.23.15 (MB.23 audit, PASS WITH GAPS, 2026-11-17) so the deferred gap stays actionable; MB.23 closed for 0.2.0 scope.

# Goal

Close `MB.23.5.1`'s two proof gaps: prove the scheduler's reload transaction is idempotent, and prove a payload-carrying `0x45` is refused by the **streaming** `Decoder` (not only by `Frame::decode`).

# Context

`docs/design/dev-fanotify-reload.md` `T21` says "`reload_all` is idempotent", but `crates/blastit-scheduler/tests/pool.rs` has only `reload_retires_every_idle_worker_of_every_route` and `a_spawn_sent_before_a_reload_is_retired_on_arrival` — no second `Ctl::Reload`. `crates/blastit-frame/src/frame.rs::ctl_reload_refuses_a_payload_and_a_non_zero_id` loops over payloads with `Frame::decode`; the streaming refusal (`Decoder::push`/`next_frame`/`finish`) is asserted only for the clean round trip. `docs/testing/AUDIT-MB.23.md` names both as inherited gaps.

# Acceptance

- Two `Ctl::Reload` messages in one wakeup (and back to back) retire each idle view once, never twice, never panic, and never dispatch to a stale view.
- A `0x45` frame with one payload byte fed to the real `Decoder` is refused by `finish()`/`next_frame()` with the same error as `Frame::decode`, and no partial frame is delivered.
- `cargo test -p blastit-scheduler --locked`, `cargo test -p blastit-frame --locked`, `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MINOR proof gaps). Do not add a route payload to `Ctl::Reload`.
