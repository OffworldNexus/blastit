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
