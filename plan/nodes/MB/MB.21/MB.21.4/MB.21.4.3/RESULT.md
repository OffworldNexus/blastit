# Result MB.21.4.3 (attempt 2)

**`STATUS: needs_split`** — the Rust half of the leaf is landed and the whole tree is green, but the
**Python cutover** (item (e): `fold.py` → `blastfile_apply_edits`, `_blastit.audit` off the retired
reader, and the four Python suites' fixtures) did not fit this session. It is the *only* thing left;
§3 says exactly how to finish it, and §2 says what already works for it.

Attempt 1's `needs_split` was answered by widening `paths` and by the ruling that `ProposedGrant` grows
the table/function scope. Both are honoured here: every sentence, the §18 remainder and the editor's
new field are implemented **with** their asserting suites (all ten protected changes are inside
`paths`), and the editor is the one writer both Rust entry points use.

## 1. What landed

| Node item | Change | Pinned by |
|---|---|---|
| (a) §13.2's six boot warnings | `error.rs`: `trait {} is attached to no view and no global traits`, `store {} has no grant in any view or trait`, `view {} may run any statement on store {} (unrestricted)`, `view {}: body raw passes the request body through unvalidated`, `view {}: egress any opens any outbound address and port`; **new** `ManifestWarning::EntrypointNotReached` (``entrypoint php "…" is reached by no view: declare a `view` that forwards to it``), emitted by `validate::warnings` when `php` is declared and no view exists. Triggers, order and counts unchanged | `validate::tests::*`, `critical_blastfile_parity::the_validate_warnings_are_surfaced_unchanged` (asserts the six sentences verbatim, in order) |
| (b) §10.2 | `raw_ingress_refusals`: ``view "<name>": body raw hands the request body to the application unvalidated, … write `body raw unvalidated` in that view, or declare the codec it takes (json, form, none)``; its unit tests are now `Blastfile` fixtures and assert the sentence **verbatim** | `validate::tests::{raw_ingress_without_unvalidated_is_the_only_refused_shape, the_acknowledgement_is_what_opens_the_gate, every_unacknowledged_view_is_named_in_route_order}`, `stage0::raw_ingress_gate_tests`, `critical_raw_ingress::{production_stage_one_refuses_raw_ingress_without_unvalidated, the_pretty_refusal_is_plain_text_naming_the_same_route, production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it}` |
| (c) §11.3's nouns | `UnrestrictedGroup`: `unrestricted is only allowed inline on a view (grant on store …)`; `Location::Group` renders `trait "…"` (a `[group.x]` and a `trait x` are one node) | `validate::{an_unrestricted_grant_in_a_group_is_refused_at_boot, locations_read_well_in_errors}`, `blastfile_traits::unrestricted_inside_a_trait_is_refused`, `critical_blastfile_traits::unrestricted_inside_a_trait_is_refused` |
| (d) §18 remainder | `ManifestErrors`' `Display` is one line per problem: no `manifest has N error(s)` header, no `  - ` prefix, no trailing newline, `""` for none; `PhpEntryInvalid` renders `{location}: php entry {rule}` — the entry is compared, never printed | `blastfile_errors::the_error_list_is_one_line_per_problem`, `critical_blastfile_errors::the_list_is_one_line_per_problem`, `parse::*`, `mutation_gaps::the_error_list_counts_iterates_and_renders` (exact `"version: missing …\nroute[0].name: …"` and `""`), `critical_php_manifest::{the_php_messages_match_the_design_table, a_refusal_never_carries_the_php_entry_it_refused}` (a DSN-shaped entry and a `$TOKEN`-shaped app, each asserted absent) |
| (e, half) the editor grows | `ProposedGrant` gains `tables: BTreeMap<String, TableGrant>` + `functions: BTreeSet<String>`; `write.rs` merges them into the view's `postgres [store] { }` block, **rewriting** the block's `select/insert/update/delete/functions` directives to the merged scope (a second `select <table>` line is a duplicate directive, not a union) and leaving `query` lines, `transactions` and comments where the author put them; new refusals `UnwritableGrantName` (a table/column/function a `Word` cannot spell) and `EmptyColumnList`, both before anything is spliced. `expected()`/P4 carry the merge, so repetition is a no-op | `edit_branches::{a_table_and_function_scope_is_written_inside_the_store_block, a_wider_column_list_rewrites_the_existing_directive, a_new_view_carries_its_table_scope, a_name_the_grant_block_cannot_spell_is_refused, an_empty_column_list_is_refused, a_comment_above_the_directive_stays_there}` |
| (e, half) the binding | `blastit._blastit.blastfile_apply_edits(source, edits_json) -> str` over `blastfile::apply_edits` (a `BlastfileEdits` document in, the proved source out) | built + linked into the wheel (`crates/blastit-py`), exercised through the editor's own suite; `fold` does not call it yet (§3) |
| (e, part) the plugin's default/help | `pytest_plugin.py`'s docstring, `--blastit-manifest` help and `blastit_manifest` ini text name `Blastfile`; its `tomllib` reader is untouched (§3) | `python/tests/test_pytest_plugin.py` (skips without an installed wheel, as before) |
| (f) the design row | §14.5's `ProposedGrant` gains `tables`/`functions` with the merge rule and the three refusals; §13.2's "every sentence lives in `validate.rs`" is corrected to `error.rs` (the `thiserror` attributes) with `validate.rs` owning the emission | `docs/design/blastfile-dsl.md`; `make docs-check` clean |
| item 1 (attempt 1, re-verified) | `fix.to` node locators (`view <view> postgres[ <store>]`, `trait connection`, `trait <name>`, `view <view>[ <METHOD>] body`) | `critical_check_engine`, `audit_group_hint`, the protected Python contract |
| items 3/5 (attempt 1, re-verified) | `Location::View` names the view, `ManifestError::position()`, `InvalidApp` stops echoing, `manifest_toml`→`manifest_text`, `cli.rs::one_line` gone, `lib.rs` doc | `validate`, `critical_blastfile_fmt`, `blastfile_discovery` |

## 2. What is *not* done, and why it is the only gap

`fold.py` (2 154 lines) and `pytest_plugin.py`'s manifest read still parse the retired document with
`tomllib`, and `_blastit.audit` still calls `Manifest::from_toml_str`. Re-pointing them needs all of:

1. **`fold.py` re-written around the binding** — read the `Blastfile`, plan from the log through
   `_blastit.audit` (the report's `routes[].proxies[].missing[]` already says what is missing per
   view/store) plus `_blastit.covering` for `--prefer-tables`, build the `BlastfileEdits` JSON, call
   `blastfile_apply_edits`, keep `_write_atomic` (the protected suite pins `.{name}.{pid}.tmp` +
   `fsync` + `os.replace` + the mode), and re-validate by auditing the edited text.
2. **`_blastit.audit`'s cutover** — `Manifest::parse_blastfile` instead of `from_toml_str`. That is one
   line, but it turns ~18 TOML fixtures and ~10 global-`groups` unit tests in
   `crates/blastit-py/src/audit.rs`'s `mod tests` red: they are this leaf's (`crates/blastit-py/**`),
   and the global-group concept does not exist in a `Blastfile` (`global_groups` is always empty,
   §7.1), so those cases become traits-on-every-view cases or go.
3. **The four Python suites** — `python/tests/test_fold.py` (2 014 lines of TOML planning cases),
   the protected `python/tests/critical/test_fold_literals_and_atomic.py` (486 lines: `fold.plan`,
   `fold._Addition`, `fold._write_atomic`, `fold.main`), and `python/tests/test_check.py` (TOML
   fixtures, `DATABASE_URL`-gated).

Everything else in the node is done and green. I reverted the `audit()` cutover *after* trying it: with
it applied and `fold.py` still on TOML, `fold` and its whole suite break, and a half-cut Python tree is
worse than a green one.

## 3. Proposed split for attempt 3 (one node, one commit)

**4.3b — the Python cutover.** `paths`: `crates/blastit-py/**`, `python/blastit/{fold,pytest_plugin}.py`,
`python/tests/{test_check,test_fold,test_pytest_plugin,test_plugin_helpers}.py`,
`python/tests/critical/test_fold_literals_and_atomic.py`, `python/tests/runtime-capture/**` (if the
fixtures need it). Acceptance: `make test-critical`, the four-file pytest command, `make check`, and a
`--prefer-tables`/`--allow-dangerous`/`--write` run whose edited file re-parses **and validates**, keeps
every comment, and adds nothing on a second run (`docs/reference/cli.md` unchanged).

The pieces it must not redo: the binding (`blastfile_apply_edits`), `ProposedGrant`'s fields, the
editor's table/function splice and its refusals, and the design row — all landed here, all tested.

## 4. Protected suites: every changed assertion

Ten protected files changed; `critical_guard.py --base 537eb2d --node MB.21.4.3` says all ten are
authorized. None is weaker:

| File | Change | Why it is not weaker |
|---|---|---|
| `crates/blastit-manifest/tests/critical_blastfile_parity.rs` | the exhaustive `ManifestWarning` `match` gains `EntrypointNotReached`; the five `contains("\"unused\"")` assertions become **exact sentence equality** for all five, plus a second file asserting the sixth's sentence | a `match` that compiles only with the arm, and five substring assertions replaced by six exact ones |
| `crates/blastit-manifest/tests/critical_php_manifest.rs` | `php entry "index.php": …` → `php entry must be …` | adds `a_refusal_never_carries_the_php_entry_it_refused` (a DSN-shaped entry + a `$TOKEN`-shaped app, both asserted absent) |
| `crates/blastit-manifest/tests/critical_blastfile_traits.rs` | `inline on a route` → `inline on a view`, `group "t"` → `trait "t"` | same rule, the design's nouns |
| `crates/blastit-manifest/tests/critical_blastfile_errors.rs` | `lines.len() == errors.len() + 1` + header → `== errors.len()`, no header, no `  - `, no trailing newline, exact join | strictly more assertions on the §18 shape |
| `crates/blastit-manifest/tests/critical_manifest_edit.rs`, `critical_manifest_edit_executor.rs` | `ProposedGrant { …, tables: Default::default(), functions: Default::default() }` | constructors only; every assertion untouched |
| `crates/blastit/tests/critical_check_engine.rs` | `fix.to` values, the `add_group` → `trait connection` assertions, three test names, one new test (`the_body_finding_names_the_view_body_node_and_never_unvalidated`: exact `fix` objects, no `unvalidated`, no retired path prefix in the whole document) | locators are asserted exactly; the new test adds the §13.3 properties |
| `crates/blastit/tests/critical_learn_executor_blocks.rs` | the two `ProposedGrant` constructors | constructors only |
| `crates/blastit/tests/critical_raw_ingress.rs` | §10.2's sentence asserted **verbatim** (`REFUSAL`), `view "upload"` instead of `route`, `body raw unvalidated` instead of `allow_raw = true`, and `dev`'s warning asserted verbatim (`WARNING`, `#[cfg(feature = "python")]`) | the refusal is now pinned whole rather than by four substrings, plus a negative (`!refusal.contains("allow_raw")`) |
| `python/tests/critical/test_check_contract.py` | `{"action": "add_query", "to": "view home postgres"}` | the documented locator, exactly |

Non-protected suites edited in place: `crates/blastit-manifest/src/{error,validate,lib}.rs` and its
`tests/{validate,mutation_gaps,parse,blastfile_errors,blastfile_traits,edit_branches}.rs`,
`crates/blastit/src/{audit_v1,check,dev_learn,cli,stage0}.rs`, `crates/blastit/tests/audit_group_hint.rs`,
`crates/blastit-py/src/{lib,audit}.rs`, `python/blastit/pytest_plugin.py`,
`docs/design/blastfile-dsl.md`, `docs/knowledge/LESSONS.md` (attempt 1's line).

## 5. Acceptance evidence (final tree)

```console
$ timeout 1800 make test-critical ; echo EXIT=$?
test result: ok. 12 passed; 0 failed; … (the critical Rust tier)
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 5.824s
OK
EXIT=0

$ timeout 1800 env UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/critical/test_check_contract.py python/tests/critical/test_fold_literals_and_atomic.py \
    python/tests/test_fold.py python/tests/test_pytest_plugin.py -q
161 passed, 39 skipped in 3.84s

$ timeout 600 uv run --no-project tools/critical_guard.py --base 537eb2d --node MB.21.4.3 | tail -2
critical-guard: OK: all 10 protected change(s) are authorized by node(s) MB.21.4.3

$ timeout 900 cargo test -p blastit-manifest --locked    # 33 `test result: ok` blocks, 0 failed

$ grep -rnE 'attached to no route|no grant in any route or group|may run any statement on proxy|body = "raw"|egress = true' \
    crates/blastit-manifest/src/error.rs
(no output; exit 1)

$ grep -rn 'blastit_manifest::edit::apply_edits\|edit::apply_edits' crates/ --include='*.rs'
(no output; exit 1)

$ timeout 5400 make check ; echo EXIT=$?
guard: OK · critical-guard (report-only) · fmt clean · clippy 0 warnings · rustdoc 0 warnings
cargo test --workspace --all-targets --locked --features python   # 166 `test result: ok`, 0 failed
python -m unittest discover -s python/tests/critical              # Ran 50 tests, OK
49 fences: 28 executed, 21 accounted, 0 failed · 18.7s
… rs-no-python, seam-check, platform-matrix, workflow-check, packaging-gates, doc-tone,
  manifest-purity, plan-validate (plan OK) …
EXIT=0

$ timeout 1200 make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.23s
```

The two greps are the node's own: no retired warning fragment is left in `error.rs`, and no call site
reaches the retired patcher. The design row and the struct agree (§1's (e)/(f) rows).

## 6. Sentences that moved (for the docs sweep)

Moved here: the five §13.2 warnings + `EntrypointNotReached`, §10.2's production refusal,
`UnrestrictedGroup`, `Location::Group`, `ManifestErrors`' `Display`, `PhpEntryInvalid`, `InvalidApp`
(attempt 1). **`docs/reference/manifest.md:118,318`** still quote the old §13.2 spellings and are not in
`paths` — a follow-up for `MB.21.5.1`, as the node says. The human-readable `check` lines
(`route …`, `group session: …`) are unchanged, because `docs/ci.md` quotes them.

## Follow-ups

- **The Python cutover** — §3. It is the only remaining work in this node.
- **`python/tests/project/`, `live_project/`** are still TOML fixtures (`MB.21.4.4`'s); once
  `_blastit.audit` reads `Blastfile`, `test_pytest_plugin.py` and `test_plugin_helpers.py` need them
  translated in the same change.
- **`docs/reference/cli.md:95`** keeps `--manifest blastit.toml` in its example; it is `MB.21.5.1`'s,
  and the *flags* it documents are unchanged (the ruling), so nothing here contradicts it.
- **`set_transaction_control`** (`docs/design/learn-single-pass.md` R18) still names a `fix.action` the
  0.2.0 engine does not emit (`observe`); a planner's decision, unrelated to this leaf.
