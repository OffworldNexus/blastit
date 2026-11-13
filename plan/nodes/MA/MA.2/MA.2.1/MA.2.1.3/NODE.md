---
id: MA.2.1.3
title: "Leaf 3: sqlpolicy literal redaction, learnable verdicts and introspection setup"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MA.2.1.1]
paths: ["crates/blastit-sqlpolicy/src/**", "crates/blastit-sqlpolicy/tests/**"]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:48"
---

# Goal

Make `blastit-sqlpolicy` the single authority for SQL literal handling in `0.1.1`: redact all eight PostgreSQL literal forms to `…`, expose `has_quoted_literal`, and implement `check_learnable` plus `is_introspection_setup_sql` so `--learn`, `fold`, `check` and `dbproxy` session replay all share one verdict.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 C, §7.2 row `R24..R25` (sql), §8 Leaf 3; `ADR-0003`; requirements `R24`, `R25`, `R31` (introspection predicate only).
- Extend `src/lexer.rs` to tokenize `N'…'`, `U&'…'` and `$tag$…$tag$`; `redact_literals` replaces the **entire** literal token, prefix included (`SELECT E'\n'` -> `SELECT …`), while `"quoted_identifiers"`, `U&"ident"`, `$?` and `KNOWN_STATIC_SETUP_SQL` exact matches survive.
- `check_learnable` refuses unparseable SQL (`RefuseReason::Unparseable`), multi-statement strings, inline literals/`…`, `COPY … PROGRAM`, and every `DANGEROUS_LEARN_FUNCTIONS` entry.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-sqlpolicy --test critical_literals --locked
$ cargo test -p blastit-sqlpolicy --locked
$ make check
```

- `crates/blastit-sqlpolicy/tests/critical_literals.rs` is **new** and pins positive + negative twins: identifiers/`U&"ident"`/`$1`->`$?`/`KNOWN_STATIC_SETUP_SQL`/`pg_catalog.version()`/`SHOW` stay allowed; `SELECT E'\n'`, `SELECT B'1'`, `SELECT U&'\0041'`, `SELECT $t$x$t$` all redact to exactly `SELECT …`; unparseable, multi-statement, `COPY … PROGRAM` and every dangerous function are refused.
- `redact_literals` is idempotent and never emits a user-derived literal byte into the returned string.

# Notes

- ~300 changed lines; `crates/blastit-sqlpolicy/**` only (parallel with `MA.2.1.2`).
- No new dependency: the workspace already vendors `pg_query`; do not edit `Cargo.toml`.
- `cargo-mutants` on this crate is the `MA` gate (`make mutants-rust PKG=blastit-sqlpolicy`), not part of this leaf.
