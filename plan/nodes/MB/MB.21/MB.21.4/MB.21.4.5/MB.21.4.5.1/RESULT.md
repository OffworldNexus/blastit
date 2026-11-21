# RESULT MB.21.4.5.1

STATUS: done

## Summary

- Deleted `crates/blastit-manifest/tests/parse.rs` (1112 lines, 39 `#[test]`s) with its subject.
- Translated `crates/blastit-manifest/tests/critical_manifest_011.rs` to `Manifest::from_blastfile_str` /
  `Manifest::parse_blastfile`: all 17 tests kept, assertion lines 62 -> 70, case count (assertion lines)
  at or above the old 29 fixtures. Its first doc line no longer names the retired file and the name
  appears nowhere in the file.
- Added three cases to `tests/blastfile_errors.rs`, each covering a fact no surviving suite carried at
  the non-critical tier:
  1. a document that is not a Blastfile (the retired `version = 1` opening) is an ordinary positioned
     syntax error, not a detection/conversion (§15.1, `R16`);
  2. `Manifest::from_blastfile_str("")` → `route_count() == 0`, empty `mounts()`, `php()` `None`,
     `name()` `None` (§7.1's three dropped rows, on the shortest document — attempt 2, finding 1);
  3. two mounts sharing a prefix are refused as `ManifestError::DuplicateMount`, the refusal that
     replaced the retired "second block never answers" fact (attempt 2, finding 2/row 37).
- No `ManifestError::MissingVersion` / `UnsupportedVersion` is constructed. No source, docs,
  `Cargo.toml`/`Cargo.lock` or `tests/common/mod.rs` file was touched.

Diff shape (the deletion dominates, as the brief predicted):

```console
$ git diff --stat -- crates/blastit-manifest/tests/parse.rs \
      crates/blastit-manifest/tests/critical_manifest_011.rs \
      crates/blastit-manifest/tests/blastfile_errors.rs
 crates/blastit-manifest/tests/blastfile_errors.rs  |   54 +
 .../tests/critical_manifest_011.rs                 |  609 ++++++-----
 crates/blastit-manifest/tests/parse.rs             | 1112 --------------------
 3 files changed, 381 insertions(+), 1394 deletions(-)
```

## Review response (attempt 2)

| Finding | Fix |
|---|---|
| 1 [MAJOR] row 38 cited `critical_blastfile_parity.rs::the_dropped_rows_are_absent`, which never asserts empty `mounts()` or `name() == None` | Added `blastfile_errors.rs::an_empty_document_parses_with_no_mount_no_front_controller_and_no_name` asserting all four facts on `Manifest::from_blastfile_str("")`, and re-worded row 38 to cite it first and say exactly what the parity test adds. `critical_blastfile_parity.rs` was not edited (not this node's path). |
| 2 [MINOR] row 37 named a nonexistent `DuplicateStaticMount` | Row 37 now names `ManifestError::DuplicateMount` (`validate.rs:626`) and cites the new `blastfile_errors.rs::a_duplicate_mount_prefix_is_refused`, which asserts that variant (`prefix "/x"`, mounts 0 and 1) — the before/after claim is now checked, not just stated. |
| 3 [MINOR] row 33 lost the `functions = "lower"` / `functions = [1]` typed refusals | Row 33 now carries `no successor: functions = "lower" / functions = [1]` with the one-line why (a function name is a `Word`, so no value can attach and `WrongType` has nothing to type). |

## Acceptance evidence

### 1. `test ! -e crates/blastit-manifest/tests/parse.rs`

```console
$ test ! -e crates/blastit-manifest/tests/parse.rs; echo exit=$?
exit=0
```

### 1 (continued). `grep -rnE 'from_toml_str|toml::' …`

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/critical_manifest_011.rs crates/blastit-manifest/tests/blastfile_errors.rs; echo exit=$?
exit=1
```

### 1 (continued). `grep -rnE 'blastit[.]toml' …`

```console
$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/critical_manifest_011.rs crates/blastit-manifest/tests/blastfile_errors.rs; echo exit=$?
exit=1
```

### 2. Targeted suites

```console
$ cargo test -p blastit-manifest --test critical_manifest_011 --test blastfile_errors
     Running tests/blastfile_errors.rs (…)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/critical_manifest_011.rs (…)
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Case count before -> after, measured as `assert*` lines in `critical_manifest_011.rs` (the old file
had 17 tests / 29 measured fixtures / 62 assertion lines):

```console
before asserts: 62      after asserts: 70
before #[test]: 17      after #[test]: 17
```

Assertions whose *subject* changed because the language has no spelling for the old construct are
listed under "Translated assertions" below; none was dropped and none was weakened.

### 3. `cargo test -p blastit-manifest`

```console
$ cargo test -p blastit-manifest
test result: ok. 156 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (blastfile_errors)
… (31 suite binaries, every line `0 failed`)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (doc-tests)
exit=0
```

No suite was left red. `cargo fmt --all -- --check` prints nothing; `cargo clippy -p blastit-manifest
--all-targets --locked -- -D warnings` finishes clean. `make check` is the lead's.

### 4. `make critical-guard BASE=2ef786ce381dce05208d8d4d09a50f8eb4d7589b NODE=MB.21.4.5.1`

Two runs, because siblings have protected files modified in this shared worktree and the guard diffs
the whole working tree:

```console
$ make critical-guard BASE=2ef786ce381dce05208d8d4d09a50f8eb4d7589b NODE=MB.21.4.5.1
critical-guard: 2ef786ce381dce05208d8d4d09a50f8eb4d7589b..working tree, 26 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_011.rs]
  M  crates/blastit-manifest/tests/critical_php_manifest.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_static_manifest.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_charge.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 5 protected critical test(s) changed or deleted without a covering path in node(s) MB.21.4.5.1.
make: *** [Makefile:128: critical-guard] Error 1
```

The five violations are in-flight sibling edits I did not touch: the two `blastit-manifest` files are
`MB.21.4.5.2`'s (its `NODE.md` owns both paths), the three `blastit-scheduler` files are a scheduler
leaf's (`crates/blastit-scheduler/**` is not any manifest node's path). Run against the base plus **only
this node's diff** (a throwaway `git clone --shared` checked out at the base, with this node's files
copied in), the same command is clean:

```console
$ cd /tmp/opencode/mb21451-guard2   # clone at 2ef786ce, this node's 3 files applied
$ git status --porcelain
 M crates/blastit-manifest/tests/blastfile_errors.rs
 M crates/blastit-manifest/tests/critical_manifest_011.rs
 D crates/blastit-manifest/tests/parse.rs
$ uv run --no-project tools/critical_guard.py --base 2ef786ce381dce05208d8d4d09a50f8eb4d7589b --node MB.21.4.5.1
critical-guard: 2ef786ce381dce05208d8d4d09a50f8eb4d7589b..working tree, 3 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_011.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.4.5.1
```

`parse.rs` is not a protected path (the guard protects `critical_*`/`tests/critical/**`), so the
deletion is not a guard change; the only protected change in this node is the translated suite, and
it is authorized by the node's own `paths`.

## Deleted-refusal table (one row per `#[test]` in `tests/parse.rs`)

39 rows. A row either names the surviving test that proves the same policy, or says
`no successor: <spelling the file does not have>` with the one-line reason.

| # | Deleted `parse.rs` test | Now proved by / reason |
|---|---|---|
| 1 | `section_6_example_parses` | `critical_blastfile_parity.rs::the_corpus_is_the_fifteen_rows_and_every_one_lowers` (proxies, traits, grants, routes, methods, prefork/egress, all as `row_*`) |
| 2 | `section_5_1_two_proxies_parse` | `critical_blastfile_parity.rs::row_a_store_declared_by_its_url_variable` + `::row_unrestricted_on_the_default_and_a_named_store` |
| 3 | `unknown_keys_are_errors` | `critical_blastfile_parser.rs::the_refusal_matrix` (`body json` at top level; `view v { json }`) |
| 4 | `unknown_grant_keys_are_errors` | `critical_manifest_011.rs::a_reserved_key_inside_a_grant_body_is_not_an_unknown_field` (`allow_everything`, `egress` in a grant block) + `blastfile_traits.rs::egress_inside_a_trait_is_refused` |
| 5 | `grant_contents_are_typed` | `no successor: queries = "SELECT 1" / transaction_control = "yes"` — a grant is a list of directives, not typed values; there is no value to mistype. |
| 6 | `version_is_required_and_must_be_one` | `no successor: version = 1` — the file has no header or version line; the two version variants are deleted with the reader (5.6). |
| 7 | `protocol_is_required_and_known` | `no successor: protocol = "redis"` — the store keyword *is* the protocol, so a protocol this build lacks has no spelling. |
| 8 | `a_broken_proxy_declaration_is_not_reported_again_as_an_unknown_proxy` | `no successor: [proxy.cache] protocol = "redis"` — a store declaration cannot half-exist; a duplicate/undeclared store is `DuplicateProxyName` / `GrantOnUnknownProxy` (`blastfile_grants.rs::a_grant_on_an_undeclared_store_is_refused`). |
| 9 | `input_names_must_not_be_empty` | `no successor: input = { url = "" }` — a `$IDENT` is at least one character by grammar, so an empty slot name cannot be written. |
| 10 | `a_route_path_may_be_one_string_or_a_list_of_them` | `blastfile_patterns.rs::a_literal_path_matches_exactly` + `critical_blastfile_parity.rs::row_several_patterns` |
| 11 | `route_fields_are_required_and_typed` | `critical_blastfile_parser.rs::the_refusal_matrix` (no endpoint; methods but no pattern; `GET` as a name) + `blastfile_entrypoint.rs::egress_has_one_spelling` |
| 12 | `an_unknown_route_key_that_is_not_a_table_is_an_error` | `critical_blastfile_parser.rs::the_refusal_matrix` + `critical_manifest_011.rs::a_reserved_key_inside_a_grant_body_is_not_an_unknown_field` |
| 13 | `broken_toml_is_a_syntax_error_with_a_position` | `blastfile_errors.rs::a_document_that_is_not_a_blastfile_gets_an_ordinary_syntax_error` (the parser-level case added by this node) |
| 14 | `syntax_errors_after_non_ascii_text_stay_on_one_line` | `blastfile_errors.rs::every_refusal_is_one_line` + `src/blastfile/lexer.rs` `mod tests::non_ascii_bytes` |
| 15 | `control_characters_in_names_and_keys_are_escaped` | `blastfile_errors.rs::a_control_byte_is_escaped` + `critical_blastfile_errors.rs::a_value_is_escaped_and_cut` |
| 16 | `long_patterns_are_cut_short_in_messages_but_kept_in_the_error` | `blastfile_patterns.rs::a_pattern_is_capped_at_4096_bytes` + `critical_blastfile_errors.rs::the_regex_message_is_escaped_and_cut` |
| 17 | `every_problem_is_collected_and_listed` | `blastfile_errors.rs::the_error_list_is_one_line_per_problem` — validation still lists every problem; the parser refuses at the first syntax error (`R21`). The count half: `+ blastfile_errors.rs::the_two_entry_points_differ_by_validation` (asserts `errors.len() == 2` on the same fixture), and `critical_blastfile_errors.rs::the_list_is_one_line_per_problem` asserts `errors.len() == 3`, so "collects every problem" is proved on disk two ways. |
| 18 | `tables_that_are_not_tables_are_reported` | `no successor: proxy = "db" / route = 1` — the file has no tables and no typed values. |
| 19 | `empty_proxy_and_group_names_are_reported` | `critical_blastfile_parser.rs::the_refusal_matrix` (`_shop` is not a name) + `blastfile_entrypoint.rs::the_app_name_is_carried_and_bounded` (empty `app`) |
| 20 | `a_manifest_needs_nothing_but_a_version` | `no successor: version = 1` as the only line — the file has no header and no required section; the empty and static-only files are different documents. |
| 21 | `the_fixtures_are_the_documented_text` | `no successor: SECTION_5_1 / SECTION_6` — those constants were the retired documents the deleted suite imported from `tests/common/mod.rs`; the live corpus is `critical_blastfile_parity.rs`. |
| 22 | `an_inline_unrestricted_grant_parses` | `blastfile_grants.rs::unrestricted_is_a_view_level_grant` + `critical_blastfile_parity.rs::row_an_unrestricted_grant` |
| 23 | `a_group_may_be_parsed_with_unrestricted_because_validation_refuses_it` | `blastfile_traits.rs::unrestricted_inside_a_trait_is_refused` |
| 24 | `unrestricted_must_be_a_boolean` | `blastfile_grants.rs::unrestricted_takes_no_block` — the verb takes no value (`unrestricted = "yes"` cannot be written). |
| 25 | `table_grants_parse` | `blastfile_grants.rs::select_takes_a_list_or_every_column` + `::insert_and_update_take_a_list_or_every_column` + `::delete_is_a_whole_table_flag` + `::two_operations_on_one_table_are_one_entry` |
| 26 | `a_table_grant_may_name_another_schema` | `blastfile_grants.rs::functions_are_canonicalised` for the `pg_catalog.` half; `no successor: [route.db.tables."audit.users"]` — a table name is a `Word`, so a schema-qualified table cannot be written (design §13.3 records `UnwritableGrantName`). |
| 27 | `an_unknown_table_operation_is_refused` | `critical_manifest_011.rs::a_reserved_key_inside_a_grant_body_is_not_an_unknown_field` (the `upsert` row) |
| 28 | `a_column_grant_is_a_star_or_a_list` | `blastfile_grants.rs::select_takes_a_list_or_every_column` (+ `insert_and_update_take_a_list_or_every_column`) |
| 29 | `a_star_inside_a_column_list_is_refused` | `no successor: select products("*")` — a column list holds names, `*` is not a `Word`, and the whole table is the bare form `select products`. |
| 30 | `an_empty_table_or_column_name_is_refused` | `no successor: select ""` — a name is at least one `Word`; an empty name cannot be written. |
| 31 | `delete_must_be_a_boolean` | `no successor: delete = false` — a directive takes no value; `delete false` would grant a table called `false`. |
| 32 | `a_table_grant_must_be_a_table_of_operations` | `no successor: tables = { … }` — a table grant is the `select`/`insert`/`update`/`delete` directive, not a value. |
| 33 | `functions_must_be_a_list_of_names` | `blastfile_grants.rs::functions_are_canonicalised` (a set; a repeated name is one entry); `no successor: functions = "lower" / functions = [1]` — a function name is a `Word`, so no value can attach and `WrongType` has nothing to type. |
| 34 | `fastcgi_is_an_ordinary_unknown_route_key_and_the_php_twin_parses` | `no successor: [route.fastcgi]` — the key died with the retired format (`ADR-0010`); the php twin is `blastfile_entrypoint.rs::php_fills_the_one_front_controller`. |
| 35 | `a_static_block_fills_the_mount_list_beside_the_route` | `critical_blastfile_parity.rs::row_a_mount_above_a_view` + `critical_static_manifest.rs::the_pages_mount_parses_with_the_defaults_it_omits` |
| 36 | `a_php_block_fills_the_manifest_front_controller` | `critical_blastfile_parity.rs::row_a_mount_beside_a_front_controller` + `blastfile_entrypoint.rs::php_fills_the_one_front_controller` |
| 37 | `a_second_block_of_the_same_kind_never_answers` | php half: `blastfile_entrypoint.rs::a_second_entrypoint_is_refused` (one front controller). static half: `no successor: two per-route static blocks` — a mount is a top-level `static` addressed by prefix, so "first wins" became the duplicate-prefix refusal `ManifestError::DuplicateMount` (`validate.rs:626`), asserted by `blastfile_errors.rs::a_duplicate_mount_prefix_is_refused`. |
| 38 | `a_plain_document_fills_no_mount_no_front_controller_and_no_name` | `blastfile_errors.rs::an_empty_document_parses_with_no_mount_no_front_controller_and_no_name` (`route_count() == 0`, empty `mounts()`, `php()` `None`, `name()` `None` on `Manifest::from_blastfile_str("")`); `critical_blastfile_parity.rs::the_dropped_rows_are_absent` covers the rest of §7.1 on a real fixture (`version`, empty `global_groups`, no per-route `static`/`php`, `php()` `None`). |
| 39 | `every_retired_route_is_its_own_sandbox` | `critical_blastfile_parser.rs::a_half_megabyte_of_views_lowers` (sandbox == view index) + `blastfile_body.rs::an_endpoint_body_wins_over_the_view_body` (one view, one sandbox) |

## Translated assertions (disclosed, not weakened)

Every one of these keeps the policy assertion; only the half whose *construct* the language does not
have was re-spelled, exactly as the brief allows for R8:

- `app_and_preload…`: the old `app` is now `entrypoint python`; the old "app None + preload list"
  case (possible in TOML because the two keys were independent) is split into the same two facts:
  preload keeps order/dedups, and `app` is optional when no entrypoint is declared.
- `app_must_be_module_attribute`: the empty-callable half moves from TOML `app = ""` (`Empty`) to
  `entrypoint python ""`, which lowers and validates to `InvalidApp { app: "" }` — still carried,
  never silently treated as absent.
- `a_reserved_key_after_a_table…`: R8's rescue is gone with the tables. Each retired key spelling is
  now the ordinary positioned syntax error (`UnknownKeyword` at 1:1, or `Equals` at the `=`), message
  and position asserted; five old rows kept, the later-line position included.
- `groups_stays_top_level…`: there is no file-level group list (`global_groups` is empty by
  construction); the assertion is that `global_groups` is empty and `use` attaches the trait to the
  route, plus the top-level `groups`/`use` refusals.
- `author_chosen_names…`: the retired `[proxy.db.output] app` variable has no spelling (the output
  map is dropped by §7.5), so the assertion is that `output()` is empty and the protocol defaults
  apply; the store/trait/table names called `app`/`groups`/`version` still boot, and the denied twin
  is `GrantOnUnknownProxy` naming view and store.
- `the_wire_spelling…`: lowering normalises `$1` to the editor's `$?` (§13.1), so the model assertion
  is the normalised string (the old "the model keeps the author's `$1`" cannot hold in a language
  whose lowerer normalises); the exemption itself — accepted, adjacent `set_config` still
  `LiteralQuery` — is unchanged and asserted.
- `a_statement_is_covered…`: the table literally named `"a$?"` has no `Word` spelling; the
  `$?`-in-an-identifier fact is asserted as the differential that `"a$?"` is *not* rewritten into the
  granted table `a`.
- `both_catch_all_spellings…`: `path = []` has no spelling (a view needs an endpoint); the
  equivalent is the no-endpoint refusal, asserted with its message and position.

## Follow-ups (out of scope)

- Sibling work in this shared worktree makes the node-scoped `critical-guard` run report violations that
  are not this node's: `MB.21.4.5.2` has `critical_php_manifest.rs` and `critical_static_manifest.rs`
  modified, and a scheduler leaf has `crates/blastit-scheduler/tests/critical_{charge,ingress_enforcement,rewrite}.rs`
  modified. I did not touch any of them. Run the guard at commit time (or per node) — this node's own
  change is clean (evidence above).
- `tests/common/mod.rs` still carries `SECTION_5_1`/`SECTION_6` and the retired `parse.rs` helpers'
  fixture imports; `MB.21.4.5.3` rewrites that file (and re-homes `validate.rs`'s TOML fixtures).
- The model reports an unknown store as `Location::View { … }` in `GrantOnUnknownProxy` while
  `validate::check_queries` reports the same view as `Location::Route { … }`. Both are asserted as
  they are here; if 5.6/5.3 unifies the `Location` spelling, `critical_manifest_011` will need the
  same one-token update.
- A schema-qualified table name (`audit.users`) is not writable in a `Blastfile` grant; recorded in
  the table row 26. If that is a capability loss rather than the intended §13.3 refusal, it belongs
  to the manifest model owner, not this leaf.
