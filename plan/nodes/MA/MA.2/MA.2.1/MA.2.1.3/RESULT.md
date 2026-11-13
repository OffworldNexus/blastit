# RESULT MA.2.1.3

## Summary

`blastit-sqlpolicy` is now the single authority for SQL literal handling and
`--learn` verdicts (R24, R25, R31; ADR-0003).

- **Lexer (`src/lexer.rs`).** Added `RegionKind::NationalString` (`N'…'`) and
  `RegionKind::UnicodeString` (`U&'…'`), both `''`-escaped with no backslash
  escape, alongside the existing `'…'`, `E'…'`, `B'…'`, `X'…'`, `"…"` and
  `$tag$…$tag$` regions. `U&"ident"` stays code + `QuotedIdent`, so it is never
  treated as a literal. `RegionKind::is_literal()` is the one place that decides
  what "a literal" means.
- **`src/literals.rs`.** `has_quoted_literal` and `redact_literals`: the whole
  token, prefix included, becomes `…` (`SELECT E'\n'` -> `SELECT …`,
  `SELECT U&'\0041'` -> `SELECT …`, `SELECT $t$x$t$` -> `SELECT …`), while
  quoted identifiers, `U&"ident"`, `$1`/`$?` and comments survive.
  `KNOWN_STATIC_SETUP_SQL` (`SET TIME ZONE 'UTC'`,
  `SELECT set_config('TimeZone', $?, false)`) is returned unchanged. Redaction
  is idempotent and never copies a byte from a literal.
- **`src/learn.rs`.** `check_learnable` refuses, in order: `…`
  (`RedactedPlaceholder`), exact `KNOWN_STATIC_SETUP_SQL` (allowed before the
  parser, because `$?` is not valid SQL), parse failures (`Unparseable`), more
  than one statement (`MultipleStatements`), `COPY … PROGRAM` (`CopyProgram`),
  any remaining quoted literal (`InlineLiteral`) and every
  `DANGEROUS_LEARN_FUNCTIONS` call (`DangerousFunction`). `is_introspection_setup_sql`
  accepts `KNOWN_STATIC_SETUP_SQL`, a single `SET`/`SHOW`, and a single
  read-only catalogue `SELECT` over a curated function/table allowlist
  (`information_schema` names must be qualified; only `pg_catalog` entries are
  matched bare, because only `pg_catalog` leads the search path); it rejects
  writes (`SELECT … INTO`, `FOR UPDATE`, data-modifying CTEs), other catalogues
  (`pg_authid`) and anything it does not understand.
- `src/analysis.rs` exposes `syntax`, `one_of` and `unwrap` as `pub(crate)` so
  the new module reuses the crate's one JSON walk instead of duplicating it.

## Evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --test critical_literals --locked
test session_setup_is_replayed ... ok
test only_qualified_information_schema_tables_are_replayed ... ok
test every_dangerous_function_is_refused ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
running 219 tests
test result: ok. 219 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.16s
running 23 tests
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 8 tests
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ cargo fmt --all -- --check
fmt-exit=0
```

```console
$ make check
uv run --no-project tools/guard.py
uv run --no-project tools/critical_guard.py --base HEAD
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo test -p blastit-sqlpolicy --test critical_literals
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
check-exit=0
```

## Review Resolution (attempt 2)

Attempt 1 passed every acceptance command but the goldfish review found two
MAJOR security defects and two MINOR ones. Each is fixed in the existing code
and pinned by a new protected critical test; no test was weakened.

- **MAJOR 1 — parser message leaked the offending literal.**
  `RefuseReason::Unparseable` stored libpg_query's raw message, which quotes the
  token it choked on (`syntax error at or near "s3cr3t"`), and `Display` printed
  it. `src/learn.rs` now has `sanitize_parser_message`: it drops the tail from
  ` at or near ` and replaces any surviving double-quoted span with `"…"`.
  `check_learnable` stores the sanitised message (so `Debug` is safe too) and
  `Display` sanitises again for a value built by hand. New critical test
  `no_refusal_prints_a_literal_byte` asserts neither `s3cr3t` nor `hunter2`
  appears in `to_string()` of **every** `RefuseReason` variant, nor in the
  reason/`Debug`/redacted text of end-to-end refusals. Proof the test guards the
  fix: with `sanitize_parser_message` stubbed to return its input, the test
  fails with `Unparseable("Invalid statement: unterminated quoted string at or
  near \"'s3cr3t\"") -> "the statement is not valid SQL: Invalid statement:
  unterminated quoted string at or near \"'s3cr3t\""`; restored, it passes.
- **MAJOR 2 — unqualified `information_schema` names were replayable.**
  The `""` arm of `is_introspection_relation` bare-matched every allowlist
  entry, so `SELECT * FROM tables|columns|schemata` (ordinary `public` tables)
  was admitted as introspection. The arm now strips `pg_catalog.` and matches
  only those entries; `information_schema.*` requires the qualification. New
  critical test `only_qualified_information_schema_tables_are_replayed` pins
  the three negatives plus `information_schema.pg_type`/`public.*`, and keeps
  `pg_catalog.version()`, `information_schema.tables|columns|schemata`,
  `pg_catalog.pg_type` and bare `pg_type` positive.
- **MINOR 3 — `pg_get_userbyid` replayed `pg_authid` role names.** Removed from
  `INTROSPECTION_FUNCTIONS`; the doc comment says why. New critical test
  `pg_get_userbyid_is_not_replayed`.
- **MINOR 4 — `CALL` and three-part names hid dangerous functions.**
  `find_dangerous_function` now also reads `CallStmt.funccall` (pg_query stores
  the `FuncCall` message directly there, not in a one-key `Node` wrapper), and
  `dangerous_function_name` fails closed on a three-part
  `database.schema.function` name whose last part is dangerous. New critical
  test `a_call_and_a_three_part_name_cannot_hide_a_dangerous_function` covers
  `CALL pg_read_file($1)`, `CALL pg_catalog.pg_read_file($1)`,
  `CALL pg_catalog.dblink_exec($1)` and the two `pg_catalog.pg_catalog.*` forms;
  `public.pg_read_file($1)` stays grantable (different function).

## Files

- `crates/blastit-sqlpolicy/src/lexer.rs` (modified): `N'…'`/`U&'…'` regions,
  `is_literal`, lexer tests.
- `crates/blastit-sqlpolicy/src/literals.rs` (new): `KNOWN_STATIC_SETUP_SQL`,
  `has_quoted_literal`, `redact_literals` + unit tests.
- `crates/blastit-sqlpolicy/src/learn.rs` (new; hardened in attempt 2):
  `LearnVerdict`, `RefuseReason`, `sanitize_parser_message`,
  `DANGEROUS_LEARN_FUNCTIONS`, `is_dangerous_learn_function`,
  `check_learnable`, `is_introspection_setup_sql` + unit tests.
- `crates/blastit-sqlpolicy/src/lib.rs` (modified): modules and re-exports.
- `crates/blastit-sqlpolicy/src/normalize.rs`, `src/statements.rs` (modified):
  the two new literal kinds in their region matches.
- `crates/blastit-sqlpolicy/src/analysis.rs` (modified): `syntax`, `one_of`,
  `unwrap` are `pub(crate)`.
- `crates/blastit-sqlpolicy/tests/critical_literals.rs` (new): 23 protected
  positive/negative twins, including the four review-finding regressions. No
  `#[ignore]`.

## Follow-ups

- **`docs/knowledge/LESSONS.md` (outside this node's paths).** `pg_query`'s
  serde JSON uses `""` for absent strings and `[]` for absent repeated fields,
  not `null`, so `is_some_and(|v| !v.is_null())` is the wrong absence test
  (`catalogname: ""` and `locking_clause: []` both look "present"). Also,
  `analysis::analyze` deliberately refuses every `pg_catalog` relation, so
  catalogue introspection must be walked from the parse tree instead of
  analysed. Two more for that file: `CallStmt.funccall` holds a bare `FuncCall`
  message (no one-key `Node` wrapper), so a generic `{"FuncCall": …}` walk
  misses `CALL p(...)`; and `pg_query::Error` renders as `Invalid statement:
  … at or near "<token>"`, quoting the request's own bytes, so any refusal that
  prints a parser error must strip the tail first.
- **Wiring (later nodes).** `dbproxy` should call `redact_literals` on the
  normalised text before `SqlDenied`/`WireSqlObs`, and replay only
  `is_introspection_setup_sql` matches (max 8). `blastit-manifest` should use
  `has_quoted_literal`/`KNOWN_STATIC_SETUP_SQL` for `LiteralQuery` and
  `RedactedQueryPlaceholder`.
- **`fold` input shape.** `check_learnable` expects the statement as PostgreSQL
  receives it (`$1`). A log line is already normalised (`$?`, `…`): `…` is
  refused as `RedactedPlaceholder`, and a `$?`-only statement is refused as
  `Unparseable`. If `fold` is wired to it, it must pass the wire text or accept
  that refusal (the safe direction).
