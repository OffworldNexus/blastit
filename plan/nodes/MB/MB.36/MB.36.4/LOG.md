# Log MB.36.4

## 2026-11-17T14:07 [planner]

created (task) Move immortal preload and the zygote fork into blastit-view-python behind the design's spawn-context seam, with no cycle back into blastit

## 2026-11-17T15:57 [planner]

edited fields

## 2026-11-18T13:55 [lead]

status ready -> active

## 2026-11-18T13:55 [lead]

attempt 1/3 started

## 2026-11-18T13:55 [lead]

attempt 1/3 started; base 2910123

## 2026-11-18T14:24 [lead]

status active -> review

## 2026-11-18T14:34 [lead]

REVIEW FAIL (round 1): MAJOR — the design D1 pin is missing: no test proves the production constructor yields DropTo+Seccomp, so an inverted `dev.is_none()` would boot production views Shared with every suite green. MINORs: 7 new cfg_attr(allow(dead_code)) beyond the one immortal exception; safe `pub fn set_env` over thread-unsafe setenv/getenv (UB reachable from safe Rust); ELOOP vs ENOTDIR; "never returns on success"; census wording (check never boots stage1); stale line numbers/comments naming PHP callers that do not exist until MB.36.5.

## 2026-11-18T14:34 [lead]

status review -> ready

## 2026-11-18T14:34 [lead]

status ready -> active

## 2026-11-18T14:34 [lead]

attempt 2/3 started

## 2026-11-18T15:14 [lead]

status active -> done
