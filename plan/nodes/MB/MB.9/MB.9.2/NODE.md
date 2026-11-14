---
id: MB.9.2
title: Port static serving onto the injected Upstream trait without behaviour change
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.1]
paths:
  - "crates/blastit-scheduler/src/**"
  - crates/blastit/src/stage1.rs
  - "crates/blastit-scheduler/tests/conformance_static.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T01:54"
---

# Goal

Make the shipped `0.2.0` static responder an **implementation** of `Upstream` rather
than a branch in dispatch. At boot the composition root builds the route's static
`Arc<dyn Upstream>` from the manifest via the registry; `RouteInfo` holds that handle
(non-optional); `scheduler.rs` calls `plan`/`exchange` like it will for any other
backend. Behaviour is byte-identical: refusal statuses, immutable/no-cache headers,
`ETag`/`304`/`HEAD`, directory-index vs SPA precedence, and forward-to-app on a miss.

# Context

- Design of record: `docs/design/php-fastcgi.md` §Backend contract and `ADR-0008`.
  The trait, the registry and the case table come from `MB.9.1`; this node adds the
  first implementation and changes no contract.
- **The adapter.** Wrap `crate::static_asset` (resolution, `AnswerKind`) and
  `crate::static_response` (`StaticResponse`, body pump) behind a
  `struct StaticUpstream { … }` that implements `Upstream`:
  - `plan` performs today's method/body gate and path resolution, and returns the
    refusal as `UpstreamError::Refused { status, reason }`; a miss that may forward
    returns the plan that lets the next hop serve it (`static_forward`), exactly as
    `rewrite.rs` decides today.
  - `exchange` writes head and body through `ResponseSink` under the existing credit
    and returns `ExchangeOutcome` with one `ObservationKind::StaticFile` record
    (already-served file, resolved URL path) and the byte counts.
  - `static_asset.rs` and `static_response.rs` keep their pure logic; they must not
    learn about the trait's wiring. `rewrite.rs` stays the pure precedence module and
    is called by the impl, **not** by dispatch.
- **Injection points.** `crates/blastit-scheduler/src/route.rs` gains
  `upstream: Arc<dyn Upstream>`; drop the `static_policy`/`static_forward` fields
  from the hot path (the policy stays inside the impl). `crates/blastit/src/stage1.rs`
  is the composition root that reads the manifest and builds `RouteInfo` (both the
  `dev` and production paths, plus the `--learn` fallback slot), so it registers the
  static builder and passes the built handle. There is no `Option<Box<dyn …>>`
  fallback and no "default" upstream: a route whose manifest asks for a backend that
  is not registered fails at boot with `./blastit.toml:<line>`.
- **Dispatch.** `scheduler.rs` must contain no backend-kind `match`, no `cfg!` and no
  per-backend call to `static_asset`/`static_response`. Any remaining direct call in
  `scheduler.rs`/`rewrite.rs` is a finding, not a shortcut.
- **Tests.** Add `crates/blastit-scheduler/tests/conformance_static.rs`: drives the
  shared `blastit_backend::conformance` cases against the real `StaticUpstream` on a
  temp root, and proves dispatch through a **fake `Upstream`** double (in-process,
  no socket, no pool) that records the calls. Protected suites
  (`critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs`)
  must pass **unmodified**; this node's paths do not include them. If a protected
  contract has to change, stop and report it as its own node.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test conformance_static --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --test critical_static_resolve --test critical_rewrite --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --locked
$ make critical-guard BASE=develop NODE=MB.9.2
$ test -n "$(grep -rn 'Arc<dyn Upstream>' crates/blastit-scheduler/src)" && echo "RouteInfo holds the injected handle"
$ ! grep -rn 'static_asset::\|static_response::' crates/blastit-scheduler/src/scheduler.rs && echo "dispatch no longer calls the static responder directly"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ make rs-lint
$ make check
```

# Notes

- Determinism first: the conformance driver builds its static root under a tempdir and
  never touches the repo's fixtures; no Docker, no network.
- If removing `static_policy`/`static_forward` from `RouteInfo` ripples into
  non-protected tests (`routing.rs`, `isolation.rs`, `fairness.rs`), update those
  tests as part of this node — but never a `critical_*.rs` file.
- The handle is built once per boot, cloned per route as `Arc`, and never rebuilt or
  looked up by name during a request.
- Budget ~600 changed lines. If the adapter cannot stay that small, land static only
  (head/refusals/observations) and record the exact residue as a follow-up in
  `RESULT.md` — do not start the WSGI port here.
