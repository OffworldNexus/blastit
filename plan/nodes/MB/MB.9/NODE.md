---
id: MB.9
title: "Backend Upstream Contract (DI/IoC): one trait for static, WSGI and embedded PHP"
kind: epic
status: superseded
priority: 2
depends_on: [MB.3.3]
paths: ["crates/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-17T14:07"
---

# Goal

Replace "dispatch knows about backends" with one **injected, language-agnostic
contract**. `crates/blastit-backend` defines the object-safe `Upstream` trait, its
request/response/observation/error types, the boot factory/registry and the shared
conformance case table. Static and WSGI stop being branches in dispatch and become
implementations of that trait, constructed once at boot from the manifest and held
per route as `Arc<dyn Upstream>`; dispatch calls the trait and never branches on
backend kind. After this epic a new backend (embedded PHP in `MB.3.6`, Node in `MB.4`) is
a new implementation plus a registered builder — not a new arm in the scheduler.

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  runtime-provider model (the retired design that first wrote those shapes is deleted with
  `ADR-0010`); this epic may not invent a different trait, registry shape or crate layering. If the design is unimplementable inside the
  seams below, stop and `human add` the conflict instead of widening them silently.
- **Honest starting point.** Static is not injected today:
  `crates/blastit-scheduler/src/static_asset.rs` (`AnswerKind`, resolution) and
  `static_response.rs` are called directly from `scheduler.rs` / `rewrite.rs`;
  `RouteInfo` carries `Option<StaticPolicy>` + `static_forward`. The WSGI path is
  wired in place in the view request path. There is no contract crate and no
  registry. Everything here is new wiring plus two ports, not a rename.
- **No behaviour change.** The ports must preserve: static refusal statuses
  (`400`/`404`), immutable/no-cache headers, `ETag`/`304`/`HEAD`, directory-index vs
  SPA precedence and forward-to-app on a miss; WSGI status, hop-by-hop stripping,
  body credit and `app.output` events. The protected suites
  `critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs`
  and `critical_ingress_enforcement.rs` pass **unmodified**; if a protected contract
  must change, stop and report it as its own node.
- **Dependency direction.** `blastit-backend` → `blastit-http` + `blastit-manifest`
  types only. Implementations depend inward on the contract, never the reverse.
  `blastit-scheduler` depends on the contract, never on a backend implementation.
- **Sequencing.** `MB.9.1` (contract) → `MB.9.2` (`MB.9.2.1` head/stages, `MB.9.2.2`
  `StaticUpstream`) → `MB.9.3` (WSGI) → `MB.9.4` (conformance wired into `make check`).
  `MB.9.1` depends on `MB.3.4` for one reason only: the design's serialized
  `Cargo.lock` chain (`MB.3.4.1` → `MB.9.1` → `MB.9.2.1` → `MB.3.6.1` → `MB.3.8`), and the
  lockfile must stay single-owner (`MB.3.4.1` = design leaf `MB.3.4a`). `MB.3.6` then
  adds the `php` row, and `MB.4` must not start implementation before this epic closes.

# Acceptance

```console
$ cargo test -p blastit-backend --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test conformance_static --test conformance_wsgi --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --test critical_static_resolve --test critical_rewrite --test critical_ingress_enforcement --locked
$ make critical-guard BASE=develop NODE=MB.9
$ test -n "$(grep -rn 'Arc<dyn Upstream>' crates/blastit-scheduler/src)" && echo "route holds an injected handle"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ ! grep -rn 'lazy_static\|OnceCell\|OnceLock' crates/blastit-backend/src crates/blastit-scheduler/src && echo "no global registry"
$ grep -q '^rs-test-conformance:' Makefile && echo "conformance is a make target"
$ make rs-test
$ make check
```

# Notes

- No docs, no manifest grammar, no e2e: this epic is Rust seams and tests only.
- PHP is **not** part of this epic: `MB.3.6` implements the same trait and adds
  the third conformance row. Node (`MB.4`) implements it later and must not need a
  new dispatch branch either.
- Engine changes outside the contract (new dependency, new crate beyond
  `blastit-backend`) are a plan change, not an implementer decision.
- If a port cannot keep a protected suite green without touching it, that is a
  finding for the `lead`, not a licence to edit `critical_*.rs`.
