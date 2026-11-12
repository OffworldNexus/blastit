---
id: MA.3.5
title: "Audit runnable-docs guard, critical tiers and scoped mutants"
kind: task
status: ready
owner: "test-curator"
priority: 3
depends_on: [MA.3.2, MA.3.4]
paths: ["docs/testing/AUDIT-MA.3.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-12T19:16"
---

# Goal

Audit the `MA.3` verification tier: prove the runnable-docs harness cannot silently hide a snippet, and that the `MA.2` policy crates still carry honest critical and mutation coverage, recording every gap as a follow-up.

# Context

- Epic gate requires a `test-curator` audit; the milestone gate requires `cargo-mutants` on `blastit-manifest` and `blastit-sqlpolicy`.
- Harness under audit: `python/tests/runnable_docs/**` plus `python/tests/runnable_docs/inventory.toml`; critical tiers: `crates/*/tests/critical_*.rs`, `python/tests/critical/`.
- Load skill `testing`.

# Acceptance

```console
$ make docs-test && make test-critical
$ make mutants-rust PKG=blastit-manifest
$ make mutants-rust PKG=blastit-sqlpolicy
```

- `docs/testing/AUDIT-MA.3.md` states per tier what runs, what is protected by `tools/critical_guard.py`, which mutants survived, and a verdict for each (`killed by <test>` / equivalent / documented gap).
- Adversarial harness checks, each with command + result in the report: (a) adding an unaccounted `console` fence makes `make docs-test` red; (b) deleting an inventory entry makes it red; (c) a fence whose expected output is wrong fails; (d) `make docs-test` works with `HOME`/`PATH` scrubbed.
- Every remaining gap is listed with a proposed node title and owner; the report ends `VERDICT: PASS` only when no unprotected critical gap is left.

# Notes

- Writes only `docs/testing/AUDIT-MA.3.md`. New tests are follow-up nodes, not this task.
- The two mutation runs are long (>10 min each) and Rust-only; they need the `Makefile` `PYO3_*` environment, so run them through `make`.
- Do not re-run Docker suites here; `MA.3.4` owns them.
