# Result MB.21.4.3 (attempt 3)

**`STATUS: done`** — the leaf as narrowed after attempt 2 is complete: the `--json` fix actions name
`Blastfile` nodes, §13.2's six warnings, §10.2's production sentence, §11.3's nouns and §18's remaining
gaps are re-spelled/closed with their asserting suites, the editor's proposal type carries the
table/function scope `fold` needs, and the whole tree is green.

Everything below was re-verified on the final tree: `make check` exit `0` (166 `test result: ok`), the
narrowed pytest command green, `cargo test -p blastit-manifest --locked` green, both greps silent,
`critical_guard.py --base 537eb2d --node MB.21.4.3` authorizing all **10** protected changes, and
`make docs-check` clean.

## 1. What landed

| Node item | Change | Pinned by |
|---|---|---|
| `--json` `fix.to` are node locators | `audit_v1.rs`: `view <view> postgres[ <store>]` (`add_query`/`remove_query`), `trait <name>` (`add_group`, a trait's `remove_query`), `view <view>[ <METHOD[s]>] body` (`raw_ingress`). The body locator reads **who declared the body** out of lowering — `Route::body_owner()` — never `route.methods()`, because `view upload { body raw; POST "/upload" {} }` and `view upload { POST "/upload" { body raw } }` have the same method list and different nodes (§13.3) | `critical_check_engine::{every_add_query_reparses_under_parse_and_validate, connection_setup_sql_names_the_connection_trait_grant, ordinary_statements_keep_the_per_view_add_query, only_connection_setup_names_a_trait_and_never_a_refusal, the_body_finding_names_the_view_body_node_and_never_unvalidated}` (the last asserts **all three** shapes: `view raw body`, `view upload POST body`, and the view-level `view note body`), `audit_group_hint::*`, the protected Python contract |
| §13.2's six boot warnings | `error.rs`: `trait {} is attached to no view and no global traits`, `store {} has no grant in any view or trait`, `view {} may run any statement on store {} (unrestricted)`, `view {}: body raw passes the request body through unvalidated`, `view {}: egress any opens any outbound address and port`; **new** `EntrypointNotReached` (``entrypoint php "…" is reached by no view: declare a `view` that forwards to it``), emitted by `validate::warnings` when `php` is declared and no view exists. Triggers, order and counts unchanged | `validate::tests::*`, `critical_blastfile_parity::the_validate_warnings_are_surfaced_unchanged` (the six sentences verbatim, in order, plus the sixth from its own file) |
| §10.2 | `raw_ingress_refusals`: ``view "<name>": body raw hands the request body to the application unvalidated, … write `body raw unvalidated` in that view, or declare the codec it takes (json, form, none)``; its unit tests are `Blastfile` fixtures asserting the sentence verbatim | `validate::tests::{raw_ingress_without_unvalidated_is_the_only_refused_shape, the_acknowledgement_is_what_opens_the_gate, every_unacknowledged_view_is_named_in_route_order}`, `stage0::raw_ingress_gate_tests`, `critical_raw_ingress` (the sentence asserted whole, plus `!contains("allow_raw")`) |
| §11.3's nouns | `UnrestrictedGroup`: `unrestricted is only allowed inline on a view (grant on store …)`; `Location::Group` renders `trait "…"` (a `[group.x]` and a `trait x` are one node) | `validate::{an_unrestricted_grant_in_a_group_is_refused_at_boot, locations_read_well_in_errors}`, `blastfile_traits::unrestricted_inside_a_trait_is_refused`, `critical_blastfile_traits::unrestricted_inside_a_trait_is_refused` |
| §18 remainder | `ManifestErrors`' `Display` is one line per problem — no header, no `  - `, no trailing newline, `""` for none; `Location::View` names the view; `InvalidApp` and `PhpEntryInvalid` no longer echo their value; `ManifestError::position()` feeds `cli.rs`'s path prefix | `blastfile_errors`, `critical_blastfile_errors`, `parse`, `mutation_gaps::the_error_list_counts_iterates_and_renders`, `critical_php_manifest::{the_php_messages_match_the_design_table, a_refusal_never_carries_the_php_entry_it_refused}`, `validate::a_view_refusal_names_the_view_and_no_index` |
| the editor's grant scope | `ProposedGrant` gains `tables`/`functions`; `write.rs` merges them into the view's `postgres [store] { }` block, rewriting its `select/insert/update/delete/functions` directives to the merged scope (a second `select <table>` line would be a duplicate directive, not a union) and leaving `query` lines, `transactions` and comments in place; new refusals `UnwritableGrantName`/`EmptyColumnList` before anything is spliced; `expected()`/P4 carry the merge, so repetition is a no-op | `edit_branches::{a_table_and_function_scope_is_written_inside_the_store_block, a_wider_column_list_rewrites_the_existing_directive, a_new_view_carries_its_table_scope, a_name_the_grant_block_cannot_spell_is_refused, an_empty_column_list_is_refused, a_comment_above_the_directive_stays_there}` |
| the binding | `blastit._blastit.blastfile_apply_edits(source, edits_json) -> str` over `blastfile::apply_edits` | **`python/tests/test_edits_binding.py`** (4 cases: a scope written + idempotent, the edited file re-parses/validates through `check_audit`, bad JSON refused, an inline literal refused) — a Rust test cannot reach it, `crates/blastit-py` is a `cdylib` |
| the plugin's default/help | `pytest_plugin.py`'s docstrings, `--blastit-manifest` help and `blastit_manifest` ini text name `Blastfile`; its `tomllib` reader is `MB.21.4.7`'s | `python/tests/test_pytest_plugin.py` |
| the design | §14.5's `ProposedGrant` row gains `tables`/`functions` with the merge rule and the three refusals; §13.2's "every sentence lives in `validate.rs`" corrected to `error.rs` (the `thiserror` attributes) | `docs/design/blastfile-dsl.md`; `make docs-check` |
| one editor entry point | `check --learn` and `dev --learn` write through `blastfile::apply_edits` + one `edit::write_atomic`; no doc calls the retired patcher the editor any more | `critical_learn_executor_blocks`, `check::tests::*`; the second grep in §3 |
| item 5's renames/follow-ups | `manifest_toml` → `manifest_text` (positional PyO3 signatures), `cli.rs::one_line` deleted (one home: `config::display_path`), `lib.rs`'s "every route is static" re-spelled | `critical_blastfile_fmt`, `blastfile_discovery` |

`python/tests/test_pytest_plugin.py:461` now pins `'view "note-cache": egress any opens any outbound
address and port'`. That is the producer's own sentence, measured:

```console
$ # a throwaway test printing ManifestWarning::EgressRoute { route: "note-cache" }.to_string()
SENTENCE=view "note-cache": egress any opens any outbound address and port
```

and `crates/blastit-py/src/audit.rs`'s `manifest.warnings` is exactly `warning.to_string()` for each
warning. **Why the suite cannot run on this host:** its whole module is
`pytest.mark.skipif(not DATABASE_URL, …)` ("needs a real PostgreSQL"), no `DATABASE_URL`/
`BLASTIT_TEST_PG_URL` is set here, and the two local servers that listen (`127.0.0.1:5432`,
`127.0.0.1:5433`) answer `Password for user …` for both `remy` and `postgres`, so reaching one would
mean guessing a credential (out of scope, and `.env` is off-limits). Independently, its `project`
fixture asserts `blastit` is installed as a `pytest11` entry point, which needs
`scripts/build-wheel.sh && uv pip install` first. CI (`.github/workflows/ci.yml:560`) sets
`DATABASE_URL` and installs the wheel, so the re-spelled assertion is the one it will run.

## 2. Acceptance evidence (final tree)

```console
$ timeout 1800 make test-critical ; echo EXIT=$?
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # critical Rust tier
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 12.316s
OK
EXIT=0

$ timeout 1800 env UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/critical/test_check_contract.py python/tests/test_pytest_plugin.py -q
22 passed, 39 skipped in 5.69s
# test_pytest_plugin.py skips as a whole: the module is skipif(not DATABASE_URL) and this host has
# no reachable one (see §1's note — the two local servers want a password), and its `project`
# fixture also needs the wheel installed as a pytest11 entry point. CI sets DATABASE_URL.

$ timeout 600 cargo test -p blastit-manifest --locked
33 `test result: ok` blocks, 0 failed

$ grep -rnE 'attached to no route|no grant in any route or group|may run any statement on proxy|body = "raw"|egress = true' \
    crates/blastit-manifest/src/error.rs
(no output; exit 1)

$ grep -rn 'blastit_manifest::edit::apply_edits\|edit::apply_edits' crates/ --include='*.rs'
(no output; exit 1)

$ timeout 5400 make check ; echo EXIT=$?
guard: OK · critical-guard (report-only here) · fmt clean · clippy 0 warnings · rustdoc 0 warnings
cargo test --workspace --all-targets --locked --features python   # 166 `test result: ok`, 0 failed
python -m unittest discover -s python/tests/critical              # Ran 50 tests, OK
49 fences: 28 executed, 21 accounted, 0 failed · 22.3s
… rs-no-python, seam-check, platform-matrix, workflow-check, packaging-gates, doc-tone,
  manifest-purity, plan-validate (plan OK) …
EXIT=0

$ timeout 600 uv run --no-project tools/critical_guard.py --base 537eb2d --node MB.21.4.3 | tail -2
critical-guard: OK: all 10 protected change(s) are authorized by node(s) MB.21.4.3

$ timeout 1200 make docs-check
uvx --offline zensical build --strict
No issues found
```

`python/tests/test_edits_binding.py` runs in CI's `python/tests` job like its siblings; locally it is
green against the staged/in-tree extension (`4 passed`).

## 3. Protected suites and files outside the globs

Ten protected files changed, all inside `paths` and all **strengthened**, never weakened:

| File | Change | Why it is not weaker |
|---|---|---|
| `crates/blastit-manifest/tests/critical_blastfile_parity.rs` | the exhaustive `ManifestWarning` `match` gains `EntrypointNotReached`; five `contains("\"unused\"")` assertions become six **exact sentences**; `Spec` gains `endpoint_body` so the expected model carries who declared the body | a match that compiles only with the arm; substring assertions replaced by exact ones |
| `crates/blastit-manifest/tests/critical_php_manifest.rs` | `php entry "index.php": …` → `php entry must be …` | adds `a_refusal_never_carries_the_php_entry_it_refused` (a DSN-shaped entry and a `$TOKEN`-shaped app, both asserted absent) |
| `crates/blastit-manifest/tests/critical_blastfile_traits.rs` | `inline on a route` → `inline on a view`; `group "t"` → `trait "t"` | same rule, the design's nouns |
| `crates/blastit-manifest/tests/critical_blastfile_errors.rs` | `lines.len() == errors.len() + 1` + header → `== errors.len()`, no header, no `  - `, no trailing newline, exact join | strictly more assertions on §18's shape |
| `crates/blastit-manifest/tests/critical_manifest_edit.rs`, `critical_manifest_edit_executor.rs` | `ProposedGrant { …, tables: Default::default(), functions: Default::default() }` | constructors only |
| `crates/blastit/tests/critical_check_engine.rs` | the locator assertions, the `trait connection` ones, three test names, and the body test now covering **three** shapes (`view raw body`, `view upload POST body`, `view note body`) | exact `fix` objects, no `unvalidated`, no retired path prefix in the whole document |
| `crates/blastit/tests/critical_learn_executor_blocks.rs` | two `ProposedGrant` constructors | constructors only |
| `crates/blastit/tests/critical_raw_ingress.rs` | §10.2's sentence asserted verbatim (`REFUSAL`), `dev`'s warning verbatim (`WARNING`, `#[cfg(feature = "python")]`), `view "upload"`/`body raw unvalidated` instead of the retired nouns, plus `!refusal.contains("allow_raw")` | the refusal pinned whole instead of by four substrings |
| `python/tests/critical/test_check_contract.py` | `{"action": "add_query", "to": "view home postgres"}` | the documented locator, exactly |

**Files outside the node's globs:** one — `docs/knowledge/LESSONS.md`, one line (the in-tree
`python/blastit/_blastit*.so` shadowing a fresh build). `docs/reference/manifest.md`,
`docs/reference/cli.md` and `docs/ci.md` were **not** edited (they are `MB.21.5.1`'s). No `plan/`
frontmatter was hand-edited.

Non-protected files edited in place: `crates/blastit-manifest/src/{error,validate,model,lib}.rs`,
`crates/blastit-manifest/src/blastfile/{lower,mod,write}.rs`, `crates/blastit-manifest/src/edit.rs`,
and its `tests/{validate,mutation_gaps,parse,blastfile_errors,blastfile_traits,edit_branches}.rs`,
`crates/blastit/src/{audit_v1,check,dev_learn,cli,stage0}.rs`,
`crates/blastit/tests/audit_group_hint.rs`, `crates/blastit-py/src/{lib,audit}.rs`,
`python/blastit/pytest_plugin.py`, `python/tests/{test_pytest_plugin.py,test_edits_binding.py}`,
`docs/design/blastfile-dsl.md`.

## 4. Sentences that moved (for the docs sweep)

Moved here: the five §13.2 warnings + `EntrypointNotReached`; §10.2's production refusal;
`UnrestrictedGroup`; `Location::Group`; `ManifestErrors`' `Display`; `PhpEntryInvalid`; `InvalidApp`;
every `--json` `fix.to`. The human-readable `check` lines (`route …`, `group session: …`, `declares
body = "raw" without allow_raw = true`) are unchanged because docs pages quote them.

## Follow-ups

- **`MB.21.4.7` owns the Python cutover** — `fold.py` → `blastfile_apply_edits`, `_blastit.audit` off
  the retired reader (one line plus the ~18 TOML fixtures and the global-`groups` cases in
  `crates/blastit-py/src/audit.rs`'s tests), `pytest_plugin.py`'s `tomllib` read, and
  `python/tests/{test_fold,test_pytest_plugin,test_plugin_helpers,test_check}.py`'s fixtures. It
  re-uses the binding, `ProposedGrant`'s fields, the editor's table/function splice and its refusals —
  all landed and tested here.
- **`MB.21.5.1` owns the doc sweep**: `docs/reference/manifest.md:118,318` quote the old §13.2
  sentences, and **`docs/ci.md`'s `--json` example still carries the retired `fix.to` values**
  (`"to": "[route.db].queries"`, `{"action": "add_group", "to": "[group.connection.db].queries"}`) —
  the payload is now `view catalog postgres` / `trait connection`. `docs/reference/cli.md:95`'s
  `--manifest blastit.toml` example is the same sweep; its flags are unchanged.
- **`test_pytest_plugin.py`'s document body** is `python/tests/project/`'s, which `MB.21.4.4`
  translates; the sentence this leaf re-spelled is producer-exact today.
- **`set_transaction_control`** (`docs/design/learn-single-pass.md` R18) still names a `fix.action` the
  0.2.0 engine does not emit (`observe`) — a planner's decision, unrelated to this leaf.
