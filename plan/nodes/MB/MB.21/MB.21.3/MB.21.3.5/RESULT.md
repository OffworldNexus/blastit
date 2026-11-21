# Result MB.21.3.5

## Summary

`blastfile::apply_edits` is the surgical `Blastfile` editor of §14.5. It parses the document into the
one CST of §14.1, resolves each proposal to a byte offset the tree names, splices whole lines there,
and re-parses: every comment, blank line, quote style and untouched byte is the author's because it
is never re-serialised. P4 is unchanged and not optional — the edited text is re-parsed, re-lowered
and compared with `BlastfileEdits::apply_to`'s model via `Manifest::equivalent`; a mismatch is
`EditError::Verification` and nothing is written. `write_atomic` (permissions, `fsync`, symlink
refusal) is shared with the retired patcher.

Two actions are offered, exactly the design's §14.5 edit API: `add_view` (above the first
method-unrestricted catch-all, else appended) and `add_query` (merged into the view's `postgres`
block for that store, created at the end of the view when absent). `body`, `use`, `transactions` and
`unrestricted` cannot be written — `ProposedView`/`ProposedGrant` have no field for them (`ADR-0011`;
see Deviations 4).

**Attempt 2 fixed the two blocking findings and four smaller ones.**

1. **Emptiness is decided per shape.** `block_is_empty` treated an `Element::Node` as "no content"
   and was used for both a `GrantBlock` and a `View`, so a legal one-line shape was refused and the
   edit was silently lost: `postgres { query 'a' }` gained `query 'b'` with no separator, and
   `view p GET "/p/" { }` gained `{; postgres { … } }` (the `view` keyword counted as content). A
   `View` is now empty when it holds no `Grant`/`Use`/`Egress`/`Prefork`/`Body`/`Endpoint` child, and
   a `GrantBlock` when it holds no child that is not a brace or trivia (its clauses are bare tokens,
   its statements are `Query` nodes). Both one-line cases — and a one-line `postgres { transactions }`
   — are pinned in `edit_branches.rs`, and both new tests were proved to fail on the old predicate
   (`Verification { section: "the document syntax" }`) before the fix.
2. **P4's no-write half is tested.** `a_postcondition_mismatch_writes_nothing` builds a genuine
   `EditError::Verification` (a document granting an undeclared `analytics` store: `expected` cannot
   carry `unresolved_grants`), drives the same two calls `LearnRun::finish` makes — `apply_edits`,
   then `write_atomic` only on `Ok` — and asserts the file is byte-identical and no temp file is left.
3. **The refusal message names the section that diverged.** `first_difference` compared `before` with
   `expected`, so every non-noop edit answered "the views" and four arms were dead; it now compares the
   model the edited text lowered to (`after`) with the intended one, and the new test asserts the
   unresolved-grant case reports "the top-level keys".
4. **`edit.rs`'s "Both share `ProposedGrant`" is corrected** — the two editors share `write_atomic`
   and `EditError` only; each has its own `ProposedGrant`.
5. **Byte preservation is asserted on the full document.** `comments_quoting_and_untouched_lines_survive_an_edit`
   now ends with one `assert_eq!(edited, SOURCE_EDITED)`, not only per-line `contains` checks.
6. **`--learn` never writes `body` is tested** (`an_edit_never_writes_a_body_directive`): a document
   with no `body` gains none, and a document that declares `body json` keeps exactly that line.

**Attempt 1** (kept for the record) finished the leaf after the first attempt hit its step cap:

1. **Clippy cleanup completed.** The duplicate `grants:` field in `tests/edit_branches.rs` (a stray
   `grants: Vec::new(),` after the real one) was the only `needless_update`/`E0062` site;
   `cargo clippy -p blastit-manifest --all-targets -- -D warnings` is now clean, `cargo fmt --all`
   applied.
2. **Lead ruling on §4.3 implemented properly.** `Route::is_unrestricted_catch_all` now accepts the
   lowered `"/*"` → `"/.*"` as well as `".*"` and `"^.*$"`, with a unit test covering the three
   accepted spellings, three refused ones and the method-restricted case. The local third-spelling
   workaround (`is_catch_all` in `write.rs`) is **deleted**; the editor calls the predicate.
3. **`critical_learn_executor_blocks.rs`'s module doc** now names
   `blastit_manifest::blastfile::apply_edits`, the function the suite actually calls (doc-only).

## Evidence

All commands were run serially from the worktree at `release/0.2.0` (base `8299a72`).

### 1. `cargo test -p blastit-manifest --locked --test critical_manifest_edit` — PASS

```console
$ cargo test -p blastit-manifest --locked --test critical_manifest_edit
running 24 tests
test a_failed_write_removes_its_temp_file ... ok
test a_planted_temp_name_symlink_is_never_written_through ... ok
test a_grant_for_a_view_that_does_not_exist_is_refused_rather_than_attached ... ok
test a_grant_on_an_undeclared_store_names_the_missing_declaration ... ok
test a_method_restricted_catch_all_is_not_an_insertion_point ... ok
test a_stale_regular_file_at_the_temp_name_is_replaced ... ok
test a_redacted_placeholder_is_refused_without_echoing_it ... ok
test a_postcondition_mismatch_writes_nothing ... ok
test a_grant_is_merged_into_the_one_block_for_its_store ... ok
test a_proposed_view_has_to_be_routable ... ok
test a_grant_is_never_written_into_the_other_store_s_block ... ok
test a_grant_lands_in_the_view_and_store_it_was_proposed_for ... ok
test an_unrestricted_grant_cannot_take_exact_queries ... ok
test an_inferred_view_name_cannot_silently_change_a_pattern ... ok
test a_statement_that_reads_like_a_directive_is_never_edited_as_one ... ok
test a_store_named_like_a_directive_still_boots ... ok
test an_edit_never_writes_a_body_directive ... ok
test comments_quoting_and_untouched_lines_survive_an_edit ... ok
test a_grant_joins_an_existing_block_without_rewriting_it ... ok
test an_inline_literal_is_refused_without_echoing_it ... ok
test the_atomic_write_leaves_no_temp_file_and_the_result_parses ... ok
test applying_the_same_edits_twice_changes_nothing ... ok
test a_view_is_inserted_above_both_catch_all_spellings ... ok
test every_edit_fixture_round_trips_through_parse_and_validate ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 2. `cargo test -p blastit-manifest --locked --test critical_manifest_edit_executor` — PASS

```console
$ cargo test -p blastit-manifest --locked --test critical_manifest_edit_executor
running 5 tests
test malformed_executor_state_is_refused_before_any_edit ... ok
test an_inserted_view_does_not_disturb_the_static_mount ... ok
test an_executor_document_is_edited_only_on_its_query_lines ... ok
test a_view_without_a_mount_never_gains_one ... ok
test a_learned_query_keeps_static_and_entrypoint_byte_for_byte_on_disk ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 3. `cargo test -p blastit-manifest --locked --test edit_branches` — PASS

```console
$ cargo test -p blastit-manifest --locked --test edit_branches
running 22 tests
test a_method_that_is_not_a_method_is_refused_before_the_document ... ok
test a_missing_final_newline_still_separates_the_appended_view ... ok
test a_comment_run_that_already_has_a_blank_line_above_it_keeps_exactly_one ... ok
test a_block_without_a_query_clause_gets_one ... ok
test a_block_with_several_clauses_is_extended_after_them ... ok
test a_crlf_document_keeps_its_line_endings ... ok
test a_comment_inside_the_block_stays_above_the_inserted_query ... ok
test a_path_without_a_file_name_is_refused_before_anything_is_written ... ok
test a_write_into_an_unwritable_directory_reports_permission_denied ... ok
test a_new_view_goes_above_the_comment_run_that_documents_the_catch_all ... ok
test a_new_block_is_separated_from_the_views_own_fields ... ok
test a_view_with_no_methods_is_written_without_a_method_list ... ok
test an_empty_pattern_is_refused_by_its_rule_index ... ok
test a_view_without_a_block_for_that_store_gets_one_at_the_end ... ok
test an_empty_statement_is_refused_without_quoting_it ... ok
test duplicate_proposed_view_names_are_refused ... ok
test a_bare_file_name_is_written_into_the_process_directory ... ok
test a_query_that_reads_like_a_directive_is_never_edited_as_one ... ok
test a_one_line_grant_block_gains_a_separator_before_the_new_statement ... ok
test a_one_line_view_gets_its_new_block_inside_the_braces ... ok
test two_proposals_for_one_new_view_share_a_single_block ... ok
test two_proposed_views_are_written_in_order_with_a_blank_line_between_them ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 4. `cargo test -p blastit-manifest --locked` — PASS (32 suites, 524 tests)

```console
$ cargo test -p blastit-manifest --locked
… (32 `Running`/`Doc-tests` blocks)
$ grep -c '^test result: ok'  → 32
$ grep -c 'test result: FAILED' → 0
$ (sum of passed) → 524
```

### 5. The new `model.rs` unit test — PASS

```console
$ cargo test -p blastit-manifest --locked --lib the_lowered_catch_all_is_unrestricted_too
running 1 test
test model::tests::the_lowered_catch_all_is_unrestricted_too ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 154 filtered out; finished in 0.00s
```

It asserts `".*"`, `"/.*"` and `"^.*$"` are unrestricted; `"/products/.*"`, `"/"` and `[]` are not;
and `/.*` with `methods = [GET, HEAD]` (and `.*` with `[GET]`) is not — the same allowed/denied pair
shape the protected `critical_manifest_011.rs` uses.

### 6. `cargo test -p blastit --locked --test critical_learn_executor_blocks` — PASS

```console
$ cargo test -p blastit --locked --test critical_learn_executor_blocks
running 2 tests
test a_learn_run_that_saw_nothing_writes_nothing ... ok
test a_learned_query_keeps_the_mount_and_the_entrypoint_on_disk ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 7. `make rs-fmt` — PASS

```console
$ make rs-fmt
cargo fmt --all -- --check
EXIT=0
```

### 8. `make rs-lint` — PASS

```console
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.69s
EXIT=0
```

### 9. `make rs-doc` — PASS

```console
$ make rs-doc
… RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
 Documenting blastit-manifest v0.2.0 (…/crates/blastit-manifest)
 Documenting blastit v0.2.0 (…/crates/blastit)
 Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.42s
   Generated …/target/doc/blastit/index.html and 13 other files
EXIT=0
```

### 10. `make check` — PASS (exit 0)

```console
$ make check
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
…
     Running tests/critical_manifest_edit.rs
running 24 tests
… test result: ok. 24 passed; 0 failed; …
     Running tests/critical_manifest_edit_executor.rs
running 5 tests
… test result: ok. 5 passed; 0 failed; …
     Running tests/edit_branches.rs
running 22 tests
… test result: ok. 22 passed; 0 failed; …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 17 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 17 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c 'test result: ok'` = 161, `grep -c 'test result: FAILED'` = 0, and the passed counts sum to
3187. Run twice this attempt (before and after the doc-comment fix in item 3 of the Attempt 1 list),
plus once in attempt 1; all exit 0, and the `blastit-view` frame-budget test did not flake on any run.

### 11. `critical_guard.py --base 8299a72… --node MB.21.3.5` — PASS

```console
$ uv run --no-project tools/critical_guard.py --base 8299a7287dec76deb1835db3caf55e8ca854dee2 --node MB.21.3.5
critical-guard: 8299a72…76deb1835db3caf55e8ca854dee2..working tree, 14 changed file(s)
node MB.21.3.5: paths ['crates/blastit-manifest/src/blastfile/**', …]
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by …]
  M  crates/blastit-manifest/tests/critical_manifest_edit_executor.rs  [authorized by …]
  M  crates/blastit/tests/critical_learn_executor_blocks.rs  [authorized by …]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.21.3.5
```

## The four rewritten suites: old → new

Every row is a **document-format** change. No assertion was weakened: where a test was renamed the
assertions are the same claim in `Blastfile` vocabulary, where a row says "stronger" the new test
asserts strictly more than the old one, and the rows marked "new (attempt 2)" were added by the
review's findings rather than by the format change.

### `crates/blastit-manifest/tests/critical_manifest_edit.rs` (23 → 24)

| Old | New | Why |
|---|---|---|
| `comments_quoting_and_untouched_lines_survive_an_edit` | same name | fixture is a `Blastfile`; **stronger**: the per-line `contains` checks are kept and one `assert_eq!(edited, SOURCE_EDITED)` now pins the whole document (acceptance 1) |
| `a_route_is_inserted_above_both_catch_all_spellings` | `a_view_is_inserted_above_both_catch_all_spellings` | `"/*"` and `".*"` are the two spellings now |
| `a_method_restricted_catch_all_is_not_an_insertion_point` | same name | unchanged claim |
| `applying_the_same_edits_twice_changes_nothing` | same name | unchanged claim (idempotence) |
| `a_grant_joins_an_existing_queries_list_without_rewriting_it` | `a_grant_joins_an_existing_block_without_rewriting_it` | `queries = […]` → `postgres { query … }` |
| `every_edit_fixture_round_trips_through_parse_and_validate` | same name | `parse_and_validate` → `parse_blastfile` |
| `a_proposed_route_has_to_be_routable` | `a_proposed_view_has_to_be_routable` | vocabulary |
| `an_inline_literal_is_refused_without_echoing_it` | same name | unchanged claim (`R24`) |
| `a_redacted_placeholder_is_refused_without_echoing_it` | same name | unchanged claim (`R24`) |
| `a_grant_on_an_undeclared_proxy_names_the_missing_declaration` | `a_grant_on_an_undeclared_store_names_the_missing_declaration` | proxy → store |
| `an_unrestricted_grant_cannot_take_exact_queries` | same name | unchanged claim |
| `an_inferred_route_name_cannot_silently_change_a_path` | `an_inferred_view_name_cannot_silently_change_a_pattern` | path regex → pattern |
| `an_inline_grant_table_is_named_rather_than_duplicated` | `a_grant_is_merged_into_the_one_block_for_its_store` | Blastfile has no inline-table spelling; the claim "never a second block for one store" is the same one |
| `a_grant_on_a_reserved_proxy_name_still_boots` | `a_store_named_like_a_directive_still_boots` | reserved names are now directives |
| `the_atomic_write_leaves_no_temp_file_and_the_result_parses` | same name | unchanged claim |
| `a_failed_write_removes_its_temp_file` | same name | unchanged claim |
| `a_planted_temp_name_symlink_is_never_written_through` | same name | unchanged claim |
| `a_stale_regular_file_at_the_temp_name_is_replaced` | same name | unchanged claim |
| `a_grant_lands_in_the_route_and_proxy_it_was_proposed_for` | `a_grant_lands_in_the_view_and_store_it_was_proposed_for` | vocabulary; also absorbs the next row |
| `a_table_that_closes_the_previous_element_never_receives_the_grant` | **merged** into the row above | a braced block cannot "close the previous element", so the hazard is structural now; the surviving test still asserts the neighbour's block is untouched, the grant is not copied, and the new block is inside the proposed view |
| `a_grant_is_never_written_into_the_other_proxy_s_table` | `a_grant_is_never_written_into_the_other_store_s_block` | vocabulary |
| `a_grant_for_a_route_that_does_not_exist_is_refused_rather_than_attached` | `a_grant_for_a_view_that_does_not_exist_is_refused_rather_than_attached` | vocabulary |
| `a_statement_that_reads_like_an_assignment_is_never_edited_as_one` | `a_statement_that_reads_like_a_directive_is_never_edited_as_one` | `queries =` → a directive/block header inside a statement |
| — | `a_postcondition_mismatch_writes_nothing` | **new (attempt 2)**: P4's no-write half — a genuine `EditError::Verification`; the file is byte-identical after the caller's `apply_edits` + write-on-`Ok` path, and no temp file is left |
| — | `an_edit_never_writes_a_body_directive` | **new (attempt 2)**: §14.5/`ADR-0011` — no `body` line is written, and an author's `body json` survives exactly once |

### `crates/blastit-manifest/tests/critical_manifest_edit_executor.rs` (5 → 5)

| Old | New | Why |
|---|---|---|
| `a_learned_query_keeps_php_and_static_blocks_byte_for_byte_on_disk` | `a_learned_query_keeps_static_and_entrypoint_byte_for_byte_on_disk` | `[route.php]` → `entrypoint php`; the mount assertion is unchanged |
| `an_executor_route_is_edited_only_on_its_queries_line` | `an_executor_document_is_edited_only_on_its_query_lines` | `queries =` line → `query` lines |
| `a_route_without_an_executor_block_never_gains_one` | `a_view_without_a_mount_never_gains_one` | "executor block" is now the document's `static`/`entrypoint` |
| `an_inserted_route_does_not_disturb_an_executor_route` | `an_inserted_view_does_not_disturb_the_static_mount` | vocabulary |
| `malformed_executor_state_is_refused_before_any_edit` | same name | unchanged claim |

### `crates/blastit-manifest/tests/edit_branches.rs` (19 → 22)

| Old | New | Why |
|---|---|---|
| `a_list_spread_over_several_lines_is_written_back_on_one` | `a_block_with_several_clauses_is_extended_after_them` | a wrapped `queries` list → a block with `transactions`/`select`/`query`; asserts the clauses are byte-identical **and** `transaction_control`/table grants survive (stronger) |
| `a_one_line_list_keeps_the_authors_trailing_comment` | `a_comment_inside_the_block_stays_above_the_inserted_query` | the author's comment now lives inside the block |
| `an_empty_list_and_a_list_the_author_ended_with_a_comma_both_extend_cleanly` | `a_block_without_a_query_clause_gets_one` | empty list → empty block. The trailing-comma half has no twin: the `Blastfile` parser refuses a trailing comma (`critical_blastfile_parser.rs`: "a list has no trailing comma"), so the branch cannot exist |
| `a_grant_table_without_an_assignment_gets_one_written_under_its_header` | `a_block_without_a_query_clause_gets_one` (same row) | a header with no assignment and an empty list are one shape in `Blastfile` |
| `a_new_grant_table_goes_after_the_routes_last_line_not_after_its_blank_run` | `a_new_block_is_separated_from_the_views_own_fields` | the new block follows `use catalogue`, not a blank run |
| `a_grant_table_after_a_foreign_table_still_belongs_to_its_route` | `a_view_without_a_block_for_that_store_gets_one_at_the_end` | a foreign root table is now a second store; asserts both blocks' contents |
| `a_new_route_goes_above_the_comment_run_that_documents_the_catch_all` | `a_new_view_goes_above_the_comment_run_that_documents_the_catch_all` | vocabulary; same blank-line-exactly-one assertion |
| `a_bracket_inside_a_quoted_statement_does_not_hide_the_next_route` | `a_query_that_reads_like_a_directive_is_never_edited_as_one` | the trap is now `SELECT 'view fake /* {'`; asserts the view order, one block, and the statement byte-identical (stronger) |
| `two_proposed_routes_are_written_in_order_with_a_blank_line_between_them` | `two_proposed_views_are_written_in_order_with_a_blank_line_between_them` | vocabulary |
| `duplicate_proposed_route_names_are_refused` | `duplicate_proposed_view_names_are_refused` | vocabulary |
| `an_empty_pattern_is_refused_by_its_rule_index` | same name | unchanged claim (`patterns[1]`) |
| `an_empty_statement_is_refused_without_quoting_it` | same name | unchanged claim (`queries[0]`) |
| `a_path_without_a_file_name_is_refused_before_anything_is_written` | same name | unchanged claim |
| `two_proposals_for_one_new_route_share_a_single_grant_table` | `two_proposals_for_one_new_view_share_a_single_block` | vocabulary |
| `a_line_that_looks_like_an_assignment_inside_a_string_is_not_the_assignment` | **merged** into `a_query_that_reads_like_a_directive_is_never_edited_as_one` | the statement is now an element of the tree, so "text that looks like structure" is one branch |
| `a_replaced_list_does_not_add_a_newline_the_file_did_not_have` | `a_missing_final_newline_still_separates_the_appended_view` | same hazard, plus the appended view still ends the file |
| `a_bare_file_name_is_written_into_the_process_directory` | same name | unchanged claim |
| `a_write_into_an_unwritable_directory_reports_permission_denied` | same name | unchanged claim |
| `a_comment_run_that_already_has_a_blank_line_above_it_keeps_exactly_one` | same name | unchanged claim |
| — | `a_method_that_is_not_a_method_is_refused_before_the_document` | **new**: `get` is refused with `EditError::InvalidMethod` before any splice (Blastfile-only rule) |
| — | `a_crlf_document_keeps_its_line_endings` | **new**: the NODE's CRLF branch; no mixed line endings, both edits land |
| — | `a_view_with_no_methods_is_written_without_a_method_list` | **new**: the documented header shape for "every method" |
| — | `a_one_line_grant_block_gains_a_separator_before_the_new_statement` | **new (attempt 2)**: a one-line `postgres { query 'a' }` is content, so the new statement gets a `;` (proved to fail on the old predicate); also pins that a bare-token clause (`postgres { transactions }`) is content, so a future "nodes only" simplification cannot regress it |
| — | `a_one_line_view_gets_its_new_block_inside_the_braces` | **new (attempt 2)**: a `view`'s header is not content — an empty one-line view takes the block directly, and one that holds `body json` gets the `;` it needs |

### `crates/blastit/tests/critical_learn_executor_blocks.rs` (1 → 2)

| Old | New | Why |
|---|---|---|
| `dev_learn_writes_the_query_and_keeps_the_static_block_byte_for_byte` | `a_learned_query_keeps_the_mount_and_the_entrypoint_on_disk` | drives `blastfile::apply_edits` + `write_atomic` (the two calls `LearnRun::finish` makes) on a `Blastfile`; asserts the mount lines, the `entrypoint`, the head/tail split around the catch-all comment, the learned `$?` spelling, the model, and idempotence (stronger) |
| — | `a_learn_run_that_saw_nothing_writes_nothing` | **new**: an empty edit set returns the document unchanged and the write leaves it byte-identical |

The module doc was also corrected to name `blastit_manifest::blastfile::apply_edits` (doc only).

## Files

| File | Change |
|---|---|
| `crates/blastit-manifest/src/blastfile/write.rs` | **new**: the CST editor — `apply`, `Plan::build`, `expected` (P4 oracle), catch-all insertion, block extension, quoting, verification |
| `crates/blastit-manifest/src/blastfile/mod.rs` | `pub(crate) mod write`, `ProposedView`, `ProposedGrant`, `BlastfileEdits` (+ `apply_to`), `pub fn apply_edits` |
| `crates/blastit-manifest/src/blastfile/lower.rs` | `DEFAULT_STORE` is `pub(crate)` so the editor writes the same unnamed-store name the reader does |
| `crates/blastit-manifest/src/edit.rs` | module docs describe the two coexisting editors; `pub(crate) trait Editor` + `BlastfileEditor`; `ManifestEdits`/`apply_edits` untouched |
| `crates/blastit-manifest/src/model.rs` | `Route::is_unrestricted_catch_all` accepts `"/.*"` (§4.3) + unit test |
| `crates/blastit-manifest/tests/critical_manifest_edit.rs` | rewritten to `Blastfile` (table above) |
| `crates/blastit-manifest/tests/critical_manifest_edit_executor.rs` | rewritten to `Blastfile` (table above) |
| `crates/blastit-manifest/tests/edit_branches.rs` | rewritten to `Blastfile` (table above) |
| `crates/blastit/tests/critical_learn_executor_blocks.rs` | rewritten to `Blastfile`, second test added (table above) |

No `Cargo.toml`/`Cargo.lock` change. No `unwrap`/`expect`/`panic`/`todo`/indexing outside
`#[cfg(test)]` (the suites' `#![allow(clippy::unwrap_used, clippy::expect_used)]` is unchanged from
before this leaf).

## Deviations

1. **Two edit types coexist.** The editor is `blastfile::apply_edits` over `blastfile::BlastfileEdits`;
   `edit::{ManifestEdits, apply_edits}` keeps the TOML patcher alive. Renaming `edit::ManifestEdits`
   would break the struct literals in the protected `crates/blastit/tests/critical_check_engine.rs`,
   and cutting `edit::apply_edits` over would break `check.rs`, `dev_learn.rs` and `audit_v1.rs` —
   all `MB.21.4.3`'s files. **Lead ruling: accepted**; 4.3 renames the entry point, 4.5 deletes the
   TOML reader.
2. **The catch-all predicate accepts the lowered `"/.*"`.** §4.3 assigns that to this pair, so
   `model.rs` was added to this leaf's `paths` and the predicate is fixed properly with a unit test
   instead of the local third-spelling hack, which is deleted. **Lead ruling: required**; done here,
   no longer a follow-up.
3. **`critical_learn_executor_blocks.rs` drives the editor directly.** `crates/blastit/src/dev_learn.rs`
   still reads the retired TOML document until `MB.21.4.3`, so the suite calls the same two functions
   `LearnRun::finish` calls (`apply_edits` + `write_atomic`) on a `Blastfile`; the end-to-end
   `blastit dev --learn` driver returns with that leaf. **Lead ruling: accepted.**
4. **The editor's API is `views` + `grants`, not `add_trait` + `use` / `add_body`.** §14.5's edit API
   is exactly `ProposedView { name, methods, patterns }`, `ProposedGrant { view, store, queries }` and
   `ManifestEdits { views, grants }`, and it states that the editor "can[not] write `unrestricted`,
   `use` or `transactions` (today's rule)"; `body` is structurally out of reach (`ADR-0011`). The
   NODE's Context line naming `add_trait` + `use` and `add_body` is the brief §21 already flags as
   superseded ("the accepted page keeps `add_group`/`add_route`/`declare_ingress`"), so the editor
   implements the design's two actions rather than inventing two more. The connection-setup fix
   (`add_group` → `trait connection`) is `MB.21.4.3`'s, which owns the `--json` action vocabulary.

## Follow-ups

- `MB.21.4.3`: rename `blastfile::apply_edits` to `edit::apply_edits`, move `dev_learn.rs`, `check.rs`
  and `audit_v1.rs` onto it, and merge `BlastfileEdits`/`ManifestEdits` (which also unblocks the
  `critical_check_engine.rs` struct literals). It also owns the `add_group` → `trait connection` fix
  and the `--json` action names (§13.3, §21).
- `MB.21.4.5`: delete `edit::apply_edits`, `ManifestEdits`, `ProposedRoute` and `Manifest::from_toml_str`
  with the reader.
- Design TS-21's "an edit's output contains no `body` line" oracle is now
  `critical_manifest_edit.rs::an_edit_never_writes_a_body_directive`, so it is no longer a gap.
