---
id: MB.3.6.1.1
title: "Scheduler AF_UNIX seccomp, pool ctl frames and HTTP head refusals - design MB.3.6a1 part 1"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.5, MB.9]
paths:
  - "crates/blastit-sys/src/seccomp.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-frame/src/frame.rs"
  - "crates/blastit-http/tests/critical_head_refusals.rs"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - crates/blastit/src/supervise.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T15:14"
---

HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

# Goal

Land the process-level plumbing FastCGI needs before a session exists: the scheduler's
`AF_UNIX` seccomp allowance with `bind`/`listen` denied, the `0x45`–`0x47` pool ctl frames,
the HTTP head-refusal rows, and the two exhaustive `Ctl` matches that would otherwise fail
with `E0004` (design leaf `MB.3.6a1`, part 1). No dependency edge, no `Cargo.lock`.

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.2 (the three exact `seccomp.rs`
  sites), §4.8.2 (`PoolSpawned = 0x45`, `PoolReady = 0x46`, `PoolExited = 0x47`;
  `PoolSpawned` is fd-less), §4.7 rows 15/16/21/22 (head refusals) and §8's
  `MB.3.6.1.1` row; §8.2's `MB.3.6.1` row is the brief re-sync.
- Files (the whole leaf, 6): `crates/blastit-sys/src/seccomp.rs`,
  `crates/blastit-frame/src/{ctl,frame}.rs`,
  `crates/blastit-http/tests/critical_head_refusals.rs`,
  `crates/blastit-scheduler/tests/common/mod.rs`, `crates/blastit/src/supervise.rs`.
- What lands:
  - `seccomp.rs`: (1) `SeccompRole::sockets()` allows `socket(args[0] == AF_UNIX)` for
    `Scheduler` (`!matches!(self, Self::Http)`) while `AF_INET`/`AF_INET6`/`AF_NETLINK`
    stay `EPERM`; (2) `build()` adds an `if matches!(role, SeccompRole::Scheduler)` block
    denying `SYS_bind` and `SYS_listen` (`EPERM`) and keeps both allowed for
    `SeccompRole::View`; (3) unit tests `scheduler_af_unix_allowed_bind_listen_denied` and
    the `view_twin` (`AF_UNIX` `bind`/`listen` allowed). Do **not** narrow the BPF further
    than `args[0]`: the path wall is the pinned `parentfd` + the `0600` socket.
  - `ctl.rs`/`frame.rs`: `PoolSpawned`/`PoolReady { route, sock_dev, sock_ino }`/
    `PoolExited` with their payloads and round-trip tests; `Ctl::Spawn`'s doc comment
    updated to "view or pool, chosen by the route's executor".
  - `critical_head_refusals.rs` (**new file**): the design's head refusal rows — NUL/CR-LF
    header (`400`), `65 KiB` header name (`> 64 KiB` → `431`), `CL`+`TE` and duplicate
    `CL` (`400`) — by the §4.7 named test functions. Additions only.
  - `crates/blastit-scheduler/tests/common/mod.rs` **and**
    `crates/blastit/src/supervise.rs:430-443`: the exhaustive `Ctl` matches gain
    `Ctl::PoolSpawned { .. } | Ctl::PoolReady { .. } | Ctl::PoolExited { .. } => {}`
    (otherwise `E0004`; `scheduler.rs:1043` already has an `other =>` catch-all).
- No `Cargo.toml`, no `Cargo.lock`: the workspace edges and the lock are
  `MB.3.6.1.2`'s. Keep `--locked`.

# Acceptance

```console
$ cargo test -p blastit-sys --locked
$ cargo test -p blastit-frame --locked
$ cargo test -p blastit-http --test critical_head_refusals --locked
$ cargo test -p blastit --locked
$ cargo test -p blastit-scheduler --test critical_rewrite --locked
$ make check
```

`cargo test -p blastit --locked` is in the list because this leaf edits the production
`Ctl` match in `crates/blastit/src/supervise.rs`.

# Notes

- Order inside the leaf: seccomp + ctl frames first (they unblock `MB.3.6.2.1` and
  `MB.3.6.3.1`), then the head rows, then the two `Ctl` matches.
- Do not add a session, a pool spawn or a `check` channel here: those are `MB.3.6.2`,
  `MB.3.6.3` and `MB.3.6.4`.
- `critical_*.rs` files other than the new `critical_head_refusals.rs` are **not** in
  `paths`; `critical_static_manifest.rs` and `critical_static_cli.rs` stay untouched.
