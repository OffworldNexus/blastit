---
id: MB.3.13
title: "PhpEmbedRuntime: embedded view worker loop + stage0 BLASTIT_LIBPHP discovery and ABI boot gate"
kind: epic
status: done
priority: 2
depends_on: [MB.3.11, MB.3.12, MB.3.5.4, MB.3.37, MB.3.39, MB.3.40, MB.3.41]
paths:
  - "crates/blastit-view/src/**"
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_php_embed.rs
  - "crates/blastit/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-15T06:25"
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

## Egress doctrine (from MB.3.12)

The boundary is the **seccomp filter** — `SeccompRole::View { egress: false }` refusing
`socket(AF_INET)`/`AF_INET6` with `EPERM`. The INI policy (`allow_url_fopen=0`) is
**defence in depth, never the wall**: it does not stop the `curl` extension, `sockets`, or
a PDO driver opening its own connection. `MB.3.12` proves both layers and proves the strong
one holds with the weak one removed; this worker must install the real filter at the real
install site and pass `Egress` from the manifest, so the INI value and the filter can never
disagree. If a test can only show one layer, say which one it proves.

## Prerequisite from MB.3.12 (vtable contract)

Call the five vtable symbols and check `blastit_php_minor()` against the loaded
`php_version()` **before the first request** — a mismatch is a boot refusal naming the
host's PHP minor and the shim that was found, never a first-request fault. `MB.3.12`
also proved that `php_execute_script` **swallows** the bailout (returns `false`; the outer
`zend_first_try` does not fire for `exit()`/fatals): treat a `false` return as a status,
not as success.

## Prerequisite from MB.3.11

`MB.3.11` made `python` a cargo feature and kept `blastit-view`'s `RawFrame` framing
(`wire`/`flow`) compiled in both builds, but its items are crate-private. This node needs
them **public**: expose the framing surface (and whatever `view.chan` types `MB.3.12`'s
vtable sinks need) from `blastit-view` as part of this node, with a doctest that proves an
external crate can name them. Do not re-implement framing here — one transport.
