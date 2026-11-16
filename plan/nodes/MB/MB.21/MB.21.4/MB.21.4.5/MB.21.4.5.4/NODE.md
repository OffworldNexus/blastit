---
id: MB.21.4.5.4
title: "Build the --learn routing table from the model and re-point the boot helpers and env/sqlpolicy examples at Blastfile"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-manifest/src/routing.rs"
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/infer_routes.rs
  - crates/blastit/src/php_view.rs
  - crates/blastit/tests/critical_check_engine.rs
  - "crates/blastit-env/src/env.rs"
  - "crates/blastit-env/src/lib.rs"
  - "crates/blastit-env/src/postgresql.rs"
  - "crates/blastit-env/tests/environment.rs"
  - "crates/blastit-sqlpolicy/src/learn.rs"
  - "crates/blastit-sqlpolicy/src/tables.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T16:58"
updated: "2026-11-16T18:17"
---

# Goal

`blastit dev --learn` gets its routing table from the model it already holds — no synthesised document, no
re-parse — and the last Rust doc examples and unit fixtures outside the manifest crate stop building
manifests from retired text.

A developer reading `crates/blastit-env`'s rustdoc sees `postgres $DATABASE_URL` and a view, not a format
that no longer exists; `cargo test` in that crate proves the same wiring it proved before.

# Context

- Design §15.2's row: the text synthesis in `stage1::learn_routing_table` "becomes a model-side
  `RoutingTable::for_learn(&manifest)`, no text". Today it formats a document, escapes every string by
  hand, and calls the reader; slot `0` is the unsatisfiable `"a^"` and declared routes shift to `1..=N`.
- `RoutingTable::for_learn` lives in `crates/blastit-manifest/src/routing.rs` beside `new`, over the same
  `CompiledRoute` vector. Its `fallback` entry comes first. The signature is the author's choice; the
  invariants are not: slot `0` never matches, `route()` indices shift by exactly one, methods and
  patterns are the parsed view's, and an uncompilable pattern fails with the same
  `ManifestError::InvalidRegex` `new` produces.
- Do not change behaviour: `dev_learn` and `tests/critical_learn_executor_blocks.rs` assert the learned
  document is **spliced**, not regenerated. Keep `dev_learn::LEARN_FALLBACK_ROUTE` as slot 0's name in
  events. Delete `toml_basic_string` with the synthesis.
- Measured remainder: `src/stage1.rs` (1 reader call + 2 fixtures), `src/infer_routes.rs` (1),
  `src/php_view.rs` (`PAGE`, one WSGI page, one `parse_and_validate`), `tests/critical_check_engine.rs`
  (1, protected), `blastit-env/src/{env,postgresql,lib}` (6), `blastit-env/tests/environment.rs` (1),
  `blastit-sqlpolicy/src/learn.rs` (1 sentence), `blastit-sqlpolicy/src/tables.rs` (a module fence showing
  the retired grant table).
- `blastit-env`'s examples must read as the user's language: `postgres $DATABASE_URL`,
  `postgres analytics $ANALYTICS_URL`, and the grant block from `docs/reference/blastfile.md`. Keep every
  wiring assertion (in-place rewrite, `ca_cert` disappearing, the split output variables, the
  environment-variable refusal).
- `Proxy::output` is not expressible in a `Blastfile` (§7.5 drops the rename map) but `blastit-env` still
  renders it. Where a test needs that shape, build the model through `Proxy::new` and say in a comment why
  — never resurrect a document. Deleting `Proxy::output` and `TableGrants::to_toml` (whose only caller is
  `crates/blastit-py`, outside this epic's `paths`) is a follow-up, not this leaf.

# Acceptance

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit/src crates/blastit-env crates/blastit-sqlpolicy crates/blastit/tests/critical_check_engine.rs
$ grep -rnE 'blastit[.]toml' crates/blastit/src crates/blastit-env crates/blastit-sqlpolicy crates/blastit/tests/critical_check_engine.rs
$ cargo test -p blastit --lib
$ cargo test -p blastit --test critical_check_engine
$ cargo test -p blastit-env --all-targets --doc
$ cargo test -p blastit-sqlpolicy --all-targets
$ make rs-doc
$ make critical-guard BASE=<sha before this node> NODE=MB.21.4.5.4
```

1. Both greps print nothing over those paths.
2. `RoutingTable::for_learn` exists, `learn_routing_table` contains no string formatting of a document
   (paste the function), and `stage1`'s unit test still asserts slot `0`, the shifted indices, the method
   filter and the non-ASCII pattern.
3. Every command above exits `0`; `make rs-doc` is clean, so the env examples and the sqlpolicy fence are
   real `Blastfile`; `critical-guard` is clean.
4. `RESULT.md` lists the two follow-ups — `Proxy::output` and `TableGrants::to_toml` + its Python caller —
   under `## Follow-ups`, with the file each lives in.

# Notes

- Zero new dependencies: `for_learn` builds `CompiledRoute`s directly, it does not add a second table type.
- No public API beyond `for_learn`; `RoutingTable::new` keeps its signature and its error type.
- `crates/blastit-manifest/src/routing.rs`'s own doctests read the retired format too — they are in your
  `paths` because this leaf is the one that owns that file until `MB.21.4.5.6`.
