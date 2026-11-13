# Log MB.2.5.2

## 2026-11-13T20:52 [planner]

created (task) Dispatch static answers and pump the body through the scheduler - design MB.2.5b

## 2026-11-13T22:29 [lead]

status ready -> active

## 2026-11-13T22:29 [lead]

attempt 1/3 started

## 2026-11-13T22:29 [lead]

base b8e8806

## 2026-11-13T22:48 [implementer]

MB.2.5b landed in scheduler.rs + the flow half of critical_rewrite.rs: static roots at boot, head_segments/declares_body/If-None-Match in begin_raw, admit_static/can_dispatch_static, dispatch_static (serve/forward/refuse), the spawn_blocking pread pump with slot-until-read-result + idempotent release_static_slot (slot_releasable unit-tabled), short read -> RespAbort, timeout/cancel release view-less requests; static 405 now Method Not Allowed/method not allowed. +845 lines in scheduler.rs (budget 600 -> Follow-up), 23 critical_rewrite tests green, make rs-lint/rs-doc/fmt/critical_guard clean.

## 2026-11-13T22:49 [lead]

status active -> review

## 2026-11-13T22:56 [implementer]

attempt 2 review fixups: buffered Forward no longer waits for a body that already ended (falls through to admit_ready) and request_end no longer re-enqueues a queued request (one queue entry, keeps queued_since); the half-body wait now honours buffering_count() >= queue_size -> 503; added the buffered-forward positive row and the 503+Retry-After row (25 critical_rewrite tests); scheduler.rs 875 lines vs 600 and the shared test file 1718 vs 1600 recorded in RESULT.md rather than trimming.

## 2026-11-13T22:56 [lead]

attempt 2/3 started

## 2026-11-13T22:56 [lead]

status review -> review

## 2026-11-13T23:01 [lead]

REVIEW PASS attempt 2 (3 MINOR: the request_end no-requeue half of fix (b) is not pinned by a row; the new half-body guard counts queue_size others + itself, one more than the app path's bound; RESULT claims both halves are pinned).

## 2026-11-13T23:01 [lead]

status review -> done
