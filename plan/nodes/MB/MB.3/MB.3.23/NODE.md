---
id: MB.3.23
title: "WorkerRuntime: the worker-side contract paired with Upstream (design backend-runtimes §3)"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.11]
paths: ["crates/blastit-view/src/**", "crates/blastit/src/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T17:04"
updated: "2026-11-14T18:29"
---

# Goal

Add the worker-side `WorkerRuntime` trait from `docs/design/backend-runtimes.md`: the
contract a per-route worker implements so the scheduler never learns which language it is
serving.

# Context

- Design of record: `docs/design/backend-runtimes.md` §3 (contracts) + `ADR-0009`.
- Pairs with the scheduler-side `Upstream` (`ADR-0008` Decision 1 survives). `Upstream` is
  the dispatch contract; `WorkerRuntime` is what the child process runs. `MB.9.3`'s
  `ViewUpstream` holds one of these and must serve `Executor::Wsgi` **and** `Executor::Php`
  with no backend-kind branch in dispatch.
- Object-safe, no dependency beyond what `blastit-view` already has; the concrete runtimes
  land in `MB.3.24` (WSGI) and `MB.3.13` (PHP).

# Acceptance

```console
$ cargo test -p blastit-view --locked
$ make check
```
- The trait is object-safe (`fn _obj(_: Box<dyn WorkerRuntime + Send>)`) and its returned
  future is `Send`.
- The `view.chan` framing (`wire`/`flow`, made public by `MB.3.13`'s prerequisite note) is
  the only transport the trait may name — no socket type, no language type.
- A compile-time test that a second implementation can be written **outside** this crate
  (so the trait is genuinely the seam, not a private convenience).
