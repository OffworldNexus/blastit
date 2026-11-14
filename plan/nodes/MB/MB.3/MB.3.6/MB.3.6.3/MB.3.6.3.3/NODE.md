---
id: MB.3.6.3.3
title: "Protected critical_fastcgi_pool suite: fd table, pgid, escapee and U1 - design MB.3.6b part 3"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.3.2]
paths: [crates/blastit/tests/critical_fastcgi_pool.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T14:39"
---

# Goal

Land the protected pool suite: the `S0` fd-table negative oracle, `setsid()` escapee
non-publication, pgid membership, the `U1` freeze/unlink proof and `PoolReady` inode
equality — all against the harness `MB.3.6.3.2` exposed (design leaf `MB.3.6b`, part 3).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.8.3 (the normative lifecycle and
  unwind matrix), §7.1/§7.2 (`R11` pool file, `R2`/`R3`/`R20` production rows) and §8's
  `MB.3.6.3.3` row; §8.2's `MB.3.6.3` row.
- Files (the whole leaf, 1): `crates/blastit/tests/critical_fastcgi_pool.rs`.
- What lands:
  - `S0`/`S3` fd-table negative oracle: the `php-fpm -t` child and the pool master hold
    only stdio — neither `ctl.chan`, `parentfd` nor `obs_fd`.
  - pgid membership (`S4`/`S6`): every route process is in the route's process group; a
    `setsid()` escapee is **not** published as ready.
  - `U1` freeze/unlink: a failed verify/bind runs `kill(-pgid, SIGKILL)`, `unlinkat`, restore
    `init_uid:init_gid 0555`, and sends **no** `PoolReady`.
  - `PoolReady` inode equality: the published `(sock_dev, sock_ino)` is the inode the
    scheduler hands over, and two routes never share a parent inode.
  - `parent_symlink_or_writable_ancestor_refused_before_spawn_leaves_sentinel_untouched`
    (pre-boot intermediate symlink `storage/framework -> /sentinel` or a route-writable
    ancestor → `FastCgiListenParentInvalid`, zero processes spawned, sentinel untouched).
- Duplicate the `children()`/`fds()` helpers **locally**: `smoke.rs` is a separate test
  binary and cannot be imported (precedent `critical_dev_and_fail_closed.rs:482`).
- Single-UID fixture: the `fchownat`/`0555`/`0600` assertions are `#[ignore]`d as
  `requires-root`; the root-only legs run in `e2e/rootless` under `MB.3.7`.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --test critical_fastcgi_pool --locked
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
$ grep -q 'requires-root' crates/blastit/tests/critical_fastcgi_pool.rs && echo "root-only legs are explicitly gated"
$ make critical-guard BASE=develop NODE=MB.3.6.3.3
$ make check
```

# Notes

- **Protected tests: add, never weaken.** This file is new and becomes protected once
  committed; `critical_static_cli.rs` and `critical_static_manifest.rs` stay untouched.
- `PoolHarness`/`PoolReadyInfo` come from `blastit::supervise::pool_test` (`.2`'s inline
  module) and are distinct from `Ctl::PoolReady` — do not conflate the two types.
- Determinism: no Docker, no real `php-fpm` binary; the fake helper is the only child.
