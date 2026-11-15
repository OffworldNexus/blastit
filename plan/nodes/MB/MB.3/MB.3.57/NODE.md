---
id: MB.3.57
title: "Port blastit-sys to aarch64-gnu and x86_64-musl so the 4-leg matrix can exist"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["crates/blastit-sys/**", "crates/blastit/**", "crates/blastit-php-sapi/**", "e2e/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T11:56"
updated: "2026-11-15T12:17"
---

# Goal

Make `blastit-sys` (and anything else that fails) compile for the two targets the owner put
in scope — `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-musl` — so `MB.3.18` can
build and probe the four-leg matrix instead of dropping three quarters of it. Compiling is
the floor, not the goal: the code must *work* there, not compile empty.

# Context

`MB.3.18` found the matrix cannot exist today:

- `cargo check --target x86_64-unknown-linux-musl` → **10 errors in `crates/blastit-sys/src/sock.rs`**
  (control-message widths);
- `cargo check --target aarch64-unknown-linux-gnu` → **`E0425 SYS_fchmodat2`** at
  `crates/blastit-sys/src/sock.rs:396`.

Both targets are installed on this host (`rustup target list --installed` shows
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`), so
`cargo check --target` is available and cheap. `aarch64-unknown-linux-musl` is **not**
installed — say so and label that leg CI-only rather than pretending.

The owner's verdict: a 2-wheel/1-archive/3-shim matrix is a **report of a blocker, not the
release we promised** (aarch64 and Alpine/musl are in scope by explicit decision). The port
comes first; if it turns out too large, that is reported back as an owner decision, not
assumed.

# What the source says (verify, do not trust)

- `sock.rs:112-117`: `(*cmsg).cmsg_len = libc::CMSG_LEN(size_of::<c_int>() as u32) as libc::size_t;`
  and `CMSG_SPACE(..) as usize`. On glibc `cmsg_len` is `size_t`; on musl `cmsghdr.cmsg_len`
  is `socklen_t` (`u32`). The casts encode one libc's types — check whether the same
  assumption is wrong for **any** 32-bit or size-varied target, not just musl.
- `sock.rs:396`: `libc::syscall(libc::SYS_fchmodat2, …)` — the constant is absent for
  aarch64 in the pinned `libc`. Also decide and record what happens on a kernel without
  `fchmodat2` (`ENOSYS`): a documented fallback, or a refusal that names the requirement.

# Acceptance

```console
$ cargo check --target aarch64-unknown-linux-gnu --workspace --all-targets
$ cargo check --target x86_64-unknown-linux-musl --workspace --all-targets
$ cargo test -p blastit-sys --locked
$ make check
```
- Both `cargo check --target` runs are **clean** (warnings included — the workspace denies
  them). Paste the tails.
- `aarch64-unknown-linux-musl` too, if a target can be installed/obtained; otherwise say
  plainly that it is CI-only here.
- The behaviour is unchanged and still proved where it can run: `cargo test -p blastit-sys`
  and `make check` green on `x86_64-gnu`, including whatever exercises `fchmodat2` and the
  SCM_RIGHTS control-message path.
- A test pins the width logic in a target-independent way (e.g. asserting `cmsg_len` equals
  the value the receiving side computes, and that a descriptor sent with a payload survives),
  so a third target cannot silently break it.
- **No silencing**: no `#[allow(dead_code)]`, no `cfg(not(target_…))` around a whole file or
  function body that leaves it empty, no `todo!()`/`unimplemented!()` behind a cfg. The code
  works on those targets or the node reports why it cannot.
- If the `ENOSYS` fallback needs a decision (refuse vs degrade), state it in the RESULT and
  in the code comment; do not leave it implicit.

# Notes

- Do **not** redo `MB.3.18`'s work: the two-building legs, `scripts/platform_matrix.py`'s
  self-defending counts and the workflows stay as they are; `MB.3.18` re-runs after this.
- If the port needs a toolchain that cannot be obtained here (an arm/musl linker, qemu for
  execution), say exactly which and what is unproven — compiling is provable with
  `cargo check` even when running is not.
- Rough size matters to the report: if this is more than roughly one session of work, say so
  early with the list of what remains, because the owner decides whether 0.2.0 ships
  x86_64-gnu only with a written note and the legs follow in 0.2.1.
