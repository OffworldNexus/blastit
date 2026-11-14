# MB.3.25 — `SeccompRole::EmbeddedView` + shared R14 hardening, without widening `View`

STATUS: **done**

## What changed

| File | Change |
|---|---|
| `crates/blastit-sys/src/seccomp.rs` | `SeccompRole::EmbeddedView { egress }`; shared rules `clone3 -> ENOSYS`, `clone & CLONE_NEW* -> EPERM`, `setsid`/`setpgid -> EPERM`; `EmbeddedView`-only `execve`/`execveat`/`fork`/`vfork`/non-`CLONE_THREAD` `clone -> EPERM`; `MAX_PROGRAM` doc (66/63 worst case); 7 new unit tests + the role/socket/table tests extended (133 total in-crate) |
| `crates/blastit-sys/tests/critical_embedded_seccomp.rs` *(new)* | Protected public-surface oracle: role names/flags, both roles' deny/allow twins through `install`, `pthread_create`, egress |
| `crates/blastit-sys/src/lib.rs` | `#[cfg(any(test, feature = "testutil"))] pub mod testutil;` |
| `crates/blastit-sys/Cargo.toml` | `[features] testutil = []` (opt-in, **not** a default) |

No `crates/blastit/**`, no existing `critical_*.rs`, no `Cargo.lock`, no `plan.py` mutation.

### BPF shape (still forward-only classic BPF, fixed stack array)

`clone` reuses `data::ARG0`; `clone3` reads no argument. Shared `DENIED` stays exactly 35
entries. The four shared rules sit in `build_for` for **every** role (http, scheduler and
dbproxy included), which is a narrowing only — a role may lose `setsid`/namespaces, never gain
one — and the design's requirement that both `View` and `EmbeddedView` carry them is therefore
a subset of what ships. Pinned exact widths: `View` **61** instructions (`x86_64`) / 60
(`aarch64`); `EmbeddedView` **66** / 63, both `<= MAX_PROGRAM` (96) and both asserted in
`the_widest_program_is_the_size_the_comment_claims`.

### `feature = "testutil"` wiring — chosen: opt-in, **no default**

`testutil` is a normal feature with no `default = [...]`, so every existing dependency of
`blastit-sys` keeps getting the production library with zero test API and no `Cargo.toml`
change elsewhere. The protected suite therefore must not need the feature — and does not:
it forks with `libc::fork` (a normal dependency) and drives the installed filter with raw
syscalls, so `make test-critical`'s no-feature command runs **5 real tests**. (The alternative,
`default = ["testutil"]`, would have leaked test API into every workspace dependency by feature
unification, per `docs/knowledge/LESSONS.md`.)

Absence proof:

```console
$ cargo build -p blastit-sys --no-default-features --lib --locked
$ nm -C target/debug/libblastit_sys.rlib 2>/dev/null | grep -c testutil
0
$ cargo build -p blastit-sys --features testutil --lib --locked
$ nm -C target/debug/libblastit_sys.rlib 2>/dev/null | grep -c testutil
29
```

### Division between the two oracles (brief-permitted split)

`crates/blastit-sys/tests/critical_embedded_seccomp.rs` is the **public-surface** oracle (the
exact install path a consumer uses). The private-program oracle — exact pinned instruction
counts, the literal sorted list of the 35 shared syscall numbers (independent of the
`libc::SYS_*` constants), the `CLONE_NEW*` mask bits, and the
`View`-table-is-unchanged set equality — stays in `seccomp.rs`'s unit tests, where it can see
`Program`; an integration test cannot see `cfg(test)` items without the feature, and silently
running zero tests was the trap in the brief. The protected file adds behavioural probes for
`ptrace`, `bpf`, `mount` and `unshare` on both roles, so `make test-critical` alone sees a
widened table. The `DENIED` table itself is grouped by theme, not sorted: the oracle sorts a
copy and compares it to the literal numbers. `the_view_table_is_unchanged_and_embedded_only_adds_its_documented_calls`
is the zero-widening assertion: `View`'s comparison chain equals `DENIED ∪ {socket,
socketpair, clone3, clone, setsid, setpgid}` and contains none of `EMBEDDED_DENIED`.

`vfork` is proven permitted on `View` by that program-set test rather than by executing it: a
real `vfork` child shares this test process's stack, which is not sound in Rust. On `aarch64`
the `fork`/`vfork` syscalls do not exist, so only `execve`/`execveat` are in `EMBEDDED_DENIED`
there; glibc's `fork`/`vfork` compile to `clone`, which the `CLONE_THREAD` rule refuses.

### The zero-widening oracle bites (mutation proof)

The literal `SHARED_DENIED_NUMBERS` list is deliberately independent of `DENIED`'s
`libc::SYS_*` constants. Mutating `("bpf", libc::SYS_bpf)` to `libc::SYS_bpf + 1000`
(silently un-denying `bpf`) makes the new unit oracle fail; the file was then restored
and re-run green:

```console
$ perl -pi -e 's/\("bpf", libc::SYS_bpf\)/("bpf", libc::SYS_bpf + 1000)/' crates/blastit-sys/src/seccomp.rs
$ cargo test -p blastit-sys --locked --lib the_shared_refusal_table_pins_the_kernel_numbers
assertion `left == right` failed: a shared refusal changed number or disappeared
  left:  [… 320, 323, … 438, 440]   # 321 is gone, 1321 arrived
  right: [… 320, 321, 323, … 438, 440]
test result: FAILED. 0 passed; 1 failed; 132 filtered out
$ cp /tmp/opencode/mb325-seccomp.bak crates/blastit-sys/src/seccomp.rs
$ cargo test -p blastit-sys --locked --lib the_shared_refusal_table_pins_the_kernel_numbers
test result: ok. 1 passed; 0 failed; 132 filtered out
```

## Acceptance

```console
$ cargo test -p blastit-sys --locked
EXIT:0 — 133 unit + 6 integration + 2 doc tests passed, 0 failed
  seccomp::tests::clone3_is_enosys_and_a_thread_still_starts_on_both_views ... ok
  seccomp::tests::every_new_namespace_clone_flag_is_refused_by_both_views ... ok
  seccomp::tests::setsid_and_setpgid_are_refused_by_both_views ... ok
  seccomp::tests::embedded_view_refuses_process_creation_and_exec_while_view_keeps_them ... ok
  seccomp::tests::the_view_table_is_unchanged_and_embedded_only_adds_its_documented_calls ... ok

$ cargo test -p blastit-sys --features testutil --locked
EXIT:0 — 133 + 6 + 2 passed, 0 failed

$ cargo test -p blastit --locked
EXIT:0 — 11 targets, 244 tests, 0 FAILED

$ cargo test -p blastit-sys --test critical_embedded_seccomp --locked
running 6 tests
test the_roles_describe_themselves ... ok
test embedded_view_refuses_exec_process_creation_and_new_namespaces ... ok
test view_keeps_exec_and_process_creation_but_shares_the_hardening ... ok
test both_views_refuse_clone3_and_still_start_a_thread ... ok
test both_views_refuse_representative_shared_calls ... ok
test embedded_view_egress_reflects_the_flag ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make check
EXIT:0 — 115 suites, 2569 tests; `grep -cE 'test result: FAILED|^error(\[|:)'` = 0
critical-guard: no change to a protected critical test
protected critical tests hold no skip construct
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups

- `ARCHITECTURE.md` §7.3 (the spec this module cites) now contradicts the code: it has no
  `clone3 -> ENOSYS` exception and lists neither `clone`/`CLONE_NEW*`, `setsid`/`setpgid` nor
  `SeccompRole::EmbeddedView`. The sync belongs to `MB.3.8`/its nodes, not this leaf.
- `crates/blastit/src/zygote.rs:168` still installs `SeccompRole::View { egress }`; selecting
  `EmbeddedView` for `[route.php]` workers belongs to the PHP embed-runtime leaves
  (`MB.3.13.x` / `MB.3.26`), which own that call site. This leaf only makes the role real.
- The pinned widths are architecture-exact; the `aarch64` numbers (60/63) were derived and are
  enforced by the same test, but only the `x86_64` leg ran on this host.
- Pre-existing plan files under `plan/` are the dispatch harness's own writes; this session
  only added `RESULT.md` and the code/test files above.
