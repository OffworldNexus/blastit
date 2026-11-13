---
id: MA.2.1.7
title: "Pin schema-qualified introspection replay refusal (R31) in critical_literals"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-sqlpolicy/tests/critical_literals.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T13:53"
updated: "2026-11-12T13:58"
---

# Goal

Pin that `is_introspection_setup_sql` refuses a cross-schema function call and a three-part
relation name, in `crates/blastit-sqlpolicy/tests/critical_literals.rs` (R31).

# Context

Finding G4 of `docs/testing/AUDIT-MA.2.1.md`. The predicate decides what `dbproxy` replays once
under a routed target, i.e. what runs *without* a grant. Two checks are untested:

- `function_name` accepts a bare name or `<schema>.<name>` only when the schema is `pg_catalog`;
  a two-part mutation that accepts any schema makes `SELECT public.version()` replayable, and that
  route-defined function then executes without a grant.
- `is_introspection_relation` refuses any non-empty `catalogname`; removing that guard makes
  `SELECT * FROM otherdb.pg_catalog.pg_type` replayable.

Both mutants survive the current 23-test suite (verified by hand mutation); the candidate tests
below fail under the mutant and pass on the current tip.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --test critical_literals --locked
```

- `!is_introspection_setup_sql("SELECT public.version()")`
- `!is_introspection_setup_sql("SELECT * FROM otherdb.pg_catalog.pg_type")`
- the positives still hold: `SELECT pg_catalog.version()`, `SELECT * FROM pg_catalog.pg_type`,
  `SELECT * FROM information_schema.tables`.
- the existing 23 tests stay unchanged and green; no `#[ignore]`.

# Notes

- Protected file: add tests only, covered by this node's `paths`.
- If the implementer judges a three-part relation unreachable for this predicate, still pin the
  two negatives: they are the crate's public contract and cost two lines.
