---
id: MB.3.6
title: "Dispatch [route.php] over view.chan and wire dev/check/--learn PHP attribution"
kind: epic
status: superseded
priority: 3
depends_on: []
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - crates/blastit/Cargo.toml
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_php_learn.rs
  - "crates/blastit-sys/src/seccomp.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-frame/src/frame.rs"
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-18T20:36"
---

# Goal

Make the PHP route real end to end inside the product — **as an implementation of the
shared `Upstream` contract**, not as a branch in dispatch: the scheduler hands the request
to the route's PHP `view` worker over the unnamed `view.chan` `RawFrame` `socketpair`,
drives `PhpEmbedRuntime`, streams response headers and body chunks back, turns the worker's
stderr into `app.output` events and the per-request cost into accounting (ADR-0010 §1);
`blastit dev` runs a `[route.php]` route with zero `sudo` and no external PHP process; and
`blastit check` / `--learn` attribute PHP routes and their grants like every other backend.

# Context

- Design of record: `ADR-0010` §1–§2 (the embedded `libphp` worker, the `view.chan`
  `RawFrame` stream, `[route.php]` with `entry`/`workers`), `ADR-0009`'s `WorkerRuntime`, and
  this brief's own §4.2/§4.5/§4.10 shapes (the handle lives in `RouteRuntime.stages` beside
  `RouteInfo`, never a field on it; the ctl frames; `check`/`--learn` attribution). Grammar
  arrives from `MB.3.5`, the worker from `MB.3.13`; this epic adds no third copy of either,
  and there is no pool to synchronise.
- **The contract shape (do not re-pin it here).** The embedded PHP view implements
  `blastit_backend::Upstream`: `plan(&self, head: &UpstreamHead)`,
  `exchange(&self, plan: UpstreamPlan, body: Box<dyn BodySource + Send>, sink: Box<dyn
  ResponseSink + Send>) -> BoxFuture<'_, Result<ExchangeOutcome, UpstreamError>>`,
  `capacity`/`ensure_ready`/`signal`. `ExchangeOutcome::{Answered { status, bytes_in,
  bytes_out, protocol_status, dropped, observations }, Declined { next, plan }}`;
  `Observation { target, #[serde(flatten)] kind }`. `Scheduler::session(upstream, plan,
  body, sink, permit: RoutePermit)` is **`pub(crate)`** — tests assert behaviour, never
  name the types. Dispatch runs `plan`/`exchange` and contains **no `match` on backend
  kind**, no `cfg!` and no direct call into a worker channel.
- **The exchange is a `socketpair`, not a named socket.** PHP is served over the worker's
  unnamed `view.chan` `RawFrame` channel (ADR-0010 §1); tests drive the same `Upstream`
  trait through the in-process fake session. There is no pool parent fd and no
  `PinnedSocket` connect for PHP.
- **`Cargo.lock` is single-owner.** The retired `MB.3.6.1.2` link went with the external
  path (`MB.3.17`), so the chain is `MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.8`; no
  child of this epic lists `Cargo.lock`.

| Plan id | Status | Lands |
|---|---|---|
| `MB.3.6.1.1` | `superseded` | scheduler `AF_UNIX` + `bind`/`listen` `EPERM`, pool ctl frames, HTTP head refusals |
| `MB.3.6.1.2` | `superseded` | workspace Cargo edges + `Cargo.lock`, the external codec driver |
| `MB.3.6.2.1` | `superseded` | the external session pump, `PinnedSocket` connect, registration |
| `MB.3.6.2.2` | `superseded` | external dispatch + conformance suites |
| `MB.3.6.3.1` | `superseded` | `B1`–`B3`, `parentfd` argv handoff, `BLASTIT_CHECK_*` |
| `MB.3.6.3.2` | `superseded` | pool config rendering, the `S0`–`S7`/`U1` spawn sequence |
| `MB.3.6.3.3` | `superseded` | protected pool suite |
| `MB.3.6.4.1` | `ready` | two-shape `http_obs` reader, `executor_ran`, learn static proposal |
| `MB.3.6.4.2` | `ready` | `check::serve_and_run`, 3-way branch order, protected learn suite |

`MB.3.6.1`–`MB.3.6.3` are `superseded` sub-epics whose pool machinery died with `MB.3.17`;
their historical briefs keep the record. The live work is `MB.3.6.4` and its two leaves, on
top of the embedded worker from `MB.3.13`.

# Acceptance

- [ ] The live leaves (`MB.3.6.4.1`, `MB.3.6.4.2`) are `done` with `RESULT.md` evidence, and
the epic gate confirms:
```console
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --test critical_php_learn --locked
$ cargo test -p blastit-sys --locked && cargo test -p blastit-frame --locked
$ grep -rq 'impl Upstream for' crates/blastit-scheduler/src && echo "the embedded PHP view is an Upstream implementation"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ make check
```
- [ ] The PHP `view` worker runs as the route's uid with no `sudo` and is published ready
  only after `SeccompRole::EmbeddedView { egress }` and the uid drop succeed (ADR-0010 §1);
  every failure path leaves no ready worker behind.
- [ ] `blastit check -- <suite>` runs a real suite against the documented `dev` server,
  records the observations, starts no parent dbproxy, and maps a non-zero suite exit to `3`.

# Notes

- **Protected tests: add, never weaken.** The one new protected file is
  `critical_php_learn.rs`, owned by the live `MB.3.6.4.2`. The pool-era
  `critical_php_dispatch.rs`, `conformance_php.rs` and `critical_head_refusals.rs` had no
  live owner (`MB.3.6.1.1`/`MB.3.6.2.2` are superseded) and are not part of this epic.
  `critical_static_manifest.rs` and `critical_static_cli.rs` stay green without an edit.
- Budget: each leaf ≤ 7 files and ≤ 550 changed lines, each new `src/upstream/<kind>.rs`
  ≤ 500 production lines. Never merge a later leaf into an earlier one.
- The no-sudo `dev` promise is the page's headline: if the worker cannot start unprivileged
  as designed, stop and `human add` the conflict instead of shipping a `sudo` step.
