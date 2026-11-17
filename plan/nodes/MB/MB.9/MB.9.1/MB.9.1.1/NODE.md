---
id: MB.9.1.1
title: "Add crates/blastit-backend: Upstream trait, contract types, PinnedSocket, Registry - design MB.9.1 part 1"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: [MB.3.3, MB.3.4, MB.3.5.1]
paths:
  - Cargo.lock
  - "crates/blastit-backend/Cargo.toml"
  - "crates/blastit-backend/src/lib.rs"
  - "crates/blastit-backend/src/types.rs"
  - "crates/blastit-backend/src/registry.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-17T14:07"
---

# Goal

Create `crates/blastit-backend` — the single contract crate every backend implements:
the object-safe `Upstream` trait, every §4.2.1 type, `RouteState`/`RoutePermit`,
`PinnedSocket`, the `BackendBuilder`/`Registry` boot factory and `BootError` (design leaf
`MB.9.1`, part 1). Pure: no sockets, no filesystem, no subprocess, no language or server
knowledge.

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  runtime-provider model; every exact shape is restated in this brief, because the retired
  design that first wrote them is deleted with `ADR-0010`. The brief carries: `depends_on` includes `MB.3.5.1`, the single source is
  `Executor::kind()`, the first build runs **without** `--locked`, and `RouteId`,
  `RouteState`, `PinnedSocket`, `UpstreamSignal::ExecutorReady { socket_inode }` and
  `WireRequest.executor_ran` are all in this leaf.
- Files (the whole leaf, 5): `Cargo.lock`, `crates/blastit-backend/Cargo.toml`,
  `crates/blastit-backend/src/{lib,types,registry}.rs`.
- Contract shape (do not invent a variant):
  - `pub trait Upstream: Send + Sync` with `kind() -> &'static str`,
    `capacity() -> Capacity`, `ensure_ready() -> BoxFuture<'_, Result<(), UpstreamError>>`,
    `signal(UpstreamSignal)` (default no-op),
    `plan(&self, head: &UpstreamHead) -> Result<UpstreamPlan, UpstreamError>` and
    `exchange(&self, plan: UpstreamPlan, body: Box<dyn BodySource + Send>,
    sink: Box<dyn ResponseSink + Send>) -> BoxFuture<'_, Result<ExchangeOutcome, UpstreamError>>`.
  - `ExchangeOutcome::{Answered { status, bytes_in, bytes_out, protocol_status, dropped, observations }, Declined { next, plan }}`;
    `Declined` writes no head.
  - `Observation { target: ObsTarget, #[serde(flatten)] kind: ObservationKind }` with
    `ObservationKind` **internally tagged** (`#[serde(tag = "kind", rename_all = "snake_case")]`);
    `ParamDecision`/`OutputStream` derive `Serialize`/`Deserialize`.
  - `RouteId(u32)` (`Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`, minted only
    by `Registry::build_route`/`BuildCtx`); `RouteState` with
    `try_acquire(self: &Arc<Self>) -> Option<RoutePermit>` where the permit is `Send +
    'static` and its `Drop` decrements `inflight`; `PinnedSocket` (`Clone, Debug`, **not**
    `PartialEq`/`Eq`) with `new(Arc<OwnedFd>, …)` validating only the leaf name
    (`PinnedSocketError::{BadName, Poisoned}`, `PoisonError` mapped, never `unwrap`) plus
    its accessors and `bound_inode`/`set_bound_inode`.
  - `BodySource`, `CancelToken`, `BodyEvent`, `ReplyHead`, `ResponseSink`, `SinkError`,
    `ProtocolStatus`, `UpstreamError` (`Refused`/`Unavailable`/`Fault`/`Cancelled`),
    `Refusal` (sole status authority `status()`), `SessionPort`, `ExchangeChannel`,
    `PoolPort`, `BackendBuilder`, `RouteSpec`, `BuildCtx`/`BuildLimits`, `Registry`.
  - `Registry::new(Vec<Arc<dyn BackendBuilder>>) -> Result<Self, BootError>` rejects
    duplicate kinds; `build_route(&RouteSpec, &BuildCtx) -> Result<Vec<Arc<dyn Upstream>>, BootError>`
    builds the executor first and wraps it with `"static"` when `spec.static_policy.is_some()`,
    returning `[static?, executor?]`. No fallback backend: the only boot error is a kind
    with no registered builder, naming the manifest line from `Manifest::route_lines()`
    (`./Blastfile:<line>: no builder registered for backend "…"`).
  - `BootError { manifest, line, route, kind, detail }` implements `std::error::Error`
    (required by `SchedulerError::Boot`'s `#[error("{0}")]`), hand-written `Display`.
- Crate rules: `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, **no** `async_trait`,
  no `cfg!` feature switch, no globals/`lazy_static`/`OnceCell`.
- Dependencies: `blastit-http` (`BodyFraming`), `blastit-manifest` **types** and
  `serde = { version = "1.0.229", features = ["derive"] }` only. Never
  `blastit-scheduler`, `blastit-view` or `blastit`.
- **`Cargo.lock` is yours and this is the second link of the release chain**
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.8`; the retired `MB.3.6.1.2` link went with
  the external PHP path, `MB.3.17`): run the first
  build offline **without** `--locked` to write the lock, then every later command with
  `--locked`. `members = ["crates/*"]` means no root `Cargo.toml` edit.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-backend
$ cargo test -p blastit-backend --locked
$ ! grep -rn 'UnixStream\|TcpStream\|std::fs\|Command::new\|tokio' crates/blastit-backend/src && echo "contract crate does no I/O"
$ cargo tree -p blastit-backend -e normal | grep -q blastit-http && cargo tree -p blastit-backend -e normal | grep -q blastit-manifest && echo "inward edges only"
$ make check
```

# Notes

- In-crate tests: object safety (`fn _obj(_: Arc<dyn Upstream>) {}`), a `Send` future
  assertion, duplicate/unknown registry errors with the manifest line, the
  `Refused`→status / `Unavailable`→`503` / `Fault`→`502` / `Cancelled`→no-status mapping,
  `wire_request_line_is_exact`, and a real `WireSqlObs` line parsed through `ObsTarget`.
- `conformance.rs` is `MB.9.1.2`'s; do not pre-create it. `Cargo.toml` is yours and no
  later leaf edits it.
- If the trait + types overflow 550 changed lines, the fallback split is
  `MB.9.1.1.1` (crate + `types.rs`, owns `Cargo.lock`) + `MB.9.1.1.2` (`registry.rs` +
  `lib.rs`, no lock) — record it in `RESULT.md` rather than growing a module the design
  never asked for.
