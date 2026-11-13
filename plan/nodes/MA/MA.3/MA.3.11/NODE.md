---
id: MA.3.11
title: Protected critical twins for the sqlpolicy learn and table policy
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-sqlpolicy/tests/critical_learn_policy.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T01:19"
updated: "2026-11-13T02:34"
---

# Goal

Put the `blastit-sqlpolicy` learn/table policy oracle into the protected tier, so deleting a policy
check cannot also delete its only test (`F8` and the SP8 survivor in `docs/testing/AUDIT-MA.3.md`).

# Context

- Mutation audit at `dba348c`: 11 anchored mutants across `check_learnable`, `is_introspection_setup_sql`,
  `TableGrant::allows`, `canonical_table` and `redact_literals`; 10 killed, 1 survivor, and **all
  ten kills came from `#[cfg(test)]` unit tests inside `crates/blastit-sqlpolicy/src/`**.
  `tests/critical_literals.rs` killed none of them, so today the only oracle for "`COPY … PROGRAM`
  is refused", "multi-statement text is refused", "`CALL p(...)` cannot hide a dangerous function"
  and "a column list does not allow a whole-row access" is unprotected source code.
- SP8: `introspection_node`'s `MAX_INTROSPECTION_DEPTH` bound is untested. Removing it makes an
  18-deep nested catalogue `SELECT` replay as session setup (`replayed=false` → `true` on pristine
  vs mutated builds); the parser refuses nesting beyond ~22, so the reachable window is 17..~22.
- `tools/critical_guard.py` forbids editing existing protected files but always allows a **new**
  `crates/*/tests/critical_*.rs`, which is why this node adds a file instead of editing
  `critical_literals.rs`.

# Acceptance

```console
$ cargo test -p blastit-sqlpolicy --test critical_learn_policy --locked
$ make test-critical
$ make critical-guard BASE=dba348c
```

- New protected file `crates/blastit-sqlpolicy/tests/critical_learn_policy.rs` with allow **and**
  deny twins for: `COPY … PROGRAM` (both `FROM PROGRAM` and `TO PROGRAM`) refused while
  `COPY t TO STDOUT` stays grantable; multi-statement text refused with the statement count;
  inline literal refused at learn time while the redacted shape is what a grant carries;
  `CALL pg_catalog.pg_read_file(…)` and a three-part `pg_catalog.pg_catalog.pg_read_file(…)`
  refused while `public.pg_read_file(…)` and `shop.pg_read_file(…)` stay grantable;
  `is_introspection_setup_sql("SET x; DROP TABLE t") == false`; `SELECT * FROM auth_user` is not
  replayed while `SELECT pg_catalog.version()` and `SELECT to_regtype($1)` are; an 18-deep nested
  catalogue `SELECT` is **not** replayed (kills SP8); `Extent::Whole` needs `"*"` and is not
  satisfied by a column list; `public.t` and `t` are one table.
- Each test carries a one-line comment naming the requirement (`R24`, `R25`, `R31`, §7.2).
- Re-running the anchored mutants from `AUDIT-MA.3.md` (or `make mutants-rust PKG=blastit-sqlpolicy`
  once `cargo-mutants` is installed per `H-002`) shows ≥ 1 protected-suite kill per mutant group.
- No existing protected file is modified; `make critical-skip-check` stays green.

# Notes

- New protected file only; do not edit `crates/blastit-sqlpolicy/src/**` or
  `tests/critical_literals.rs`.
- `pg_query` is already a normal dependency of the crate, so the tests need no new dependency.
- If a test needs a deeply nested statement, build it with `format!` in a loop (as the audit probe
  did) rather than pasting a 200-line literal.
