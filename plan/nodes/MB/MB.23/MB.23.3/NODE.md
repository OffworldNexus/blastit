---
id: MB.23.3
title: "blastit-sys unprivileged fanotify watcher (x86_64/aarch64 x gnu/musl) with AT_HANDLE_FID handle table, critical event tests and the zero-inotify binary guard"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.2]
paths:
  - "crates/blastit-sys/src/fanotify.rs"
  - "crates/blastit-sys/src/lib.rs"
  - "crates/blastit-sys/tests/critical_fanotify.rs"
  - "scripts/check-no-inotify.sh"
  - Makefile
  - docs/knowledge/LESSONS.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:12"
updated: "2026-11-16T23:22"
---

# Goal

Add the one and only filesystem watcher: `crates/blastit-sys/src/fanotify.rs`, a safe
`io::Result` wrapper over the Linux file-handle fanotify group, plus the permanent
zero-`inotify` guard that makes the ban mechanical. The watcher is deliberately
path-agnostic — it reports `(dir, name, mask)` and knows nothing about views, routes or
the loaded-file union; that policy lives in `dev_reload.rs`.

Public surface (frozen by `MB.23.2`):

```rust
pub struct Watcher { /* OwnedFd + HashMap<HandleKey, PathBuf> */ }
impl Watcher {
    pub fn new() -> Result<Self, WatcherError>;                  // 0x0c03, O_RDONLY
    pub fn mark_dir(&mut self, dir: &Path) -> Result<(), WatcherError>;
    pub fn read(&mut self, out: &mut Vec<Event>) -> Result<Readout, WatcherError>;
}
pub struct Event { pub mask: u64, pub dir: PathBuf, pub name: Vec<u8> }
pub enum WatcherError { Unavailable { op: Op, errno: i32 }, NoFileHandle { fs_type: String }, /* … */ }
```

and `scripts/check-no-inotify.sh`, which `make check` runs.

# Context

- `libc 0.2.191` already ships `fanotify_init`, `fanotify_mark`, `name_to_handle_at`,
  `fanotify_event_metadata` (`repr(align(8))`), `fanotify_event_info_header`,
  `fanotify_event_info_fid`, `file_handle`, `__kernel_fsid_t`, every `FAN_*` constant and
  `AT_HANDLE_FID` on `x86_64`/`aarch64` × `gnu`/`musl`. Define **no** structs and **no**
  syscall numbers; the only real work is type hygiene (`c_uint` init/mark flags, `u64`
  event masks, `mask as u64` portable across the gnu `u64` / musl `c_ulonglong` split) and
  hiding the gnu/musl `fanotify_mark` signature difference behind one method with one
  `as _` inside — no `cfg` in the public API.
- Parse rules from the research (§1.2): info records are a chain from `metadata_len` to
  `event_len`; `hdr.len` includes a trailing NUL and is padded to 4; the name area is
  `hdr.len - 20 - handle_bytes`; stop at the first NUL; `read(2)` into a 16 KiB
  `#[repr(C, align(8))]` buffer; `EAGAIN` when drained and `EINVAL` when the buffer is too
  small are both normal; `fd == FAN_NOFD` and `FAN_Q_OVERFLOW` mean "events were dropped".
- Handles: build `HandleKey { fsid: [u32;2], handle_type: i32, handle: Vec<u8> }` in
  `mark_dir` from `name_to_handle_at(AT_FDCWD, dir, &mut handle, &mut mount_id,
  AT_HANDLE_FID)`; on `EINVAL` from an old kernel retry with `flags = 0`; resolve an event
  by looking up its `(fsid, type, bytes)` and join the name. Never `open_by_handle_at`.
  Key on the bytes, not `handle_bytes` (12 on both tmpfs and xfs) and not `st_dev`
  (`f_fsid` ≠ `st_dev`).
- Nothing in this crate may call `inotify_*`; `crates/blastit-sys/src/lib.rs` re-exports
  the module behind the crate's existing rules (`unsafe` confined to this file, `// SAFETY:`
  on every block, no `unwrap`/`expect`/`panic!`).
- `scripts/check-no-inotify.sh` (new, `readelf`-based like `scripts/check-portable-elf.sh`)
  must fail, with a sentence naming the file, when:
  1. any argument ELF carries a dynamic symbol matching `inotify_(init1?|add_watch|rm_watch)`
     (`readelf --dyn-syms` and `nm -D` when available);
  2. `cargo tree -e normal --locked -p <crate>` names a `notify`/`inotify` crate;
  3. `inotify_init|inotify_add_watch|inotify_rm_watch` appears in a tracked source file
     outside its documented allowlist (`docs/research/**`, `plan/**`, the script itself)
     — and it must also refuse `fanotify_init` **outside**
     `crates/blastit-sys/src/fanotify.rs` and `crates/blastit/src/dev_reload.rs`, so a
     second watcher cannot be added quietly.
  `make check` (via `rs-no-python`) runs it on `target/debug/blastit`; `MB.23.6` re-runs it
  on the wheel's `_blastit*.so`.

# Acceptance

```console
$ cargo test -p blastit-sys --locked fanotify
$ cargo test -p blastit-sys --locked --test critical_fanotify
$ cargo test -p blastit-sys --locked --doc
$ cargo clippy -p blastit-sys --all-targets --locked -- -D warnings
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
$ bash -n scripts/check-no-inotify.sh
$ make check
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.23.3
```

1. `critical_fanotify.rs` proves, against the real kernel, on a tmpfs and on the working
   directory's filesystem: a create-write-close yields `FAN_CLOSE_WRITE`; an atomic
   editor save (`write tmp` + `rename`) yields `FAN_MOVED_TO` (and `FAN_MOVED_FROM` for the
   old name) and never a partial `FAN_CREATE`-only event; `mask` is read as bits, never
   `==`; a non-UTF-8 name arrives as bytes, not a lossy string; marking a file (not a
   directory) is `ENOTDIR`; `read` on a drained fd is `EAGAIN`.
2. `FAN_MODIFY` is carried through unchanged when a test group asks for it (proving the
   mask encoder), and the production mask constant is asserted **not** to contain it.
3. An unprivileged overlay mount (`unshare(CLONE_NEWUSER|CLONE_NEWNS)`, lower/upper on
   tmpfs) proves the event's parent handle is byte-identical to
   `name_to_handle_at(AT_HANDLE_FID)` and that plain `flags = 0` returns `EOPNOTSUPP`
   there — so a regression to `flags = 0` fails this test.
4. `WatcherError::Unavailable` maps `ENOSYS`, `EPERM`, `EINVAL`, `EOPNOTSUPP` and `ENOSPC`
   to the documented `op`/`errno` pair in a unit test that injects each errno (no kernel
   needed), and `NoFileHandle` names the filesystem type.
5. `scripts/check-no-inotify.sh target/debug/blastit` prints one `ok` line and exits `0`;
   with a planted `inotify_add_watch` in a scratch copy of a source file it exits `1` and
   names the file (this negative case is a test in the script's own `--self-test` mode).
6. `make check` is green, and the root `Cargo.toml`/`Cargo.lock` are untouched.

# Notes

- The real-kernel suite needs a host where the unprivileged FID group works — the gate
  host and the CI runners are such hosts; Docker's default profile is not. A refusal is a
  **failure** whose message names the errno and the fix ("run `make check` on the host, or
  allow `fanotify_init` in the devcontainer's profile"), never a skip: no `#[ignore]`, no
  `needs_root`, no silent `return`. Record that environment fact once in
  `docs/knowledge/LESSONS.md` with the measured errno, and keep every non-kernel assertion
  (mask encoding, errno→`WatcherError` mapping, handle keying, script self-test) in unit
  tests that need nothing from the kernel.
- `make check` needs `target/`; only run it once `MB.21` is `done`.
- Do not add a `poll`/`stop` thread, a background task, or an async API: the supervisor
  polls the fd. This crate returns events and nothing else.
- Do not let a view identity, a route number or a dependency map enter this crate's API or
  tests: one watcher, one event stream, one consumer. A change that needs to know which
  view loaded a file belongs in `MB.23.5`, not here.
