---
id: MB.3.49
title: "MB.3.45 review MINORs: can_dispatch's guard has no leg, a DEBUG print in a protected test, and four stale claims"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.3.45]
paths:
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/tests/critical_charge.rs"
  - "docs/reference/log-events.md"
  - "crates/blastit-events/src/lib.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T06:12"
updated: "2026-11-15T06:23"
---

# Goal

Close the seven MINOR findings from MB.3.45's reviewer. One of them is a hole in the
fix's own guard (an unpinned predicate), one is a leftover `eprintln!` in protected
tests, one is a user-visible sentence this node made false, and the rest are comments
that describe the pre-MB.3.45 code.

# Context

MB.3.45 (commit 5ee6a57) moved request completion to `ViewIn::Closed`, introduced
`Request::view_answered`, and made every emit precede its charge. Its reviewer left:

1. `crates/blastit-scheduler/src/scheduler.rs:3748` and `:4080` still say charge is
   "called from two places" and that a cancellation/504 is charged "at release" — 11
   emit paths now charge first. Say: every emit is preceded by its charge; release is
   the fallback for a response that never ended.
2. `:4571` says `head_sent` is "the flag that says a RESP_HEAD came from the view" —
   the exact conflation item 5 removed. Name `view_answered` and note that `respond`
   also sets `head_sent`.
3. `docs/reference/log-events.md:41` says `bailed` leaves "the view process … still
   alive", contradicted by MB.3.45's own leg (a view that answers and then retires is
   `bailed`). Say "answered before it went away; not a death mid-request", and fix the
   same sentence in `crates/blastit-events/src/lib.rs:201`.
4. `crates/blastit-scheduler/tests/critical_charge.rs:104` has a leftover
   `eprintln!("DEBUG first dispatched")` printing in three protected legs. Delete it.
5. **The hole:** `scheduler.rs:2073` `can_dispatch`'s `view.exited.is_none()` is
   unpinned — the reviewer deleted it and all 17 suites stayed green. Add a leg that
   sends a second request in the ctl-before-frames window and asserts it is not
   dispatched to the exiting view (paste the red with the predicate removed).
6. `:2003`/`:3610` charge `client_cancel`/`refuse_body` while the slot is still held to
   `DONE`, so the fair-queue debit (`fair.rs:490`) now happens at the cancel/refusal
   rather than at `DONE`. Either debit the residual at release or record the endpoint
   change in the doc comment; do not leave it implicit.

# Acceptance

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
$ cargo test -p blastit-scheduler --locked
$ make check
$ make docs-check
$ uv run --no-project tools/critical_guard.py --base 5ee6a57 --node MB.3.49
```
- `can_dispatch`'s predicate has a leg that reddens when it is deleted (paste both).
- No `eprintln!` left in `critical_charge.rs`; MB.3.46's and MB.3.45's nine legs are
  otherwise untouched.
- The `bailed` gloss reads the same in `log-events.md` and `crates/blastit-events`;
  `make docs-check` green.
- The three stale comments match the code, and the fair-queue endpoint change is either
  fixed or stated.

# Notes

- `critical_charge.rs` is protected and this node's `paths` name it: add/edit legs
  only, never weaken one.
- Do not change request semantics beyond item 6; items 1–4 are prose and one deletion.
