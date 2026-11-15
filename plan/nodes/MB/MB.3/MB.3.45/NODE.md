---
id: MB.3.45
title: "Scheduler-answered requests must not pay the queue wait, and three claims that no longer match the code"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.41]
paths:
  - "crates/blastit-view/src/php.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/tests/critical_charge.rs"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - "docs/reference/log-events.md"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T04:11"
updated: "2026-11-15T06:10"
---

# Goal

Close the three MINORs from `MB.3.41`'s reviewer **and** the three binding
scheduler-accounting items recorded against this node after `MB.3.46` could not
deliver them. Two are public or reviewer-visible claims that no longer match the
code; one is a weaker assertion in the very test that was just de-flaked; items 4–6
need `src/**` and a public sentence, and `MB.3.46` was scoped to
`crates/blastit-scheduler/tests/**`, so this node is their only owner.

# Context

`MB.3.41` added `DoneOutcome::Bailed` (wire code 3), narrowed `view_died` back to
"the view process died mid-request", and claimed a scheduler-written `502`/`504`
never pays the queue wait. Its reviewer left items 1–3; `MB.3.46` closed two of its
four legs and left items 4–6 here (see `plan/nodes/MB/MB.3/MB.3.46/RESULT.md`
Follow-ups 1–3 and the second `[lead]` entry in this node's `LOG.md`). All six are
binding: the node must not be closed with any of them open.

1. `docs/reference/log-events.md:39-40` still lists the **closed** `outcome` set
   without `bailed`. It is a public page: an operator reading it cannot name the
   outcome their own bailout produced. One clause is enough: "`bailed` — the handler
   exited, died or fataled out of a `5xx` it had answered; the view process is still
   alive".
2. `crates/blastit-view/src/php.rs:1902-1904` asserts the protocol error as
   `err.to_string().contains(..)`. It is the test that went red on a CI flake; assert
   by type **and** message:
   `matches!(&ended, Err(ViewError::Protocol(msg)) if msg == "RESP_BODY before RESP_HEAD")`.
   Strictly stronger: no rewording of another error, and no other `ViewError`
   variant, can satisfy it.
3. `crates/blastit-scheduler/src/scheduler.rs:3945-3946` says release "charges and
   emits it if that DONE never comes", but `client_cancel` returns early on a
   complete request (`:1955`) and `expire_requests` only `continue`s (`:2793`): a live
   view that loses one `DONE` holds that event until `view_gone` or a
   `> stuck_threads` retire (default 4). Either release completed requests in
   `client_cancel`, or narrow the comment to exactly the paths that do release. A
   held event is a hole in the accounting the epic's `R19` claim rests on, so say in
   `RESULT.md` what a test would catch.
4. **A scheduler-written answer emits `cost_ns: 0`.** Every scheduler-written answer
   (`finish_immediately` `:3517-3518`, `reject` `:3474-3475`, `fail_request` `:3555`,
   `expire_requests` `:2805`) calls `emit_request` **before** `release()` runs
   `charge(..)`, and `emit_request` reads `charged_ns`. Probed on a `504`:
   `cost=0`, `duration=21 ms`. Needs a `src/**` change — charge before the emit where
   the span is already final, or expose the flow's debit — plus a leg.
5. **`respond()` sets `head_sent = true` for a head the *scheduler* wrote**
   (`scheduler.rs:3434-3441`), so `answered_a_bailout(head_sent, status, bailed)`
   (`:4077`) bills a `504` whose view later sends `DONE{Bailed}` its
   arrival→finished wall, **queue wait included**, instead of dispatch→completion.
   That contradicts `R19`'s "a scheduler-generated `502`/`504` keeps its
   dispatch→completion charge and never pays the queue wait"
   (`ARCHITECTURE.md:1536-1542`). A `5xx` the scheduler wrote needs its own "a view
   answered this" bit, distinct from "a head went out", plus a leg where the
   scheduler answers and the view then completes.
6. `docs/reference/log-events.md` must also state what `cost_ns` is for a request the
   scheduler answered itself — the same paragraph as item 1. As built that is `0`
   (item 4); once item 4 charges before the emit it is the dispatch→completion
   charge. Whichever you ship, the page, item 4's code and `ARCHITECTURE.md:1540`
   must say the same thing.

# Acceptance

```console
$ cargo test -p blastit-view --locked --features php
$ cargo test -p blastit-scheduler --locked
$ make docs-check
$ make check
$ uv run --no-project tools/plan.py validate
```

- **Item 1.** The `outcome` paragraph lists eight names and includes the `bailed`
  gloss above:
  `grep -n 'outcome` is one of' -A3 docs/reference/log-events.md | grep bailed`
  prints a hit, and `make docs-check` exits 0. Red: the grep prints nothing (the page
  still closes the set at seven) or Zensical `--strict` fails.
- **Item 2.** `cargo test -p blastit-view --locked --features php
  a_response_body_before_the_head_is_a_protocol_error` passes with the assertion
  written as `matches!(&ended, Err(ViewError::Protocol(msg)) if msg == "RESP_BODY
  before RESP_HEAD")` — no `to_string().contains(..)` left in `php.rs`. Red proof:
  temporarily change the expected string to `"RESP_HEAD before RESP_BODY"`, the test
  fails, paste the tail, restore. The `--features php` run is the only witness:
  without it `php.rs` is not compiled at all (the bare run reports `0 filtered out`
  for this test).
- **Item 3.** The release claim and the code agree: either the comment at
  `scheduler.rs:3935-3950` names only the paths that call `release` (no promise about
  a `DONE` that never comes), or `client_cancel` no longer returns early on a
  completed request. Take the code path only with a leg in
  `crates/blastit-scheduler/src/scheduler.rs`'s `#[cfg(test)] mod tests` (or the
  smallest helper the change introduces) proving a completed request's event is not
  held; `crates/blastit-scheduler/tests/**` is **not** in this node's paths. Paste
  the run, and say in `RESULT.md` which path you took.
- **Item 4.** Pinned by `#[test] a_scheduler_written_504_is_charged_before_its_request_event`
  in `crates/blastit-scheduler/src/scheduler.rs`'s `#[cfg(test)] mod tests` (not
  `tests/**`): a scheduler-answered `504`'s `request` event carries a **non-zero**
  `cost_ns` equal to its dispatch→completion charge and `<= duration_ms × 1e6`.
  `cargo test -p blastit-scheduler --locked a_scheduler_written_504` passes. Red:
  restoring the emit-before-charge shape at `:3517-3518` makes the event read
  `cost_ns: 0` and the test fails — paste that run.
- **Item 5.** Pinned by `#[test] a_scheduler_answered_504_stays_out_of_the_bailout_bill`
  in the same `mod tests`: the "a view answered this" bit is **false** for a
  scheduler-written head, so a `504` the scheduler answered and the view then
  completes with `DONE{Bailed}` is charged dispatch→completion
  (`cost_ns < duration_ms × 1e6` with a non-zero queue wait, i.e. not
  arrival→finished). Red: feeding the new bit from `head_sent` (today's `:3440`)
  bills the queue wait and the test fails — paste that run.
- **Item 6.** The same paragraph states the scheduler-answered `cost_ns` rule, and it
  matches item 4's shipped code and `ARCHITECTURE.md:1540`; `make docs-check` exits
  0. Red: the page says `0` while item 4 charges a non-zero `cost_ns` (compare the
  sentence against the `cost_ns` item 4's leg asserts), or the paragraph contradicts
  `ARCHITECTURE.md:1540`.
- No assertion weakened anywhere; `make check` exits 0 and
  `uv run --no-project tools/plan.py validate` prints `plan OK`.

# Notes

- `crates/blastit-view`'s default features do not compile `php.rs`, so the
  `--features php` run (item 2) is the only witness; a green default run proves
  nothing about that file.
- `crates/blastit-scheduler/tests/**` is **not** in this node's paths — every Rust
  leg above lives in `scheduler.rs`'s `#[cfg(test)] mod tests`. If one is genuinely
  unreachable there, `RESULT.md` must name the test that would catch it and why this
  tier cannot hold it; do not silently drop a leg.
- `docs-check` owns `--strict`: a broken anchor in the edited paragraph fails it.
- `ARCHITECTURE.md` and `docs/design/` are not in this node's paths. If items 4–5
  falsify a claim there beyond `:1540`, record it under `## Follow-ups` in
  `RESULT.md`.
- Load `security-invariants` before touching the charge/emit paths: `R19`'s
  accounting is a security invariant — a compromised view must not buy queue time,
  and a scheduler-answered request must not be billed for a worker it never had.
- Do not touch `crates/blastit-php-sapi/tests/critical_shim_contract.rs` or
  `crates/blastit-scheduler/tests/critical_*.rs`.
