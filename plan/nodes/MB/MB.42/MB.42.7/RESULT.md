# MB.42.7 — `blastit-sys` seccomp unit tests are fork-safe

## Status: done

`cargo test -p blastit-sys` no longer forks a child that can allocate. The
measured hang (`futex_wait_queue` in a forked child, then an OOM kill while
spawning a thread in that child) is gone: every child body in
`crates/blastit-sys/src/seccomp.rs`'s tests now makes **raw syscalls only**
between `fork()` and `_exit()`.

## Root cause (confirmed by reading the code, not guessed)

`seccomp::tests` forked from inside the multi-threaded `libtest` runner and then
called `install(role)` in the child. `install()` counts threads by reading
`/proc/self/status` (`proc::thread_count` → `std::fs::read_to_string`), which
**allocates**; `clone3_is_enosys_…` additionally called `std::thread::spawn` in
the child, which allocates and starts a thread. At the instant of the fork
another harness thread could hold the allocator's lock, and the child has no
other thread to release it — deadlock (`futex_wait_queue`), and, for the thread
spawn, unbounded allocation until the OOM killer. The child also inherited the
non-`FD_CLOEXEC` descriptor that `fd::tests::dup_onto_*` deliberately leaves in
the test process, so the hung child held a sibling test's pipe open.

## What changed (test-only; no production behaviour touched)

`crates/blastit-sys/src/seccomp.rs` (`#[cfg(test)] mod tests`):

1. **`Plan` — the filter resolved in the parent.** `Program::resolve` writes
   into a fixed `[libc::sock_filter; MAX_PROGRAM]` stack array, so the parent
   builds and resolves the BPF program where allocating is safe and the child
   only hands the array to the kernel.
2. **`install_raw(&Plan)`** does `prctl(PR_SET_NO_NEW_PRIVS)` + `seccomp(2)`
   (with the `prctl(PR_SET_SECCOMP)` fallback) from raw syscalls. `install()`
   itself is never called in a child any more, because its `/proc` read
   allocates.
3. **`in_filtered_child` / `filtered_child`** are the only `fork()` users, with
   a comment naming the async-signal-safe rule; the rule is also stated once at
   the top of the module's tests (what is forbidden: `Vec`, `String`,
   `format!`, `println!`, `assert!`, `unwrap`, `Box`, `panic!`, any freeing
   `Drop`, anything that allocates or locks).
4. **`close_inherited()`** runs `close_range(3, u32::MAX, 0)` in the child
   before installing the filter, so no sibling test's non-`FD_CLOEXEC`
   descriptor is held open (`ENOSYS` on kernels < 5.9 is ignored: the child
   exits microseconds later).
5. **Raw probe helpers** replaced the `Option<i32>` helpers: `raw_socket`,
   `raw_socketpair`, `raw_ptrace`, `raw_wait`, `raw_clone`, `raw_clone3`,
   `raw_fork`/`raw_vfork`, `raw_execve`/`raw_execveat`, `raw_setsid`,
   `raw_setpgid`, `raw_errno` (`*libc::__errno_location()`, no `io::Error`).
   They return `0` when permitted and the `errno` otherwise, so a child's answer
   *is* its exit code and every assertion/format happens in the parent after
   `waitpid`.
6. **Two bodies moved to a fresh process** (`run_probe` re-runs this same test
   binary with `--test-threads=1 --nocapture probe_runner` and a probe name in
   `BLASTIT_SYS_SECCOMP_PROBE`), because they cannot be async-signal-safe:
   - `install_refuses_a_process_that_is_not_single_threaded` needs a second
     thread *and* `install()`, so it runs in a process of its own;
   - `clone3_is_enosys_and_a_thread_still_starts_on_both_views` needs
     `std::thread::spawn` after the filter, one process per role.
   The probe writes `blastit-sys-probe:<name>:<code>` to fd 1 with a raw
   `libc::write` and `process::exit`s with the code; the parent parses that
   marker, so a test filter that matched nothing (exit `0`, nothing run) is a
   failure rather than a false pass.
7. Coverage grew slightly: `socketpair_is_unix_only_for_every_role` and
   `every_role_refuses_ptrace` now iterate all seven roles (they used to skip
   one and two respectively).

`crates/blastit-sys/src/fd.rs`:

8. `dup_onto_leaves_a_usable_descriptor_without_cloexec` now takes the `dup2`
   target into an `OwnedFd`, so a panicking assertion closes it instead of
   leaking a descriptor without `FD_CLOEXEC` into a sibling test's child.
9. `dup_onto_onto_itself_only_clears_the_flag` restores `FD_CLOEXEC` on the
   pipe end it deliberately cleared, for the same reason.

`crates/blastit-sys/src/testutil.rs`:

10. `child_status`'s `SAFETY` comment now states the rule a body must follow
    (async-signal-safe only, no allocation), pointing at `seccomp.rs` as the
    worked example. No behaviour change.

**No `RUST_TEST_THREADS`/`--test-threads` pin was added to the `Makefile`** — the
bug is fixed, not papered over; the tests run at the default thread count.

## Acceptance

### `cargo clippy -p blastit-sys --all-targets --locked -- -D warnings`

```text
    Checking blastit-sys v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-sys)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.09s
```

### `cargo test -p blastit-sys --locked`, five times back-to-back, default threads

```text
run 1: exit=0 wall=0.73s 5 ok-suites, failures=0
run 2: exit=0 wall=0.71s 5 ok-suites, failures=0
run 3: exit=0 wall=0.69s 5 ok-suites, failures=0
run 4: exit=0 wall=0.71s 5 ok-suites, failures=0
run 5: exit=0 wall=0.71s 5 ok-suites, failures=0
```

(5 suites = lib unit tests, 3 `critical_*` integration binaries, doc tests; no
hang, no failure. Before this change the same command hung for minutes and was
once OOM-killed.)

### `cargo test -p blastit-sys --locked -- --test-threads=1`

```text
== --test-threads=1: exit=0 wall=0.92s
test result: ok. 175 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### `cargo test -p blastit-sys --locked -- --test-threads=8`

```text
== --test-threads=8: exit=0 wall=0.73s
test result: ok. 175 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The probe path is really exercised (it would panic with "probe … never ran"):

```text
running 175 tests
test seccomp::tests::clone3_is_enosys_and_a_thread_still_starts_on_both_views ... ok
test seccomp::tests::probe_runner ... ok
test seccomp::tests::install_refuses_a_process_that_is_not_single_threaded ... ok
test result: ok. 175 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### `make fast-check`

```text
== make fast-check: exit=0 wall=168.5s
   Compiling blastit v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.27s
     Running tests/critical_handoff.rs (target/debug/build/blastit/9bb580532eaf7322/out/critical_handoff-9bb580532eaf7322)

running 14 tests
...
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

The whole gate (`guard`, `critical-guard`, `rs-fmt`, `rs-lint`, `rs-doc`,
`rs-test`, `py-lint`, `plan-validate`, `universal-smoke`) completes in **168.5 s**
with no hang and no failure — this unblocks the `MB.42.2` gate.


## Follow-ups

- `crates/blastit-sys/tests/critical_embedded_seccomp.rs` still runs
  `install()` (which reads `/proc/self/status`) inside its own forked children.
  It is a protected critical suite, so it was left untouched, but it carries the
  same latent (much rarer) pathology; a follow-up node with `critical_guard.py
  --node <id>` authorization should convert it to the precomputed-filter pattern
  used here (`Plan` + raw install).
- `proc::tests::chdir_moves_the_working_directory` and the `privs::tests`
  children also call allocating helpers in a forked child (out of this node's
  measured failure and its stated scope: `seccomp.rs` + the `fd.rs` `dup_onto`
  descriptors). Worth the same sweep if `blastit-sys` ever hangs again.

## Progress (run 2)

Both follow-ups above are closed, plus a production fix the sweep uncovered.
Every `fork()` in `crates/blastit-sys`'s tests now has an async-signal-safe child,
and the rule is written at each one.

### The production fix that made it possible: `thread_count` allocates nothing

`proc::thread_count` is called by **both** `fork()` and `install()`, and both run
in a child of `fork()`. It read `/proc/self/status` with
`std::fs::read_to_string`, so it allocated — inside the very child the crate's
own `MAX_PROGRAM` doc says must not allocate ("`install` runs in a freshly
forked child, where allocating is not safe"). It now opens the file with raw
`open(O_RDONLY|O_CLOEXEC)` and scans it into a **fixed 512-byte stack buffer**
(`scan_threads`, a three-phase byte scan that carries its state across reads, so
the field may straddle a chunk). Same return values, same errors, same
`/proc`-only source: behaviour is unchanged, but `fork()` and `install()` are now
allocation-free on their success paths. Three new unit tests pin the scanner
against a synthetic status file written through a pipe (including a field that
straddles the chunk boundary, a missing field, and a line without a number).

### `PreparedFilter`: prepare in the parent, install with two syscalls

`seccomp::install(role)` = arch + thread-count checks, then build, then
`prctl`+`seccomp`. The BPF program lives in a fixed stack array, so the *build*
never allocated; the *thread count* did. With that fixed, the remaining defect in
`install` was that its failure path built an `io::Error` (a `Box`).

So the BPF half is now its own public type:

```rust
pub struct PreparedFilter { /* [sock_filter; MAX_PROGRAM] + len */ }
impl PreparedFilter {
    pub fn for_role(role: SeccompRole) -> io::Result<Self>;   // resolves in the caller
    pub fn install(&self) -> Result<(), InstallFailure>;      // prctl + seccomp only
}
pub struct InstallFailure { pub stage: &'static str, pub errno: i32 }  // Copy, heap-free
```

`install(role)` is now `PreparedFilter::for_role(role)?.install()` plus the two
checks, and it turns an `InstallFailure` into exactly the same
`Error::Seccomp { stage, source }` message as before (`stage` is still
`"PR_SET_NO_NEW_PRIVS"` or `"installing the filter"`; `source` is
`io::Error::from_raw_os_error(errno)`, the same `errno` read at the same call).
`install_program` is gone — its body *is* `PreparedFilter::install` — so there is
now one implementation of "hand a program to the kernel" instead of two. The
unit tests' duplicated `Plan`/`install_raw` (added in run 1) are deleted in
favour of the real type, which is what keeps the child and production paths from
drifting apart.

### `crates/blastit-sys/tests/critical_embedded_seccomp.rs` (authorized)

Protected file, authorized in `NODE.md` and verified with `critical_guard.py`
below. **Same six tests, same six legs, same expectations** — a child-body
rewrite, not a relaxation:

- `in_child(label, role, body)` is the only `fork()`; it calls
  `close_range(3, u32::MAX, 0)` (`close_inherited`) and then
  `blastit_sys::install(role)` — the *public* entry point, allocation-free now —
  runs `body`, and `_exit`s with one code. `254` means "could not install", and
  the parent says so; every other code is the leg's verdict, asserted in the
  parent after `waitpid`.
- All probe helpers return `i32` (`0` permitted, else the `errno`, read with
  `*libc::__errno_location()`), so no child builds an `io::Error`.
- One body must still allocate — `std::thread::spawn`, which proves glibc's
  `pthread_create` falls back to `clone` behind the filter. It runs in a fresh
  process (`run_probe` via `Command`, marker-verified) and, because a new
  `#[test]` would have changed the suite's test count, the probe entry point
  **is** `both_views_refuse_clone3_and_still_start_a_thread` itself: with
  `PROBE_VAR` unset it spawns the two probes, with it set it runs one and exits.
- The probe uses `PreparedFilter` rather than `install`: a `libtest` harness
  always has a thread of its own, so `install`'s single-threaded precondition
  cannot hold there. That is a test process, not a boot.

### `proc::tests` and `privs::tests`

- `fork_unchecked_makes_a_child_that_is_not_the_parent`: `getpid`/`getppid` only
  — already safe; the rule is now named in its body.
- `fork_accepts_a_single_threaded_child`: calls the real `fork()`, whose only
  work is `thread_count` (now allocation-free) plus `waitpid` — so the leg keeps
  the checked entry point and is safe.
- `chdir_moves_the_working_directory` / `chdir_reports_a_missing_directory`:
  moved to a **fresh process** (`run_probe`). The subject (`crate::chdir`, whose
  rustix `cstr` conversion allocates) and the reader
  (`std::env::current_dir`) both allocate; the leg still calls `crate::chdir`
  and still asserts the same thing.
- `set_umask_*`, `set_rlimits_*`, `harden_child_*`, `set_dumpable_round_trips`:
  stay in forked children — `libc::umask`, `setrlimit`/`getrlimit`, `prctl` are
  syscalls with no allocation, so no change was needed beyond the rule comment.
- The five `drop_privileges` legs (`harden_child_after_drop_privileges`,
  `dropping_privileges_after_hardening`, `drop_privileges_without_root`,
  `drop_privileges_to_a_view_uid`, `drop_privileges_clears_a_rootless_capability_set`):
  moved to fresh processes. `drop_privileges` allocates by design — its own doc
  says so ("the capability sets are read back from `/proc/self/status` … it is
  written for a child of a deliberately single-threaded process") — so it cannot
  run in a `fork()` child of the harness. Every assertion (ids, supplementary
  groups, `setuid(0)` failing, the `"setgroups(0, NULL)"` stage, the cleared
  capability sets) is preserved verbatim inside the probe body; `root_only()`
  still gates the parent, so the skip line is unchanged.
- The shared machinery (`run_probe`, `report_probe`, `PROBE_VAR`,
  `PROBE_MARKER`) now lives once, in `crate::testutil`, and `seccomp`'s
  run-1 copy is gone.

### Acceptance (run 2)

`cargo test -p blastit-sys --locked`, five times back-to-back, default threads:

```text
run 1: exit=0 wall=0.74s ok-suites=5 failed=0
run 2: exit=0 wall=0.67s ok-suites=5 failed=0
run 3: exit=0 wall=0.70s ok-suites=5 failed=0
run 4: exit=0 wall=0.72s ok-suites=5 failed=0
run 5: exit=0 wall=0.67s ok-suites=5 failed=0
```

```text
== --test-threads=8: exit=0 wall=0.72s
test result: ok. 180 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
== --test-threads=1: exit=0 wall=0.75s
test result: ok. 180 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

`cargo test -p blastit-sys --locked --test critical_embedded_seccomp` — **6
tests before this edit, 6 after**:

```text
running 6 tests
test the_roles_describe_themselves ... ok
test embedded_view_refuses_exec_process_creation_and_new_namespaces ... ok
test both_views_refuse_representative_shared_calls ... ok
test embedded_view_egress_reflects_the_flag ... ok
test view_keeps_exec_and_process_creation_but_shares_the_hardening ... ok
test both_views_refuse_clone3_and_still_start_a_thread ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```text
cargo clippy -p blastit-sys --all-targets --locked -- -D warnings
    Checking blastit-sys v0.2.0 (.../crates/blastit-sys)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s
```

```text
uv run --no-project tools/critical_guard.py --base 4dc0c2f5... --node MB.42.7
critical-guard: 4dc0c2f569d60195bf6b682b0175294ab4d156f7..working tree, 13 changed file(s)
node MB.42.7: paths ['crates/blastit-sys/**']
  M  crates/blastit-sys/tests/critical_embedded_seccomp.rs  [authorized by crates/blastit-sys/**]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.42.7
```

```text
== make fast-check: exit=0 wall=169.2s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

### Still open

- `crates/blastit-sys/tests/critical_child_sequence.rs` (`:69`) forks a child
  whose stdout/stderr are pipes and runs the whole `child::enter` sequence,
  which formats errors and reads `/proc` — production code that allocates *by
  design* in a child of a deliberately single-threaded process (§2.1). It is a
  different, larger job than this node (the sequence itself would have to become
  allocation-free) and this node's Goal names `critical_embedded_seccomp.rs`
  only, so it was not touched.
- `PreparedFilter::install`'s failure path is heap-free, but `install`'s wrapper
  still builds the typed `io::Error` when the kernel refuses a filter — a boot
  failure path, never a test child's path.
