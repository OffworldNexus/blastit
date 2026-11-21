# Log MB.26.8

## 2026-11-17T06:14 [planner]

created (task) A queued request must never be handed to a retired PHP view (502 view_died under 8-way concurrency)

## 2026-11-17T06:29 [planner]

edited fields

## 2026-11-17T06:29 [strategist]

status proposed -> ready: mandatory for 0.2.0: queued requests behind a retiring PHP worker must respawn and return 200, never 502 view_died

## 2026-11-17T09:22 [lead]

status ready -> active

## 2026-11-17T09:22 [lead]

attempt 1/3 started

## 2026-11-17T09:22 [lead]

base a4b930e (pool MB.24.3 + PHP shape MB.24.4 in place)

## 2026-11-17T09:23 [lead]

lead resumed: base 68a6400 (a4b930e = the MB.24.4 commit; 68a6400 adds plan bookkeeping only). Dispatching implementer for attempt 1.

## 2026-11-17T09:47 [lead]

implementer done: DONE{Bailed} sets View::retiring so dispatch_queued grows a replacement; parent red {200:80,502:80} -> {200:160} x20 batches; deterministic pool.rs leg red at parent.

## 2026-11-17T09:47 [lead]

status active -> review

## 2026-11-17T10:11 [lead]

make check on the polished tree: only the known docs/reference/cli.md fence 1 (--no-reload, MB.23.5); 169 test targets ok. Committing.

## 2026-11-17T10:11 [lead]

status review -> done
