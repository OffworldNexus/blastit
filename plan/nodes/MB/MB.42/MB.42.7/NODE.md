---
id: MB.42.7
title: "Make blastit-sys seccomp unit-test forked children async-signal-safe (fork-inside-libtest deadlock)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2.4]
paths: ["crates/blastit-sys/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T22:52"
updated: "2026-11-19T23:19"
---

# Goal
Make `cargo test -p blastit-sys` **impossible to deadlock** by keeping every forked child in `crates/blastit-sys`'s tests strictly **async-signal-safe** between `fork()` and `_exit()`.

**Authorized protected-test edit (lead, 2026-11-19):** `crates/blastit-sys/tests/critical_embedded_seccomp.rs` may be hardened in this node. Its forked children (`libc::fork()` at `:244`, via `try_fork()` at `:163,340,368`) call `install()`, which reads `/proc/self/status` and therefore allocates inside a multi-threaded `libtest` process — the identical deadlock pathology. Apply the same rule there (resolve the plan in the parent, child does `prctl`+`seccomp` + raw syscalls + `close_range` + single `_exit` code, parent asserts after `waitpid`). **Keep every assertion and every leg** — this is a child-body rewrite, not a relaxation: do not delete a case, do not `#[ignore]`, do not weaken an expectation.


# Why (measured, not theoretical)
A `make fast-check` run hung for ~10 minutes in `blastit-sys`'s unit tests: `seccomp::tests` called `libc::fork()` from inside the **multi-threaded `libtest` runner**, and the forked child deadlocked in `futex_wait_queue` — the allocator / test-harness mutex was held by *another* thread at the instant of the fork, so it can never be released in the child. The child also inherited the non-`O_CLOEXEC` write end of a pipe belonging to `fd::tests::dup_*`'s test, which kept that read open. Killing the child unblocked everything. This can hang CI for any commit that touches `blastit-sys`.

# Exact Deliverables
1. **Async-signal-safe child bodies.** In `crates/blastit-sys/src/seccomp.rs`'s tests, every `unsafe { libc::fork() }` child must run **only** raw syscalls between the fork and `libc::_exit(code)`. Forbidden in the child: `Vec`, `String`, `format!`, `println!`/`eprintln!`, `assert!`/`assert_eq!`, `unwrap`, `Box`, any `Drop` that deallocates, any `panic!`, and any function that can allocate or take a lock. Encode the result as a **precomputed** value (build the buffer *before* the fork; in the child only `libc::write(fd, ptr, len)`), and let the parent do all assertion/formatting after `waitpid`.
2. **No inherited descriptors.** Give any pipe the test creates `O_CLOEXEC` (or close the inherited ends in the child with a raw `close()` before the syscall under test), so a sibling test's pipe cannot be held open by this child. Check `crates/blastit-sys/src/fd.rs`'s `dup_*` tests use `O_CLOEXEC` too.
3. **No `fork()` in a multi-threaded test binary where avoidable.** Where the thing under test does not actually require a real process (e.g. a seccomp filter install, a syscall refusal), prefer asserting it in-process or in a **child process spawned via `std::process::Command`** (a fresh single-threaded process) over `fork()` inside `libtest`. Keep `fork()` only where a real fork is the subject, and then apply item 1.
4. A short comment at each remaining `fork()` naming the async-signal-safe rule, so the next editor cannot reintroduce an allocation.

# Notes
- This is test-only code; **do not change production behaviour** in `seccomp.rs`/`fd.rs`.
- Do not modify `crates/*/tests/critical_*.rs` (this is `src/**` unit-test code).
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `cargo test -p blastit-sys --locked` run **5 times back-to-back** with `--test-threads=8` (or the default) with **no hang** and no failure; paste the wall-clock of each run
- `cargo test -p blastit-sys --locked -- --test-threads=1` clean
- `cargo clippy -p blastit-sys --all-targets --locked -- -D warnings`
- `make fast-check`
