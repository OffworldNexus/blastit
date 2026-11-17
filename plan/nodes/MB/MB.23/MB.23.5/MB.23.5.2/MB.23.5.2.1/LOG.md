# Log MB.23.5.2.1

## 2026-11-17T13:44 [planner]

created (task) Supervise the reload: one dev Source, an armed deadline, fresh per-boot reporters, the boot line and the live-reload critical suite

## 2026-11-17T13:45 [lead]

status ready -> active

## 2026-11-17T13:45 [lead]

attempt 1/3 started

## 2026-11-17T13:45 [lead]

base 54ad5d9 (dev_reload.rs + cli --no-reload already on disk from MB.23.5.2 attempt 1; that attempt left make check at EXIT=0, keep it there)

## 2026-11-17T14:13 [lead]

finisher done: 10/10 critical_dev_reload, make check EXIT=0 (0 FAIL/0 FAILED), make docs-test 48 fences 0 failed with the dev-flags fence EXECUTED, release check warning-free, guard clean. Only code change was the skip-token wording.

## 2026-11-17T14:13 [lead]

status active -> review

## 2026-11-17T14:21 [lead]

REVIEW FAIL (6 MAJOR, 3 MINOR): --no-reload parses and is read nowhere so the advertised flag is inert; register leaks a route's previous report read end and on_fd ignores EOF/HUP so the dead fd polls eagerly (busy spin); ENOSPC latches with no warning and MAX_MARKS silently breaks; dirs_to_mark re-stats every union path on every datagram/tick; sync_marks' Dead outcome is swallowed; the reloading gate can stay set forever with no live view. MINORs: cycle discarded, overflow leg witnesses only the Effect, blocking socketpairs.

## 2026-11-17T14:21 [lead]

status review -> ready

## 2026-11-17T14:21 [lead]

status ready -> active

## 2026-11-17T14:21 [lead]

attempt 2/3 started

## 2026-11-17T15:06 [lead]

attempt 2 done: all 6 MAJOR + 3 MINOR fixed; --no-reload real end-to-end (0 fds/0 marks/0 hooks, old body), register/on_fd leak+spin fixed, Exhausted/ENOSPC/cap latch with one warning and no boot line, is_dir at absorb (no re-stat), sync_marks Dead matched, gate only for cycle+live views, real overflow leg, non-blocking sockets; plus the polled_sources-after-read_ctl defect that caused the intermittent hang. make check EXIT=0, docs-test 48/30/0, 12/12 reload suite.

## 2026-11-17T15:06 [lead]

status active -> review

## 2026-11-17T15:16 [lead]

edited fields

## 2026-11-17T15:16 [lead]

REVIEW FAIL attempt 2 (1 BLOCKER, 2 MAJOR, 3 MINOR): the same index-shift class survives in the LOG source loop -- it iterates 0..self.sources.len() while  only has polled_sources slots, so a same-wakeup Ctl::Spawn shifts it onto the reload fds' flags and read_source blocks forever on an empty pipe (that is the suite's hang, reproduced inside make check). stage0.rs + the design doc are now in this leaf's paths (the forward is minimal and correct; amending the plan beats reverting it). The reload gate still opens for a cycle this leaf cannot complete, so a later genuine scheduler exit is swallowed. MINORs: the closed-end test sends an empty datagram instead of closing the peer, the Dead arm has no test, fire.cycle stays discarded.

## 2026-11-17T15:16 [lead]

attempt 3/3 started

## 2026-11-17T15:16 [lead]

REVIEW FAIL attempt 2: the same index-shift class survives in the log-source loop, which still iterates over every source while the ready array only holds polled_sources entries, so a same-wakeup Spawn shifts it onto the reload fds and read_source blocks forever on an empty pipe. stage0.rs and the design doc are now in this leaf paths. The reload gate still opens for a cycle this leaf cannot complete.

## 2026-11-17T15:16 [lead]

status review -> active

## 2026-11-17T15:38 [lead]

attempt 3: log-source loop bounded by polled_source_range (red when written 0..live_sources) with a spawn/report collision test; the reloading gate and Fire::cycle deleted so any component exit is fatal again, with a SIGKILL-after-blastfile test; on_fd takes poll flags and the closed-peer test drops the peer; BootLine + EIO test. critical_dev_reload 14/14 x3 and under load; make check EXIT=0.

## 2026-11-17T15:38 [lead]

status active -> review

## 2026-11-17T15:48 [lead]

edited fields

## 2026-11-17T15:48 [lead]

attempt 3 FAIL on two defects confined to the NEW protected test (not the feature): a views.len()==2 wait that times out because a lazily-retired route never respawns without a request, and a shutdown-stage assertion that rejects the legitimate http-died. The reviewer verified every one of the six earlier MAJORs plus FIX 1/3 as genuinely fixed. Deliberately NOT escalating to expert: this is not a repeated identical failure (6 MAJOR -> 1 BLOCKER -> 2 test assertions, monotonically shrinking) and the remaining work is two test assertions the reviewer has already prescribed. Bumping max_attempts to 4 with the same cheap owner.

## 2026-11-17T15:48 [lead]

status review -> ready

## 2026-11-17T15:48 [lead]

status ready -> active

## 2026-11-17T15:48 [lead]

attempt 4/4 started

## 2026-11-17T16:13 [lead]

attempt 4: the lazy-retirement wait replaced by the pids that answered; any named <component>-died accepted and the doc comment now states the test proves a component dying during a reload is fatal, NOT that no gate exists (that witness is 5b-ii's). 9 full-suite runs, 0 failures (reviewer had 1 in 9); make check EXIT=0.

## 2026-11-17T16:13 [lead]

status active -> review

## 2026-11-17T16:19 [lead]

edited fields

## 2026-11-17T16:19 [lead]

attempt 4 FAIL on one MAJOR confined to the test wording: a third legitimate fatal path (init dying via send_ctl write failure prints ctl.chan: Broken pipe, with no shutdown stage) was not accepted, so the flake merely moved. The reviewer verified FIX A exact, the design fix gone, and crates/blastit/src byte-identical to the tree it already approved. Bumping max_attempts to 6 and instructing: stop enumerating wordings; assert the fatal contract only.

## 2026-11-17T16:19 [lead]

status review -> ready

## 2026-11-17T16:19 [lead]

status ready -> active

## 2026-11-17T16:19 [lead]

attempt 5/6 started

## 2026-11-17T16:25 [lead]

attempt 5: wording enumeration deleted; the test asserts non-zero exit + no surviving view, with a log witness recorded but never required, and a comment forbidding re-narrowing. 8 runs, 5 with all 8 CPUs busy, 0 failures; make check EXIT=0; src byte-identical (tree sha before/after).

## 2026-11-17T16:25 [lead]

status active -> review

## 2026-11-17T16:31 [lead]

reviewer attempt 5 VERDICT PASS, zero findings. make check EXIT=0, docs-test dev-flags EXECUTED, 6/6 loaded runs. Committing.

## 2026-11-17T16:31 [lead]

status review -> done
