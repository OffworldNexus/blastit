---
id: MB.9.4
title: Run one shared conformance table against every implementation and gate make check
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.3]
paths: ["crates/blastit-backend/tests/conformance_table.rs", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T10:51"
---

# Goal

Turn the shared conformance table into the drift barrier: one behavioural suite driven
from the contract's case table, run against every implementation, wired into `make check`.
A new backend that misses a row fails the gate; a PHP implementation that behaves
differently from static/WSGI cannot merge (design leaf `MB.9.4`).

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract (the table, the drivers, the
  anti-drift rule), with the in-module contract test plus `tests/conformance_table.rs`
  restated here; the retired design is deleted with `ADR-0010`. Scope: **exact**
  `IMPLEMENTATIONS` assertion, per-class `Expect` completeness, not a generic "some cases
  exist" check.
- Files (the whole leaf, 2): `crates/blastit-backend/tests/conformance_table.rs` and
  `Makefile`.
- What lands:
  - `conformance_table.rs`:
    - `assert_eq!(IMPLEMENTATIONS, …)` against the exact slice (`static`, `wsgi`, `php`,
      `fake`, with kind + capability bits), so dropping any capability bit fails the test;
    - per-class `Expect` completeness invariants (every class the design names has ≥ 1
      case, every expected status is client-visible, ids unique);
    - a fake `Upstream` double in the test that satisfies the runner contract without I/O,
      proving the runner drives any `&dyn Upstream` (no socket, no pool, no subprocess, no
      sleeping); two `kind()`-only fakes must be identical;
    - `decline_identity`, plan-rebuild equality, and a missing builder with its manifest
      line.
  - `Makefile`: a `rs-test-conformance` target running the backend table test plus both
    scheduler drivers, offline and `--locked` (`3` binaries; the retired `MB.3.6.1.2`/
    `MB.3.6.2.2` bumps went with the external PHP path, `MB.3.17`), added to the `check`
    dependency line so the suite gates every change.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ make rs-test-conformance
$ ! grep -rn 'case\.id' crates/*/tests/conformance*.rs && echo "table-driven, not hand-listed"
$ test "$(grep -c '^rs-test-conformance:' Makefile)" -eq 1 && sed -n '/^check:/p' Makefile | grep -q 'rs-test-conformance' && echo "conformance gates make check"
$ grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_static.rs && grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_wsgi.rs
$ cargo test -p blastit-backend --locked
$ make check
```

# Notes

- Do not move or rename the drivers: `MB.3.6.1.2`/`MB.3.6.2.2` add siblings, they do not
  restructure this gate.
- The fake double is a test fixture, not a shipped "null backend": it must not be reachable
  from `crates/blastit-backend/src` and must never be registered in a boot path.
- Keep the target fast (< 2 min) and unprivileged: no `sudo`, no network, no containers.
