---
id: MB.3.5.2
title: "SocketProbe, boot probes and the manifest reference entry - design MB.3.5 part 2"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.5.1]
paths:
  - "crates/blastit-manifest/src/probe.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/error.rs"
  - crates/blastit/src/probe.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/supervise.rs
  - "crates/blastit-manifest/tests/critical_fastcgi_manifest.rs"
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T14:12"
---

# Goal

Add the pure `SocketProbe` seam and the boot probes for `[route.fastcgi]`, then document
the block: `blastit-manifest/src/probe.rs` holds the trait and errors,
`crates/blastit/src/probe.rs` holds the syscall bodies (`DevProbe`/`ProductionProbe`),
`crates/blastit/src/lib.rs` becomes the single owner of the module list, `supervise.rs`
narrows its public surface, and `docs/reference/manifest.md` gets all three §4.1 edits
(design leaf `MB.3.5`, part 2).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.1 (probe rules and the exact fatal
  messages), §4.8.1/§4.8.2 (parent and ancestor validation), §8's `MB.3.5.2` row and
  §8.2's `MB.3.5` row.
- Files (the whole leaf, 7): `crates/blastit-manifest/src/{probe,lib}.rs`,
  `crates/blastit/src/{probe,lib,supervise}.rs`,
  `crates/blastit-manifest/tests/critical_fastcgi_manifest.rs`,
  `docs/reference/manifest.md`.
- What lands:
  - `blastit-manifest/src/probe.rs`: pure `SocketProbe` (`check` + `check_entry`) and
    `SocketProbeError` (`Missing`/`NotSocket`/`NotRegularFile`/`Unreadable{uid}`/
    `OutsideRoot`/`Timeout`/`ForeignOwner{uid}`/`Symlink`/`ParentWritableOrSymlink`/`Io`),
    `ProbeUid`, `check_fastcgi_entries(.., probe, uid)` and
    `check_fastcgi_sockets -> Vec<(usize, OwnedFd)>`. `lib.rs` appends **only**
    `pub mod probe;` — the two `pub use model::{…}` names are `MB.3.5.1`'s.
  - `crates/blastit/src/probe.rs`: the syscall bodies of `check_entry` plus
    `DevProbe`/`ProductionProbe`. `rustix` is already a dependency there, so
    `crates/blastit-manifest/Cargo.toml` stays `rustix`/`libc`-free (**no** manifest
    `Cargo.toml` change).
  - `crates/blastit/src/lib.rs` (single owner): `pub mod probe;` + `pub mod supervise;`.
  - `crates/blastit/src/supervise.rs`: `Parts`, `Supervisor` and `Supervisor::new`
    become **`pub(crate)`** so `private_interfaces` stays silent; `render_pool_conf`
    (added by `MB.3.6.3.2`) and the later inline `pool_test` module stay `pub`.
  - Protected `crates/blastit-manifest/tests/critical_fastcgi_manifest.rs` (new file):
    `connect_and_trace_are_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`,
    `shared_listen_parent_is_boot_fatal`, `probe_timeout_is_boot_fatal` (a fake probe
    that never replies → `Timeout`, boot-fatal), the `NotRegularFile` /
    `Unreadable { uid }` / `OutsideRoot` injections through the fake `check_entry`, the
    positive page manifest, and the two `StaticDeadDeclaration` twins.
  - `docs/reference/manifest.md`: all three §4.1 edits (the `[route.fastcgi]` entry, the
    static-interaction lines, the reference row).
- The `supervise.rs` clippy gate belongs to `MB.3.6.3.2`, not here. No `Cargo.toml`, no
  `Cargo.lock`.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_fastcgi_manifest --locked
$ cargo test -p blastit --locked
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
$ grep -q 'pub mod probe;' crates/blastit/src/lib.rs && grep -q 'pub mod supervise;' crates/blastit/src/lib.rs && echo "single owner of lib.rs"
$ make docs-check
$ make check
```

# Notes

- `critical_fastcgi_manifest.rs` is a **new** file (additions are free);
  `critical_static_manifest.rs` stays untouched and green.
- `check_fastcgi_sockets` returns `Vec<(usize, OwnedFd)>`; `MB.3.6.4.2` threads it to the
  child `dev` as `BLASTIT_CHECK_POOL_PARENT_FD`.
- No `std::fs::canonicalize()`: the design refuses caller-path resolution; the parent is
  pinned by descriptor and re-verified by inode.
