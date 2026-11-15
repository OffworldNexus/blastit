---
id: MB.3.45
title: "MB.3.41 review MINORs: bailed in the public outcome table, the flake asserts by type, and the release claim"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.41]
paths:
  - "crates/blastit-view/src/php.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "docs/reference/log-events.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T04:11"
updated: "2026-11-15T04:28"
---

# Goal

Close the three MINOR findings from MB.3.41's reviewer. Two are user-visible or
reviewer-visible claims that no longer match the code; one is a weaker assertion in
the very test that was just fixed for a CI flake.

# Context

MB.3.41 added `DoneOutcome::Bailed` (wire code 3) and narrowed `view_died` back to
"the view process died mid-request". Its reviewer left:

1. `docs/reference/log-events.md:40` still lists the **closed** `outcome` set without
   `bailed`. It is a public page: an operator reading it cannot name the outcome their
   own bailout produced. One clause is enough: "`bailed` — the handler exited, died or
   fataled out of a `5xx` it had answered; the view process is still alive".
2. `crates/blastit-view/src/php.rs:1902-1904` asserts the protocol error as
   `err.to_string().contains(..)`. It was the flake test; assert by type and message
   (`matches!(&ended, Err(ViewError::Protocol(msg)) if msg == "RESP_BODY before RESP_HEAD")`)
   so a rewording of some other error cannot satisfy it. This is strictly stronger.
3. `crates/blastit-scheduler/src/scheduler.rs:3945-3946` says release "charges and
   emits it if that DONE never comes", but `client_cancel` returns early on a complete
   request (`:1955`) and `expire_requests` only `continue`s (`:2793`): a live view that
   loses one DONE holds that event until `view_gone` or a `> stuck_threads` retire
   (default 4). Either release completed requests in `client_cancel`, or narrow the
   comment to the paths that do release — and if you take the code path, say in
   `RESULT.md` what a test would catch, because a held event is a hole in the
   accounting the epic's `R19` claim rests on.

# Acceptance

```console
$ cargo test -p blastit-view --locked --features php
$ cargo test -p blastit-scheduler --locked
$ make check
$ uv run --no-project tools/plan.py validate
```
- `docs/reference/log-events.md` lists `bailed` with the gloss above, and
  `make docs-check` stays green.
- The flake test asserts `ViewError::Protocol` **by type and message**; flipping the
  message in the expected string reddens it (paste the run).
- `scheduler.rs`'s release claim matches the code: either `client_cancel` releases a
  completed request (with a test that a completed request's event is not held) or the
  comment names exactly the paths that release and the follow-up is recorded. Do not
  leave a sentence that over-promises.
- No assertion weakened anywhere; `make check` exit 0.

# Notes

- `crates/blastit-view`'s default features do not compile `php.rs`, so the
  `--features php` run is the only witness for item 2.
- Do not touch `crates/blastit-php-sapi/tests/critical_shim_contract.rs` or
  `crates/blastit-scheduler/tests/critical_*.rs`.
