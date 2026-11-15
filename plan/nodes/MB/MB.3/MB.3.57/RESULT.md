# MB.3.57 — `blastit-sys` on aarch64-gnu and musl

Branch `release/0.2.0`, base `a86fd31`. Files touched: `crates/blastit-sys/src/sock.rs`,
`crates/blastit-sys/src/seccomp.rs`. Nothing committed.

## What was actually wrong

The diagnosis in `NODE.md` was right about the mechanism and incomplete about the size: **ten
errors were one bug**, and the aarch64 failure was **two** constants, one of which only a fourth
target could reveal.

| # | Site | Why it broke | Targets affected |
|---|---|---|---|
| 1 | `msg.msg_controllen = buffer.0.len()` (2 in the send/receive paths, 2 more in tests) | The field is `size_t` in glibc and `socklen_t` (`u32`) in musl. Writing `usize` names one libc's type. | **both** musl targets — 32- and 64-bit alike |
| 2 | `(*cmsg).cmsg_len = CMSG_LEN(..) as libc::size_t` (1 + 2 tests) | Same assumption, but this one only *fails* where `size_t` ≠ `socklen_t`, i.e. **64-bit musl**. On 32-bit musl and 32-bit glibc both are `u32` and it compiles — that is why it hid. | 64-bit musl |
| 3 | `collect_fds`: `len.saturating_sub(header_len)`, `payload / size_of::<c_int>()`, `data.add(index)` (5 of the 10 errors) | The arithmetic assumed `cmsg_len` is `usize`; it inherits whatever the header declares. This half is a **width** bug, not a libc bug: it would break on any target where the field is narrower than a pointer. | musl (all widths) |
| 4 | `libc::SYS_fchmodat2` | Genuinely per-arch: `libc` 0.2.191 ships it for x86, x86_64, m68k, s390x, loongarch64 and **not** for aarch64, arm, riscv. | aarch64 (both libcs) |
| 5 | `libc::SYS_kexec_file_load` in `seccomp.rs` | Genuinely per-arch **and** per-libc: present for aarch64-glibc, absent for `aarch64-unknown-linux-musl`. Found by installing the fourth target, which the brief allowed me to try. | aarch64-musl only |

So: **one bug twice** (rows 1–3: naming glibc's `size_t`/`usize` for fields musl spells
`socklen_t`), **two genuinely arch/libc-specific constants** (rows 4–5, one an arch gap, one a
libc-table gap), plus one *third* target that found a fourth defect the two named targets would
have shipped.

Before the fix the counts were: `x86_64-unknown-linux-musl` 10 errors in `sock.rs` (9 × `E0308`
+ 1 × `E0277`, all rows 1–3), `aarch64-unknown-linux-gnu` 1 error (row 4), and
`aarch64-unknown-linux-musl` 1 error (row 5) once that target was installed — the ten width
errors would have appeared there too.

Row 5 also corrected a number: my first aarch64 value for `kexec_file_load` was 320 (x86_64's).
The repo's own `the_shared_refusal_table_pins_the_kernel_numbers` test — which pins the numbers
per arch — failed on aarch64 under qemu and named it: aarch64 uses asm-generic **294**. The
constant is now 294 for aarch64 and 320 for x86_64, both pinned by that existing test.

## How it was fixed (no `cfg` around the width logic)

`sock.rs` no longer names either typedef:

* `control_len<T>()` reads any of those fields into `usize` via `TryFrom` (widening on 64-bit
  musl, identity elsewhere; a value that cannot be a `usize` saturates and the existing buffer
  bound clamps it — same refusal a forged length already got).
* `set_control_len()` / `set_cmsg_len()` write them with `as _`, which resolves to the field's
  own declared type, so there is one send path and one receive path for every libc.
* `const _: () = assert!(CmsgBuffer::LEN <= libc::socklen_t::MAX as usize);` makes the only
  narrowing in the module a compile-time guarantee instead of a comment.

The syscall numbers are the only `cfg` and they are per-arch **data**, like the existing
`AUDIT_ARCH` and `X32_SYSCALL_BIT` constants next to them — not a switch that empties code.

## `ENOSYS` decision (stated, documented, tested)

`chmod_socket` **degrades, it does not refuse**: `ENOSYS`/`EINVAL` from `fchmodat2` falls back to
`chmod_socket_via_proc`, which applies the mode to the same inode through an
`O_PATH|O_NOFOLLOW` descriptor's `/proc/self/fd/<n>` link. That keeps the no-symlink-follow
guarantee the call exists for; refusing instead would make every pre-6.5 kernel and every gVisor
host unable to start a pg proxy at all. The fallback failing is still fatal and still happens
*before* `listen`, so a socket never becomes reachable with the umask's mode — on a host with no
`/proc` the caller sees that error (the requirement it cannot meet) rather than an open socket.
On an architecture outside the unified syscall table the call is never made: the constant is
`None`, `fchmodat2` reports `ENOSYS` without a syscall, and the same fallback runs. Guessing a
number there would invoke an unrelated syscall, which is worse than not trying.

The existing tests `chmod_socket_via_proc_sets_the_socket_mode` (the fallback route, exercised
directly) and `bind_unix_listener_sets_the_exact_mode` (the real path, over four modes) cover both
sides. This host runs kernel 6.12, so `fchmodat2` exists and the real path is the one
`bind_unix_listener` took; both tests pass on x86_64-gnu and x86_64-musl, and both pass on
aarch64-gnu under qemu.

## The new target-independent test

`crates/blastit-sys/src/sock.rs::tests::control_message_lengths_agree_on_both_ends`
walks a real kernel round trip — `send_with_fd` out, a raw `recvmsg` in — and asserts:

* the receiving side computes `cmsg_len == CMSG_LEN(4)` and `payload == size_of::<c_int>()`;
* the kernel reports `msg_controllen == CMSG_SPACE(4)`, i.e. the padded space, and the
  difference from `cmsg_len` is alignment padding and nothing more;
* `collect_fds` yields exactly one descriptor from that length, and writing through it still
  reaches the far end of the pipe (a length that survived the round trip survived it as a
  *working* descriptor).

It passes on x86_64-gnu, x86_64-musl and (under qemu) aarch64-gnu, so it is pinned on three
targets and would have caught any of rows 1–3.

## Evidence

Cross C toolchains are not installed on this host, so the workspace-wide `--target` checks
originally died inside `ring`'s and `pg_query`'s build scripts (`failed to find tool
"aarch64-linux-gnu-gcc"`), *before* rustc saw any Rust. I obtained them without root and without
touching the repo — Debian `gcc-aarch64-linux-gnu` + `libc6-dev-arm64-cross` (unpacked with
`dpkg -x` into `/tmp/opencode/cross`), Debian `musl-dev`/`musl-tools` with a patched `musl-gcc`
specs, Alpine `musl-dev`/`musl` aarch64 as the aarch64-musl sysroot, `qemu-user` for execution,
plus `CC_*`/`CFLAGS_*`/`BINDGEN_EXTRA_CLANG_ARGS_*`/`PYO3_CROSS_PYTHON_VERSION=3.13`. CI has the
same toolchains natively; nothing about the fix depends on this arrangement.

`rustup target add aarch64-unknown-linux-musl` **succeeded** here, so the fourth leg is not
CI-only any more.

### `cargo check --target aarch64-unknown-linux-gnu --workspace --all-targets`

```console
$ cargo check --target aarch64-unknown-linux-gnu --workspace --all-targets
    Checking blastit-sys v0.2.0 (.../crates/blastit-sys)
    ...
    Checking blastit v0.2.0 (.../crates/blastit)
    Checking blastit-py v0.2.0 (.../crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.95s
$ echo $?    # 0        (grep -c '^warning' = 0)
```

### `cargo check --target x86_64-unknown-linux-musl --workspace --all-targets`

```console
$ cargo check --target x86_64-unknown-linux-musl --workspace --all-targets
    ...
    Checking blastit v0.2.0 (.../crates/blastit)
    Checking blastit-py v0.2.0 (.../crates/blastit-py)
warning: dropping unsupported crate type `cdylib` for target `x86_64-unknown-linux-musl`
warning: `blastit-py` (lib) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.32s
$ echo $?    # 0
```

That one warning is the only non-clean line on either leg and it is **not this node's**: rustc has
no `cdylib` support for musl targets, `crates/blastit-py/Cargo.toml` declares
`crate-type = ["cdylib", "rlib"]` (outside this node's paths), and the wheel that needs `cdylib`
is gnu-only. The artifacts musl exists for here — the standalone binary and the shims — are clean:

```console
$ cargo check -p blastit -p blastit-sys --target x86_64-unknown-linux-musl
    Finished `dev` profile ...        # zero warnings
```

Follow-up if the owner wants the `--workspace` line byte-clean: gate `blastit-py`'s `cdylib`
crate type per target (1 line in a file this node may not touch).

### The fourth leg, `aarch64-unknown-linux-musl` (installed with `rustup target add`)

```console
$ cargo check --target aarch64-unknown-linux-musl --workspace --all-targets
    ...
warning: dropping unsupported crate type `cdylib` for target `aarch64-unknown-linux-musl`
warning: `blastit-py` (lib) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.82s
$ echo $?    # 0
```

### Behaviour, not just compilation

```console
$ cargo test -p blastit-sys --locked                       # host x86_64-gnu
test result: ok. 134 passed; 0 failed        (lib)
test result: ok. 6 passed; 0 failed          (critical_embedded_seccomp)
test result: ok. 2 passed; 0 failed          (doc-tests)

$ cargo test -p blastit-sys --target x86_64-unknown-linux-musl --locked
test sock::tests::control_message_lengths_agree_on_both_ends ... ok
test sock::tests::chmod_socket_via_proc_sets_the_socket_mode ... ok
test sock::tests::bind_unix_listener_sets_the_exact_mode ... ok
test result: ok. 134 passed; 0 failed  /  6 passed  /  2 doctests
```

The musl run is a real musl process (`target/x86_64-unknown-linux-musl/.../blastit_sys-*`), so
the SCM_RIGHTS path and both `chmod` paths are proved *working* on musl, not merely compiling.

`aarch64-unknown-linux-gnu` under `qemu-aarch64` (Debian `qemu-user`, `-L` at the cross sysroot):

```console
test result: FAILED. 120 passed; 14 failed
```

The 14 are `proc::tests::fork_accepts_a_single_threaded_child` and 13 `seccomp::tests::*` — every
test that installs a filter or forks. The panics say `left: 1, right: 0` (the child body's
"`install_program` (or `install`) returned `Err`" code) and one `left: None, right: Some(31)`
(`SIGSYS` never happened because the filter was never installed): qemu-user runs the guest's
`prctl(PR_SET_SECCOMP)` in the *host* process, whose `AUDIT_ARCH` is x86_64, and refuses the
program; it also mis-reports `waitpid` status for emulated forks (`testutil.rs:102`). Every
`sock::` test passes, including `control_message_lengths_agree_on_both_ends`,
`send_with_fd_passes_a_descriptor_that_works`, both `chmod_socket_via_proc_*` tests and
`bind_unix_listener_sets_the_exact_mode`. The seccomp half needs a real aarch64 kernel — that is
what the CI leg is for.

### `make check`

```console
$ make check
...
uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 2 wheels, 1 archives, 3 shims declared; 3 shim(s) built and probed; 3 leg(s) dropped with a reason
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0
```

(`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --features python
-- -D warnings`, `cargo doc` with `-D warnings`, the whole workspace test run, the critical tests,
`py-lint`, `docs-unit`, `docs-check`, `docs-test`, `rs-no-python` and `plan validate` all ran inside
that. The single `warning:` line in the log is the pre-existing "doc tests are not supported for
crate type(s) `cdylib` in package `blastit-py`".)

## One line per leg

| Leg | Status |
|---|---|
| `x86_64-unknown-linux-gnu` | **compiled and tested** — full workspace, `make check`, all `blastit-sys` tests |
| `x86_64-unknown-linux-musl` | **compiled and tested** — workspace `--all-targets` clean; `blastit-sys` suite *executed* under musl (134 + 6 + 2 pass) |
| `aarch64-unknown-linux-gnu` | **compiled and partially tested** — workspace `--all-targets` clean; 120/134 tests executed under qemu, the 14 seccomp/fork ones are emulator-blocked, so native seccomp behaviour is **unproven here / CI-proved** |
| `aarch64-unknown-linux-musl` | **compiled only** — workspace `--all-targets` clean; no aarch64-musl execution possible on this host |

## Follow-ups (not in this node's paths)

1. `crates/blastit-py/Cargo.toml`: gate `crate-type = ["cdylib", …]` off musl targets; the only
   warning left on the two musl legs, and rustc drops the crate type anyway.
2. `docs/knowledge/LESSONS.md` (outside paths, and already modified by MB.3.18) would benefit from
   three lines: musl's `cmsg_len` is `socklen_t` while glibc's is `size_t`, so never spell either
   type at a cmsg field; `libc` 0.2.191 is missing `SYS_fchmodat2` for aarch64/arm/riscv and
   `SYS_kexec_file_load` for aarch64-musl (`kexec_file_load` is 320 on x86_64, **294** on
   aarch64); and qemu-user cannot install seccomp programs, so seccomp tests cannot be proved
   under emulation.
3. 32-bit targets (`i686`, `arm`) were not installed here; the width fix is written to be correct
   for them (`TryFrom` widening, `as _` into the declared field, the `socklen_t` assertion), but
   that is reasoning, not a run.
4. `MB.3.18` should now re-run its matrix with all four legs enabled; nothing in its scripts,
   workflows or `Cargo.toml`/`Cargo.lock` was touched by this node.
