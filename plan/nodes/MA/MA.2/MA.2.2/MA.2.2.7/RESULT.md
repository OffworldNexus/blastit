# RESULT — MA.2.2.7

**Status:** done

**Changed:** `crates/blastit-manifest/tests/critical_manifest_edit.rs` (test-only; +2 tests,
+85 lines). No production code, no `Cargo.toml`/`Cargo.lock`, no `docs/**`, no `#[ignore]`.
`a_failed_write_removes_its_temp_file` and every other existing test are untouched.

## What the two new tests pin

`write_atomic` writes to the predictable sibling `.<name>.<pid>.tmp` (audit gap G3, mutant
`mf.write_atomic_create_new_off`). Both tests use the node's `TempDir` helper, so each runs in
its own directory.

- `a_planted_temp_name_symlink_is_never_written_through` (`#[cfg(unix)]`, no ignore): writes
  `blastit.toml` and `victim.txt` (`"victim\n"`), plants a **relative** symlink
  `.blastit.toml.<pid>.tmp -> victim.txt` at the exact temp name, then calls `write_atomic`.
  Asserts: the call **succeeds** (the tip's documented remove-and-retry, not an error it does
  not produce), `victim.txt` is still byte-identical, the manifest holds the new content, the
  planted name is gone (`symlink_metadata` fails), `temp_files` is empty, and the directory is
  exactly `["blastit.toml", "victim.txt"]`.
- `a_stale_regular_file_at_the_temp_name_is_replaced` (denied twin): plants a **regular file**
  holding `"stale = 1\n"` at the temp name and keeps an open read handle on it before the
  write. After `write_atomic` the open (now unlinked) inode still reads `"stale = 1\n"` — the
  retry unlinked the stale file and created a fresh one, it did not open the stale file in
  place; the old name is gone, the manifest is exactly `SOURCE`, no `.tmp` entry remains.

The second assertion is the stronger of the two for the regular-file case: content equality
alone would also pass a non-truncating write into the stale file, while the open handle proves
which inode received the bytes.

## Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_manifest_edit --locked
running 18 tests
test a_failed_write_removes_its_temp_file ... ok
test a_planted_temp_name_symlink_is_never_written_through ... ok
test a_grant_on_an_undeclared_proxy_names_the_missing_declaration ... ok
test a_stale_regular_file_at_the_temp_name_is_replaced ... ok
test a_redacted_placeholder_is_refused_without_echoing_it ... ok
test a_method_restricted_catch_all_is_not_an_insertion_point ... ok
test an_inline_grant_table_is_named_rather_than_duplicated ... ok
test an_inline_literal_is_refused_without_echoing_it ... ok
test an_unrestricted_grant_cannot_take_exact_queries ... ok
test a_proposed_route_has_to_be_routable ... ok
test a_grant_on_a_reserved_proxy_name_still_boots ... ok
test an_inferred_route_name_cannot_silently_change_a_path ... ok
test a_route_is_inserted_above_both_catch_all_spellings ... ok
test a_grant_joins_an_existing_queries_list_without_rewriting_it ... ok
test comments_quoting_and_untouched_lines_survive_an_edit ... ok
test applying_the_same_edits_twice_changes_nothing ... ok
test the_atomic_write_leaves_no_temp_file_and_the_result_parses ... ok
test every_edit_fixture_round_trips_through_parse_and_validate ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

Crate-wide, no regression from the edited file:

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
test result: ok. 9 / 17 / 18 / 3 / 13 / 33 / 11 / 6 / 29 passed; 0 failed; 0 ignored
   Doc-tests blastit_manifest
test result: ok. 4 passed; 0 failed; 0 ignored
```

## Mutation proof (scratch copy, tree never mutated)

`rsync -a --exclude target --exclude .git` to `/tmp/opencode/blastit-ma2278-mut` (shared with
MA.2.2.8), `CARGO_TARGET_DIR=/tmp/opencode/blastit-ma2278-target`, one anchor-checked edit per
run, pristine source restored from a copy between runs.

```console
# pristine scratch tip
running 18 tests ... test result: ok. 18 passed; 0 failed; 0 ignored

# mf.write_atomic_create_new_off: edit.rs `.create_new(true)` -> `.create(true)` (both of 2
# occurrences, lines 403 and 413: the first open and the retry)
$ grep -c 'create_new(true)' crates/blastit-manifest/src/edit.rs
2
$ perl -pi -e 's/\.create_new\(true\)/.create(true)/g' crates/blastit-manifest/src/edit.rs
running 18 tests
test a_planted_temp_name_symlink_is_never_written_through ... FAILED
  panicked at critical_manifest_edit.rs:688: assertion `left == right` failed: the write followed
  the planted symlink
  left: "# The shop's manifest.\nversion = 1\napp = \"shop.wsgi:application\"\n...\n# learned one query\n"
  right: "victim\n"
test a_stale_regular_file_at_the_temp_name_is_replaced ... FAILED
  panicked at critical_manifest_edit.rs:734: assertion `left == right` failed: the write went into
  the stale temp file instead of replacing it
  left: "# The shop's manifest.\nversion = 1\napp = \"shop.wsgi:application\"\n..."
  right: "stale = 1\n"

test result: FAILED. 16 passed; 2 failed; 0 ignored
```

The other 16 tests (including `a_failed_write_removes_its_temp_file`) stay green under the
mutant, so the two new tests are the only oracle for the `O_EXCL` property, exactly the G3 gap.

## `make check`

```console
$ make check
MAKE_CHECK_EXIT=0
guard: OK (10 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 6 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  ...
  report only: 4 breaking change(s) would fail an enforced run
cargo fmt --all -- --check                        # clean
cargo clippy --workspace --all-targets --locked -- -D warnings   # Finished
cargo doc --workspace --no-deps --locked ... RUSTDOCFLAGS=-D warnings   # Finished
cargo test --workspace --all-targets --locked     # all suites ok
...
All checks passed!                                # ruff
Build finished.                                   # zensical --strict
plan OK                                           # tools/plan.py validate
```

`critical-guard` runs report-only without `--node` and lists the three other protected files
that parallel MA.2.2.5/2.2.6 sessions are editing at the same time; per node it authorizes this
file and nothing else:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.2.7
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_edit.rs]
critical-guard: FAILED: 3 protected critical test(s) changed ... without a covering path in node MA.2.2.7
```

(the three failures are the other nodes' files, not this node's; `make check` itself exits 0.)

## Follow-ups

- **`write_atomic` rustdoc drift (wording-only, production file).** `edit.rs` module doc and
  the function doc still say a planted symlink "is refused"; the code refuses the *open*, then
  unlinks the stale temp entry and retries — which is the behaviour these tests pin (write
  succeeds, target untouched). Reword to "removed, never written through" in a separate node;
  `src/edit.rs` is outside this node's `paths`.
