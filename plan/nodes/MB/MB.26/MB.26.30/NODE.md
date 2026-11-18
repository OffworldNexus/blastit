---
id: MB.26.30
title: "Protected critical_backend_capability suite: the design R12 refusal matrix in both build configurations"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/tests/critical_backend_capability.rs, crates/blastit/src/stage0.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Write the protected `crates/blastit/tests/critical_backend_capability.rs` suite that design
R12 requires: the capability gate is currently proved by prose and by hand.

# Context

- `MB.3.11` made `python` a cargo feature; the interpreter-free build must **refuse**, not
  skip. Its gate wiring is now witnessed by `tests/no_python_build.rs` (which is
  `#![cfg(not(feature = "python"))]`, i.e. empty in the wheel build).
- R12 is about the *decision*, not one build: a manifest/CLI combination this build cannot
  serve is `exit 1` with an actionable sentence, and a combination it can serve boots. The
  protected suite must hold that in both configurations.
- Message shapes live in `crates/blastit/src/stage0.rs` (`no_python_support`) and must name
  the way out; a bare "not supported" is a failure.

# Acceptance

```console
$ cargo test -p blastit --test critical_backend_capability --locked
$ cargo test -p blastit --locked
$ make check
```
- Denied cases (each asserting exit 1 + the sentence, not just a non-zero code): a WSGI
  `app` in a `python`-less build; `--learn`; a `[route.php]` route in a `python`-less
  build (served by the embedded `libphp` worker of `MB.3.13`, never `uv add`).
- Accepted twins: a static-only manifest boots and serves 200 in the same binary; a
  `[route.php]` manifest once `MB.3.13` lands is not refused by *this* gate.
- The suite must fail if the gate is removed (comment the call site and it goes red) —
  say which assertion proves that in a comment.
- Where a build configuration makes a row unrunnable, the row is gated by the same cfg and
  documented, never silently dropped (`grep` must show why).

# Notes

- New protected file: allowed by `tools/critical_guard.py`. Do not edit
  `no_python_build.rs` into it; they answer different questions (this one is R12's oracle).

# Provenance

Re-filed from `MB.3.15` at the `v0.2.0` gate: R12 runtime capability refusal works and is tested in `no_python_build.rs` and `static_feature_off.rs`; promoting the cross-build refusal matrix to a protected `critical_backend_capability.rs` (using `Blastfile` `entrypoint php` instead of retired `[route.php]`) is post-`0.2.0` coverage hardening.
