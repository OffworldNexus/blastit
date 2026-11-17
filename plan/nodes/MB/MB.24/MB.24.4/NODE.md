---
id: MB.24.4
title: "Wire per_process_cap = 1 for entrypoint php and prove parallel routes end-to-end (protected critical_parallel_views)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.24.3]
paths:
  - crates/blastit/src/stage1.rs
  - crates/blastit/tests/critical_parallel_views.rs
  - "crates/blastit/tests/common/**"
  - crates/blastit/tests/critical_php_backend.rs
  - crates/blastit/tests/critical_php_embed.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T01:27"
updated: "2026-11-17T01:27"
---

# Goal

Wire `entrypoint php` routes to `per_process_cap = 1` so the pool `MB.24.3` built actually
forks a PHP worker per concurrent request, and prove it with one protected wall-time suite
that measures the developer-visible promise: **4 concurrent 200 ms requests to ONE PHP route
finish in under 450 ms** (serialized would be at least 800 ms), the same 4 to ONE Python
route also finish in under 450 ms, and `BLASTIT_MAX_WORKERS` still strictly caps total
concurrency.

# Context

- Route table construction is `crates/blastit/src/stage1.rs:306-330` (`RouteInfo::new(...)`
  for dev and `routes_from_manifest` in production); the per-route cap field and the
  pool are `MB.24.3`'s (`crates/blastit-scheduler/src/route.rs:15-28`).
- The PHP decision is the manifest's: `manifest.php().is_some()` (`entrypoint php` is one
  front controller for every view, `crates/blastit-manifest/src/model.rs:845-900`). A
  Python view keeps `per_process_cap = view_worker_cap()`.
- PHP's own runtime already reports `max_inflight() == 1`
  (`crates/blastit-view/src/php.rs:10-17`); nothing in the worker changes here.
- Test harness to reuse: `crates/blastit/tests/common/` (dev boot + request helpers),
  `crates/blastit/tests/critical_php_embed.rs`'s `libphp_path()`/`shim_path()`/stub
  machinery, and `BLASTIT_LIBPHP`. The PHP leg is a hard failure where the shim and library
  exist (`.github/workflows/php-sapi.yml` runs the real pair) and the Python leg runs on
  every `make check`.
- Several concurrent slow requests need a PHP page and a WSGI app that both `sleep 200 ms`
  then answer `200`; keep each fixture under 20 lines and give every request a distinct
  marker so a swapped response is detectable.
- `paths` includes a **new** protected file, which is always allowed; no existing critical
  test is edited.

# Acceptance

```console
$ cargo test -p blastit --test critical_parallel_views
$ BLASTIT_LIBPHP=/usr/lib/libphp8.5.so cargo test -p blastit --test critical_parallel_views
$ cargo test -p blastit --test critical_php_backend --test critical_php_embed
$ make check
$ make e2e                      # epic gate: process topology + rlimit
```

`critical_parallel_views` must assert, not print:

1. **PHP parallel** — 4 concurrent 200 ms requests to one `entrypoint php` route: all four
   are `200`, each response carries its own request's marker, and wall time is
   `< 450 ms` (fail with the measured number if not; add a serialized control leg that
   measures `>= 800 ms` when only one worker is allowed, so the test cannot pass on a
   fast machine by accident).
2. **Python parallel, unchanged** — the same 4 requests to one WSGI route: `200`, `< 450 ms`.
3. **The cap holds** — 8 concurrent requests with `BLASTIT_MAX_WORKERS=4`: never more than
   4 in flight (observed through `process.spawned`/`request` events or a scheduler
   counter), all 8 answered `200`, and the total is two waves, not eight parallel ones.
4. **Pool shrinks** — after `idle_timeout` the extra PHP workers retire
   (`process.retired` reason `idle`) and the prefork count remains.

# Notes

- No `#[ignore]`, no skip: where the library or shim is absent the test fails with the
  message `critical_php_embed` already uses for a missing toolchain.
- Do not raise `RLIMIT_NPROC`; if the fourth worker cannot be forked the test shows the
  errno rather than retrying.
- Never name a second PHP transport; the only PHP here is the one `entrypoint php`.
