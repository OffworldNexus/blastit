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
