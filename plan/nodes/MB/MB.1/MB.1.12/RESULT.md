# RESULT — MB.1.12: promote the policy-surface oracles to the protected tier

**Status:** done. Test-tier promotion only; no production code, no `Cargo.*`, no plan status touched.

## What the protected file now owns

`crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` (new, 803 lines, **22 `#[test]`**,
`tools/critical_guard.py`-protected). It owns, at grant level (`Allowlist::check`) and at extents level
(`analyze` + `TableGrants::check`), every behaviour the audit found unprotected:

- **`normalize` shaping (AUDIT-MB.1 `F1`)** — reformatted-allowed vs `--`-hidden second statement /
  in-code `;` / other column / literal-for-placeholder / casing / NBSP-denied; the `IN` placeholder
  list at 1..8 placeholders and every spacing of it; the `IN ($?)`-grant-denies-`IN ('x')` escape
  (dedicated test, below); the `.`-guard (`probe_s.in(…)` keeps its arity); `IN` with no space in
  front of it; `VALUES` rows collapsing only as identical rows; a run holding a `;` never collapsing;
  a placeholder group being a shape only after `IN`; the bulk `INSERT … RETURNING` grant and the
  idempotence of the normalised text.
- **Table/column grant extents (`F2`)** — `select = [cols]` reaches exactly those columns and not
  `SELECT *`; `"*"` reaches every column of its own table and no other; read-grant vs
  insert/update/delete twins; insert-grant vs read twin; `INSERT` with no column list needing `"*"`;
  subquery-reached table; **join alias reported under its base table** (both sides required);
  **`INSERT … SELECT` read side**; group merge unioning operations and absorbing a list into `"*"`.
- **Lock extents (§6, D3c)** — `FOR UPDATE` needs `update` on **one** column (enough to lock, not to
  write), `update = []` grants no lock, a covering grant writes `update = ["id"]` and never
  `update = "*"`, and a lock that cannot name a column is reported, not widened.
- **Transaction-control grammar** — `BEGIN`/`START`/`COMMIT`/`ROLLBACK TO SAVEPOINT`/`SAVEPOINT`
  allowed only with the flag and only as themselves (`BEGIN; DROP TABLE t`, `BEGIN READ` denied).

The two standard-tier files are **unchanged in content**: 0 lines removed, `#[test]` counts identical
(`policy_module_edges.rs` 10, `policy_module_tables.rs` 18). Each of the 21 promoted cases carries a
one-line pointer at its origin:

```
$ git diff --numstat crates/blastit-sqlpolicy/tests/
8	0	crates/blastit-sqlpolicy/tests/policy_module_edges.rs
13	0	crates/blastit-sqlpolicy/tests/policy_module_tables.rs
$ grep -c 'Protected twin (MB.1.12' crates/blastit-sqlpolicy/tests/policy_module_{edges,tables}.rs
8
13
```

## Acceptance evidence

### 1. The file exists and the crate is green

```console
$ ls -l crates/blastit-sqlpolicy/tests/critical_policy_surface.rs
-rw-r--r-- 1 remy remy 28788 Nov 13 18:43 crates/blastit-sqlpolicy/tests/critical_policy_surface.rs

$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
running 224 tests
test result: ok. 224 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.39s
     Running tests/critical_learn_policy.rs ... running 10 tests
test result: ok. 10 passed; 0 failed; ...
     Running tests/critical_literals.rs ... running 25 tests
test result: ok. 25 passed; 0 failed; ...
     Running tests/critical_policy_surface.rs ... running 22 tests
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/lexer_progress.rs ... running 6 tests
test result: ok. 6 passed; ...
     Running tests/policy_module_edges.rs ... running 10 tests
test result: ok. 10 passed; ...
     Running tests/policy_module_tables.rs ... running 18 tests
test result: ok. 18 passed; ...
running 8 tests            # doctests
test result: ok. 8 passed; ...
```

### 2. The reproduced escape now fails a PROTECTED suite

Probe on a `/tmp` copy only (`/tmp/opencode/mb112-probe`, `CARGO_TARGET_DIR=/tmp/opencode/mb112-probe-target`),
`is_placeholder_list` replaced by `fn is_placeholder_list(_pieces, _items) -> bool { true }`. No file in
the repository was edited. The copy is deleted (`rm -rf`, verified absent).

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked --test critical_policy_surface
failures:

---- an_in_list_is_one_shape_whatever_its_length stdout ----
thread '...' panicked at crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:240:9:
not a list of placeholders: "SELECT id FROM notes_note WHERE owner_id IN ()"

---- an_in_placeholder_list_grant_does_not_admit_an_inline_literal stdout ----
thread '...' panicked at crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:168:5:
widened: `normalize` collapsed an inline literal into the placeholder-list shape, so a grant for
`IN ($?)` admits `IN ('x')` (both normalise to "SELECT id FROM notes_note WHERE owner_id IN ($?, ...)")

failures:
    an_in_list_is_one_shape_whatever_its_length
    an_in_placeholder_list_grant_does_not_admit_an_inline_literal

test result: FAILED. 20 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
error: test failed, to rerun pass `-p blastit-sqlpolicy --test critical_policy_surface`
```

The same mutated tree still passes both pre-existing protected suites — this is the audit's `F1`
blindness, now covered by the new file:

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked --test critical_literals --test critical_learn_policy
     Running tests/critical_learn_policy.rs ...
test result: ok. 10 passed; 0 failed; ...
     Running tests/critical_literals.rs ...
test result: ok. 25 passed; 0 failed; ...
```

```console
$ rm -rf /tmp/opencode/mb112-probe /tmp/opencode/mb112-probe-target
$ ls -d /tmp/opencode/mb112* 2>/dev/null
   (nothing)
```

### 3. Coverage floor and the no-deletion rule

```console
$ grep -c '#\[test\]' crates/blastit-sqlpolicy/tests/critical_policy_surface.rs
22
```

22 ≥ 12, and it covers every minimum the node lists: `IN ($?)` grant denies `IN ('x')`
(`an_in_placeholder_list_grant_does_not_admit_an_inline_literal`), join alias
(`a_join_alias_is_reported_under_its_base_table`), subquery
(`a_table_reached_through_a_subquery_is_reported`), `INSERT … SELECT`
(`the_read_side_of_an_insert_select_is_reported`), `SELECT *`
(`a_column_list_reaches_exactly_its_columns`, `a_star_grant_reaches_every_column_but_a_list_is_not_a_star`,
`an_insert_without_a_column_list_needs_the_whole_table`), lock extents
(`a_lock_needs_update_on_one_column`, `a_covering_grant_writes_the_lock_it_can_name`,
`a_lock_without_a_column_is_reported_not_widened`).

### 4. Guard + gate

`--node MB.1.12` alone flags one file, and it is **MB.1.13's** concurrently edited
`crates/blastit/tests/critical_check_engine.rs`, not mine:

```console
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.12
critical-guard: 51dced9..working tree, 9 changed file(s)
node MB.1.12: paths ['crates/blastit-sqlpolicy/tests/critical_policy_surface.rs',
 'crates/blastit-sqlpolicy/tests/policy_module_edges.rs',
 'crates/blastit-sqlpolicy/tests/policy_module_tables.rs']
  M  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 1 protected critical test(s) changed or deleted without a covering path ...
exit=1
```

With the parallel node listed too (the guard's documented multi-node form), the tree is clean — no
protected change is mine, and the new file is an addition, which the guard always allows:

```console
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.12 --node MB.1.13
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.1.12, MB.1.13
exit=0
```

```console
$ make rs-lint
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.99s
exit=0

$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.35s
exit=0

$ uv run --no-project tools/plan.py validate
plan OK
exit=0
```

`cargo fmt --all -- --check` reports diffs only in `crates/blastit/tests/{audit_group_hint,
critical_check_engine}.rs` (MB.1.13's in-flight edits); none in the three files of this node.

## Files

- `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` — **new**, 22 tests, protected.
- `crates/blastit-sqlpolicy/tests/policy_module_edges.rs` — **+8 lines, 0 removed** (one-line pointers).
- `crates/blastit-sqlpolicy/tests/policy_module_tables.rs` — **+13 lines, 0 removed** (one-line pointers).

## Follow-ups

1. `policy_module_edges.rs` / `policy_module_tables.rs` now duplicate 21 protected cases; a later
   curation pass could keep only the rendering/serde twins there and drop the duplicated bodies (the
   audit's `T1` wording). Deliberately not done here: the node forbids deleting an assertion, and
   duplication costs ~0.03 s in `make check`.
2. `docs/testing/AUDIT-MB.1.md` `F1`/`F2` can be marked closed at the epic gate; `F3`
   (`crates/blastit/tests/audit_group_hint.rs`) is MB.1.13's scope, not this node's.
3. The standard-tier `in_needs_no_space_in_front_of_it` and `the_dot_is_the_guard…` cases are pure
   `normalize` text assertions; if `cargo-mutants` ever shows an equivalent mutant only they kill,
   the protected responsibility stays with the grant-level twins.
