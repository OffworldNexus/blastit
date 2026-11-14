---
id: MB.3.5.2
title: "Entry + libphp probes, boot predicates and the manifest reference entry for [route.php]"
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
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T14:39"
---

# Goal

Add the pure `EntryProbe` seam and the boot probes for `[route.php]`, then document
the block: `blastit-manifest/src/probe.rs` holds the trait and errors,
`crates/blastit/src/probe.rs` holds the syscall bodies (`DevProbe`/`ProductionProbe`),
`crates/blastit/src/lib.rs` becomes the single owner of the module list, `supervise.rs`
keeps `Parts`/`Supervisor` out of the manifest's dependency graph, and
`docs/reference/manifest.md` gets the `[route.php]` entry (design leaf `MB.3.5`, part 2).

# Context

- Design of record: `ADR-0010` §2 (`entry` required, optional downward `workers`, no
  `listen`/`socket`/`port`/`env`) and §3 (Stage-0 `libphp` discovery plus the Stage-1
  `(st_dev, st_ino)` re-check this leaf's probe mirrors); the retired external-PHP design is
  deleted with `ADR-0010` and is not a source.
- Files (the whole leaf, 7): `crates/blastit-manifest/src/{probe,lib}.rs`,
  `crates/blastit/src/{probe,lib,supervise}.rs`,
  `crates/blastit-manifest/tests/critical_php_manifest.rs`,
  `docs/reference/manifest.md`.
- What lands:
  - `blastit-manifest/src/probe.rs`: pure `EntryProbe` (`check` + `check_entry`) and
    `EntryProbeError` (`Missing`/`NotRegularFile`/`NotPhpFile`/`Unreadable{uid}`/
    `OutsideRoot`/`Timeout`/`ForeignOwner{uid}`/`Symlink`/`ParentWritableOrSymlink`/`Io`),
    `ProbeUid` and `check_php_entries(.., probe, uid)`. There is no socket and no parent-fd
    handoff: `[route.php]` is served over the worker's `view.chan` `socketpair`
    (ADR-0010 §1). `lib.rs` appends **only**
    `pub mod probe;` — the two `pub use model::{…}` names are `MB.3.5.1`'s.
  - `crates/blastit/src/probe.rs`: the syscall bodies of `check_entry` plus
    `DevProbe`/`ProductionProbe`. `rustix` is already a dependency there, so
    `crates/blastit-manifest/Cargo.toml` stays `rustix`/`libc`-free (**no** manifest
    `Cargo.toml` change).
  - `crates/blastit/src/lib.rs` (single owner): `pub mod probe;` + `pub mod supervise;`.
  - `crates/blastit/src/supervise.rs`: `Parts`, `Supervisor` and `Supervisor::new`
    become **`pub(crate)`** so `private_interfaces` stays silent; there is no pool config
    and no `pool_test` module (both went with the external path, `MB.3.17`).
  - Protected `crates/blastit-manifest/tests/critical_php_manifest.rs` (this leaf's cases):
    `entry_missing_is_boot_fatal`, `workers_zero_and_sixty_five_are_boot_fatal`,
    `entry_outside_root_is_boot_fatal`, `entry_writable_by_route_uid_is_boot_fatal`,
    `probe_timeout_is_boot_fatal` (a fake probe that never replies → `Timeout`, boot-fatal),
    the `NotRegularFile` /
    `Unreadable { uid }` / `OutsideRoot` injections through the fake `check_entry`, the
    positive page manifest, and the two `StaticDeadDeclaration` twins.
  - `docs/reference/manifest.md`: the `[route.php]` entry (`entry` required, optional
    `workers`, the rejected keys), the static-interaction lines and the reference row.
- No pool config and no `supervise.rs` clippy gate belong here (that owner node is
  superseded). No `Cargo.toml`, no `Cargo.lock`.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_php_manifest --locked
$ cargo test -p blastit --locked
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
$ grep -q 'pub mod probe;' crates/blastit/src/lib.rs && grep -q 'pub mod supervise;' crates/blastit/src/lib.rs && echo "single owner of lib.rs"
$ make docs-check
$ make check
```

# Notes

- `critical_php_manifest.rs` is already protected (`MB.3.5.4`); this leaf adds cases to it
  and leaves `critical_static_manifest.rs` untouched and green.
- There is no socket to hand over: the child `dev` reads the manifest and discovers `libphp`
  itself (ADR-0010 §3), so `MB.3.6.4.2` threads no parent fd.
- No `std::fs::canonicalize()`: the design refuses caller-path resolution; the parent is
  pinned by descriptor and re-verified by inode.
