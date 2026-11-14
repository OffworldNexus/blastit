---
id: MB.9.1
title: "Add crates/blastit-backend: the upstream contract trait, types, boot factory and conformance table"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.3, MB.3.4]
paths: [Cargo.lock, "crates/blastit-backend/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T01:54"
---

# Goal

Add `crates/blastit-backend`: the single contract crate every backend implements. It
defines the object-safe `Upstream` trait (plan, exchange, observation), the
request/response/observation/error types, the `BackendBuilder` + `Registry` boot
factory, and the shared conformance **case table**. It is pure: no sockets, no
filesystem, no subprocess, no language or server knowledge.

# Context

- Pin the names and shapes from `docs/design/php-fastcgi.md` §Backend contract
  exactly: `pub trait Upstream` with
  `kind() -> &'static str`,
  `plan(&RequestHead) -> Result<UpstreamPlan, UpstreamError>`,
  `exchange<'a>(UpstreamPlan, BodyStream<'a>, &'a mut dyn ResponseSink) ->
  BoxFuture<'a, Result<ExchangeOutcome, UpstreamError>>`, plus `Observation`,
  `ObservationKind { WireRequest, Query, Param, StaticFile, Output }` and
  `UpstreamError { Refused { status, reason }, Unavailable, Fault }`.
- Object safety is a hard requirement: `Arc<dyn Upstream>` must compile. Use the
  design's `BoxFuture` alias; **no `async_trait`, no new dependency, no `cfg!`
  feature switch** that changes the trait per backend.
- `Registry` is keyed by the manifest backend name. `Registry::build(&Manifest) ->
  Result<Vec<Arc<dyn Upstream>>, BootError>`; duplicate registration and an unknown
  backend are boot errors naming the manifest line (`./blastit.toml:<line>`). No
  default/fallback implementation, no `Option<Box<dyn …>>` on the request path.
- `src/conformance.rs` holds the typed case table only (case id, class, request
  head, expected outcome) — data, not a test runner; table integrity and the fake
  `Upstream` double are `MB.9.4`. The classes are the four the design names:
  refusals, body caps, observation records, error mapping.
- Dependencies: `blastit-http` (head/status) and `blastit-manifest` **types** only.
  It must not depend on `blastit-scheduler`, `blastit-view`, `blastit-fastcgi` or
  `blastit`; no I/O crates at all.
- This node owns `Cargo.lock` (and only this node in `MB.9`). It depends on `MB.3.4`
  solely to serialise the lockfile (`MB.3.4` also owns it); the contract code has no
  FastCGI dependency. `members = ["crates/*"]` means no root `Cargo.toml` edit.
- In-crate tests stay here: object safety (`fn _obj(_: Arc<dyn Upstream>) {}`),
  duplicate/unknown registry errors with the manifest line, error → status mapping.
  No protected test is touched.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-backend --locked
$ make rs-lint
$ grep -rq 'pub trait Upstream' crates/blastit-backend/src
$ grep -rq 'Arc<dyn Upstream>' crates/blastit-backend/src
$ test -f crates/blastit-backend/src/conformance.rs && echo "case table lives in the contract"
$ grep -rq 'pub fn build' crates/blastit-backend/src && grep -rq 'BootError' crates/blastit-backend/src
$ ! grep -rn 'UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-backend/src && echo "contract crate does no I/O"
$ ! grep -rn 'async_trait\|lazy_static' crates/blastit-backend && echo "no new dependency, no globals"
$ make check
```

# Notes

- The crate is the seam, not a framework: no plugin loader, no dynamic discovery, no
  `inventory`-style self-registration. The composition root (`crates/blastit`) names
  its builders explicitly at boot.
- Keep it under ~600 changed lines including tests; the trait + types + registry +
  table fit comfortably. If they do not, the design is over-specified — say so in
  `RESULT.md` rather than growing a module the design never asked for.
- `--learn`/`check` consume `Observation` only; do not add per-backend fields there.
- Follow `rust-standards`: no globals, no `unwrap` on fallible paths, MSRV 1.88.
