# Result MB.21.4.10

## Summary

The last user-facing sentences that told a developer to write the retired manifest format are gone, and
the report's table-scope value is a pasteable `Blastfile` fragment under `table_grants.blastfile`. Three
halves, in the node's order:

1. **The renderer.** `TableGrants::to_blastfile(&self, locator)` emits only directives a view's
   `postgres` block accepts: one `# <locator>` comment, `functions a, b`, then per table in sorted order
   `select t(cols)` / `insert t(cols)` / `update t(cols)` / `delete t`, `select t` (no parentheses) for
   `Columns::All`, nothing for an absent operation, `""` for an empty grant. Every shape the grammar has
   no spelling for is a `#` comment and **no directive**: a lock whose column cannot be named, an empty
   column list (`SELECT count(*)`), a table or column name that is not one `Blastfile` word (a `.`, a
   space, a quote, a control byte — `is_word` copies the lexer's own `word_run`), and a function name that
   is not `Word ("." Word)*`. Names in those comments are quoted with `{:?}`, so a name carrying a newline
   cannot become a directive of its own. `columns_toml`, `key_toml` and `string_toml` are deleted with the
   TOML they wrote; `table_directive`/`column_names`/`is_word` replace them.
2. **The report.** `covering`'s parameter is `locator`; `table_grants_json` takes the view and store and
   renders `view <name> postgres` / `view <name> postgres <store>`; the JSON key is `blastfile` at both
   producers. `fold.py` builds the same locator from the route name (`route.db` survives nowhere), and
   `pytest_plugin.py`'s paragraph says what the key is instead of calling it "the retired snippet".
3. **The sentences.** Each keeps its trigger and order and names the `Blastfile` directive a developer
   deletes or writes — including `blastit check`'s human output, whose two body sentences named the retired
   assignments. `stage0::php_worker_budget` is deleted with its call site and with `php_boot`'s budget
   parameter rather than re-worded, because `PhpPolicy.workers` is `None` by construction.

## The seven survivors, exact replacement

| File:line | Before | After |
|---|---|---|
| `crates/blastit/src/check.rs:457` | `declare [proxy.<name>] with protocol = "postgresql"` | `` declare a `postgres` store: `postgres $DATABASE_URL` `` |
| `crates/blastit/src/check.rs:492` | `name them in [proxy.{}]` | `` name them in that store's block: `postgres { user $DB_USER; password $DB_PASSWORD }` `` |
| `crates/blastit/src/audit_v1.rs:1528` | `set transaction_control = true on [route.{}]` | `` add `transactions` to `view <route>`'s `postgres` block (or to a trait it `use`s) `` |
| `crates/blastit/src/stage0.rs:1424` | `route "app": [route.php] workers = …` | **deleted**: `php_worker_budget` + its call site (`PhpPolicy.workers` is always `None`) |
| `crates/blastit/src/host.rs:2064` | claim label `route "app": [route.php] entry` | `route "app": entrypoint php "public/index.php"` |
| `crates/blastit-view/src/php.rs:100` | `entry … must be an absolute path ([route.php] entry)` | `` entry … must be an absolute path (the path `entrypoint php` resolved to) `` |
| `crates/blastit-manifest/src/validate.rs:556/562` | `dead("[route.ingress]")`, `dead("[route.{proxy}]")` | `dead(body_directive(..))` → `body json`/`body raw unvalidated`; `dead(store_block(..))` → `postgres { … }` / `postgres <name> { … }` |
| `crates/blastit/src/audit_v1.rs:1258/1264` (`blastit check`'s human output) | `declared body = "json" does not match observed …`, `declares body = "raw" without allow_raw = true` | `` declared body json does not match observed … ``, `` declares body raw without `unvalidated` — write `body raw unvalidated`, or the codec it takes `` |
| `crates/blastit-manifest/src/validate.rs:541/559` | `"spa = true never reaches the app"`, `dead("spa = true", …)`, `dead("prefork = true")`, `dead("groups")` | `fallback "<file>" never reaches the app`, `dead("prefork")`, `dead("use")`; and `SPA_NEVER_REACHES_FRONT_CONTROLLER` / `dead("spa = true")` in `check_php_declarations` → `fallback "<file>" never reaches the front controller` |

`StaticDeadDeclaration.why` and `PhpDeadDeclaration.why` became `String` because the fallback's file name
is interpolated; `Display` is untouched (`{location}: {why}; {key} can never run` and `{location}: {why}`),
so trigger and order are the same and only the words moved. The no-op keyword refusal
(`WorkersRemoved`) is not weakened — deleting the budget check removed no grammar rule.

Three sentences not in the node's table were corrected because the node's own edit left them describing
text that changed: `validate.rs`'s `dead("groups")` → `dead("use")` (the `use` directive replaced the
inline group list), `error.rs`'s `StaticDeadDeclaration`/`PhpDeadDeclaration` doc lists, and the two
`crates/blastit-view/src/php.rs` rustdoc mentions of `[route.php] entry` (65, 176) on the field whose
refusal now names `entrypoint php`.

## Per-string tests (each asserts the new text; refusals keep a denial side)

| New text | Test |
|---|---|
| `body …` / `postgres [<name>] { … }` / `prefork` / `use` dead declarations | `crates/blastit-manifest/src/validate.rs::tests::a_dead_declaration_is_named_the_way_a_blastfile_writes_it` — the whole `Vec<String>` of refusals is compared, in order, and each is asserted to carry no `[route.` and no `spa` |
| `fallback "<file>" never reaches the front controller` | `validate.rs::tests::a_php_dead_declaration_names_the_fallback_directive` — exact three-line refusal list, `CONNECT`/`TRACE` twins kept |
| `` add `transactions` to `view …`'s `postgres` block `` | `crates/blastit/src/audit_v1.rs::tests::a_transaction_hint_names_the_directive_and_the_view` — exact hint, plus `!contains("[route.")` and `!contains("transaction_control")` |
| `` `postgres $DATABASE_URL` `` | `crates/blastit/src/check.rs::tests::a_pytest_django_project_without_a_postgresql_proxy_is_a_start_failure` — keeps its `expect_err`, now asserts the store line and `!contains("[proxy.")` |
| `` `postgres { user $DB_USER; password $DB_PASSWORD }` `` | `check.rs::tests::a_store_with_no_input_variables_is_a_start_failure_that_names_the_block` — asserts the sentence and both retirements (see Follow-ups: the guard is unreachable through `build`, so the sentence is pinned directly) |
| `body json` / `body raw unvalidated` in `blastit check`'s output | `crates/blastit/tests/critical_check_engine.rs::every_finding_kind_and_its_interaction_counts` keeps its byte-for-byte document (both lines re-spelled) and now asserts `!contains("body = \"")` and `!contains("allow_raw")`; `an_empty_suite_…` pins the raw-ingress line too |
| `entrypoint php` budget (deleted) | `stage0.rs::tests::a_php_boot_with_no_workers_cap_passes_any_budget` stays; the budget parameter it used to pass is dropped from `php_boot`, so the test states the rule without inventing an argument |
| claim label | `crates/blastit/src/host.rs::tests::a_writable_tree_is_refused_and_a_protected_one_is_not` — the refusal is asserted to carry `route "app": entrypoint php "public/index.php"` and no `[route.` |
| `` entry … (the path `entrypoint php` resolved to) `` | `crates/blastit-view/src/php.rs::tests::a_configuration_that_could_not_serve_is_refused_both_ways` — the relative-entry case moved from `is_err()` to the exact string |
| `select notes_note(id, owner_id)` | `tables.rs::tests::a_covering_grant_renders_the_directives_a_view_holds`; `policy_module_tables.rs::a_covering_grant_is_rendered_as_the_directives_a_view_holds`; `audit.rs::tests::the_covering_grant_of_a_set_of_statements_is_taken_at_once` and `a_locking_statement_gets_a_column_named_never_a_star`; `test_pytest_plugin.py:1115` |

`static_mounts.rs` could not host the `R4` **denial** twin: `Manifest::assemble` and `Route::with_static`
are `pub(crate)`, so an integration test cannot build a route carrying the retired per-route static block
(the only subject `check_static_declarations` has). The denial twin therefore lives in `validate.rs`'s own
`mod tests`, beside the existing `retired_static` helper. The **accepted** twin is where the node says it
is, unchanged: `crates/blastit-manifest/tests/critical_static_manifest.rs:582-598`
(`a_mount_that_never_reaches_the_app_may_not_declare_them`), where the view that does reach the app keeps
`body none` / `prefork` / `use` / its grant and the whole file validates.

## The rename, with its rendering rules

```
TableGrants::to_blastfile("view notes postgres")
  → "# view notes postgres\nselect notes_note(id, owner_id)\n"
TableGrants::covering(all statements)          → one call, tables/columns/functions sorted
Columns::All                                   → "select t"           (no parentheses)
Columns::List(empty)                           → no directive, one advice comment (see below)
delete = true                                  → "delete t"
functions = {"unaccent"}                       → "functions unaccent" (first line after the header)
lock_only with a named column                  → "# t: `update t(id)` only locks its rows (`FOR UPDATE`) — …"
lock_only without one                          → advice comment, and no `update` line
a table/column name that is not one Word       → no directive for that table, one advice comment
a function name that is not Word("." Word)*    → left out of the `functions` line, one advice comment
empty grant                                    → ""
```

A **Word** is the reader's own rule (`lexer.rs`'s `word_run`): a non-empty ASCII run of letters, digits
and `_`, where `-` is taken only right after one of those. `audit.users` is not one — rendered raw the
reader refuses it (`` `.` is only part of a qualified function name ``, `blastit fmt` rc=2) — and a name
carrying a newline would end the line and become a directive of its own. Both are one comment, and the
name inside it is quoted with `{:?}`/`json.dumps`, so neither can escape the comment. A function is
`Word ("." Word)*`, so `pg_catalog.unaccent` renders and `my schema.f` is a comment.

**Every rendered line parses.** An empty column list (`SELECT count(*) FROM t` names the table and reads no
column) has no spelling: `table_grant` needs at least one word, and a bare `select t` would grant every
column. So the entry is omitted and the author gets one comment, the same pattern an unnamable lock already
used:

```
# t: the statements only name it, so there is no column to write for `select` — a table grant
takes at least one, so name one here (its primary key, say): without the entry the statement is refused.
```

`crates/blastit-manifest/tests/grant_fragment.rs` holds that promise mechanically, in six tests: a column
grant (with `functions`), a `*` grant, an empty column list, the written `update = []` twin, a qualified
relation (`audit.users`) and a name carrying a newline. Each rendered value is wrapped in a real
`view notes "/notes/" { postgres { … } }` document, read by `Manifest::from_blastfile_str` **and**
`Manifest::validate`, and the refused shapes are asserted to grant nothing (no `t()`, no widening to
`select t`, no `select audit.users`) and to add no line (the newline case is exactly two lines). A
rendering that emitted syntax the reader refuses fails there. It lives in `blastit-manifest` because the
reader does: `blastit-sqlpolicy` cannot dev-depend on `blastit-manifest` (that crate depends on it), and
this node owns no `Cargo.toml` edit.

These directive lines are character-identical to `pytest_plugin._table_snippet`'s (the terminal hint keeps
printing them without the header, the one allowed difference), and the hint prints the same advice sentence
for an empty column list — the one maintained duplication, pinned on both sides
(`test_fold.py::TestTheTableScopeHint`, a PostgreSQL-free module: the plugin's own suite is gated on a real
database, so the branch that decides which lines may print is proved here instead — empty list, qualified
relation, newline name, column name and function name, five lines and no `counter()`).

Tests removed with their subject (TOML escaping has no producer now):
`toml_quotes_what_it_has_to`, `a_control_character_is_escaped_in_toml`,
`a_toml_key_is_bare_only_when_it_can_be`, and `a_relation_outside_the_default_schema_is_written_as_it_is_named`
(which pinned the unparseable `select audit.users(id)`). Their replacement is
`a_name_the_reader_cannot_write_is_a_comment_never_a_directive` (dotted table, dotted column, newline table,
qualified and spaced functions) plus `a_word_is_the_readers_own_run`, which pins `is_word` against the
lexer's boundary cases (`a-`, `a--b`, `a_-b`, `-a`, `héllo`).

Producer evidence for the Python hint and the pinned literal, through the built extension and module:

```console
$ … python -c 'print(pytest_plugin._table_snippet({"tables": {…"counter": {"select": []}…}, "functions": ["unaccent"]}))'
functions unaccent
# counter: the statements only name it, so there is no column to write for `select` — a table grant
takes at least one, so name one here (its primary key, say): without the entry the statement is refused.
select notes_note(id, owner_id)
delete notes_note
select wide
```

Producer evidence for the pinned Python literal (`test_pytest_plugin.py:1115`), through the built extension:

```console
$ PYTHONPATH=target/python-local uv run --no-project python -c '… _blastit.covering([…], "view notes postgres")'
"# view notes postgres\nselect notes_note(id, owner_id)\n"
matches pinned literal: True
tables: {"notes_note": {"delete": false, "insert": null, "select": ["id", "owner_id"], "update": null}}
```

## Protected files

Two `critical_*` files changed, both covered by this node's `paths` (`crates/blastit-sqlpolicy/tests/**`,
`crates/blastit/tests/**`) and both authorized by the guard (below).

### `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` (named by its own `paths` entry)

```diff
@@ -737,12 +737,12 @@ fn a_covering_grant_writes_the_lock_it_can_name() {
-    let toml = covering.to_toml("route.db");
-    assert!(toml.contains("update = [\"id\"]"), "{toml}");
-    assert!(toml.contains("only locks its rows"), "{toml}");
+    let blastfile = covering.to_blastfile("view notes postgres");
+    assert!(blastfile.contains("update t(id)"), "{blastfile}");
+    assert!(blastfile.contains("only locks its rows"), "{blastfile}");
     assert!(
-        !toml.contains("update = \"*\""),
-        "a lock is never `\"*\"`: {toml}"
+        !blastfile.contains("update t\n"),
+        "a lock never widens to every column: {blastfile}"
     );
@@ -758,12 +758,15 @@ fn a_lock_without_a_column_is_reported_not_widened() {
-    let toml = covering.to_toml("route.db");
+    let blastfile = covering.to_blastfile("view notes postgres");
     assert!(
-        !toml.contains("update ="),
-        "no update entry is invented: {toml}"
+        !blastfile.contains("update"),
+        "no update entry is invented: {blastfile}"
+    );
+    assert!(
+        blastfile.contains("name one here"),
+        "the author is told: {blastfile}"
     );
-    assert!(toml.contains("name one here"), "the author is told: {toml}");
     assert!(
         !allowed(&covering, sql),
         "a grant that cannot name a column cannot allow the lock"
```

Every structural claim is kept: (a) **a lock names a column it reads** — `update t(id)`, the same fact
`update = ["id"]` stated; (b) **a lock never widens** — the widening spelling in Blastfile is `update t`
(every column), so `!contains("update t\n")` pins exactly what `!contains("update = \"*\"")` pinned; the
`update = ["id"]`-free shape is additionally pinned by the unnamable-lock test's `!contains("update")`.
No assertion was dropped or relaxed; the third one gained a sentence (the advice comment) it already relied
on through the exact-string twin.

### `crates/blastit/tests/critical_check_engine.rs` — **for `lead` to review**

Two changes in one file, both re-spellings that keep the assertion shape: the transaction hint (below) and
`blastit check`'s two body sentences. The second one's byte-for-byte document moved with the output, and the
test gained the negative half the review asked for:

```diff
-  route form: declared body = \"json\" does not match observed application/x-www-form-urlencoded
-  route raw: declares body = \"raw\" without allow_raw = true
+  route form: declared body json does not match observed application/x-www-form-urlencoded
+  route raw: declares body raw without `unvalidated` — write `body raw unvalidated`, or the codec it takes
...
-    assert_eq!(audit_v1::human(&report), expected);
+    let human = audit_v1::human(&report);
+    assert_eq!(human, expected);
+    // Every sentence a developer reads names syntax the reader accepts: the
+    // retired assignment spellings would not parse if typed into the view.
+    assert!(!human.contains("body = \""), "{human}");
+    assert!(!human.contains("allow_raw"), "{human}");
```

(`an_empty_suite_is_no_observations_and_suppresses_unexercised_routes` pins the same raw-ingress line in its
own four-problem document, unchanged in shape.)

Not named in this node's `## The sentences` table, but it pinned the retired hint text and cannot stay green
once that text is corrected (`make test-critical` failed on it at `:1269`, the exact reason the sentence
exists). It is inside `crates/blastit/tests/**`, so the guard authorizes it; the diff **strengthens** the
assertion — one retired-string check became "names `transactions`", "names the view", and "does not name the
retired field":

```diff
@@ -1268,8 +1268,20 @@ fn transaction_control_is_a_hint_never_a_silent_flag() {
     assert_eq!(plan.transaction_control.len(), 1);
+    // The hint names the keyword a `Blastfile` writes, and the view it goes in;
+    // the retired per-route field it used to name is gone (ADR-0012 §16).
     assert!(
-        plan.transaction_control[0].contains("transaction_control = true"),
+        plan.transaction_control[0].contains("`transactions`"),
+        "{}",
+        plan.transaction_control[0]
+    );
+    assert!(
+        plan.transaction_control[0].contains("view notes"),
+        "{}",
+        plan.transaction_control[0]
+    );
+    assert!(
+        !plan.transaction_control[0].contains("transaction_control"),
         "{}",
         plan.transaction_control[0]
     );
```

`:1074`'s guard (`nothing like `[route.` may reach the `--json` document`) is untouched and still holds;
`blastfile_errors.rs`, `blastfile_discovery.rs`, the retired fixtures in `audit.rs:3679` and
`python/tests/critical/test_check_contract.py` are untouched.

## ADR-0012 / ADR-0003

`docs/decisions/ADR-0012-blastfile-dsl.md` gains decision **16**:

> **The v2 manifest is a `Blastfile`, so no schema key, field, function or sentence carries the retired
> extension** (owner decision, 2026-11-16: "zero `.toml` field names in CLI or JSON output"). The report's
> table-scope field is `table_grants.blastfile`, and the renderer that fills it is
> `TableGrants::to_blastfile`, which emits the directives a view's `postgres` block accepts. That
> supersedes the report's earlier field name only — the field's meaning, the directive syntax inside it,
> and `ADR-0003`'s atomic-write and redaction rules are unchanged.

`docs/decisions/ADR-0003-*.md` and `docs/decisions/README.md` are untouched (byte-identical to base): the
report's key name never appeared in `ADR-0003`'s text, so the supersession is recorded as the decision it
was rather than retro-edited into that ADR. `docs/reference/blastfile.md` is untouched — measured, it never
names `table_grants`, so there is no documented example to keep in step (that gap is `MB.21.5.1`'s).

## Evidence

Every `# Acceptance` item, command, and tail. Logs under `/tmp/opencode/`.

**1. No retired spelling left in a code string of the named files.**

```console
$ grep -rnE '\[(route|proxy|group)\.[a-z<>{}]|transaction_control = true|spa = true|prefork = true' \
    crates/blastit-manifest/src/validate.rs crates/blastit/src/audit_v1.rs crates/blastit/src/check.rs \
    crates/blastit/src/stage0.rs crates/blastit/src/host.rs crates/blastit-view/src/php.rs \
    crates/blastit-sqlpolicy/src/tables.rs | grep -vE '^[^:]+:[0-9]+: *(//|\*)'
rc=1                       # no output
```

**2. No `to_toml`, `"toml"` key or `.toml"` in the renderer's or the report's surface.**

```console
$ grep -rnI -E 'to_toml|"toml"|\.toml"' crates/blastit-py crates/blastit-sqlpolicy python/blastit \
    python/tests/test_fold.py python/tests/test_pytest_plugin.py
rc=1                       # no output at all (`-I` skips `_blastit*.so`; `__pycache__` clean)
```

**3. The value is directives under a `.blastfile` key.**

```console
$ grep -rn '"blastfile"' crates/blastit-py/src/audit.rs
110://!                            "functions": [], "blastfile": "# view notes postgres\n…",
326:///   "blastfile": "# view shop postgres\nfunctions unaccent\nselect shop_product(id, name)\n",
370:        map.insert("blastfile".to_string(), json!(tables.to_blastfile(locator)));
1565:            "blastfile".to_string(),
2097:, 2102:, 2105:, 2134:, 2139:, 2155:, 2160:  grants["blastfile"]   (the seven assertions)
3534:            document["blastfile"].as_str().expect("a string"),

$ grep -rn 'select notes_note(id, owner_id)' crates/blastit-sqlpolicy/src/tables.rs \
    crates/blastit-py/src/audit.rs crates/blastit-sqlpolicy/tests/ python/tests/test_pytest_plugin.py
crates/blastit-sqlpolicy/src/tables.rs:984, :991
crates/blastit-py/src/audit.rs:3538
crates/blastit-sqlpolicy/tests/policy_module_tables.rs:414
python/tests/test_pytest_plugin.py:1116   (the pinned literal, `:1205` a manifest fixture)
```

**4. Tests.**

```console
$ timeout 180 cargo test -p blastit-sqlpolicy --locked
test result: ok. 234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.31s
test result: ok. 10 passed … / 7 passed … / 25 passed … / 22 passed … / 10 passed … / 19 passed … / 8 passed (doctests)
exit 0

$ cargo test -p blastit-py
test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.61s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
exit 0

$ cargo test -p blastit --lib audit_v1::   → 6 passed      $ … --lib check::   → 12 passed
$ cargo test -p blastit --lib stage0::     → 53 passed     $ … --lib host::    → 24 passed
(the node's single command lists four filters, which cargo rejects: `error: unexpected argument 'check::'`;
 run one filter per invocation, as above — the gate text is recorded, not weakened)

$ cargo test -p blastit-view --lib php
test result: ok. 0 passed … 58 filtered out        (the `php` module is behind `feature = "php"`;
$ cargo test -p blastit-view --features php --lib php
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 58 filtered out — the real run)

$ cargo test -p blastit-manifest --test static_mounts
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit-manifest --test grant_fragment
test a_grant_with_no_directive_at_all_renders_nothing_to_paste ... ok
test a_whole_table_grant_round_trips_as_a_bare_select ... ok
test a_column_grant_round_trips_exactly ... ok
test a_name_with_a_newline_never_becomes_a_directive ... ok
test a_qualified_relation_is_a_comment_and_the_document_still_boots ... ok
test an_empty_column_list_renders_advice_the_reader_accepts ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit --lib
test result: ok. 214 passed; 0 failed; 1 ignored; …   (the ignored one is `immortal`/`php_view`'s
                                                     pre-existing re-exec child, not this node's)
$ cargo test -p blastit --test critical_check_engine
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Python suites (extension staged from `target/debug/lib_blastit.so`, as `make test-critical` does):

```console
$ PYTHONPATH=target/python-local:python uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_fold.py -q
102 passed in 0.59s     # 100 + `TestTheTableScopeHint`'s two: the hint's directives and its refusals

$ … python/tests/test_pytest_plugin.py -q (alone)
39 skipped              # module-level skipif: no PostgreSQL here (DATABASE_URL/BLASTIT_TEST_PG_URL unset);
                        # CI's `pytest-plugin` job runs it. The pinned literal is pinned twice in Rust with
                        # the same directive string, and the producer's output for that shape was checked
                        # against it directly (above), so it cannot drift unnoticed.
```

**5. Gates.**

```console
$ uv run --no-project tools/critical_guard.py --base fa7f2ef --node MB.21.4.10
  M  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [authorized by crates/blastit-sqlpolicy/tests/**]
  M  crates/blastit/tests/critical_check_engine.rs              [authorized by crates/blastit/tests/**]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.4.10     exit 0

$ make test-critical
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python   → Finished
python -m unittest discover -s python/tests/critical
Ran 50 tests in 10.490s
OK                                                                                      exit 0

$ timeout 5400 make check                                                              exit 0
166 `test result: ok` lines (165 + the new `grant_fragment` suite); guard OK; critical-guard OK;
rs-fmt clean; clippy -D warnings clean; rustdoc -D warnings clean; py-lint "All checks passed!" +
"65 files already formatted"; docs-unit OK; docs-check "No issues found"; docs-test fences executed;
manifest-purity ok; plan validate "plan OK"   (/tmp/opencode/mb21410-makecheck4.log, 4565 lines)

$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.18s                                                                exit 0
```

## Files

Changed: `crates/blastit-manifest/src/{validate,error}.rs`, `crates/blastit/src/{audit_v1,check,stage0,host}.rs`,
`crates/blastit-view/src/php.rs`, `crates/blastit-sqlpolicy/src/tables.rs`,
`crates/blastit-sqlpolicy/tests/{critical_policy_surface,policy_module_tables}.rs`,
`crates/blastit/tests/critical_check_engine.rs`, `crates/blastit-manifest/tests/grant_fragment.rs` (new),
`crates/blastit-py/src/{audit,lib}.rs`,
`python/blastit/{fold,pytest_plugin}.py`, `python/tests/{test_fold,test_pytest_plugin}.py` (the hint's
PostgreSQL-free unit tests moved to `test_fold.py`),
`docs/decisions/ADR-0012-blastfile-dsl.md`. No `Cargo.toml`/`Cargo.lock` edit; no build artefact deleted.

## Follow-ups

- **Both unrenderable shapes are fixed here, not carried.** An empty column list used to render `select t()`
  (refused) or would widen to `select t` (every column); a table/column name the reader cannot write used to
  be interpolated raw (`audit.users` refused; a newline became a directive). Both now render one advice
  comment and no directive, in both producers, and `grant_fragment.rs` parses **and validates** the result.
  What remains for `MB.21.5.1` is a *reference-page* sentence if the docs want to say what a count-only
  statement needs, or what to do about a schema-qualified relation; the grammar itself needs no change.
- **`check.rs`'s "no input variables" guard is unreachable through `blastit-env::build`**: every store a
  `postgres` block can declare resolves at least one input and produces at least one output, so `real` and
  `swapped` are both non-empty. The sentence is pinned directly (`no_input_variables`, asserted by
  `a_store_with_no_input_variables_is_a_start_failure_that_names_the_block`); if the env model ever grows a
  store that produces nothing, that test is where the behaviour gets its end-to-end twin.
- **`blastit check`'s human body sentences are fixed here** (`declared body json …`, `` declares body raw
  without `unvalidated` ``). The `--json` fix payload still carries its `"allow_raw": true` **key** — that is
  a JSON field of `fix`, not a `Blastfile` spelling (design §13.3, and `critical_check_engine.rs`'s "no fix
  may carry the acknowledgement" guard reads it), so it stays. Doc comments outside this node's paths still
  quote the retired assignment (`crates/blastit-ingress/src/lib.rs:108,426`); comment-only, for `MB.21.5.1`.
- **`php_boot`'s per-view budget parameter is gone**, not ignored: `ADR-0012` §12.3 removed `workers`, so the
  seam takes the search only and `a_php_boot_with_no_workers_cap_passes_any_budget` states the rule without
  an argument that could never bite.
- **Comment-only mentions, measured 99 lines in `crates/**/*.rs`** (plus docs prose) still spell the retired
  blocks; none is rendered. Per the node, they are recorded for `MB.21.4`'s `RESULT.md`, not fixed here.
- **The report field is undocumented.** `docs/reference/blastfile.md` never names `table_grants` (measured),
  so `table_grants.blastfile` joins the report's other fields in having no reference row. Owner:
  `MB.21.5.1`.
- **`test_fold.py`'s missing-manifest fixture** renamed `nope.toml` → `nope.blastfile` so the node's
  acceptance grep (`\.toml"`) is empty; the assertion now reads `"nope.blastfile"`. Behaviour unchanged
  (a path that is not there).
- **`pytest_plugin.py:1189`'s comment** ("no retired TOML snippet is printed") was left alone as a
  comment-only mention; its assertion (`"[route." not in stdout`) is the real guard and still holds.
