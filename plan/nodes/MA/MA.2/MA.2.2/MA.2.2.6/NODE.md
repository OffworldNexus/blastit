---
id: MA.2.2.6
title: "Cover scheduler body cap before REQ_END and --learn queue overflow 503"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-scheduler/tests/critical_ingress_enforcement.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T14:56"
updated: "2026-11-12T15:08"
---

# Goal

Cover the two scheduler fail-closed branches the protected suite never names: a buffered
`json`/`form` body over `max_body_bytes` must be refused **before** `REQ_END`, and a full
`--learn` fallback queue must answer `503` instead of growing.

# Context

- Audit `docs/testing/AUDIT-MA.2.2.md`, mutants `sc.buffer_cap_off` and
  `sc.learn_queue_overflow_off`; both survived `critical_ingress_enforcement` and the whole
  crate. Only `max_body_bytes == body_len` is tested today.
- `request_body`'s `BodyPlan::Buffer` arm enforces the cap per frame (fail fast, bounded
  memory, §4.3); `request_end` re-checks it, so removing only the incremental check is not
  observable once the body completes — the oracle must stop before `REQ_END`.
- `Resolution::Unrouted` counts **waiting** requests against `queue_size`; the first request
  holds the slot, so overflow needs a third concurrent unrouted request.
- Both candidate tests were run against the pristine tip in a scratch copy (green) and under
  their mutants (red); see the audit's mutation table.

# Acceptance

```console
$ cargo test -p blastit-scheduler --test critical_ingress_enforcement --locked
```

- New: chunked `json` request, `max_body_bytes = 9`, 10 bytes of valid JSON sent, no
  `REQ_END` -> the response is `413` within the test's short timeout and the view never sees
  a `REQ_BODY` frame.
- New: `learn_mode`, `queue_size = 1`, three concurrent unrouted requests with the first
  held -> the third gets `503` and `retry_after`; releasing the first serves the second.
- The existing at-the-cap accept twin and the one-slot queueing test stay unchanged.

# Notes

- Test-only node; `tests/common/mod.rs` already exposes `Options.cfg` and `Script::Hold`.
