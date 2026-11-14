---
id: MB.9.2
title: "Port static serving onto the injected Upstream trait without behaviour change (split: MB.9.2.1 head/stages, MB.9.2.2 StaticUpstream)"
kind: epic
status: ready
priority: 2
depends_on: [MB.9.1]
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/upstream/static.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/error.rs"
  - "crates/blastit-scheduler/tests/head_allocs.rs"
  - "crates/blastit-scheduler/tests/conformance_static.rs"
  - "crates/blastit-scheduler/tests/common/fake_upstream.rs"
  - "crates/blastit-frame/src/raw.rs"
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/roles.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T10:51"
---

# Goal

Make the shipped `0.2.0` static responder an **implementation** of `Upstream` rather than
a branch in dispatch, with byte-identical behaviour: refusal statuses, immutable/no-cache
headers, `ETag`/`304`/`HEAD`, directory-index vs SPA precedence, and forward-to-app on a
miss.

Split into `MB.9.2.1` (three leaves) and `MB.9.2.2` (design §8/§8.1):

| Plan id | Design leaf | Lands |
|---|---|---|
| `MB.9.2.1.1` | `MB.9.2a` part 1 | `blastit-backend` + `blastit-http` edges + `Cargo.lock`, `ReqHeadView::scheme()`, `server_name`/`server_port`, `RouteInfo.executor`, `SchedulerConfig.{http_obs, pool_spawn, pinned_sockets, route_lines}` + `pinned_socket_fds()`, `stage1.rs` literal |
| `MB.9.2.1.2` | `MB.9.2a` part 2 | `UpstreamHead` construction, `RouteRuntime { info, stages }`, `Request.stage`, `ViewHub`/`SessionPort`, the one session task + `SessionDoneGuard`, `ResponseSink` proxy-control denial, `http_obs` writer, `run_with`/`RunError`/`builtin_registry()`, `mod upstream;`, `BuildCtx.external_executor`, admission/`Done` slot rule, `head_allocs` budget |
| `MB.9.2.1.3` | `MB.9.2a` part 3 | `lib.rs` `pub use` re-exports only, `roles::scheduler` `run_with` + `RunError` print |
| `MB.9.2.2` | `MB.9.2b` | `StaticUpstream` (`src/upstream/static.rs`), deletion of the static branches in `scheduler.rs`, `conformance_static.rs`, `tests/common/fake_upstream.rs` |

Plan ids are numeric because `tools/plan.py` only accepts numeric segments; each child's
title and body carry the design's lettering. `MB.9.2.1` depends on `MB.9.1`;
`MB.9.2.2` depends on `MB.9.2.1`.

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract and `ADR-0009`'s
  `WorkerRuntime`/`ViewUpstream`, with the shapes restated here (`RouteRuntime`,
  `Request.stage`, `with_php` after `with_static`, `UpstreamHead`, `SessionPort`, the static
  `Forward` protocol, `ViewHub` + the session task); the retired design that first wrote
  them is deleted with `ADR-0010`. The trait, the registry and the case table come from `MB.9.1`; this epic
  adds the first implementation and changes no contract.
- **The handle lives beside the descriptor.** `RouteRuntime { info: RouteInfo, stages:
  Vec<Arc<dyn Upstream>> }` (1 or 2), not a field on `RouteInfo`: adding
  `Arc<dyn Upstream>` there would break its `Eq` derive and the protected value
  comparisons. `RouteInfo` keeps `static_policy`/`static_forward` (protected suites read
  them) and gains `executor: Option<Executor>`; every dispatch branch on them is deleted
  by `MB.9.2.2`.
- **No behaviour change.** `MB.9.2.1` keeps the existing static path reachable while it
  adds the stage chain, so the protected suites are green at the end of **both** leaves;
  `MB.9.2.2` ends with exactly one path (`plan` then `exchange`). Protected suites
  (`critical_static_assets.rs`, `critical_static_resolve.rs`, `critical_rewrite.rs`,
  `critical_ingress_enforcement.rs`) pass **unmodified**; a protected contract change is
  its own node.
- **`Cargo.lock` is single-owner.** `MB.9.2.1.1` (`Cargo.lock`, third link of
  `MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`) adds `blastit-backend`
  and `blastit-http` to `crates/blastit-scheduler/Cargo.toml`; `MB.9.2.1.2`, `MB.9.2.1.3`
  and `MB.9.2.2` list neither `Cargo.toml` nor `Cargo.lock` and keep `--locked`.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test conformance_static --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --test critical_static_resolve --test critical_rewrite --test critical_ingress_enforcement --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --locked
$ grep -q 'struct RouteRuntime' crates/blastit-scheduler/src/scheduler.rs && echo "handle lives beside the descriptor"
$ ! grep -rn 'static_asset::\|static_response::' crates/blastit-scheduler/src/scheduler.rs && echo "one dispatch path"
$ grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_static.rs && echo "one shared table"
$ make check
```
- [ ] Static refusals, headers, `ETag`/`304`/`HEAD`, precedence and `Forward` are
  byte-identical to `0.2.0`, proven by the unmodified protected suites.

# Notes

- Determinism first: the conformance driver builds its static root under a tempdir and
  never touches the repo's fixtures; no Docker, no network, no sleeping.
- The handle is built once per boot by `Registry::build_route`, cloned as `Arc` per route,
  and never rebuilt or looked up by name during a request.
- Do not start the WSGI port here (`MB.9.3`) and do not touch `python/**`.
