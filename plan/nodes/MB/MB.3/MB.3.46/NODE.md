---
id: MB.3.46
title: "The bailed charge and label need an unprivileged witness: scheduler DONE legs (critical_charge)"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.13, MB.3.41]
paths: ["crates/blastit-scheduler/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T04:14"
updated: "2026-11-15T04:14"
---

# Goal

Make `MB.3.41`'s `bailed` label and its `R19` wall charge visible to
`make check` — with no real `libphp`, no Docker, and no root — by teaching the
scheduler's own view harness to answer a `5xx` and to send `DONE`'s new
outcome.

# Context

`docs/testing/AUDIT-MB.3.13-CLOSURE.md` (gap **N1**) and both closing nodes
recorded this hole and left it ownerless:

- `crates/blastit-scheduler/tests/common/mod.rs`'s `answer()` and
  `answer_with_cost()` only ever send `resp_head(200, ...)` with
  `DoneOutcome::Completed`, so **no scheduler integration test can produce a
  view-answered `5xx`**, let alone a `DONE { Bailed }`. `MB.3.41`'s own
  follow-up names the fix (`Script::AnswerStatus(u16)` + a `DONE(Bailed)` twin);
  `MB.3.38`'s follow-up 3 says the same thing and adds that a leg asserting
  `cost_ns >= duration_ms x 1e6` would close it.
- So the production wiring `MB.3.41` added is exercised today **only** by
  `crates/blastit/tests/critical_php_embed.rs` on a host with a NTS `libphp` +
  shim (`.github/workflows/php-sapi.yml` -> `make rs-test-php-image`).
  `make check` sees only the three pure helpers in `src/scheduler.rs`'s
  `#[cfg(test)]` module (`completed_outcome`, `answered_a_bailout`,
  `charge_ns`) — never their call sites.
- Mutations that survive every unprivileged suite today and redden only the
  Docker job:
  1. delete `request.bailed = bailed` in `Scheduler::view_done`'s
     completed-`5xx` arm -> every bailout is labelled `ok` and charged the clamp;
  2. delete the `deferred` half of `Scheduler::view_end` (a `5xx` charged at
     `RESP_END` from the view's report) -> the `MB.3.38` defect returns:
     3.2 ms charged for an 18 ms request;
  3. write `DoneOutcome::Completed` where `crates/blastit-view/src/php.rs`'s
     `serve_forever` writes `end.outcome` -> the label never reaches the wire.
- The harness already has what this needs: a real `AF_UNIX` socketpair view
  driven by `Script`, `resp_head(status, len)` for any status,
  `tokio::time::pause()` for a deterministic clock, and
  `event.cost_ns()` / `event.duration_ms()` / `event.outcome()` on the emitted
  `request` event (`tests/fairness.rs` is the model).

# Acceptance

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
$ cargo test -p blastit-scheduler --locked
$ make check
$ uv run --no-project tools/plan.py validate
```

- A **new protected** file `crates/blastit-scheduler/tests/critical_charge.rs`
  (adding a `critical_*.rs` file is always allowed; `make rs-test-critical`
  picks it up by glob) holds three legs, each with its accepted twin, all on a
  paused clock so the numbers are exact:
  1. **a contained bailout pays its wall** — `RESP_HEAD 500`,
     `RESP_END_COST` reporting a small fraction of the request, then
     `DONE { Bailed }`, with the test advancing the clock so
     `duration_ms` is far larger than the report: assert `outcome == "bailed"`,
     `cost_ns >= duration_ms x 1e6`, and `cpu_ns == Some(report.cpu)` (the CPU
     figure is still the view's own and is not clamped). The accepted twin in
     the same leg: the same frames with `DONE { Completed }` must be
     `outcome == "ok"` **and** charged the clamp (report >= floor, far below
     wall), so the label and the charge are both the `DONE`'s and not the
     status's.
  2. **a scheduler-written `5xx` never pays the queue wait** — a request held
     in the queue behind another for 15 ms and then timed out (`504`) is charged
     dispatch->completion, not arrival->completion. This is the branch
     `answered_a_bailout`'s `head_sent` half protects (`MB.3.38` review MINOR 1)
     and no integration test pins it today.
  3. **an out-of-set `DONE` outcome fails closed** — a frame whose outcome byte
     is `4` (a code this build does not define) must not be read as
     `Completed`: the view is retired / the request fails as `view_died`, and
     the event is emitted once. `DoneOutcome::from_code(4)` is unit-tested; the
     *scheduler's reaction* to a hostile view is not, and a `_ => Completed`
     style fallback would free a slot and label a failed request `ok`.
- `tests/common/mod.rs` grows the smallest extension that makes this possible
  (`Script::AnswerStatus { status, body, cost, done }`, or a status /
  done-outcome field on the existing `Cost`/`HeldCost`), **without** changing
  any existing script's frames: `fairness.rs`, `lifecycle.rs`, `admission.rs`,
  `flow.rs`, `isolation.rs`, `memory.rs`, `head_allocs.rs` and the four
  `critical_*.rs` files that share the harness stay green untouched.
- Prove the legs bite by temporarily reverting each of the three mutations above
  (`request.bailed = ...`, `deferred`, `end.outcome`) in your branch and pasting
  the red runs into `RESULT.md`; restore them and paste green. A leg that stays
  green under (1) or (2) is not the oracle.
- No `#[ignore]`, no skip construct, no weakened existing assertion
  (`make critical-skip-check` clean, `critical_guard` clean). Runtime of the new
  file stays in the tens of milliseconds: it is in `make check`.

# Notes

- Do **not** touch `crates/blastit-scheduler/src/scheduler.rs` or
  `crates/blastit-view/src/php.rs`: `MB.3.45` owns both at the time of writing.
  If a leg cannot be written without a production change, record it in
  `RESULT.md` and file a follow-up instead of widening this node.
- `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` and the other
  `critical_*.rs` files in that directory use the same `mod common;` harness —
  run the whole crate suite, not just the new file.
- The `outcome` vocabulary in the `request` event is `blastit-events`';
  `bailed` is already decoded and asserted there, so this node only has to prove
  the scheduler *chooses* it.
