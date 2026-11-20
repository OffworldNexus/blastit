---
id: MB.42.8
title: "Finish async-signal-safe fork cleanup: critical_child_sequence.rs forks then child::enter (allocates)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.42.7]
paths: ["crates/blastit-sys/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T23:19"
updated: "2026-11-20T06:22"
---

# Goal
Finish the async-signal-safe fork cleanup in `crates/blastit-sys`: `tests/critical_child_sequence.rs:69` still `libc::fork()`s and then runs `child::enter`, which **allocates by design** — inside a multi-threaded `libtest` process, the same pathology that hung `make fast-check` for ~10 minutes and was fixed for `seccomp.rs`/`critical_embedded_seccomp.rs` in `MB.42.7`.

`MB.42.7` deliberately scoped this out (its Goal named `seccomp.rs` and `critical_embedded_seccomp.rs`). It is the **last** known fork-then-allocate site in the crate, so it is the last remaining path by which `cargo test -p blastit-sys` can deadlock under a parallel runner.

# Exact Deliverables
1. In `crates/blastit-sys/tests/critical_child_sequence.rs`, replace the `fork()` + `child::enter` child with a **fresh process** (`std::process::Command` re-running the test binary in a marker-verified probe mode, exactly as `MB.42.7` did for the thread/allocation bodies in `critical_embedded_seccomp.rs`), so the child starts single-threaded and may allocate freely.
2. Keep **every** assertion and leg: the suite must prove the same child sequence it proves today. State the test count before and after in `RESULT.md` — they must match.
3. A short comment naming the rule (`async-signal-safe between fork() and _exit(); anything that allocates runs in a fresh process`) at any `fork()` that remains.
4. Check the rest of `crates/blastit-sys` for any other `fork()` followed by allocating code, and either fix it the same way or record in `RESULT.md` why it is safe.

# Notes
- Authorized protected-test edit: this node's `paths` cover `crates/blastit-sys/**` and this `# Goal` names the file, which is what `tools/critical_guard.py` requires.
- Do **not** add `RUST_TEST_THREADS` / `--test-threads` pins anywhere — that papers over the bug instead of fixing it.
- Test-only; do not change production behaviour.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `cargo test -p blastit-sys --locked --test critical_child_sequence` (same test count as before)
- `cargo test -p blastit-sys --locked` five times back-to-back at the default thread count, no hang, wall-clock each
- `cargo test -p blastit-sys --locked -- --test-threads=8`
- `cargo clippy -p blastit-sys --all-targets --locked -- -D warnings`
- `uv run --no-project tools/critical_guard.py --base HEAD --node <id>` shows the change **authorized**
- `make fast-check`
