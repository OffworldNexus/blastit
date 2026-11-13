# Result MA.3.11 — Protected critical twins for the sqlpolicy learn and table policy

**Status: done.** One new protected file, ten tests, ten allow/deny pairs. It kills the
SP8 survivor and the rest of the `blastit-sqlpolicy` anchored mutant set that the protected tier
could previously not see (`F8` in `docs/testing/AUDIT-MA.3.md`). No existing file was edited.

## What landed

`crates/blastit-sqlpolicy/tests/critical_learn_policy.rs` (new, protected by
`tools/critical_guard.py`; `paths` respected — nothing else in the repo changed).

| Test | Twin | Guard it pins |
|---|---|---|
| `copy_program_is_refused_and_a_plain_copy_is_grantable` | deny `FROM/TO PROGRAM` (both spellings, command never printed) / allow `COPY … TO STDOUT` | `is_copy_program` (`SP1`) |
| `multi_statement_text_is_refused_with_its_count` | deny 2- and 3-statement text with the count / allow one statement | `statements.len()` arm (`SP3`) |
| `an_inline_literal_is_refused_and_only_the_redacted_shape_is_carried` | deny all 8 literal forms, `normalized() == None` / allow `$1` → `$?` | `has_quoted_literal` (`SP2`) |
| `a_call_and_a_three_part_name_cannot_hide_a_dangerous_function` | deny `CALL pg_read_file(…)`, `CALL pg_catalog.…`, `pg_catalog.pg_catalog.…` / allow `public.` and `shop.` | `CallStmt.funccall` walk + three-part resolution (`SP4`, `SP5`) |
| `only_single_statement_catalogue_setup_is_replayed` | deny `SET x; DROP TABLE t`, `SELECT * FROM auth_user`, `pg_authid` / allow `SET`, `SHOW`, `version()`, `to_regtype($?)` and `$1` | `VariableShowStmt`, `is_introspection_select` (`SP6`, `SP7`) |
| `a_deeply_nested_catalogue_select_is_not_replayed` | deny 14/15/18 nestings that the parser accepts / allow a 4-deep nesting | `MAX_INTROSPECTION_DEPTH` (**`SP8`, the survivor**) |
| `public_t_and_t_are_one_table` | allow both spellings both ways / deny `users` for an `audit.users` grant | `canonical_table` (`SP11`) |
| `a_whole_row_read_needs_the_star_grant` | deny `SELECT *`, `INSERT INTO t VALUES (…)` under a column list / allow `"*"` and named columns | `Extent::Whole` (`SP10`) |
| `a_group_grant_allows_and_an_ungranted_statement_is_denied` | allow route grant and group-merged table+function / deny column and function by name | `TableGrants::check` + `merge_from` (§6) |
| `the_allowlist_allows_table_grants_and_denies_the_rest` | allow exact query and table-granted statement / deny with `select on t.secret not granted` | `Allowlist::check` (§6) |

Each test carries a one-line comment naming `R24`, `R25`, `R31` or `§6`/§7.2.

## Acceptance evidence

```console
$ cargo test -p blastit-sqlpolicy --test critical_learn_policy --locked
running 10 tests
test copy_program_is_refused_and_a_plain_copy_is_grantable ... ok
test multi_statement_text_is_refused_with_its_count ... ok
test a_call_and_a_three_part_name_cannot_hide_a_dangerous_function ... ok
test a_whole_row_read_needs_the_star_grant ... ok
test an_inline_literal_is_refused_and_only_the_redacted_shape_is_carried ... ok
test the_allowlist_allows_table_grants_and_denies_the_rest ... ok
test only_single_statement_catalogue_setup_is_replayed ... ok
test a_group_grant_allows_and_an_ungranted_statement_is_denied ... ok
test public_t_and_t_are_one_table ... ok
test a_deeply_nested_catalogue_select_is_not_replayed ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```console
$ cargo test -p blastit-sqlpolicy --locked          # whole crate
test result: ok. 219 passed; 0 failed   (src unit tests)
test result: ok. 10 passed; 0 failed    (critical_learn_policy)
test result: ok. 25 passed; 0 failed    (critical_literals, untouched)
test result: ok. 8 passed; 0 failed     (doctests)
```

```console
$ make test-critical
protected critical tests hold no skip construct
cargo test -p blastit-sqlpolicy --test critical_learn_policy
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
...
python -m unittest discover -s python/tests/critical
Ran 50 tests in 3.108s
OK
```

```console
$ make critical-guard BASE=dba348c
critical-guard: dba348c..working tree, 16 changed file(s)
  no change to a protected critical test
```

```console
$ make rs-lint
Checking blastit-sqlpolicy v0.1.0
    Finished `dev` profile
$ make rs-doc      # Documenting blastit-sqlpolicy ... Finished
$ make docs-check  # Build started / No issues found / Build finished in 0.31s
$ make plan-validate   # plan OK
$ cargo fmt --all -- --check   # clean
```

`make check` passes **every** gate except `py-lint`, which fails inside
`python/tests/runnable_docs/harness.py` (`configparser` unused, `F821 _require_commands`) — a file
another node (MA.3.9/MA.3.10) is rewriting in the same working tree right now. It is outside this
node's `paths`; the failure is not caused by this change.

```console
$ make check
... rs-fmt OK, rs-lint OK, rs-doc OK, rs-test OK, test-critical OK ...
uvx --offline ruff check python tools
F401 `configparser` imported but unused  --> python/tests/runnable_docs/harness.py:50:8
F821 Undefined name `_require_commands`  --> python/tests/runnable_docs/harness.py:871:9
Found 2 errors.
make: *** [Makefile:82: py-lint] Error 1
```

## Mutation proof (the point of this node)

`/tmp/opencode/ma311/work` is a copy of the workspace holding only `Cargo.toml`, `Cargo.lock` and
`crates/blastit-sqlpolicy` (the real test file copied in, `src/` byte-identical to the repo).
`/tmp/opencode/ma311/mutants.py` applies one anchored replacement per mutant, runs
`cargo test -p blastit-sqlpolicy --test critical_learn_policy`, and restores the file. **Pristine
copy: 10/10 pass.**

```console
$ uv run --no-project python mutants.py
SP1: killed — test copy_program_is_refused_and_a_plain_copy_is_grantable ... FAILED
SP2: killed — test an_inline_literal_is_refused_and_only_the_redacted_shape_is_carried ... FAILED
SP3: killed — test multi_statement_text_is_refused_with_its_count ... FAILED
SP4: killed — test a_call_and_a_three_part_name_cannot_hide_a_dangerous_function ... FAILED
SP5: killed — test a_call_and_a_three_part_name_cannot_hide_a_dangerous_function ... FAILED
SP6: killed — test only_single_statement_catalogue_setup_is_replayed ... FAILED
SP7: killed — test only_single_statement_catalogue_setup_is_replayed ... FAILED, test a_deeply_nested_catalogue_select_is_not_replayed ... FAILED
SP8: killed — test a_deeply_nested_catalogue_select_is_not_replayed ... FAILED
SP10: killed — test a_whole_row_read_needs_the_star_grant ... FAILED
SP11: killed — test public_t_and_t_are_one_table ... FAILED

10/10 killed by the protected suite
```

SP8 (`if depth > MAX_INTROSPECTION_DEPTH { return false; }` deleted, for the second time in this
audit line) is now killed by the **protected** tier instead of surviving it. Every kill names the
protected test, so the "delete the check and its only test together" hole (`F8`) is closed for the
learn/table policy.

## Deviation from the audit, and why the test is stronger

The acceptance text asks for "an 18-deep nested catalogue `SELECT` is not replayed". With
`SELECT ({inner})` nesting on `pg_query` 6.2.1, PostgreSQL's own parser refuses the text at **16**
nestings (tree depth 163; ~10 JSON levels per nesting), and the walk's bound (`128`) starts
refusing at **12**; 14 and 15 parse and are refused by the walk, 18 does not parse at all. A test
that merely asserted "18 is not replayed" would therefore pass with the bound deleted.

So the oracle asserts both: the deepest parseable nesting (14, 15, and the audit's literal 18) is
not replayed, and a 4-deep catalogue `SELECT` still is. The depth test walks 0..=32, stops at the
parser's own limit, and requires `deepest_parseable >= 14` so a shape change cannot silently make
the oracle vacuous. The mutation run shows 14 flips to replayed under SP8, so the deny is the
bound's, not the parser's.

## Follow-ups (out of scope)

- `SP9` (the fixed setup statement is redacted) is a `literals.rs` mutant, still killed only by a
  `src/**` unit test; if §7.1's "≥ 1 protected-suite kill per mutant group" is read per-mutant, it
  needs a protected assertion in `critical_literals.rs` — that file is protected, so a new file
  (like this one) is the way.
- The audit's SP8 probe numbers ("pristine: depth 18 parses=true") do not reproduce on this
  `pg_query` with `SELECT (…)` nesting; either the audit used another shape or another parser
  build. Worth one line in `docs/knowledge/LESSONS.md` (not in this node's `paths`).
- `make check` is red only on `python/tests/runnable_docs/harness.py` (`F401 configparser`, `F821
  _require_commands`), owned by the sibling node editing that file.
