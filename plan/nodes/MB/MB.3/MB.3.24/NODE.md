---
id: MB.3.24
title: WsgiRuntime on WorkerRuntime + protected critical_view_transport.rs (no behaviour change)
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.23]
paths:
  - "crates/blastit-view/src/**"
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_view_transport.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T17:04"
updated: "2026-11-14T17:04"
---

# Goal

Port the existing WSGI worker onto `WorkerRuntime` (`WsgiRuntime`) with **no behaviour
change**, and protect the transport with `crates/blastit/tests/critical_view_transport.rs`.

# Context

- `MB.3.23` defines the trait; this node proves it by moving the current WSGI worker behind
  it. The protected suite is the guarantee that the refactor did not move a wall.
- The transport rows worth pinning: `view.chan` frame framing and body credit, `app.output`
  line cap (2048), the `RESP_END_COST` report shape, cancel/abort, and a worker crash
  producing exactly one `Done(Fault)`.
- `critical_*` is a **new** file (additions always allowed); no existing protected suite may
  be edited.

# Acceptance

```console
$ cargo test -p blastit --test critical_view_transport --locked
$ cargo test -p blastit --locked
$ make check
```
- Every existing WSGI behaviour test passes **unmodified**; the protected suite adds the
  five transport rows above, each with a denied case and its accepted twin.
- `git diff` shows no change under `crates/blastit/tests/critical_rewrite.rs` or any other
  existing `critical_*.rs`.
