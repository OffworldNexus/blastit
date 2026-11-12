# RESULT MA.2.3.10 — Pin three unprotected `audit_v1` rules

- **Status:** done
- **Attempt:** 1/3
- **Scope touched:** `crates/blastit/tests/critical_check_engine.rs` (additive only: +134 lines, 0 deletions)
- **Commit:** none (per instructions)

## What landed

Three new protected tests, each pairing an allowed case with its deny twin:

| Audit | Test | Oracle |
|---|---|---|
| F6 (mutant #2) | `a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query` | `verdict.kind == "query"` + `normalized` containing `…` → `missing_grant`, `fix.action == "observe"`, `fix.to == None`; the parameterised `DELETE` twin still gets `add_query` with `to == "[route.db].queries"` |
| F7 (mutant #6) | `requests_without_any_statement_are_no_observations` | one proxy, one routed request, zero statements → exactly one `no_observations` (`kinds()` count == 1); twin with one routed statement has no `no_observations` |
| F8 (mutant #9) | `zero_requests_never_write_even_with_sql_to_learn` | `plan_learn` with `requests == 0` and an unrouted `SELECT 1` still proposes 1 route + 1 query, but `would_write(false) == false` and `json(false)["wrote"] == false`; twin with one HTTP line writes |

No existing assertion was deleted, renamed, weakened or skipped. No `#[ignore]` added.

## Acceptance

### `cargo test -p blastit --test critical_check_engine --locked`

```console
running 16 tests
test a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query ... ok
test requests_without_any_statement_are_no_observations ... ok
test zero_requests_never_write_even_with_sql_to_learn ... ok
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### `cargo test -p blastit --lib --locked`

```console
test result: ok. 167 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.29s
```

(The one ignored unit test is pre-existing, not introduced here.)

### `make test-critical`

```console
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s   # critical_dev_and_fail_closed
... all other crates/*/tests/critical_*.rs suites ok ...
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 3.761s
OK
```

### `make check`

```console
guard: OK (4 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 2 changed file(s)   # report-only under make check
... cargo fmt --all -- --check ... (silent: clean)
... cargo clippy --workspace --all-targets --locked -- -D warnings ... (silent: clean)
... RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked ... (built)
... cargo test --workspace --all-targets --locked ... all ok ...
python -m unittest discover -s python/tests/critical
Ran 50 tests in 3.112s
OK
uvx --offline ruff check python tools        → All checks passed!
uvx --offline ruff format --check python tools → 43 files already formatted
uvx --offline zensical build --strict        → No issues found
uv run --no-project tools/plan.py validate   → plan OK
```

Node-scoped guard (the enforced form a reviewer runs):

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.3.10
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node MA.2.3.10
```

## Mutation proof (scratch copy, never the working tree)

Tree copied with `rsync -a --exclude target --exclude .git --exclude site` to
`/tmp/opencode/mut-ma2310`; each mutation applied there with `sed`, the single new
test run, then the source restored from a saved copy (`trap ... EXIT`). The working
tree `audit_v1.rs` was never edited — its SHA-256 equals the backup:

```console
$ sha256sum crates/blastit/src/audit_v1.rs /tmp/opencode/audit_v1.orig
f1c408500cb88cfa5596b29a4b9cc9ad6df62e475a50f9735cdd1bd576fbb292  crates/blastit/src/audit_v1.rs
f1c408500cb88cfa5596b29a4b9cc9ad6df62e475a50f9735cdd1bd576fbb292  /tmp/opencode/audit_v1.orig
```

| Mutant | Mutation | Result |
|---|---|---|
| baseline | none | `16 passed; 0 failed` |
| #2 | dropped `&& !normalized.contains('\u{2026}')` from the `add_query` branch | `FAILED. 0 passed; 1 failed` — `a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query` |
| #6 | `if exercised == 0 \|\| (!proxies.is_empty() && statements == 0)` → `if exercised == 0` | `FAILED. 0 passed; 1 failed` — `requests_without_any_statement_are_no_observations` |
| #9 | dropped `&& self.requests > 0` from `LearnPlan::would_write` | `FAILED. 0 passed; 1 failed` — `zero_requests_never_write_even_with_sql_to_learn` |

Raw tails:

```console
=== mutant 2: dropped the ellipsis veto on add_query
failures:
    a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out
=== exit 101

=== mutant 6: no_observations dropped the zero-statement clause
failures:
    requests_without_any_statement_are_no_observations
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out
=== exit 101

=== mutant 9: would_write dropped requests > 0
failures:
    zero_requests_never_write_even_with_sql_to_learn
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out
=== exit 101
```

## Note on formatting

The working copy of `critical_check_engine.rs` carried pre-existing 2015-style import
ordering (plain `rustfmt` default) while the crate is `edition = "2024"`, so
`cargo fmt --check` was red before the tests were added. Normalising with
`rustfmt --edition 2024 --style-edition 2024 <file>` restored the committed 2024 order;
the final diff is **+134 / −0** (purely additive), and `make rs-fmt` is clean.

## Follow-ups (out of scope)

- `docs/knowledge/LESSONS.md` (not in this node's `paths`): a plain `rustfmt` run uses
  `style_edition = 2015` and reorders imports differently from `cargo fmt` on an
  `edition = "2024"` crate; always format with the crate edition or via `cargo fmt`.
- F9/F10/F11 from `AUDIT-MA.2.3.md` remain unfiled (inference extras, CLI `=` spelling,
  singular/alias rendering) — unchanged by this node.
