---
id: MB.9.2.2
title: "StaticUpstream adapter and deletion of the static dispatch branches - design MB.9.2b"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: [MB.9.2.1.3]
paths:
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/upstream/static.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/tests/conformance_static.rs"
  - "crates/blastit-scheduler/tests/common/fake_upstream.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-17T14:07"
---

# Goal

Make the shipped `0.2.0` static responder an **implementation** of `Upstream`
(`StaticUpstream`) and delete every static branch in dispatch, so a miss that may forward
returns `Declined { next, plan }` and 0.2.0's behaviour stays byte-identical (design leaf
`MB.9.2b`).

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract, with the static port table,
  the `Forward` protocol and the stage chain built by `Registry::build_route` restated here;
  the retired design is deleted with `ADR-0010`.
- Files (the whole leaf, 5): `crates/blastit-scheduler/src/upstream/{mod,static}.rs`,
  `crates/blastit-scheduler/src/scheduler.rs`,
  `crates/blastit-scheduler/tests/conformance_static.rs`,
  `crates/blastit-scheduler/tests/common/fake_upstream.rs`.
- What lands:
  - `StaticUpstream { root, policy, next: Option<Arc<dyn Upstream>>, … }` around the
    existing `static_asset.rs` (resolution, `AnswerKind`) and `static_response.rs`
    (`StaticResponse`, body pump).
  - **`plan` = `head_segments` + the `.php`/`.phar` `EntryScript` gate** (`404`,
    `BodyMode::None`, before `405`/`413`, zero source bytes, never forwarded). The method
    `405` and body `413` answers stay in `exchange` via `rewrite::decide` on
    `StaticOutcome::File` — they are not part of `plan`.
  - `exchange` = resolve + `decide`: `Serve` writes head/body through `ResponseSink` under
    the existing credit and returns one `ObservationKind::StaticFile`; `Reject` is
    `Refused`; `Forward` is `Declined { next, plan: next.plan(&plan.head)? }`.
  - `upstream/mod.rs`: `pub mod static;`.
  - `scheduler.rs`: one path — `plan` then `exchange` on `runtime.stages[request.stage]`.
    On `Declined` verify identity, release the head-phase slot, set `request.stage = 1` and
    re-enter `admit_ready` (0.2.0's `forward_static` order). Delete `Request.pumping`,
    `Scheduler.static_in` and `release_static_slot` (the session task awaits its own
    `spawn_blocking` read, so there is no second slot owner). `"static"` is appended to
    `builtin_registry()`'s list **in `scheduler.rs`**.
  - `conformance_static.rs` iterates `blastit_backend::conformance::cases()` against the
    real `StaticUpstream` on a tempdir root and includes the fake via
    `#[path = "common/fake_upstream.rs"] mod fake_upstream;`; no `case.id` literals.
- **Preserved, byte-identical:** refusal statuses (`400` traversal/absolute/backslash/
  control/invalid UTF-8/over-long; `404` symlink/special/oversize/dotfile/unreadable/
  `SpaLost`/miss-without-app), `no-store` on every scheduler-written refusal,
  `immutable`/`no-cache` keyed on the answering file, `ETag`/`Last-Modified`/`304`/`HEAD`,
  directory-index vs SPA precedence, `405` + exactly `Allow: GET, HEAD`, `413` for a
  declared body, and `Forward` re-entering the ordinary queue.
- No `Cargo.toml`, no `Cargo.lock`: `blastit-backend` was added in `MB.9.2.1.1`. Keep
  `--locked`.

# Acceptance

```console
$ cargo test -p blastit-scheduler --test conformance_static --test critical_static_assets --test critical_static_resolve --test critical_rewrite --locked
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
$ grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_static.rs && echo "one table, every driver"
$ ! grep -rn 'static_asset::\|static_response::' crates/blastit-scheduler/src/scheduler.rs && echo "dispatch no longer calls the static responder"
$ ! grep -rn 'release_static_slot\|static_in' crates/blastit-scheduler/src && echo "one slot owner: Done"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ make check
```

# Notes

- Protected suites pass **unmodified**. `critical_rewrite.rs` reads
  `RouteInfo.static_policy`/`static_forward`, which stay on the descriptor.
- The double is a test fixture, never a shipped "null backend": it must not be reachable
  from `crates/blastit-backend/src` or registered in a boot path.
- Fallback split if the adapter overruns 550 lines: `.1` (`StaticUpstream`) + `.2`
  (deleted branches + `"static"` registration) — mechanical, no path change.
