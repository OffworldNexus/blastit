---
id: MB.3.50
title: "Sweep the scheduler's charge/outcome/release doc claims instead of patching three more"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.49]
paths: ["crates/blastit-scheduler/src/scheduler.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T06:25"
updated: "2026-11-15T06:25"
---

# Goal

Stop the treadmill: the scheduler's doc comments about charging, outcomes and release
keep describing the code as it was before `MB.3.45`/`MB.3.49`, and each review pass finds
three more. Sweep the whole surface in one go, grep-driven, and make every claim about
`charge`/`emit`/`release`/`bailed`/`view_died` match the code that ships.

# Context

`MB.3.45` moved completion to `ViewIn::Closed`, introduced `Request::view_answered`, made
every emit precede its charge, and narrowed `view_died` to "the process died
mid-request". `MB.3.49` fixed the comments its own review caught. Its reviewer then found
three more in the same file, which is the signal that this needs a sweep rather than three
patches:

1. `crates/blastit-scheduler/src/scheduler.rs:3760-3761` and `:4097` name "a view that went
   away"/"a view that died with nothing written" as the release-time **uncharged**
   fallback — but `view_gone:3459` calls `fail_request`, which charges at `:3678`, before
   `release`. The real second path is a `DONE` completing a head-only response with no
   `RESP_END` (`view_done:3939-3980`). Replace the dead-view example with that path.
2. `:4210-4211` — `completed_outcome`'s doc says `bailed` is not `view_died` "because the
   process is alive and takes the next request", which `MB.3.45`'s own retire leg
   falsifies (a view that answers and then retires is `bailed`). Say what
   `blastit-events`' gloss now says: it answered before it went away, and its ordered
   `view.chan` frames are read before its close.
3. `:3764-3766` — "a client cancellation and a refused body both leave the slot held to
   the view's `DONE`" holds only when the request already has a view; the view-less arms
   release in the same turn (`:2013-2016`, `:3619-3621`). Qualify it.

# Acceptance

```console
$ cargo test -p blastit-scheduler --locked
$ make check
$ make docs-check
```
- Every claim in `scheduler.rs` about **when** a request is charged, **who** charges it,
  **what** `release` is for, and what each `Outcome` means, matches the code. Find them by
  grepping the file for `charge`, `charged`, `release`, `emit_request`, `bailed`,
  `view_died`, `head_sent`, `view_answered`, `queue` and reading every hit's paragraph —
  do not fix only the three lines above; say in `RESULT.md` how many claims you inspected
  and which you changed.
- Where a claim is surprising but true (e.g. the fair-queue debit endpoint), it stays and
  is precise; where it is false, it is corrected. No paragraph may imply a request can be
  emitted uncharged.
- Comments only: no behaviour change, no test, no assertion, no new dependency. `make check`
  and `make docs-check` green.

# Notes

- If the sweep finds a claim that is false because the CODE is wrong, stop and report it
  instead of documenting the bug — that is a different node.
