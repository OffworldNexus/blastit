---
id: MB.2.5
title: "Wire static serving and SPA/front-controller precedence into scheduler dispatch (split: MB.2.5.1 precedence, MB.2.5.2 dispatch)"
kind: epic
status: done
priority: 3
depends_on: [MB.1, MB.2.3]
paths:
  - "crates/blastit-scheduler/src/rewrite.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-14T00:32"
---

# Goal

Wire the safe resolver into the scheduler's request path and implement the documented precedence — exact file → directory `index` → declared SPA / front-controller fallback → the route's own app → `404` — on the existing `RawFrame` channel, with no view spawn for static answers.

# Context

- New pure module `crates/blastit-scheduler/src/rewrite.rs`: given the route id, the decoded `PATH_INFO`, the resolver outcome and the route's static policy, return `Serve`, `Fallback(target)`, `Forward` or `Reject(status)`. Precedence lives here so it is unit-testable without a socket.
- `scheduler.rs` dispatch: a request whose route is static is answered locally by pushing `Frame::RespHead` + body on the existing `http` channel (the same shape the early `413` path already uses at `scheduler.rs:~2582`); no `api` frame, no view fork, no uid. The `request` event still carries the route name and `rule`.
- `route.rs`: carry the static policy into `RouteInfo`. `path.rs`: only the decoding changes the design requires (decode once; `%2F` stays a separator as documented; never re-decode a fallback target).
- **Methods and errors:** `GET`/`HEAD` only, otherwise `405` + `Allow: GET, HEAD`; a request body on a static route is `413` (no `[route.ingress]`); malformed or refused path is `400`/`404` exactly as the design matrix says.
- **The bug this must not have:** a path denied by the safety model must never fall through to the SPA fallback and return `index.html` with `200`. Denial beats fallback, always.
- Do not edit `static_asset.rs`, `config.rs`, `lib.rs` (MB.2.4) or anything under `crates/blastit/**` (MB.2.6). No dependency changes.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.5
$ make rs-test
```

# Notes

- `critical_rewrite.rs` is a new protected file and covers: exact hit, directory index, SPA fallback for an unknown deep URL (`200` `text/html`), front-controller fallback target, existing-file-before-fallback, **denied path never masked by fallback**, `HEAD`, `405` and `Allow`, `413` on a body, `404` with no fallback, and no view spawn for a static answer.
- Reuse the existing batching/credit path for the body; do not introduce a second write loop or a per-request thread.
- Keep `scheduler.rs` edits minimal and localized (dispatch + head construction); the logic that can be pure goes to `rewrite.rs` so the diff is readable.
