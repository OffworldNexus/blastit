---
id: MB.42.10
title: "Make blastit compile for musl: libc::execveat is gnu-only (kills both musl archives, musllinux wheels and every musl PHP suite)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2.3]
paths:
  - crates/blastit/src/cli/handoff.rs
  - crates/blastit/tests/critical_handoff.rs
  - scripts/platform_matrix.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T00:27"
updated: "2026-11-20T01:33"
---

# Goal
Make `crates/blastit` compile for **musl** again. `MB.42.2.3`'s safe local handoff calls `libc::execveat`, which the `libc` crate declares **only** for gnu (`src/unix/linux_like/linux/gnu/mod.rs`) — there is no `execveat` in the musl module. So `cargo build -p blastit --target *-unknown-linux-musl` fails to compile, which takes out four shipped artifacts:

- `blastit-x86_64-unknown-linux-musl.tar.gz` and `blastit-aarch64-unknown-linux-musl.tar.gz`,
- `blastit-<ver>-py3-none-musllinux_1_2_{x86_64,aarch64}.whl`,
- and the worker-level PHP suites on every musl leg (`MB.42.5` had to mark the 8.6 musl leg `suites: False` / `reaches: probed` purely because of this).

Found by `MB.42.5` while proving the PHP 8.6 musl leg: the shim and the real-load probe are green on musl (`DT_NEEDED libc.musl-x86_64.so.1`, three 200s), only compiling `crates/blastit` itself fails.

# Exact Deliverables
1. In `crates/blastit/src/cli/handoff.rs`, replace the direct `libc::execveat(...)` call with something that exists on **both** libcs. `libc::SYS_execveat` **is** declared for musl (verified in `libc-0.2.190/src/unix/linux_like/linux/musl/b*/`), so `libc::syscall(libc::SYS_execveat, dirfd, pathname, argv, envp, flags)` is the portable route — with `AT_EMPTY_PATH` defined locally (`0x1000`) if the `libc`-provided constant is gnu-only, checked with a `const` assertion or a comment citing the header.
2. Keep the security properties **identical**: the exec must still use the **verified descriptor** (`AT_EMPTY_PATH` on `bin_fd`), never a re-opened path; the `#!` branch keeps `parent_fd`/`bin_fd` alive across the call; no new `unsafe` beyond the single syscall; the SAFETY comment still explains the lifetime argument.
3. No `#[cfg(target_env = "musl")]` fork of the *behaviour* — one code path that compiles on both. If a cfg is genuinely unavoidable, say exactly why in `RESULT.md`.
4. Keep `crates/blastit/tests/critical_handoff.rs` green (the ELF leg proves `execveat(AT_EMPTY_PATH)` on the verified fd — do not weaken it; if it asserts on a syscall name, adjust the assertion to the syscall number, not to a weaker property).

# Notes
- Container-only for the musl proof: **do not install a musl toolchain or cross-linker on this host**. Use an Alpine container (e.g. `docker run --rm -v "$PWD":/src -w /src alpine:3.21` with rust + `musl-dev`), or `cargo check --target x86_64-unknown-linux-musl` if that target and a linker are already available — but a `cargo check` alone is **not** proof; at minimum `cargo build -p blastit --target x86_64-unknown-linux-musl` must link inside the container.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.
- Do not modify `crates/*/tests/critical_*.rs` except `critical_handoff.rs` (authorized by this node's `paths` + `# Goal`) and only as item 4 allows.
- Never touch `docs/**`.

# Acceptance
- `cargo build -p blastit --target x86_64-unknown-linux-musl --locked` **links** (in the Alpine container), and `scripts/check-portable-elf.sh` on the musl artifact shows no `libpython`/`libphp`/`RUNPATH`
- `cargo build -p blastit --target aarch64-unknown-linux-musl --locked` builds (container or `--check` plus a stated reason if no aarch64 musl linker is available)
- gnu still builds and `cargo test -p blastit --locked --test critical_handoff` is green
- `cargo clippy -p blastit --all-targets --locked -- -D warnings` and `--no-default-features`
- the PHP **musl** worker suites now run: `make rs-test-php-image` (or the 8.6 musl leg) with `suites: True`, and `scripts/platform_matrix.py`'s `suites_because` note for the musl legs removed
- `make fast-check`
