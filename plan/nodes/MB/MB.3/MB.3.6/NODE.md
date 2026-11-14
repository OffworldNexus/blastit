---
id: MB.3.6
title: "Dispatch [route.php] and wire dev/check/--learn PHP attribution (external FastCGI fallback kept)"
kind: epic
status: ready
priority: 3
depends_on: []
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs"
  - "crates/blastit-scheduler/tests/conformance_fastcgi.rs"
  - "crates/blastit-scheduler/tests/common/mod.rs"
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/tests/conformance.rs"
  - crates/blastit/Cargo.toml
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_fastcgi_pool.rs
  - crates/blastit/tests/critical_fastcgi_learn.rs
  - crates/blastit/tests/critical_fastcgi_parent_fd.rs
  - crates/blastit/tests/support/fake_php_fpm.rs
  - crates/blastit/tests/fake_php_fpm_smoke.rs
  - "crates/blastit-sys/src/seccomp.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-frame/src/frame.rs"
  - "crates/blastit-http/tests/critical_head_refusals.rs"
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T14:39"
---

# Goal

Make the PHP route real end to end inside the product — **as an implementation of the
shared `Upstream` contract**, not as a branch in dispatch: the scheduler connects to the
route's `php-fpm` pool over its pinned Unix socket, sends `BEGIN_REQUEST` → `PARAMS` →
`STDIN`, pumps `STDOUT` back as the HTTP response, turns `STDERR` into `app.output` events
and `END_REQUEST` into accounting; init pins and freezes the pool parent; `blastit dev`
runs a PHP route beside `php-fpm` with zero `sudo`; and `blastit check` / `--learn`
attribute PHP routes and their grants like every other backend.

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.2 (injection: the handle lives in
  `RouteRuntime.stages` beside `RouteInfo`, never a field on it), §4.5 (the session and its
  ctl frames), §4.8 (pool lifecycle, `parentfd` handoff, unwind), §4.10 (`check`/`--learn`
  attribution) and §8's leaf table. Grammar arrives from `MB.3.5`, codec and guards from
  `MB.3.4`; this epic adds no third copy of either.
- **The contract shape (do not re-pin it here).** FastCGI implements
  `blastit_backend::Upstream`: `plan(&self, head: &UpstreamHead)`,
  `exchange(&self, plan: UpstreamPlan, body: Box<dyn BodySource + Send>, sink: Box<dyn
  ResponseSink + Send>) -> BoxFuture<'_, Result<ExchangeOutcome, UpstreamError>>`,
  `capacity`/`ensure_ready`/`signal`. `ExchangeOutcome::{Answered { status, bytes_in,
  bytes_out, protocol_status, dropped, observations }, Declined { next, plan }}`;
  `Observation { target, #[serde(flatten)] kind }`. `Scheduler::session(upstream, plan,
  body, sink, permit: RoutePermit)` is **`pub(crate)`** — tests assert behaviour, never
  name the types. Dispatch runs `plan`/`exchange` and contains **no `match` on backend
  kind**, no `cfg!` and no direct call into the pool.
- **The fake peer is never a `socketpair`.** Tests bind a hand-rolled temporary-directory
  `UnixListener` and wrap it in `PinnedSocket`; the client connects only through
  `/proc/self/fd/<parentfd>/<sock_name>`.
- **`Cargo.lock` is single-owner.** `MB.3.6.1.2` is the **fourth and only** link of the
  release chain that touches it here (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` →
  `MB.3.6.1.2` → `MB.3.8`); no other child lists `Cargo.lock`.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.3.6.1.1` | scheduler `AF_UNIX` + `bind`/`listen` `EPERM`, `0x45`–`0x47` pool ctl frames, HTTP head refusals, both exhaustive `Ctl` matches | `MB.3.5`, `MB.9` |
| `MB.3.6.1.2` | workspace Cargo edges + `Cargo.lock`, `"fastcgi-codec"` driver, `Makefile` | `MB.3.6.1.1` |
| `MB.3.6.2.1` | `FastCgiUpstream` pump + builder, `PinnedSocket` connect + `C1`, `"fastcgi"` registered | `MB.3.6.1.2` |
| `MB.3.6.2.2` | dispatch + conformance suites, 5-binary `Makefile` gate | `MB.3.6.2.1` |
| `MB.3.6.3.1` | `B1`–`B3`, `parentfd` argv handoff, `BLASTIT_CHECK_*`, protected parent-fd suite | `MB.3.6.2.2` |
| `MB.3.6.3.2` | `render_pool_conf`, `S0`–`S7`/`U1`, fake `php-fpm` helper | `MB.3.6.3.1` |
| `MB.3.6.3.3` | protected `critical_fastcgi_pool` suite | `MB.3.6.3.2` |
| `MB.3.6.4.1` | two-shape `http_obs` reader, `executor_ran`, learn static proposal | `MB.3.6.3.3` |
| `MB.3.6.4.2` | `check::serve_and_run`, 3-way branch order, protected learn suite | `MB.3.6.4.1` |

`MB.3.6.1`–`MB.3.6.4` are sub-epics; the nine leaves above are their children.

# Acceptance

- [ ] All nine leaves `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-fastcgi --test conformance --locked
$ cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --test conformance_fastcgi --locked
$ cargo test -p blastit --test critical_fastcgi_pool --test critical_fastcgi_parent_fd --test critical_fastcgi_learn --locked
$ cargo test -p blastit-sys --locked && cargo test -p blastit-frame --locked
$ grep -rq 'impl Upstream for' crates/blastit-scheduler/src && echo "FastCGI is an Upstream implementation"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ make check
```
- [ ] The pool runs as the route's uid with no `sudo`, `PoolReady` is published only after
  the socket is `0600` scheduler-owned and `parentfd` is restored to `init_uid:init_gid`
  `0555`, and every failure path leaves no `PoolReady` behind.
- [ ] `blastit check -- <suite>` runs a real suite against the documented `dev` server,
  records the observations, starts no parent dbproxy, and maps a non-zero suite exit to `3`.

# Notes

- **Protected tests: add, never weaken.** New files: `critical_fastcgi_dispatch.rs`,
  `critical_fastcgi_pool.rs`, `critical_fastcgi_parent_fd.rs`, `critical_fastcgi_learn.rs`
  and `critical_head_refusals.rs`. `critical_static_manifest.rs` and
  `critical_static_cli.rs` stay green without an edit.
- Budget: each leaf ≤ 7 files and ≤ 550 changed lines, each new `src/upstream/<kind>.rs`
  ≤ 500 production lines. Never merge a later leaf into an earlier one.
- The no-sudo `dev` promise is the page's headline: if the pool cannot start unprivileged
  as designed, stop and `human add` the conflict instead of shipping a `sudo` step.
