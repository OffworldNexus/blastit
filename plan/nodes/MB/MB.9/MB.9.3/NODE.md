---
id: MB.9.3
title: Port the WSGI forwarding path onto the injected Upstream trait
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.9.2]
paths:
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-view/**"
  - "crates/blastit/src/**"
  - "crates/blastit-scheduler/tests/conformance_wsgi.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T01:54"
---

# Goal

Port the **WSGI forwarding path** onto the same `Upstream` trait, so the Python view
is an injected implementation too. `RouteInfo` for an app route holds the WSGI
`Arc<dyn Upstream>` built at boot; dispatch calls `plan`/`exchange` and never asks
which backend it is talking to. A thin adapter around the existing view request path
is allowed; a `match` on backend kind in dispatch is not.

# Context

- Depends on `MB.9.2`: the injected handle, the registry wiring in
  `crates/blastit/src/stage1.rs` and the no-direct-call rule already exist; this node
  adds the second implementation, not a second mechanism.
- **The adapter.** `struct WsgiUpstream { … }` implements `Upstream`:
  - `plan` maps the request head onto the route's `IngressPolicy` (body codec plus
    the existing request credit) and returns `UpstreamError::Refused` for a method or
    body the view cannot take — the same answers the view path gives today.
  - `exchange` forwards to the view and pumps the response through `ResponseSink`
    under the existing response credit, preserving status, hop-by-hop stripping,
    `app.output` events and cancellation. It returns `ExchangeOutcome` with the
    `WireRequest`/`Query`/`Output` observations the learn path already records — no
    new field and no PHP/Python-specific variant.
  - The adapter may live in `crates/blastit-scheduler` or `crates/blastit-view` where
    the I/O already lives, but it depends **inward** on `blastit-backend`; the
    contract never depends on either.
- **No dispatch branch.** After this node, `scheduler.rs` holds one code path:
  `plan` then `exchange`. Static (`MB.9.2`) and WSGI sit behind the same handle
  shape, and `MB.3.6` can add FastCGI without touching dispatch again. Any
  `match`/`if` on backend kind, backend name or "is this static" is a finding.
- **Tests.** Add `crates/blastit-scheduler/tests/conformance_wsgi.rs`: drives the
  shared `blastit_backend::conformance` cases against the WSGI implementation using
  the existing in-process view harness (no Docker, no real Python pool where the
  harness already fakes it), and keeps a fake `Upstream` double for the dispatch
  assertions. Protected suites (`critical_rewrite.rs`,
  `critical_ingress_enforcement.rs`, `critical_static_*`) must pass **unmodified**;
  if one has to change, stop and report it as its own node.
- `--learn`/`check` must still record exactly what they record today: this node
  moves the code path, it does not change the wire record.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test conformance_wsgi --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --test critical_ingress_enforcement --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-view --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --locked
$ make critical-guard BASE=develop NODE=MB.9.3
$ ! grep -rn 'backend_kind\|is_static' crates/blastit-scheduler/src/scheduler.rs && echo "one dispatch path for every backend"
$ test -n "$(grep -rn 'WsgiUpstream' crates/blastit-scheduler/src crates/blastit-view/src)" && echo "WSGI is an Upstream implementation"
$ make rs-lint
$ make check
```

# Notes

- The learning path is the regression risk: keep a test that a WSGI route's learn
  record is identical before/after the port (same route attribution, same query
  records, same output events) — the `conformance_wsgi` driver can assert the
  `Observation` list rather than re-testing the whole CLI.
- Do not touch `python/**`: the WSGI side is the Rust forwarding path, not the app.
- If the adapter needs a signature change in `blastit-view`'s public request entry,
  make it additive and record it in `RESULT.md`; `MB.4` (Node) reuses the same seam.
- Budget ~600 changed lines. If it overruns, land the forwarding path and leave the
  learn-record parity assertion as an explicit follow-up child — a half-ported path
  with a green suite is the failure mode to avoid.
