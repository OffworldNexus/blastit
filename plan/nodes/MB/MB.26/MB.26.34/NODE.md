---
id: MB.26.34
title: "build.rs PHP toolchain probe: do not cache a no-toolchain world on a transient spawn failure"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-php-sapi/build.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

`crates/blastit-php-sapi/build.rs`'s `probe()` has the same weakness `MB.3.29`
just fixed in `toolchain.rs`: `Command::new(exe).arg("--version").output().ok()?`
turns a transient fork/exec failure (`EAGAIN`/`ETXTBSY`) into "not a toolchain",
so `find_toolchain()` can print "no PHP development toolchain found" and cache a
`cfg(php_toolchain)`-less build world on a host that has one. The only trace is a
cargo warning; `the_build_reports_one_consistent_world` passes because the
degraded world is self-consistent.

# Context

- Found by the `MB.3.29` reviewer; a CI/dev-determinism risk (a silently smaller
  test surface on a PHP host), not a shipped-runtime or security hole: the C
  shim is built by `make php-sapi`, and a missing ABI surface fails closed.
- The `MB.3.29` fix is the template: classify `ENOENT`/`EACCES`/`ENOEXEC`/
  `ELOOP` as "not this candidate", retry only `EAGAIN`/`ETXTBSY`/`EINTR` with a
  real short sleep, and surface any other failure as a warning that names the
  OS reason instead of silently degrading.

# Acceptance

```console
$ cargo clean -p blastit-php-sapi && cargo build -p blastit-php-sapi --locked
$ make check
```
- A transient spawn failure inside `probe()` no longer reads as "no toolchain":
  bounded retry for transient errnos; any unexplained failure emits a
  `cargo:warning` naming the executable and the OS error (never a silent
  degrade).
- A genuine text-file `php-config` on PATH still degrades cleanly (the
  no-toolchain warning), and `PHP_CONFIG` precedence is unchanged.
- Deterministic evidence: a test that holds the fake `php-config` open while
  invoking the probe path (or a small extracted helper exercised by a test),
  showing `ETXTBSY` is retried and the eventual warning is not the generic "no
  toolchain" one.
- `make check` green; no other change; the crate stays zero-dependency.

# Provenance

Re-filed from `MB.3.34` at the `v0.2.0` gate: build-script transient `EAGAIN`/`ETXTBSY` retry in `crates/blastit-php-sapi/build.rs` is CI/dev build determinism hardening, not a shipped-runtime defect.
