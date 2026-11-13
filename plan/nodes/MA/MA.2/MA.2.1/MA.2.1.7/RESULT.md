# RESULT — MA.2.1.7: Pin schema-qualified introspection replay refusal (R31)

**Status:** done

## Summary

Added two tests to `crates/blastit-sqlpolicy/tests/critical_literals.rs` (append-only; the
existing 23 tests are byte-identical). They pin finding G4 of `docs/testing/AUDIT-MA.2.1.md`:

- `only_pg_catalog_qualified_functions_are_replayed` — refuses a `FuncCall` whose second part is
  not `pg_catalog` (`SELECT public.version()`, `SELECT app.current_schema()`,
  `SELECT app.to_regtype($?)`) and one with three parts
  (`SELECT public.pg_catalog.version()`), while keeping the bare and `pg_catalog`-qualified
  positives (`version()`, `pg_catalog.version()`, `pg_catalog.current_schema()`,
  `pg_catalog.to_regtype($?)`).
- `three_part_relations_are_not_replayed` — refuses `catalogname`-qualified `RangeVar`s
  (`SELECT * FROM otherdb.pg_catalog.pg_type`, `SELECT * FROM otherdb.information_schema.tables`)
  while keeping `pg_catalog.pg_type` and `information_schema.tables` replayable.

No production code, docs, or Cargo file was touched. No `#[ignore]`, skip, `#[allow]`, or weakened
assertion.

## Evidence

### Acceptance (exact command from `NODE.md`)

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --test critical_literals --locked
running 25 tests
...
test only_pg_catalog_qualified_functions_are_replayed ... ok
test session_setup_is_replayed ... ok
test only_qualified_information_schema_tables_are_replayed ... ok
test three_part_relations_are_not_replayed ... ok
test every_dangerous_function_is_refused ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Mutant-kill proof (audit mutants M5 / M9)

Applied to a scratch copy of the crate at `/tmp/opencode/blastit-ma21-audit` (separate
`CARGO_TARGET_DIR`); the working tree's production code was never mutated. Same 25-test file
copied in as `tests/probe_ma217.rs`: green on pristine source, red under each mutant.

```console
== M5 any 2-part schema (function_name: accept any schema): exit 101
   test only_pg_catalog_qualified_functions_are_replayed ... FAILED
   test result: FAILED. 24 passed; 1 failed
== M9 drop catalogname guard (is_introspection_relation): exit 101
   test three_part_relations_are_not_replayed ... FAILED
   test result: FAILED. 24 passed; 1 failed
restored: True
```

### Surrounding gates

```console
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-sqlpolicy --all-targets --locked -- -D warnings
    Finished `dev` profile ... ; clippy exit 0

$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
     Running unittests src/lib.rs      test result: ok. 219 passed; 0 failed
     Running tests/critical_literals.rs test result: ok. 25 passed; 0 failed
   Doc-tests blastit_sqlpolicy         test result: ok. 8 passed; 0 failed

$ cargo fmt -p blastit-sqlpolicy -- --check
sqlpolicy fmt exit 0

$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.1.7
  M  crates/blastit-sqlpolicy/tests/critical_literals.rs  [authorized by ...critical_literals.rs]
```

`make check` stops in `rs-fmt` on the **parallel** node's in-flight
`crates/blastit-ingress/tests/critical_ingress.rs` (import reflow), which this node must not touch;
`critical-guard` (node-scoped) reports that same file as its only violation. Everything for
`blastit-sqlpolicy` is green.

## Files

- `crates/blastit-sqlpolicy/tests/critical_literals.rs` — +46 lines, 0 deletions (two new tests).

## Follow-ups

- Tree-wide `make check` cannot be green while the parallel ingress pin holds an unformatted
  `crates/blastit-ingress/tests/critical_ingress.rs` in the same working tree; the ingress node
  owns that fix. Re-run `make check` after both nodes land.
- Tooling note (docs are out of scope for this node): during the session both protected test files
  were observed with a reflowed `use` block that `cargo fmt --all -- --check` rejects; for this file
  it was restored with `cargo fmt -p blastit-sqlpolicy`, leaving an append-only diff. Worth a
  `docs/knowledge/LESSONS.md` line by a node that owns docs.
