---
id: MB.3.6.2.2
title: "FastCGI dispatch and conformance suites plus the 5-binary Makefile gate - design MB.3.6a2 part 2"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.2.1]
paths:
  - "crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs"
  - "crates/blastit-scheduler/tests/conformance_fastcgi.rs"
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T15:14"
---

# Goal

Prove the FastCGI session with its two suites and wire the second driver into the gate:
`critical_fastcgi_dispatch.rs` (protected) and `conformance_fastcgi.rs` (`"fastcgi-session"`),
plus the `Makefile` append and the 5-binary `rs-test-conformance` assertion (design leaf
`MB.3.6a2`, part 2).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.2 (every row is a test), §7.1/§7.2
  (`R9`–`R11`, `R19` caps, `R20`, `R8` rows 24/25), §4.8.3 (the post-`PoolReady`
  parent-rename sentinel) and §8's `MB.3.6.2.2` row; §8.2's `MB.3.6.2` row.
- Files (the whole leaf, 3): `crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs`,
  `crates/blastit-scheduler/tests/conformance_fastcgi.rs`, `Makefile`.
- What lands:
  - The fake peer is a `TempDir` `UnixListener` wrapped in a `PinnedSocket` built via
    `PinnedSocket::new` — **never** a `socketpair`, never a path string; the client connects
    exclusively through `/proc/self/fd/<parentfd>/<sock_name>`. The scripted thread
    **asserts the received bytes before replying**; `ensure_ready`/backoff is driven with
    `tokio::time::pause()`. No Docker, no `php-fpm`, no pool.
  - Positive rows: record order, one connection, `Status: 200..=599`/default `200`,
    re-assembly of 1-byte/8184/65535-byte chunks, non-UTF-8 `Content-Type` & `STDERR` lossy
    conversion (zero panic), exact `CONTENT_LENGTH`, `requestId = 1`, `prefork` spawn,
    cold-start waits for `PoolReady`; **post-build parent rename + symlink to a sentinel
    `UnixListener` still serves `200` from `PinnedSocket` while the sentinel records `0`
    `accept`s**; `external_mode_never_calls_ensure_pool` (recording `PoolPort` fake with
    `Arc<AtomicUsize>` + panic-on-call: `external_executor = true` → counter `== 0`;
    the `false` twin → counter `== 1` and the request waits for `PoolReady`).
  - Negative rows: no `END_REQUEST` (`502`/`RespAbort`), bad `protocol_status`, final `1xx`
    (`502`), non-HTAB CTL/DEL (`502`), the four proxy-control response headers (`502`) while
    a benign `X-Accel-Buffering: no` stays `200` with the header absent, missing colon/CRLF/
    1000 headers/64 KiB head (`502`), a non-token CGI header name
    (`X-Accel-Redirect\x00: /x`, ` X-Accel-Redirect: /x`) → `502`, over-2048-byte
    unterminated `STDERR` capped in `O(1)`, 60-lines/request `STDERR` cap, 8 KiB CGI value
    cap, `BLASTIT_MAX_WORKERS × BLASTIT_MAX_BODY_BYTES` aggregate, `drain` and a parked
    static read keeps its slot, `CL`/`TE` stripped, `appStatus`/`reserved` ignored, connect
    `ENOENT`/`ECONNREFUSED` → `503` + backoff, cancel → `ABORT_REQUEST`, **row 25: a declared
    `Content-Length` stalled past `request_timeout` → `504` before head + `ABORT_REQUEST`
    observed on the fake peer + close + slot released once**, `foreign_uid_peer_is_fault_502`,
    and the panic/`SessionDoneGuard` slot-release row.
  - `conformance_fastcgi.rs`: registers `"fastcgi-session"` and iterates
    `blastit_backend::conformance::cases()`; no `case.id` literals.
  - `Makefile`: append the driver and assert the **5**-binary `rs-test-conformance` list.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ make rs-test-conformance
$ cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --locked
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
$ ! grep -rn 'case\.id' crates/blastit-scheduler/tests/conformance_fastcgi.rs && echo "table-driven, not hand-listed"
$ ! grep -rn 'socketpair' crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs && echo "fake peer is a pinned UnixListener"
$ make check
```

# Notes

- **Protected tests: add, never weaken.** `critical_fastcgi_dispatch.rs` is a new file. The
  four existing scheduler suites (`critical_static_assets`, `critical_static_resolve`,
  `critical_rewrite`, `critical_ingress_enforcement`) must pass **unmodified**.
- New suites carry `#![allow(clippy::unwrap_used, clippy::expect_used)]` exactly like the
  existing ones.
- `Makefile` is append-only here; `MB.3.6.1.2` added the codec driver, `MB.9.4` created the
  target.
