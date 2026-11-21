---
id: MB.2.4.5
title: "critical_rewrite: pin client cancel, buffered-forward queue accounting and the bound"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T23:10"
updated: "2026-11-14T00:28"
---

# Goal

Close the three remaining un-oracled transitions of the static flow half in
`critical_rewrite.rs` (design §4.6, §7.2): the client cancel on a parked static
stream, the buffered-forward queue accounting behind a `REQ_END` that landed while
queued, and the admission bound of a buffered `Forward`.

# Context

The flow half now pins slot waves, the short read, the request timeout, the forward
handback at `max_workers = 1`, the 503 bound and the buffered-forward stall the
reviewer reproduced. Three observations from the `MB.2.4` audit remain:

1. **Client cancel has no oracle anywhere.** `grep -r client_cancel\|client_gone
   crates/*/tests` is empty; only `scheduler.rs` names them. A client that goes away
   mid-static-response must report `client_gone`, must **not** release the slot a
   `pread` still owns (`static_read_parked`), and must release it exactly once when
   the read result arrives (design §4.6: "the result releases both, and never the
   timeout").
2. **The "must not re-enqueue" half is unpinned** `scheduler.rs::request_end`'s
   buffered branch keeps one queue entry (`if queued { return Ok(()) }`) and keeps
   the queued request's `queued_since` (`if !queued { request.queued_since = None }`).
   `a_buffered_forward_that_ends_while_queued_is_still_answered` proves the request
   is *answered*, not that the queue holds it once: deleting the early return leaves
   a duplicate FIFO id that `dispatch` silently absorbs (`queue.contains` is false
   after the first dispatch removes both), and deleting the `!` makes the request
   immortal (its `expire_queue` deadline is cleared).
3. **The buffered-`Forward` bound is off by one vs the app path.** `begin_raw`'s app
   path inserts the request with `body_ready = false` *before*
   `buffering_count() >= queue_size`, so it counts itself; `forward_static` checks
   while the request still has `body_ready = true` and only then sets it false, so a
   static front controller admits one more half-body than any other route — the
   opposite of its own comment ("the app path's own bound").

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
```

New tests (paused clock, the existing `World`/`PumpTree` harness):

- **client cancel:** `max_workers = 1`, one stream parked on credit; the client
  cancels; assert the `request` event is `client_gone`, a second request queues
  while the read is outstanding, and it completes after the read returns — one
  release. (Extend the `World` http fake if it cannot send a cancel.)
- **no duplicate entry:** with `queue_size = 2` and exactly one queued buffered
  forward, a later request must be admitted (200/queued) rather than 503: a
  duplicate entry would make `queue.len()` 2 and turn it away.
- **queue timeout still fires:** the same buffered forward with the slot never
  freed must answer `503` + `Retry-After` when the paused clock passes
  `queue_timeout` (it must not sit unanswered because `queued_since` was cleared).
- **the bound:** with `queue_size = N`, the static front controller must admit the
  same number of half-bodies as an app route with the same config. Either fix
  `forward_static` (set `body_ready = false` before the check, as the app path does)
  or state the divergence in the comment and pin the actual bound.

# Notes

- Test-only unless the owner chooses to align the bound; no other product change.
- Do not weaken or delete an existing assertion, and do not add `#[ignore]`.
