# Log MB.3.45

## 2026-11-15T04:11 [lead]

created (task) MB.3.41 review MINORs: bailed in the public outcome table, the flake asserts by type, and the release claim

## 2026-11-15T04:28 [lead]

MB.3.46 (the scheduler DONE legs) could only close two of its four legs and found two production facts; both belong to this node, which is why it is the scheduler-accounting follow-up.

(4) A scheduler-WRITTEN answer never pays the queue wait only in a shape this node has to create. As built, every scheduler-written answer emits the `request` event BEFORE `release()` charges it, so `cost_ns` is 0 (probed on a 504: cost=0, duration=21 ms). Fix needs `src/**`: charge before emit, or expose the flow debit — then pin it with a leg (the node that owns this is here, not .46, because .46 is scoped to tests/**).

(5) PRODUCTION FINDING, found by the new tests: `respond()` sets `head_sent = true` for a head the SCHEDULER wrote, so a 504 whose view later sends `DONE{Bailed}` is really billed its arrival->finished wall, queue wait included. That contradicts R19's "a scheduler-generated 502/504 keeps its dispatch->completion charge and never pays the queue wait". It needs a distinct "a view answered this" bit rather than reusing head_sent, plus a leg where the scheduler answers and the view then completes.

(6) `docs/reference/log-events.md` must also say `cost_ns` is 0 for requests the scheduler answered itself (same page as the `bailed` clause from item 1).

## 2026-11-15T04:28 [lead]

edited fields

## 2026-11-15T04:36 [lead]

edited fields

## 2026-11-15T04:36 [lead]

Lead scope decision after the body repair: items 4 and 5 are scheduler-accounting legs, and a scheduler harness cannot be built inside `scheduler.rs`'s `#[cfg(test)]` module — the real fixture (a scripted AF_UNIX view, paused clock) is `tests/common/mod.rs`, and the suite that already pins the bailed charge is `tests/critical_charge.rs` (created by MB.3.46, now protected). Both are therefore added to this node's paths: the new legs extend `critical_charge.rs` rather than inventing a second harness, and this node is authorized to edit that protected file for exactly those legs (`critical_guard --node MB.3.45` is the check; do not weaken or delete MB.3.46's three legs, only add).
Priority inside this node, in order: (1) items 4 and 5, because they are production billing bugs that contradict R19 and ARCHITECTURE.md — fix the seam first (charge before emit or expose the debit; a distinct "a view answered this" bit instead of reusing head_sent) and pin each with a leg; (2) item 3, where narrowing the comment to the paths that do release is acceptable if releasing completed requests in client_cancel is risky, but then say which and file the residue; (3) items 1, 2 and 6, which are mechanical. If items 4 and 5 cannot both land in one attempt, land item 5 (the queue-wait contradiction) first and report the rest, rather than half-fixing the ordering.

## 2026-11-15T04:36 [lead]

edited fields

## 2026-11-15T05:20 [lead]

BINDING ITEM 7 (found by MB.3.48, scheduler-only, this node owns it): the required `php-sapi` job can STILL flake until this lands. `supervise.rs:805` pushes `Ctl::Exited` on ctl.chan right after reaping, and `scheduler.rs:1078 -> :3326` fails the in-flight request as `ViewDied` before the view's view.chan frames are read — the very frames (`RESP_HEAD`/`RESP_END_COST`/`DONE{Bailed}`, php.rs:445-465) that a DESIGNED retire writes before exiting. `ViewIn::Closed` is the only ordered trigger today. Reproduced 1 in 20 in the image under 8-spinner load: `outcome: view_died, cost_ns: 0, cpu_ns: null, status: 502, bytes_out: 12, queued_ms: 26.67` — i.e. a request the view ANSWERED is billed as a dead view with no cost. Fix shape (MB.3.48's RESULT f/u 1): make the ordered channel the trigger, or drain view.chan before failing the request as ViewDied. It must be pinned by a scheduler test that a view which writes its frames and then retires is `bailed`/`ok` with its cost, never `view_died` — and that a view which really dies mid-request is still `view_died`.
Order inside this node is now: item 7 first (it is a random release-blocker), then item 5, then item 4, then item 3, then the three mechanical ones (1, 2, 6).
