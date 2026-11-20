---
id: MB.43
title: "Finish async-signal-safe fork cleanup in critical_child_sequence.rs (fork then child::enter allocates)"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-sys/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T06:22"
updated: "2026-11-20T06:22"
---

Deferred out of MB.42 deliberately (moved from MB.42.8). `tests/critical_child_sequence.rs:69` forks and then runs `child::enter`, which allocates by design — inside a multi-threaded `libtest` process this is the same pathology that hung `make fast-check` for ~10 minutes and was fixed for `seccomp.rs`/`critical_embedded_seccomp.rs` in MB.42.7. It passes today (luck, not safety). Fix: run the child body in a fresh `std::process::Command` probe process (marker-verified), keep every assertion and leg, and add the async-signal-safe comment at any remaining fork. Acceptance: five back-to-back `cargo test -p blastit-sys` runs at default threads with no hang, same test count, `make fast-check`. Never add RUST_TEST_THREADS pins.
