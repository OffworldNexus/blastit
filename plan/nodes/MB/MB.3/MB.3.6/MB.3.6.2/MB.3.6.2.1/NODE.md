---
id: MB.3.6.2.1
title: "FastCgiUpstream session pump, PinnedSocket connect and fastcgi registration - design MB.3.6a2 part 1"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.1.2]
paths:
  - "crates/blastit-scheduler/src/upstream/fastcgi.rs"
  - "crates/blastit-scheduler/src/upstream/fastcgi_builder.rs"
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T15:14"
---

HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

# Goal

Make the PHP route real inside the scheduler **as an `Upstream` implementation**:
`FastCgiUpstream` connects to the route's `php-fpm` pool over its pinned Unix socket, sends
`BEGIN_REQUEST` → `PARAMS` → `STDIN`, parses `STDOUT` through the pure CGI head parser,
pumps the body under the existing credit, turns `STDERR` into `sink.output` lines and
`END_REQUEST` into accounting, and cancels with `ABORT_REQUEST` (design leaf `MB.3.6a2`,
part 1: the session pump).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.1/§4.5.2 (the whole session table),
  §4.2 (the trait and the dispatch call site), §4.8.1 (`capacity`/`ensure_ready`/`signal`)
  and §8's `MB.3.6.2.1` row; §8.2's `MB.3.6.2` row is the brief re-sync.
- Files (the whole leaf, 4): `crates/blastit-scheduler/src/{upstream/fastcgi,upstream/fastcgi_builder,upstream/mod,scheduler}.rs`.
- What lands:
  - `FastCgiPaths` is built **once** by the builder from `manifest_dir` + `entry` using the
    `openat2`-checked relative segments — **never** `std::fs::canonicalize()`; `lockdown`
    takes `&FastCgiPaths` and does no filesystem access.
  - `PinnedSocket` connect only via `/proc/self/fd/<parentfd>/<sock_name>` after `fstatat`
    (`S_ISSOCK`, `st_uid == scheduler_uid` / `invoking_uid` in `dev`), the `C1` inode/owner/
    `0600` check, and the exact `SO_PEERCRED` (`route_uid:route_gid`; `scheduler_uid`
    tolerated under gVisor).
  - One connection per request, `AF_UNIX` `SOCK_STREAM` + `SOCK_CLOEXEC`; exactly one
    `BEGIN_REQUEST` (`RESPONDER`, flags `0`, 8 bytes); `requestId = 1`, `keepConn = 0`; one
    `PARAMS` record + terminator, a `> 65 535` set is `Refused { 400, ParamsTooLarge }`
    **before** `connect`; the body is drained first so the exact `CONTENT_LENGTH` is known,
    then replayed as `STDIN` under the upload credit.
  - `STDOUT` head split on the first `\r\n\r\n` with hop-by-hop and `Content-Length`
    stripping; caps (`≤ 64 KiB`, `≤ 100` headers, `≤ 8 KiB`/value, control bytes →
    `Fault` `502` before a head is sent); `STDERR` ≤ 2048 B/line, ≤ 60 lines, the rest
    counted in `dropped`; `END_REQUEST` `protocol_status != Complete` → `Fault`; EOF before
    `END_REQUEST` → `502` (no head) or `RespAbort` (head sent); dead pool →
    `Unavailable { retry_after: Some(1) }` (`503` + `Retry-After: 1`) after `ensure_ready`;
    cancel → `ABORT_REQUEST` + close + `Cancelled`, slot released only on `SessionMsg::Done`.
  - `ensure_ready` calls `PoolPort::ensure_pool` **only** when `!external_executor` (an
    early return otherwise, so `dev`/`check` never boot a pool as the developer);
    `RouteState::new(pool_spawn == PoolSpawn::External, workers)` (Busy in `Managed` until
    `PoolReady`); `on_ctl` handles `Ctl::PoolSpawned`/`PoolReady`/`PoolExited` (stores
    `bound_inode`, sets `RouteState.ready`, backs off on exit).
  - `upstream/mod.rs`: `pub mod fastcgi; pub mod fastcgi_builder;`. `scheduler.rs`:
    `"fastcgi"` appended to `builtin_registry()`'s list.
  - Mapping: `LockdownError`/`CgiError`/`RecordError` → `Refusal`/`UpstreamError::Fault`;
    the handle lives in `RouteRuntime.stages` — do **not** add a field to `RouteInfo`.
- No `Cargo.toml`, no `Cargo.lock`: the edges landed in `MB.3.6.1.2`. Keep `--locked`.

# Acceptance

```console
$ cargo test -p blastit-scheduler --locked
$ grep -rq 'impl Upstream for' crates/blastit-scheduler/src && echo "FastCGI is an Upstream implementation"
$ grep -q 'impl BackendBuilder for' crates/blastit-scheduler/src/upstream/fastcgi_builder.rs && echo "builder registered"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ ! grep -rn 'canonicalize' crates/blastit-scheduler/src/upstream && echo "no caller-path resolution"
$ make check
```

# Notes

- **`fastcgi.rs` is the pump; `fastcgi_builder.rs` owns `FastCgiPaths`, the mapping,
  `ensure_ready`/`on_ctl` and registration.** Both files are in this leaf's `paths`, so the
  fallback split `.1`/`.2` adds no path.
- The socket identity is a security boundary: no path-based `connect`/`chown`/`chmod`
  anywhere, and no `match` on backend kind.
- Budget ≤ 500 production lines in `upstream/fastcgi.rs`, ≤ 550 changed lines for the leaf.
  If it overruns, land the pump + builder + registration and record the exact residue in
  `RESULT.md`.
