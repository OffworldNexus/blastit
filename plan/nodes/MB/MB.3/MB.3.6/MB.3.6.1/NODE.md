---
id: MB.3.6.1
title: "Pool ctl frames, workspace deps/lock and scheduler AF_UNIX seccomp - design MB.3.6a1"
kind: epic
status: ready
priority: 3
depends_on: [MB.3.5, MB.9]
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/tests/conformance.rs"
  - crates/blastit/Cargo.toml
  - "crates/blastit-sys/src/seccomp.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-frame/src/frame.rs"
  - "crates/blastit-http/tests/critical_head_refusals.rs"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - crates/blastit/src/supervise.rs
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T10:51"
---

# Goal

Land the serialized dependency/lock window and the process-level plumbing FastCGI needs
before a session exists: the scheduler's `AF_UNIX` seccomp allowance with `bind`/`listen`
denied, the `0x45`–`0x47` pool ctl frames, the HTTP head-refusal rows, and the workspace
`Cargo.toml` edges plus the `"fastcgi-codec"` conformance driver.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.3.6.1.1` | seccomp, `Ctl::PoolSpawned`/`PoolReady { route, sock_dev, sock_ino }`/`PoolExited`, head refusals, both exhaustive `Ctl` matches | `MB.3.5`, `MB.9` |
| `MB.3.6.1.2` | `Cargo.lock` + workspace edges, `"fastcgi-codec"` driver, `Makefile` | `MB.3.6.1.1` |

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.2 (the three exact `seccomp.rs`
  sites), §4.8.2 (`PoolSpawned = 0x45`, `PoolReady = 0x46` with `{ route, sock_dev,
  sock_ino }`, `PoolExited = 0x47`; `PoolSpawned` is fd-less), §4.7 rows 15/16/21/22, §4.11
  and §8's two rows; §8.2's `MB.3.6.1` row.
- `.1` also owns `crates/blastit/src/supervise.rs` (the production `Ctl` match at
  `:430-443`, which needs `Ctl::PoolSpawned { .. } | Ctl::PoolReady { .. } | Ctl::PoolExited
  { .. } => {}` or `E0004`), and `crates/blastit-scheduler/tests/common/mod.rs` (the test
  twin). Its acceptance builds `-p blastit` for exactly that reason.
- `.2` runs its **first** cargo command offline **without** `--locked` to write
  `Cargo.lock` (fourth link of `MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` →
  `MB.3.8`), then `--locked`; it appends `-p blastit-fastcgi --test conformance` to
  `Makefile`.
- `.1` lists no `Cargo.lock` and no `Cargo.toml`.
- `crates/blastit-http/tests/critical_head_refusals.rs` is a **new** protected file; no
  existing `critical_*.rs` is in either leaf's `paths`.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-sys --locked && cargo test -p blastit-frame --locked
$ cargo test -p blastit-http --test critical_head_refusals --locked
$ cargo test -p blastit-fastcgi --test conformance --locked
$ cargo test -p blastit --locked
$ make rs-test-conformance
$ make check
```
- [ ] `Scheduler` allows `socket(AF_UNIX)` and denies `SYS_bind`/`SYS_listen` with `EPERM`,
  while the `View` twin keeps both — proven by the named unit tests.

# Notes

- Do not narrow the BPF further than `args[0] == AF_UNIX`: the path wall is the pinned
  `parentfd` + the `0600` scheduler-owned socket.
- No session, no pool spawn and no `check` channel here — those are `MB.3.6.2`,
  `MB.3.6.3` and `MB.3.6.4`.
