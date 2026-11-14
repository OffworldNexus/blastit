---
id: MB.9.1
title: "Add crates/blastit-backend: the upstream contract trait, types, boot factory and conformance table"
kind: epic
status: ready
priority: 2
depends_on: [MB.3.3, MB.3.4]
paths: [Cargo.lock, "crates/blastit-backend/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T10:50"
---

# Goal

Add `crates/blastit-backend`: the single contract crate every backend implements. It
defines the object-safe `Upstream` trait, its request/response/observation/error types, the
`BackendBuilder` + `Registry` boot factory and the shared conformance **case table**. Pure:
no sockets, no filesystem, no subprocess, no language or server knowledge.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.9.1.1` | crate + `Cargo.lock`, `types.rs` (trait, every §4.2.1 type, `RouteState`/`RoutePermit`, `PinnedSocket`), `registry.rs` (`BackendBuilder`, `Registry`, `BootError`) | `MB.3.3`, `MB.3.4`, `MB.3.5.1` |
| `MB.9.1.2` | `conformance.rs` (`Case`, `Class`, `Capability`, `Expect`, `Inject`, `RouteFixture`, `HeadSpec`, `OutcomeKind`, `PeerScript`, `CancelAt`, `IMPLEMENTATIONS`, `implementation()`, `cases()`) | `MB.9.1.1` |

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.2, §4.2.1 (exact shapes), §4.2 "crates
  by seam", §4.11 and §8's two rows; `ADR-0008`. §8.2's `MB.9.1` row is the brief re-sync:
  `MB.3.5.1` is in `MB.9.1.1`'s `depends_on`, the single source of executor kinds is
  `Executor::kind()` (never "keyed by the manifest backend name"), the first build runs
  **without** `--locked` then `--locked`, and `RouteId`, `RouteState`
  (`try_acquire(self: &Arc<Self>)`), `PinnedSocket::new(Arc<OwnedFd>)` + accessors,
  `UpstreamSignal::ExecutorReady { socket_inode }` and `WireRequest.executor_ran` are all in
  this epic.
- **Dependency direction.** `blastit-backend` → `blastit-http` (`BodyFraming`) +
  `blastit-manifest` *types* (`Executor`, `FastCgiPolicy`, `IngressPolicy`, `StaticPolicy`)
  + `serde` only. It never depends on `blastit-scheduler`, `blastit-view`,
  `blastit-fastcgi` or `blastit`, and contains no I/O syscall, no `tokio` socket type, no
  `unsafe`.
- **`Cargo.lock` is single-owner.** `MB.9.1.1` is the second link of the release chain
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`); `MB.9.1.2` lists
  neither `Cargo.toml` nor `Cargo.lock`.
- **No behaviour change downstream.** The contract is consumed by `MB.9.2`/`MB.9.3` and
  `MB.3.6`; the protected scheduler suites are not touched by this epic.
- **Sequencing.** `MB.9.1.1` → `MB.9.1.2` → `MB.9.2.1.1`; `MB.9` must complete **before
  `MB.4` (Node)** is implemented.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-backend --locked
$ ! grep -rn 'UnixStream\|TcpStream\|std::fs\|Command::new\|tokio' crates/blastit-backend/src && echo "contract crate does no I/O"
$ ! grep -rn 'async_trait\|lazy_static\|OnceCell\|OnceLock' crates/blastit-backend && echo "no globals, no async_trait"
$ cargo tree -p blastit-backend -e normal | grep -q blastit-http && cargo tree -p blastit-backend -e normal | grep -q blastit-manifest && echo "inward edges only"
$ grep -q 'pub trait Upstream' crates/blastit-backend/src && grep -q 'Arc<dyn Upstream>' crates/blastit-backend/src && echo "object-safe contract"
$ grep -q 'pub fn cases' crates/blastit-backend/src/conformance.rs && grep -q 'IMPLEMENTATIONS' crates/blastit-backend/src/conformance.rs && echo "one shared table"
$ make check
```
- [ ] `Arc<dyn Upstream>` compiles, `BootError` implements `std::error::Error`, and a
  missing builder fails closed at boot naming `./blastit.toml:<line>`.

# Notes

- No plugin loader, no dynamic discovery, no `inventory`-style self-registration: the
  composition root names its builders explicitly.
- Fallback split of `MB.9.1.1` if it overruns 550 lines: `.1a` (crate + `types.rs`, owns
  `Cargo.lock`) + `.1b` (`registry.rs` + `lib.rs`, no lock) — mechanical, no path change.
- `ObservationKind::Query` is reserved: no implementation emits it in 0.2.1.
