---
id: MA.2.3.12
title: Harden free_port and load isolation in the protected dev suite
kind: task
status: ready
owner: implementer
priority: 3
depends_on: []
paths: [crates/blastit/tests/critical_dev_and_fail_closed.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T21:52"
updated: "2026-11-12T21:52"
---

# Goal

Make `crates/blastit/tests/critical_dev_and_fail_closed.rs` deterministic under load: the `free_port()` bind-then-drop pattern races other processes (EADDRINUSE, and one make-check failure in `json_mode_emits_privilege_and_never_a_port_key` while three agents ran concurrently), so `make check` is intermittently red for a reason unrelated to the code under test.

# Context

- Found at the MA.2.3 epic gate (audit + MA.2.3.8 review finding 2): `free_port()` binds, drops, then reuses the port; a concurrent bind wins the race and the failing leg reports an unrelated EADDRINUSE.
- The MA acceptance requires `make check` to pass green; a load-dependent flake is a release risk.
- This file is a **protected critical test**; this node's `paths` explicitly cover it and the only permitted changes are isolation/retry hardening plus sharper failure messages — never a relaxed assertion, skip, `#[ignore]`, or longer fixed sleep.
- Skills `rust-standards`, `testing`.

# Acceptance

```console
$ for i in $(seq 20); do cargo test -p blastit --test critical_dev_and_fail_closed --locked; done
# 20/20 green, and green with 8 concurrent copies of the suite
$ make check
```

- `free_port()` either retries on EADDRINUSE with a fresh port (bounded, e.g. 3 tries) or leases the socket to the child; the failing leg names the import/bind it meant to test instead of surfacing a bare EADDRINUSE.
- No assertion text, timeout, or wait loop is weakened; prove by diffing assertion lines against the base commit.
- Demonstrate the improvement honestly: report the EADDRINUSE/`privilege_and_never_a_port_key` rate before and after on this host under load (the pre-fix rate you measured, and 0/20 + concurrent-load runs after).

# Notes

- Only `crates/blastit/tests/critical_dev_and_fail_closed.rs` (plus, if genuinely required, a small helper module under `crates/blastit/tests/`).
- Do not change production code; if a genuine product bug surfaces, record it under Follow-ups.
