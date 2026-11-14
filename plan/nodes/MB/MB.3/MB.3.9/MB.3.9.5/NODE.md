---
id: MB.3.9.5
title: "Protected critical_fastcgi_dev_pool suite: missing binary, immediate exit, two pools, SIGTERM cleanup, grammar"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.9.4]
paths: [crates/blastit/tests/critical_fastcgi_dev_pool.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:19"
updated: "2026-11-14T13:19"
---

# Goal

Prove the five failure modes of the automatic `dev` pool with named tests, the
security-relevant ones in the protected tier.

# Context

- The owner's list, verbatim: **(1)** missing `php-fpm` binary; **(2)** pool exits
  immediately (bad config/syntax error) — the diagnostic must include the generated config
  path and the child's last stderr lines; **(3)** two routes → two pools, each its own
  process in `dev` (own uid in prod); **(4)** `SIGTERM` leaves no `php-fpm` process, no
  socket and no run dir; **(5)** the same guarantees `dev` shares with prod minus the
  privilege drop — plus the grammar itself: a manifest with `listen` is refused, and an
  absent `workers` derives a value rather than defaulting to a magic 4.
- **Runtime-discovery rows, protected (security-relevant):** `BLASTIT_PHP_FPM` pointing at
  a non-executable file is refused with the variable named and no pool spawned; a
  `PATH`-only binary is found and run; two routes resolve **one** binary (one resolved
  path in the boot document, two pools); and the resolved absolute path appears in the
  `--json` boot document (the debug-without-guessing guarantee).
- **Settlement row, protected:** a FastCGI request whose pool dies mid-flight is still
  charged at least its observed wall time (no free budget for a crashed flow) and releases
  its slot exactly once — asserted through the request event's `cost_ns`/`cpu_ns` and the
  scheduler's in-flight count, never by timing.
- Use the fake `php-fpm` helper the pool suite already has (`MB.3.6.3.2`) wherever the
  assertion is about blastit's lifecycle; the real binary case belongs to `MB.3.7`'s
  `e2e`.
- A **new** protected file is always allowed by `tools/critical_guard.py`; never edit an
  existing `critical_*.rs` to make a new behaviour pass.
- No assertion may print or compare a param value, header value or secret. The diagnostic
  assertions are the config **path** and the child's bounded stderr lines.

# Acceptance

```console
$ cargo test -p blastit --test critical_fastcgi_dev_pool --locked
$ make check
```
- One named test per mode, each asserting the failure **and** its accepted twin (a dev
  boot that does find a binary, a clean `SIGTERM`, a two-route boot keeping both pools
  alive until exit) — a test that would pass if cleanup never ran is fluff.
- Missing-binary: immediate exit, message names the package to install, not a stack trace,
  not a first-request failure.
- Immediate-exit: the generated config path appears in the diagnostic, with the child's
  last stderr lines and nothing value-bearing.
- `SIGTERM`: after exit, no descendant `php-fpm` (via `/proc`), socket `ENOENT`, run dir
  `ENOENT`.
- Two routes: two distinct derived socket paths, two processes, no shared pool.

# Notes

- Cleanup must be observable without root: the test creates and owns a temp run root and
  asserts absence with `symlink_metadata`/`ENOENT`, not a shellout to `pgrep`.
- A mode provable only under Docker/root is handed to `MB.3.7` in `## Follow-ups` rather
  than weakened here.
