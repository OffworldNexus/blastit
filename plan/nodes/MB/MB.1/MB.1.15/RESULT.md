# RESULT — MB.1.15

Close the remaining `blastit-manifest` mutation survivors and protect the patcher's wrong-route case.

- **Status:** done
- **Branch / base:** `release/0.2.0` @ `9c523e2`; this node's work is uncommitted (implementer never commits)
- **Paths touched:** exactly the three in `NODE.md` — `crates/blastit-manifest/tests/{mutation_gaps,edit_branches,critical_manifest_edit}.rs`. No source, no `Cargo.*`, no `docs/`, no `Makefile`, no `e2e/`, no `critical_*` file edited other than the one the node lists.
- **Production code unchanged:** `git diff --stat -- crates/blastit-manifest/src` is empty.

## What was added

| File | `#[test]` before → after | New cases (each with its denied twin) |
|---|---|---|
| `tests/mutation_gaps.rs` | 17 → **20** | `a_declared_proxy_name_is_a_grant_only_when_it_holds_a_table` (table accepted / string refused, exact list), `a_reserved_key_inside_a_table_grant_row_is_a_misplaced_top_level_key` (bad row refused / table named `groups` accepted), `an_unknown_grant_key_holding_a_table_is_reported_as_the_key_it_is` |
| `tests/edit_branches.rs` | 17 → **19** | `a_bare_file_name_is_written_into_the_process_directory` (write succeeds and is reported as success), `a_write_into_an_unwritable_directory_reports_permission_denied` (`PermissionDenied`, nothing written, no temp file) |
| `tests/critical_manifest_edit.rs` | 18 → **23** | `a_grant_lands_in_the_route_and_proxy_it_was_proposed_for` (+ `a_grant_for_a_route_that_does_not_exist_is_refused_rather_than_attached`), `a_statement_that_reads_like_an_assignment_is_never_edited_as_one`, `a_table_that_closes_the_previous_element_never_receives_the_grant` (+ previous element byte-identical), `a_grant_is_never_written_into_the_other_proxy_s_table` |

The critical file is **purely additive** (`git diff -U0` removes no line there); the only removed line in the whole node is `use std::path::Path;`, widened to `use std::path::{Path, PathBuf};`. No assertion was weakened, nothing skipped, no `#[mutants::skip]`.

```console
$ git diff --numstat -- crates/blastit-manifest/tests/
278     0       crates/blastit-manifest/tests/critical_manifest_edit.rs
174     1       crates/blastit-manifest/tests/edit_branches.rs
113     0       crates/blastit-manifest/tests/mutation_gaps.rs
$ git status --porcelain -- crates/blastit-manifest/src      # empty: no production change
```

The two `write_atomic` cases are hermetic: the bare-name case re-executes the test binary into its own temporary `cwd` (env-var guard, `--exact`), so it never mutates the process-wide directory of another test and does not need `--test-threads=1`; the `EACCES` case probes the mode first and *says so and asserts nothing* when the process bypasses directory permissions (uid 0 → `smoke (root)`), which is documented in its doc comment.

## Acceptance

### 1. The bare-relative-name branch, pinned hermetically

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test edit_branches --locked -- --nocapture
...
test a_bare_file_name_is_written_into_the_process_directory ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit=0
```

Failing probes on a copy of the tree (`/var/tmp/mb115-probe1`, disk scratch, `CARGO_TARGET_DIR=/var/tmp/mb115-probe-target`; `/tmp` is tmpfs on this host, see Follow-ups):

```console
# mutant 1: edit.rs:384:25 `!parent.as_os_str().is_empty()` -> `true`
$ sed -n '383,386p' /var/tmp/mb115-probe1/crates/blastit-manifest/src/edit.rs
    let directory = match path.parent() {
        Some(parent) if true => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
$ cargo test --manifest-path /var/tmp/mb115-probe1/Cargo.toml -p blastit-manifest \
      --test edit_branches --locked -- --exact a_bare_file_name_is_written_into_the_process_directory
thread 'a_bare_file_name_is_written_into_the_process_directory' panicked at crates/blastit-manifest/tests/edit_branches.rs:601:14:
a bare file name is a path in the process's own directory: Os { code: 2, kind: NotFound, message: "No such file or directory" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out
```

```console
# mutant 2: edit.rs:411:23 `error.kind() == io::ErrorKind::AlreadyExists` -> `true`
$ sed -n '409,413p' /var/tmp/mb115-probe1/crates/blastit-manifest/src/edit.rs
        Err(error) if true => {
            std::fs::remove_file(&temporary)?;
$ cargo test --manifest-path /var/tmp/mb115-probe1/Cargo.toml -p blastit-manifest \
      --test edit_branches --locked -- --exact a_write_into_an_unwritable_directory_reports_permission_denied
thread 'a_write_into_an_unwritable_directory_reports_permission_denied' panicked at crates/blastit-manifest/tests/edit_branches.rs:684:5:
assertion `left == right` failed: a refused create was reported as Os { code: 2, kind: NotFound, message: "No such file or directory" } instead of the permission problem
  left: NotFound
 right: PermissionDenied
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out
```

The same mutant, seen by `cargo-mutants` itself (independent of the hand-patched copy), is `caught`:

```console
$ TMPDIR=/var/tmp/mb115-mut cargo mutants -p blastit-manifest -F 'replace match guard !parent\.as_os_str\(\)\.is_empty\(\) with true' \
      -j 2 --timeout 90 --all-logs -- -- --exact a_bare_file_name_is_written_into_the_process_directory
Found 1 mutant to test
1 mutant tested in 65s: 1 caught
```

### 2. The two `parse.rs` survivors: input named, exact error list pinned

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test mutation_gaps --locked
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit=0
```

```console
# mutant: parse.rs:197:43 `declared.contains(key) && value.is_table()` -> `||`
# input: [proxy.app] declared, then `app = "shop.wsgi:application"` inside [[route]]
thread 'a_declared_proxy_name_is_a_grant_only_when_it_holds_a_table' panicked at .../tests/mutation_gaps.rs:464:5:
assertion `left == right` failed: the declared name hid the misplaced top-level key: manifest has 1 error(s)
  - route[0].app: expected a table, found a string
  left: [WrongType { path: "route[0].app", expected: "a table", found: "a string" }]
 right: [NotTopLevel { key: "app" }]
```

The allowed twin in the same test is the shape the mutant exists for and that must keep working: `[proxy.app]` plus `app = { queries = ['SELECT "t"."a" FROM "t"'] }` is a grant on the declared proxy and the route boots.

```console
# mutant: parse.rs:220:16 `key == "tables"` -> `!=`  (one input, both directions)
thread 'a_reserved_key_inside_a_table_grant_row_is_a_misplaced_top_level_key' panicked at .../tests/mutation_gaps.rs:488:5:
assertion `left == right` failed: the row's reserved key was reported as an unknown operation: manifest has 1 error(s)
  - route[0].db.tables.t.groups: unknown operation; expected select, insert, update or delete
  left: [UnknownTableOperation { path: "route[0].db.tables.t.groups" }]
 right: [NotTopLevel { key: "groups" }]
thread 'an_unknown_grant_key_holding_a_table_is_reported_as_the_key_it_is' panicked at .../tests/mutation_gaps.rs:529:5:
assertion `left == right` failed: the inner table was walked as if it were a grant row: manifest has 1 error(s)
  - groups after a [table] is not top-level; move it above the first table
  left: [NotTopLevel { key: "groups" }]
 right: [UnknownKey { path: "route[0].db.options" }]
test result: FAILED. 18 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

The second document is the acceptance's "unknown grant key holding a table" (`[route.db.options] first = { groups = ["x"] }`): the shipped code names `route[0].db.options`, the mutant invents a misplaced `groups` and hides the real typo. The allowed twin is `[route.db.tables] groups = { select = ["a"] }` (a table *named* `groups`), which both codes accept and which is asserted in the same test.

### 3. The patcher integrity cases are protected

```console
$ grep -c '#\[test\]' crates/blastit-manifest/tests/critical_manifest_edit.rs
23                      # was 18
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_manifest_edit --locked
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
exit=0
```

- `a_grant_lands_in_the_route_and_proxy_it_was_proposed_for` — two routes, one grant table, a `[route.warehouse]` next to a proposed `db` grant: the proposal lands on its own route/proxy, the neighbour's `queries` line comes back byte-identical, one `[route.db]` table, above the next `[[route]]`. Its denied twin `a_grant_for_a_route_that_does_not_exist_is_refused_rather_than_attached` refuses a stale route name (`MissingRoute`, "add the route first") instead of inventing a route for it.
- `a_statement_that_reads_like_an_assignment_is_never_edited_as_one` — the scanner's string state: a `queries` value that holds a line reading `queries = ['fake']` is extended as a value (author's statement preserved, exactly two entries), and with no real assignment in the table a new one is written under the header while the literal comes back byte for byte (the model's function name still `"queries = ['fake']\n"`).
- `a_table_that_closes_the_previous_element_never_receives_the_grant` — `[route.db]` immediately above the next `[[route]]`; the proposal for the *next* route is written under its own header and the previous element stays byte-identical.
- `a_grant_is_never_written_into_the_other_proxy_s_table` — `[route.warehouse] queries = …` directly above `[route.db]`: the new assignment goes under `[route.db]`, the warehouse list is untouched.

### 4. Re-measured

```console
$ make mutants-rust PKG=blastit-manifest MUTANTS_TMP=/var/tmp/mb115-mut
mutants: scratch TMPDIR=/var/tmp/mb115-mut   (xfs, 644 GiB free; /tmp is tmpfs on this host)
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 637 mutants to test
637 mutants tested in 19m: 6 missed, 558 caught, 73 unviable

$ test ! -s mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
$ grep -E 'parse\.rs:(197|220)|edit\.rs:(384|411)' mutants.out/missed.txt || echo "adjudicated survivors closed"
adjudicated survivors closed
$ wc -l mutants.out/{caught,missed,unviable}.txt
  558 mutants.out/caught.txt
    6 mutants.out/missed.txt
   73 mutants.out/unviable.txt
```

**Killed: 558 / 564 viable = 98.94 %** (before: 550 / 564 = 97.52 %), **misses 14 → 6**, 0 timeouts.

### 5. Gates

```console
$ make rs-lint
... cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
rs-lint exit=0

$ make rs-doc
... RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
rs-doc exit=0

$ rustfmt --edition 2024 --check crates/blastit-manifest/tests/{edit_branches,mutation_gaps,critical_manifest_edit}.rs
rustfmt-check=0

$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.15
critical-guard: 51dced9..working tree, 51 changed file(s)
node MB.1.15: paths ['crates/blastit-manifest/tests/mutation_gaps.rs', 'crates/blastit-manifest/tests/edit_branches.rs', 'crates/blastit-manifest/tests/critical_manifest_edit.rs']
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_edit.rs]
  ...
critical-guard: FAILED: 1 protected critical test(s) changed or deleted without a covering path in node(s) MB.1.15.
  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]

$ uv run --no-project tools/plan.py validate
plan OK
validate exit=0
```

The single `critical_guard` violation is **not this node's file**: `crates/blastit/tests/critical_check_engine.rs` belongs to the `add_group` fix-action leaf (`AUDIT-MB.1` T2) that is being edited in the same worktree. This node's protected file is reported as `authorized`. `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked` is green (36 unit + 17 + 23 + 3 + 19 + 13 + 20 + 33 + 11 + 6 + 29 + 4 doc tests).

## Mutation, per module

`mutants.out/` (this campaign) vs the whole-crate record in `docs/testing/AUDIT-MB.1.md` §Mutation evidence. Unviable counts are per source line and are unchanged (73 both times), so the "before" column is the after column minus what the new tests newly killed.

| Module | Mutants | Before caught / missed | After caught / missed | Killed (viable) |
|---|---|---|---|---|
| `edit.rs` | 261 | 233 / 11 | **239 / 5** | 95.5 % → **97.95 %** (239/244) |
| `parse.rs` | 131 | 113 / 2 | **115 / 0** | 98.3 % → **100 %** (115/115) |
| `model.rs` | 149 | 119 / 0 | 119 / 0 | 100 % (119/119) |
| `validate.rs` | 55 | 53 / 0 | 53 / 0 | 100 % (53/53) |
| `error.rs` | 22 | 18 / 0 | 18 / 0 | 100 % (18/18) |
| `uid.rs` | 11 | 10 / 1 | 10 / 1 | 90.9 % (10/11) |
| `routing.rs` | 8 | 4 / 0 | 4 / 0 | 100 % (4/4) |
| **total** | **637** | **550 / 14** | **558 / 6** | **98.94 %** (558/564) |

Newly killed (6): `edit.rs:384:25`, `:411:23`, `:794:30 (+→-)`, `:815:35 (+→-)`, `:818:17`, `:819:17`; `parse.rs:197:43`, `:220:16`. (`818`/`819`, the assignment predicate's `&& → ||` and `!` deletion, fall to the protected in-string case; `794`/`815` to the two new anchor cases — both `Verification` refusals that the shipped code does not produce.)

## Survivors left: disposition (6)

All six are **equivalent**, not gaps: no input can make them differ. Proofs below; a test that tried to distinguish one would have to construct a state `apply_edits` already refuses.

1. **`uid.rs:62:16` `>` → `>=`** in `UidLayout::view`. `uid = base + 1000 + index` as `u64`; the two arms return `u32::MAX` and `uid as u32`. They agree everywhere except possibly at `uid == u32::MAX`, where the real code returns `u32::MAX as u32 == u32::MAX` and the mutant returns `u32::MAX`. Below and above, neither arm changes. The two sibling mutants at the same line (`> → ==`, `> → <`) *are* distinguishable and are already killed by `tests/uid.rs:38` (`a_huge_base_saturates_instead_of_panicking`, base `u32::MAX`).
2. **`edit.rs:490:9` `Plan::is_noop -> false`.** `is_noop` only short-circuits `apply_edits`. With `false`, an empty plan goes through `render`, which with no patches and no new routes is `source.split_inclusive('\n').map(str::to_string).collect::<Vec<_>>().concat()` — the identity on a `&str` — and `verify` then compares that identical re-parse against `expected_routes`, which for an empty plan is the unchanged route list. Return value, side effects (none) and error paths are identical. The sibling `-> true` and `&& → ||` mutants are distinguishable and counted as caught.
3. **`edit.rs:743:15` `>` → `>=`.** The guard is `at > 0 && lines.get(at - 1).is_some_and(|line| !is_blank(line))`, with `at` one past the last non-comment line above the insertion point. Every document `apply_edits` accepts carries `version = 1` above the first table header (a `version` below a `[table]` is `NotTopLevel` and the parse fails first, `parse.rs:57-60`), so at least one non-comment line precedes the catch-all and `at >= 1`. At `at >= 1` both spellings are equal; they differ only at `at == 0`, where the mutant evaluates `0usize - 1`, i.e. a panic in a state no accepted document reaches.
4. **`edit.rs:794:30` `+` → `*`** (`(start + 1..element_end)` → `(start..element_end)`). The one extra candidate is line `start` itself, which `route_starts` selected precisely because `header.array && header.name == "route"`; the predicate requires `!header.array`, so the extra line can never be selected and the search returns the same `header_line` for every input.
5. **`edit.rs:815:35` `+` → `*`** (`(header_line + 1..section_end)` → `(header_line..section_end)`). The extra candidate is the `[route.<proxy>]` header line; the predicate requires `is_queries_assignment`, i.e. the trimmed line starts with `queries`, while `parse_header` accepts a line only when it starts with `[`. Mutually exclusive, so the same `assignment` is found for every input.
6. **`edit.rs:878:20` `&&` → `||`** (`if had_newline && !replacement.ends_with('\n')`). In this branch `replacement` never ends with a newline: the only part that could is `comment`, which is non-empty only when `one_line` holds (`edit.rs:865-873`), and with `one_line` the branch is reached only if `append_to_array` returned `None`, i.e. `closing_bracket` found no closing bracket — a line the scanner ends at depth 0 that offers no closing bracket is not a TOML array value (a document using it, e.g. `queries = "SELECT 1"  # note`, is refused by the parser: `expected: "an array", found: "a string"`). So `!ends_with('\n')` is always true and the guard reduces to `had_newline` (real) vs `true` (mutant), which differ only when `had_newline == false`; there the mutant pushes one `'\n'` and the next block (`if !had_newline && replacement.ends_with('\n') { replacement.pop() }`) removes exactly that one, leaving byte-identical output.

## Follow-ups (out of scope here)

1. `make rs-fmt`/`cargo fmt --all -- --check` was briefly red on `crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs` and `crates/blastit-sqlpolicy/src/lexer.rs` (the in-flight `MB.1.14`/`MB.1.17` work). Both settled before the gate runs above; this node never touched them.
2. `crates/blastit/tests/critical_check_engine.rs` is modified in this worktree by the `add_group` leaf, which is why the node-scoped `critical_guard` run reports one violation. It is not this node's path.
3. Tooling notes for `LESSONS.md` (this node's `paths` exclude `docs/`, so they are recorded here instead):
   - `cargo mutants` puts `CARGO_TEST_ARGS` into `cargo test` **verbatim, without a `--`**. To scope a probe to one test the invocation needs `-- -- --exact <name>`; `scripts/mutants-run.sh` always appends its own `-j/--timeout`, so a scoped probe has to call `cargo mutants` directly with `TMPDIR=/var/tmp/... -j 2 --timeout 90`.
   - The editor's auto-format and `rustfmt --edition 2024` disagree on nested import order here (ASCII, uppercase first, is what `cargo fmt --all -- --check` wants). Re-run `rustfmt --edition 2024` on a file after editing it, then `--check`.
   - On this host the shell tool refuses `cd` into `/var/tmp`, so the mutant probe copy is driven with `cargo test --manifest-path /var/tmp/...` from the repository's `cwd`, and `sed -i` patches only the copy.
4. `mutants.out/` and `/var/tmp/mb115-{mut,probe1,probe-target}` are scratch: `make mutants-clean` (MB.1.11) owns pruning.
