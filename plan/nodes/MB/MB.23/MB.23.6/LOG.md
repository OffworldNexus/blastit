# Log MB.23.6

## 2026-11-16T23:13 [planner]

created (task) Verify MB.23 live: runnable-docs --no-reload fence, zero-inotify guard on standalone + wheel, production strace, determinism and dev_reload mutation audit

## 2026-11-16T23:21 [planner]

edited fields

## 2026-11-17T19:48 [lead]

inherits three MB.23.5.3 MINORs: carry policy_out_of_step in the adoption document so a pre-existing taint survives the exec; drop the duplicated body assertion; widen the ack-latency upper bound to ~1 s or assert only against the deadline. Plus the two MB.23.5.1 proof gaps (an idempotent double-reload leg and the 0x45 refusals through the streaming Decoder).
