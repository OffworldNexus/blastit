---
id: MB.36.11
title: "Guard the artifacts: the wheel's DT_NEEDED set (R12) and the deleted transport trait names (R3)"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit/tests/**", "scripts/**", "docs/design/view-crates-architecture.md"]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-18T18:05"
updated: "2026-11-18T19:54"
---

# Goal

Add the two artifact facts the epic promised but no test asserts:

1. **R12** — a test that reads the built wheel's `blastit/_blastit*.so` and
   asserts its `DT_NEEDED` set has no `libpython` and no `libphp` (and no
   `RUNPATH`), the same way `scripts/check-portable-elf.sh` does it at build
   time. Today only the script checks it, once, during `scripts/build-wheel.sh`.
2. **R3** — a test that the deleted transport trait is gone: a source grep for
   `WorkerRuntime`, `RequestIo`, `RequestEnd` and `flush_request` is empty, and
   `OutputStream` is in `blastit-view/src/stream.rs`. A deleted trait has no
   compile-time witness, so the guard is the regression test.

# Context

`docs/testing/AUDIT-MB.36.md` §5 lists R3 and R12 as the only two accepted
requirements with no automated oracle. R2/R11/R13 are accepted as `cargo tree` /
ELF build facts that a Rust test would only restate; R12 is different because a
*shipped* file can regress long after the build script ran (a new dependency, a
re-enabled feature).

# Acceptance

```console
$ cargo test -p blastit --test critical_artifacts --locked
$ make check
```
- the wheel test skips (with one printed line and a non-zero exit only when
  `BLASTIT_REQUIRE_WHEEL=1`) when `dist/` holds no wheel, so `make check` stays
  offline and Docker-free;
- both directions: a wheel built with `python` off is refused by the test's own
  fixture check, and the trait grep's negative twin (a synthetic file naming one
  of the four names) is detected.

# Notes

The wheel path is `dist/blastit-<version>-*.whl`; `python/tests/runnable_docs/harness.py`'s
`wheel_path()` is the existing reader of that rule.
