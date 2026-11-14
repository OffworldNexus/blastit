---
id: MB.9.4
title: Run one shared conformance table against every implementation and gate make check
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.3]
paths: ["crates/blastit-backend/tests/conformance_*.rs", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:54"
updated: "2026-11-14T01:54"
---

# Goal

Turn the shared conformance table into the drift barrier: one behavioural suite,
driven from the contract's case table, run against **every** implementation, wired
into `make check`. A new backend that misses a row fails the gate; a FastCGI that
behaves differently from static/WSGI cannot merge. Scheduler tests use a fake
`Upstream` double instead of any real pool.

# Context

- Depends on `MB.9.3`: both implementations exist and each has a driver
  (`crates/blastit-scheduler/tests/conformance_static.rs`,
  `conformance_wsgi.rs`). This node owns the **table-side** harness and the gate, not
  the drivers.
- `crates/blastit-backend/tests/conformance_table.rs` (and siblings matching
  `conformance_*.rs`):
  - assert each case id is unique, every class the design names
    (`refusal`, `body_cap`, `observation`, `error_mapping`) has ≥ 1 case, and every
    expected status is a client-visible status the transport can send;
  - implement a **fake `Upstream`** double in the test that satisfies the runner
    contract without I/O, and prove the runner drives any `&dyn Upstream` by running
    the table against the fake (this is the test-double requirement: no socket, no
    pool, no subprocess, no sleeping);
  - keep the table's vocabulary language-agnostic: no `Php*`/`Python*`/`Static*`
    case names or observation kinds.
- **Anti-drift property to pin:** every driver iterates
  `blastit_backend::conformance::cases()` rather than listing cases by hand, so a
  case added in `MB.3.6` for FastCGI or later for Node executes against static and
  WSGI with no new test file. State that property in the test's doc comment and
  enforce it with the greps below.
- `Makefile`: add a `rs-test-conformance` target (runs the backend table test plus
  both scheduler drivers, offline, `--locked`) and add it to the `check` dependency
  line so the suite gates every change. Keep `make check` unprivileged and offline.
- FastCGI's row (`crates/blastit-fastcgi/tests/conformance.rs`) is **not** here: it
  lands with `MB.3.6`, which adds the fourth driver against this same table.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-backend --locked
$ make rs-test-conformance
$ test "$(grep -c '^rs-test-conformance:' Makefile)" -eq 1 && sed -n '/^check:/p' Makefile | grep -q 'rs-test-conformance' && echo "conformance gates make check"
$ grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_static.rs
$ grep -q 'conformance::cases()' crates/blastit-scheduler/tests/conformance_wsgi.rs
$ test -f crates/blastit-backend/tests/conformance_table.rs && echo "table harness exists"
$ grep -q 'pub fn cases' crates/blastit-backend/src/conformance.rs && echo "one accessor every driver iterates"
$ make critical-guard BASE=develop NODE=MB.9.4
$ make check
```

# Notes

- Do not move or rename the drivers: `MB.3.6` adds a sibling, it does not restructure
  this gate.
- The fake double is a test fixture, not a shipped "null backend": it must not be
  reachable from `crates/blastit-backend/src` and must never be registered in a
  boot path.
- Keep the target fast (< 2 min). The drivers already avoid Docker and real pools; if
  one starts a process, that is a defect in the driver, not a reason to slow the
  gate.
- `make check` is the fast unprivileged gate: no `sudo`, no network, no containers.
