---
id: MB.3.6.2
title: "FastCgiUpstream for the external fallback only: session pump, PinnedSocket connect, conformance row"
kind: epic
status: superseded
priority: 3
depends_on: [MB.3.6.1]
paths:
  - "crates/blastit-scheduler/src/upstream/fastcgi.rs"
  - "crates/blastit-scheduler/src/upstream/fastcgi_builder.rs"
  - "crates/blastit-scheduler/src/upstream/mod.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/tests/critical_fastcgi_dispatch.rs"
  - "crates/blastit-scheduler/tests/conformance_fastcgi.rs"
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T15:14"
---

# Goal

Make the PHP route real inside the scheduler **as an `Upstream` implementation**, and prove
it: `FastCgiUpstream` connects through the pinned descriptor, runs the session pump, and
registers `"fastcgi"`; the two suites (dispatch + conformance) and the `Makefile` gate
follow.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.3.6.2.1` | `FastCgiUpstream` session pump (`fastcgi.rs`) and the builder (`fastcgi_builder.rs`), `"fastcgi"` registered | `MB.3.6.1.2` |
| `MB.3.6.2.2` | `critical_fastcgi_dispatch.rs` + `conformance_fastcgi.rs`, 5-binary `Makefile` gate | `MB.3.6.2.1` |

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.5.1/§4.5.2 (the whole session table),
  §4.8.1 (`capacity`/`ensure_ready`/`signal`), §4.8.3 (the post-`PoolReady` parent-rename
  sentinel), §7.1/§7.2 and §8's two rows; §8.2's `MB.3.6.2` row.
- **`.1` is the session pump.** `FastCgiPaths` is built from `openat2`-checked **relative
  segments** — **never** `std::fs::canonicalize()`. The connect is
  `/proc/self/fd/<parentfd>/<sock_name>` only, after `fstatat` (`S_ISSOCK`, owner) and the
  `C1` inode/owner/`0600` check, with the exact `SO_PEERCRED`. `ensure_ready` returns `Ok`
  immediately when `external_executor` is true (so `dev`/`check` never boot a pool as the
  developer), and `"fastcgi"` is appended to `builtin_registry()`'s list in `scheduler.rs`.
- **`.2` is the suites + `Makefile`.** The fake peer is a `TempDir` `UnixListener` wrapped
  in a `PinnedSocket` built via `PinnedSocket::new` — **never** a `socketpair`, never a
  path string. It covers the post-ready parent-rename sentinel (`200` from the pinned
  socket, `0` `accept`s on the sentinel), row 25 (`504` before head + `ABORT_REQUEST`
  observed + slot released once), the panic/non-UTF-8 rows and
  `foreign_uid_peer_is_fault_502`, and asserts the **5**-binary `rs-test-conformance` list.
- Neither leaf edits a `Cargo.toml` or lists `Cargo.lock`: `MB.3.6.1.2` owns both edges and
  the lock in this window.
- `critical_fastcgi_dispatch.rs` is a **new** protected file; the four existing scheduler
  suites stay unmodified.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit-scheduler --test critical_fastcgi_dispatch --test conformance_fastcgi --locked
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
$ cargo test -p blastit-scheduler --locked
$ grep -rq 'impl Upstream for' crates/blastit-scheduler/src && echo "FastCGI is an Upstream implementation"
$ ! grep -rn 'backend_kind' crates/blastit-scheduler/src && echo "no backend-kind dispatch"
$ ! grep -rn 'canonicalize' crates/blastit-scheduler/src/upstream && echo "no caller-path resolution"
$ make rs-test-conformance
```
- [ ] Every §4.5.2 row is a test, not a note; the sentinel proof shows the pinned socket
  answering while the renamed path records zero `accept`s.

# Notes

- **Protected tests: add, never weaken.** The socket identity is a security boundary: no
  path-based `connect`/`chown`/`chmod` anywhere, and no `match` on backend kind.
- Fallback split of `.1` if it overruns 550 lines: `.1` (`fastcgi.rs` pump) + `.2`
  (`fastcgi_builder.rs`) — both files are already in `.1`'s `paths`, so no path changes.
- No Docker, no `php-fpm`, no sleeping in either suite.
