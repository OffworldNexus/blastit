---
id: MB.3.38
title: Charge a contained PHP bailout at least its wall time (epic MB.3.13 acceptance) and assert it
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - crates/blastit/tests/critical_php_embed.rs
  - crates/blastit/src/php_view.rs
  - "crates/blastit-view/src/php.rs"
  - "crates/blastit-scheduler/src/**"
  - docs/knowledge/LESSONS.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T01:42"
updated: "2026-11-15T02:09"
---

# Goal

Make the contained-bailout CPU bill match the epic's own bar, and assert it.

`MB.3.13`'s `# Acceptance` says: *"`exit()`/`die()`/fatal in the script kills
neither the worker nor its slot: the next request is served and **the crashed
request is charged at least its wall time**."* Design `R19` says the same from the
other end: *"crashes/`504` charge `ceiling.max(kernel_floor_ns)`"*, where
`ceiling = observed_wall_ns × max_threads`.

Today a contained bailout **reports** `RESP_END_COST`, so the scheduler treats it as
a normal request and charges the clamped report. The child's own evidence shows it:
`MB.3.13.4`'s `exit()` leg printed `cpu_ns=1543666 wall_ns=18336040 cost_ns=3221782`
— the client waited 18.3 ms and the queue was charged 3.2 ms. The protected suite
asserts only `cost_ns > 0 && cost_ns >= cpu_ns`, so this shape is green.

Deliver:

1. The remainder of `R19` for a **contained bailout**: a worker that ends its
   request through `zend_bailout` (the shim's `contained = 1`, `OUTCOME_BAILOUT`)
   must be charged `ceiling.max(kernel_floor_ns)` on the existing path — i.e. the
   completion is reported as a crash/`5xx` request, not as `outcome: "ok"`.
   `MB.3.13.4`'s follow-up records the second half: *"A contained bailout is
   reported `status: 500` with `outcome: 'ok'`"* — the label and the charge are one
   decision, so pin both.
2. The protected assertion: in `crates/blastit/tests/critical_php_embed.rs`'s
   `containment(...)`, replace `cost_ns >= cpu_ns` with the epic's sentence —
   `cost_ns >= wall_ns` (the event's own `duration_ms × 1e6`) — and assert
   `outcome` is not `"ok"` for a `5xx` bailout. Keep `cpu_ns <= wall_ns`,
   `cpu_ns > 0 || wall_ns < 1 ms`, and the respawn/slot assertions unchanged.

If the charge change turns out to belong to `blastit-scheduler`'s uniform `R19`
path rather than this epic's files, make the minimal change there and say which
module owns it in `RESULT.md`; do **not** weaken the assertion to fit today's
behaviour. If the lead decides the epic's sentence is the wrong bar, that is an
amendment to `plan/nodes/MB/MB.3/MB.3.13/NODE.md` recorded by `lead` — this node
then only removes the contradiction, never silently keeps the weaker rule.

# Context

- `docs/testing/AUDIT-MB.3.13.md` §"The CPU bill" has the numbers and the oracle
  argument (why `cost_ns >= cpu_ns` cannot distinguish a report-only charge).
- `crates/blastit-scheduler/tests/fairness.rs` pins the **non-crash** rule
  (`a_report_larger_than_the_observed_time_is_clamped`: `cost_ns <= bound`), so a
  fix must not change the reporting-worker clamp — only the bailout path.
- The scheduler is `blastit-scheduler/src/{fair.rs,scheduler.rs}`; the wire is
  `RESP_END_COST` + `DONE` (`R18`), whose `wall_ns` field is ignored on purpose.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --nocapture
exit(): status 500  cpu_ns=1543701  wall_ns=18336040  cost_ns=18336040  outcome=bailout
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit-scheduler --locked
test result: ok. … 0 failed     # fairness's non-crash clamp unchanged
```

- With the charge reverted to the reported value, `a_fatal_is_contained_and_the_next_request_is_served`
  and `an_exit_is_contained_and_the_next_request_is_served` both fail on
  `cost_ns >= wall_ns` (paste the red run).
- `make check` exits `0`; `uv run --no-project tools/critical_guard.py --base
  <parent sha> --node <this id>` reports the protected edit as authorized by this
  node's `paths`.
- `docs/reference/log-events.md` still describes the `request` event's fields
  truthfully (add `outcome` for a bailout only if the value is new).

# Notes

- `wall_ns` here is the **event's** `duration_ms × 1e6`, the number the epic's
  sentence is about — not `RESP_END_COST`'s ignored `wall_ns`.
- A crash that kills the worker outright (`view_died` → `502`) already charges the
  ceiling; this node is about the *contained* bailout that keeps the slot.
