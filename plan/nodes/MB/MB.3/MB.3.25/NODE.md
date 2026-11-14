---
id: MB.3.25
title: "SeccompRole::{View, EmbeddedView} and feature = testutil without widening View"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.23]
paths:
  - "crates/blastit-sys/src/**"
  - "crates/blastit-sys/tests/**"
  - "crates/blastit-sys/Cargo.toml"
  - "crates/blastit/src/**"
  - crates/blastit/Cargo.toml
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T17:04"
updated: "2026-11-14T19:02"
---

# Goal

Add `SeccompRole::EmbeddedView` (the PHP worker's filter) beside `View`, and a
`feature = "testutil"` for tests that must install/inspect a filter without shipping the
hook in release builds.

# Context

- `docs/design/backend-runtimes.md` §2/§6 and `ADR-0010`: the embedded PHP worker needs the
  same `View` guarantees plus whatever the SAPI shim genuinely requires — and any extra
  syscall must be **justified for every platform or not added at all** (the musl rule).
- `EmbeddedView` must not silently widen `View`: a syscall added for PHP is either safe for
  WSGI too (then it goes in `View`, with a test) or it stays narrow and PHP loses the
  capability.
- `testutil` gates test-only entry points (installing a filter, reading the active role) so
  the production build carries no test API.

# Acceptance

```console
$ cargo test -p blastit-sys --locked
$ cargo test -p blastit --locked
$ make check
```
- `EmbeddedView` is a real role with a documented syscall table, and a test asserts the
  `View` table is **unchanged** (an accidental widening is the failure mode).
- With `--no-default-features`, `testutil` symbols are absent (`nm`/`cfg` assertion).
- Egress still reflects the manifest: no role grants `AF_INET` unless `egress` is on.
