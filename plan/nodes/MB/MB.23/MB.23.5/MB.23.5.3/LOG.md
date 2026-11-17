# Log MB.23.5.3

## 2026-11-17T12:37 [planner]

created (task) Refresh the dbproxy policies on a reload without dropping :8080, and the PHP live-reload row

## 2026-11-17T12:38 [lead]

edited fields

## 2026-11-17T13:43 [lead]

edited fields

## 2026-11-17T18:10 [lead]

status ready -> active

## 2026-11-17T18:10 [lead]

attempt 1/3 started

## 2026-11-17T18:10 [lead]

base fde43da; row 8 (EPERM) already landed in MB.23.5.2.2 -- confirm it, do not duplicate

## 2026-11-17T18:46 [lead]

edited fields

## 2026-11-17T18:46 [lead]

attempt 1 landed the dbproxy half green (policy wire module, replace_route_policies with Arc<Allowlist> swap, ListenerPolicy cell, ack loop, stage-0 socketpair + configuration document, 20 critical legs). The sender and the PHP row both live in supervise.rs, which was in no paths entry, so paths are extended with supervise.rs + cli.rs + roles.rs and the work continues as attempt 2 on the same node rather than a child. Row 10 does not exist yet -- stated, not passed off.

## 2026-11-17T18:46 [lead]

attempt 2/3 started

## 2026-11-17T19:03 [lead]

attempt 2: the sender writes u32 LE len + JSON through the poll set with a 100 ms ack and emits exactly one manifest.warning naming the cause on any failure while the retirement still runs; the PHP report end is wired for both backends (fresh pair per boot, keep list, load_reporting) and row 10 RAN on this host's real pair: one dev.reload with routes [app, api] and v2 body on both after the save. --features python,php selects the php leg (was 0).

## 2026-11-17T19:03 [lead]

status active -> review

## 2026-11-17T19:13 [lead]

edited fields

## 2026-11-17T19:13 [lead]

REVIEW FAIL attempt 2 (2 MAJOR, 3 MINOR): a timed-out ack leaves its byte unread so the NEXT save reads it as its own answer and a refused document is reported as a silent success; acceptance 7 (a save that adds/removes a store or listener must warn) is not implemented. paths extended with stage1.rs so acceptance 7 can land here; MINORs: the ack is a synchronous 100 ms stall instead of the design's poll state, the config refusal names a --dbproxy-policy-fd token no role accepts, the warm-compiled-template row is unasserted.

## 2026-11-17T19:13 [lead]

attempt 3/3 started

## 2026-11-17T19:39 [lead]

attempt 3: PolicyOutcome drains a stray ack before writing and taints the image on Broken so a late ack can never be read as the next document success (two-document late-ack test); policy_pairs is one definition, stage 1 fills boot_pairs and store_set_warning emits one manifest.warning naming added/removed route->proxy before the dbproxy guard, nothing fails open; the sync exchange is kept with a measured 100.7 ms bound recorded as a deviation; the refusal names policy_fd/dev_policy; the warm compiled template leg added. make check EXIT=0.

## 2026-11-17T19:39 [lead]

status review -> review

## 2026-11-17T19:48 [lead]

reviewer attempt 3 VERDICT PASS (3 MINOR): the exec boundary loses the out-of-step taint (carry it in the adoption document -- filed for MB.23.6), a duplicated assertion, and a host-load-sensitive 500 ms bound. Committing.

## 2026-11-17T19:48 [lead]

status review -> done
