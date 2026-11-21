# Log MB.23.5.2.2

## 2026-11-17T13:45 [planner]

created (task) Reload the preload cycle and the Blastfile without dropping :8080: the adoption document, the exec, stage-1 adoption and the two debug seams

## 2026-11-17T16:31 [lead]

status ready -> active

## 2026-11-17T16:31 [lead]

attempt 1/3 started

## 2026-11-17T16:31 [lead]

base 83c6bb9; inherits the follow-ups its sibling could not close: Fire::cycle/needs_cycle must come back, the true gate witness, and the manifest/preload re-read that makes docs/local.md's row true

## 2026-11-17T17:17 [lead]

implementer done: Fire::cycle + gate (with the true witness), validate-then-execve with one manifest.warning, the adoption document and stage-1 adoption (no bind/no fork, adopted http.chan, fresh view dirs), restart handover via the config document (not an env var, which would be stripped), the three internal tokens, both debug seams gated out of release, rows 7a-7d + EPERM + learn skip. 19/19 x3 serial and x12 under load; make check EXIT=0.

## 2026-11-17T17:17 [lead]

status active -> review

## 2026-11-17T17:26 [lead]

REVIEW FAIL (3 MAJOR, 4 MINOR): the seam guard never checks BLASTIT_TEST_RELOAD_PAUSE so make seam-check cannot catch it leaking (design and the node both claim otherwise); two_route_blastfile returns the manifest the fixture already wrote, so the valid-save route assertions are unfalsifiable and the added-route private-dir branch is never exercised; the adoption document refusal contract has no test at all. MINORs: drain expiry kill_alls http/dbproxy so a long request plus a Blastfile save kills dev; the ctl_failure_is_fatal reloading arm is unwitnessed; a false doc claim about clearing the gate; an unwrap_or_default exec with an empty document and a silent 8-token truncation.

## 2026-11-17T17:26 [lead]

status review -> ready

## 2026-11-17T17:26 [lead]

status ready -> active

## 2026-11-17T17:26 [lead]

attempt 2/3 started

## 2026-11-17T17:35 [lead]

attempt 2: PAUSE is the third seam the guard checks (present 3 in debug, refused in a release-shaped file); the valid save now adds a checkout route so the emitted routes order, the 200 and the added route private dir are falsifiable; Adoption::read has a table-driven refusal contract; drain expiry abandons the cycle with one warning instead of kill_all; the reloading arm is asserted directly; the gate test states what it truly witnesses; adoption_document returns Result and the 8-token prefix is refused not truncated. 19/19 x8, lib 281.

## 2026-11-17T17:35 [lead]

status active -> review

## 2026-11-17T17:44 [lead]

REVIEW FAIL attempt 2 (1 MAJOR, 2 MINOR): abandon_cycle/wedged_routes and the progress_cycle expiry arm are NEVER EXECUTED -- the only abandon leg calls the warning formatter -- so deleting reloading=false or restoring kill_all leaves the suite green and RESULT FIX 4's red-if-regressed claim is false. FIX 1/2/3/5/6/7 verified genuinely addressed. MINORs: design 4.6 still describes views.is_empty() with no 10 s CYCLE_GRACE bound; a gate comment cites a leg that exercises no flag.

## 2026-11-17T17:44 [lead]

status review -> ready

## 2026-11-17T17:44 [lead]

status ready -> active

## 2026-11-17T17:44 [lead]

attempt 3/3 started

## 2026-11-17T17:59 [lead]

attempt 3: the abandon path is genuinely executed (a real Supervisor with three real sleep children and an injected expired Draining deadline); deleting reloading=false fails the gate assertion and restoring kill_all fails with the wedged view signalled -- both mutations checked by hand and reverted. Design 4.6/4.10 now state the 10 s CYCLE_GRACE and abandon outcome; the false FIX 4 claim is corrected in place. NOTE: one early run failed with its panic lost to tail -2 and never reproduced in 73 later runs.

## 2026-11-17T17:59 [lead]

status active -> review

## 2026-11-17T18:09 [lead]

reviewer attempt 3 VERDICT PASS with one open MINOR (an unread, unreproduced panic from attempt 3 -- kept as an unexplained unknown; 17 more runs green). Committing.

## 2026-11-17T18:09 [lead]

status review -> done
