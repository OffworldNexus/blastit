---
id: MB.3.13
title: "PhpEmbedRuntime: embedded view worker loop + stage0 BLASTIT_LIBPHP discovery and ABI boot gate"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.11, MB.3.12, MB.3.5.4]
paths:
  - "crates/blastit-view/src/**"
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_php_embed.rs
  - "crates/blastit/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T14:39"
---

# Goal

`PhpEmbedRuntime`: serve `[route.php]` requests inside the per-route unprivileged `view`
worker over `view.chan`, and gate the whole thing at boot with `BLASTIT_LIBPHP` /
distro NTS `libphp.so` discovery plus the ABI check.

# Context

- ADR-0010 §1/§4: the worker is a normal `view` worker — `UID_BASE + 1000 + i`,
  `SeccompRole::View { egress }`, private `/proc/self/cwd` with only granted proxy sockets,
  `max_requests` recycling. It `dlopen`s the host NTS `libphp.so` and the matching shim
  from `MB.3.12`, then drives the vtable from `MB.3.11`'s interpreter-free loop.
- Boot gate, **before any listener binds**: resolve `BLASTIT_LIBPHP` (read in the stage-0
  config composition root, before the stage-1 `execve` swap) or the distro names
  (`libphp8.4.so`, `8.3`, `8.2`, `libphp.so`), verify NTS (not ZTS) and minor-ABI match
  against the installed shim, and exit 1 with the ADR's two exact messages (missing /
  ZTS-or-mismatch) naming the route and the fix. Never lazily on first request.
- Measurement: `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` around `php_execute_script` in the
  worker loop, reported on `view.chan` through the same path WSGI uses, so the scheduler's
  existing `Cost` clamp turns it into credit. Exact and unforgeable by PHP userland.
- This is the node that makes `MB.9.3`'s `ViewUpstream` able to serve `Executor::Php`
  without any backend-kind branch in dispatch.

# Acceptance

```console
$ cargo test -p blastit --locked
$ make check
```
- A one-line `[route.php] entry = "public/index.php"` fixture serves a response through an
  embedded worker on a host with `libphp-embed`; the request's CPU nanoseconds appear on
  the completion path and reach the `request` event's `cpu_ns`.
- Boot refusals (protected, `crates/blastit/tests/critical_php_embed*.rs`): a
  `BLASTIT_LIBPHP` pointing at a non-library, a ZTS build, and a minor/shim mismatch each
  exit 1 with the ADR's message shape and start no listener; a host with no libphp at all
  refuses only when a `[route.php]` route exists (a static/WSGI manifest still boots).
- `exit()`/`die()`/fatal in the script kills neither the worker nor its slot: the next
  request is served and the crashed request is charged at least its wall time.
- No `libphp` symbol is reachable in `init`, `http`, `scheduler` or `dbproxy`
  (`/proc/<pid>/maps` assertion in the worker/init split test).

# Notes

- Depends on `MB.3.11` (python-off view loop), `MB.3.12` (shim vtable) and `MB.3.5.4`
  (`[route.php]` grammar). Do not start before them.
- OPcache zygote/pre-fork startup order is an ADR open question; choose the post-fork
  `php_module_startup` path unless `docs/design/backend-runtimes.md` settles it otherwise,
  and say which in the RESULT.
