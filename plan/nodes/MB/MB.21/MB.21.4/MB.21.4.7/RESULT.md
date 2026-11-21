# Result MB.21.4.7 (attempt 3)

**`STATUS: done`** — the Python cutover is complete. Attempt 3 fixes the two
CI-only failures a local `DATABASE_URL` skip hid — the plugin's strict line still
said `egress = true` in one pinned sentence, and the fixture's stale grant said
`LIMIT 21` where the suite pins `LIMIT 1` — and closes the two minors
(`unused_global` is no longer printed or documented as reachable; the CI red
window is named).

`make check` exits `0` (166 `test result: ok`), `make test-critical` runs 50
Python tests with zero skips, and the protected change set is authorized. The 39
`test_pytest_plugin.py` cases still **skip on this host** (no reachable
PostgreSQL — §3); what they pin is now *rendered* from the real
`_summary`/`_finish`, the real audit and the fixture by seven unskipped tests,
and the live cases themselves are not claimed as run.

## 1. What landed

| Node item | Change | Pinned by |
|---|---|---|
| `fold` reads a `Blastfile` | `fold.py` plans against `blastit._blastit.blastfile_model(source)`, a binding over `Manifest::from_blastfile_str` (§15.2's one reader): stores (`name`, `protocol`, `url`, `inputs`), traits (grants by store), views (`name`, lowered methods/patterns, `use`, `egress`, grants). No Python parses the text. | `test_fold.py` (100), the protected atomicity suite (22), `audit::tests::{the_model_names_the_stores_traits_and_views, the_model_refuses_a_document_that_is_not_a_blastfile}` |
| `fold` writes only through the editor | `fold.apply_additions` builds one `BlastfileEdits` document (`{"views": [], "grants": [{view, store, queries, tables, functions}]}`) and calls `blastit._blastit.blastfile_apply_edits`. The line-level scanner/patcher is deleted. `--manifest`/`--write`/`--prefer-tables`/`--allow-dangerous` keep their semantics. | the editor's P4 (`crates/blastit-manifest/src/blastfile/write.rs`), `test_fold.py::TestEdit`, `TestCli` |
| `--prefer-tables` / `--allow-dangerous` | the merged table scope goes in `ProposedGrant.tables`, functions in `.functions`; the editor rewrites the view's `postgres [store] { }` directives to the merged scope (§13.1) and refuses an unwritable name or an empty column list. `fold` still refuses `pg_catalog`, a lock with no column, a literal, a redacted value and a multi-statement text first. | `test_fold.py::{TestPlanning, TestLockGrants, TestDangerousStatements, TestFunctionGrants, TestAFunctionGrantSurvives}`, `TestTheCliDiagnostics::test_write_refuses_a_manifest_it_cannot_edit_safely` |
| `_blastit.audit` off the retired reader | `audit()` calls `Manifest::from_blastfile_str` + `validate_default`. | `audit.rs`'s 61 tests, `the_audit_reads_the_blastfile_reader_and_not_the_retired_one` |
| `audit.rs` fixtures translated | 18 TOML fixtures → `Blastfile`; the global-`groups` cases → traits attached by `use`; `unused_global` empty by construction (§7.1). | `cargo test -p blastit-py`, `make check` |
| the plugin reads a `Blastfile` | `_postgresql_proxies`/`_postgresql_url_variable` call `_blastit.blastfile_model`; `--blastit-proxy`/`--blastit-prefer-tables`/`--blastit-live-server` help and the runtime messages name the `Blastfile`/store/view. | `test_plugin_helpers.py`, `test_pytest_plugin.py` |
| **the plugin's hints are `Blastfile`** | `_finish`/`_summary` emit `needs transactions (add \`transactions\` to its postgres block)`, `has no egress any`, and `put them in a trait attached to every view (\`trait connection { postgres { … } }\`, then \`use connection\`)`; the strict line says `without egress any`. Every retired spelling in the docstrings (`groups = ["connection"]`, `egress = true`, `transaction_control = true`, `[route.<proxy>.tables]`) is gone. | `test_pytest_plugin.py::{test_the_terminal_summary_is_short_and_actionable, test_strict_fails_the_session}` **and** the unskipped `test_project_manifest.py::{test_the_summary_hints_name_directives_the_reader_accepts, test_the_strict_line_names_the_directives_the_file_spells}`, which render the strings from the real `_summary`/`_finish` |
| **the `unused_global` branch is gone** | `_summary` no longer prints `grant(s) of a global group nothing matched` — a `Blastfile` has no global groups (§7.1), so the count is always 0. The report key stays and is documented as always empty; removing it from the document is `MB.21.4.5`'s Rust sweep (`MB.21.5.1`'s page). | `test_project_manifest.py::test_the_summary_hints_name_directives_the_reader_accepts` (`"global group" not in text`) |
| **the prefer-tables hint is `Blastfile`** | `_table_snippet` renders `select <table>(…)` / `insert` / `update` / `delete` and `functions …` from the report's `table_grants.tables`, instead of printing the report's retired `toml` snippet. | `test_pytest_plugin.py::test_prefer_tables_prints_the_snippet`; `test_project_manifest.py::test_the_summary_hints_name_directives_the_reader_accepts` (the rendered directives) |
| **the project fixture** | `python/tests/project/blastit.toml` → `python/tests/project/Blastfile`: the same 13 views, the same grants (`note-delete`'s stale `… LIMIT 1`, `orders`' `… LIMIT 1`), the same deliberate holes, `note-cache { egress any }`, `note-delete {id:int}`. | `python/tests/test_project_manifest.py::{test_the_fixture_is_a_blastfile_the_one_reader_accepts, test_the_fixture_grants_the_stale_limit_one_the_suite_pins}`, `blastit check` (§3) |
| **the repair loop** | `repaired_manifest` reads the fixture with `_blastit.blastfile_model`, applies the report to the model (`query`, `transactions`, `egress any`, `trait connection` + `use connection`, a `fallback` view), and `dumps_blastfile` writes it; the old `tomllib`/`dumps_manifest`/`_toml` TOML writer is deleted. | `python/tests/test_project_manifest.py::test_a_repaired_model_carries_every_fix_the_report_names` |
| the plugin's warnings | `test_pytest_plugin.py`'s `manifest.warnings` assertion pins §13.2's producer sentence verbatim (`view "note-cache": egress any opens any outbound address and port`). | that test; `error.rs` |
| reachability | `grep -rnE 'tomllib|from_toml_str' python/blastit` is empty; `fold`, the plugin and `audit()` each run their real parse path. | acceptance grep; the reachability test |

`blastfile_model` now carries two more fields the repair needs: each store's
`inputs` (`{slot: VARIABLE}`, so a split store round-trips) and each view's
`egress`. Both are asserted in `the_model_names_the_stores_traits_and_views`.

`_write_atomic` is unchanged (`.{name}.{pid}.tmp`, `O_EXCL | O_NOFOLLOW`,
`fsync`, `os.replace`, directory `fsync`, mode preservation); only *what* it
writes changed (the editor's proved text).

## 2. Changed assertions, and why none is weaker

### Protected files

`python/tests/critical/test_fold_literals_and_atomic.py` — every literal-form
refusal, the `…` refusal, the `--allow-dangerous` twin, the value-free stderr
assertions and the three temp-file/atomicity tests are kept. Changed: the
fixture is a `Blastfile`; `planned()` takes `fold.model(MANIFEST)`;
`test_the_happy_path…` asserts the editor's `query '…'` text **and** the
reader's model; the file name in the temp-file tests is `Blastfile`. 22 before,
22 after.

`python/tests/critical/test_check_contract.py` — one fixture: the bad manifest
is `view a "/" {\n` instead of `version = 1\n[[route]\n…`; exit `2`, empty
stdout, the `blastit check:` prefix and "the suite never ran" are unchanged.

### `test_fold.py`

Deleted because the subject is gone (no silent skip):

| Deleted | Why | Replacement coverage |
|---|---|---|
| `TestTheScan` (6) | `_Scan`, `_find_unquoted`, `_quoted_end` are gone; the editor never splits text | `blastit-manifest/tests/{blastfile_fmt.rs, edit_branches, critical_blastfile_print.rs}` |
| `TestTheEditAgainstHostileShapes` (4) | one-line arrays, multi-line strings, inline tables and nested `[route.db.tables.x]` do not exist in a `Blastfile` | the same Rust suites |
| `test_a_one_line_array_becomes_a_multi_line_one`, `test_a_grant_written_as_sections_alone_takes_a_queries_key`, `test_a_nested_table_section_is_a_grant_too`, `test_a_queries_key_that_is_not_an_array_is_refused` | one `postgres { }` block per store; no arrays, no "sections of a grant", no `queries =` key | — |
| `test_a_route_that_appeared_is_refused`, `test_a_grant_that_changed_something_else_is_refused`, `test_a_proxy_that_changed_is_refused` | the editor's P4 compares the whole lowered model; a Python copy would be a second oracle | `critical_manifest_edit.rs`, `edit_branches`; `TestTheEditIsProved` keeps the two Python-visible last words |

Translated (same or stronger): `group`/`global group` → `trait` / `trait every
view uses`; `test_the_refusal_counts_the_sections…` → the editor refusal naming
the view **and** the store; `test_a_table_written_as_its_own_header…` → existing
table scope merged not duplicated (asserts the model and one directive);
`test_a_group_only_route_says_which_group` →
`test_a_trait_granted_view_gets_its_own_block` (exit `0` and a written,
unioned grant instead of the retired patcher's exit `1` refusal — the grant is
proved, not merely refused); `test_a_reason_becomes_one_bounded_comment` →
`test_a_hostile_reason_never_reaches_the_file`; `test_a_long_reason_is_cut` →
`test_the_header_is_bounded_and_one_line`; `test_a_statement_is_an_escaped_toml_string`
→ a round-trip through the editor plus the direct `\"`/`\\` assertions;
`test_a_table_name_needing_quotes_is_quoted` → the grammar-unspellable name
refused (the denied path); `test_a_control_character_is_escaped` → a round-trip;
`test_an_inline_tables_grant_is_left_alone…` → an unrelated store block left
alone; `test_write_refuses_a_manifest_it_cannot_edit_safely` → a real
`SELECT id FROM "weird name"` refused by the editor; `test_the_toml_to_add_is_printed`
→ `test_the_grants_to_add_are_printed`. The planning, render, CLI,
dangerous-statement, function-grant and extension classes keep every
allowed/denied pair; only the fixture and the nouns change.

### `test_pytest_plugin.py`

| Assertion | Was | Now | Why not weaker |
|---|---|---|---|
| fixture paths | `blastit.toml` | `Blastfile`, `Blastfile-repaired`, `Blastfile-two-stores`, `Blastfile-broken`, `Blastfile-tables` | the fixture is a `Blastfile`; the names are just paths |
| terminal summary | `needs transaction_control = true`; `has no egress = true`; `group attached to every route` | `needs transactions (add \`transactions\` to its postgres block)`; `has no egress any`; `trait attached to every view (\`trait connection { postgres { … } }\`, then \`use connection\`)` | each hint names a directive the one reader accepts |
| egress assertions (:336/:349) | `"no egress = true on this route"`, `"this route has egress = true"` | `"no egress any on this view"`, `"this view has egress any"` | the message names the directive the `Blastfile` has |
| strict line (:631) | `without egress = true` | `without egress any` | same counter, bootable fix; the sentence is rendered by `test_the_strict_line_names_the_directives_the_file_spells` |
| **stale grant** | fixture said `… DESC LIMIT 21` (and `orders … LIMIT 21`) | `… DESC LIMIT 1` (and `orders … LIMIT 1`), i.e. the retired document's text | normalisation keeps the constant, so the pinned `unused` list (`STALE_GRANT`) only matches `LIMIT 1`; `test_the_fixture_grants_the_stale_limit_one_the_suite_pins` asserts the fixture's grant **is** `STALE_GRANT`, has exactly two `LIMIT 1`s and no `LIMIT 21`, and `test_the_fixture_renders_the_pinned_unused_and_totals_through_the_audit` renders `unused == [STALE_GRANT]`, `totals["unused"] == 2` and `totals["missing"] == 1` through the real audit. The second `unused` line is `note-create`'s body `SELECT`, which the retired document had too; `orders` is unexercised, so its grant is never an `unused` line |
| prefer-tables snippet | `[route.db.tables]` / `notes_note = { select = ["id"], delete = true }` | `select notes_note(id)` + `delete notes_note`, and `[route.` is asserted absent | the hint is now the directive a `Blastfile` takes |
| repair | TOML writer, `groups = ["connection"]`, `egress = true`, `transaction_control = true` | model-driven writer, `trait connection` + `use connection`, `egress any`, `transactions` | the report is applied with the same rules the one editor writes |
| `"egress = true" in repaired` | — | `"egress any" in repaired` | the string that must be in the file is the one that boots |

### `audit.rs`

The 18 fixtures are `Blastfile`s; the global-group cases are traits attached with
`use` (`GLOBAL`/`GLOBAL_TABLES` → traits on every view, and `unused_global` is
empty by construction). One case is **deleted**: 
`a_global_group_table_grant_nothing_needed_is_reported_once` — a trait's unused
table scope is the view's own grant, and this report only lists a view's unused
**queries**; an unused table scope is `blastit check`'s `dead_grant`. The kept
`a_trait_table_grant_a_statement_needed_is_used` pins the positive side.
`a_statement_connection_setup_accounts_for_is_used_rather_than_unused` moves the
`set_config` control statement to the view's own `unused` (the trait is the
view's grant) and asserts `unused_global == 0`. One behaviour change, required by
that move and a strengthening: `assemble` builds the `connection_setup` section
before the route loop and excludes `setup.accounted` from a view's `unused`, so a
statement a trait grants to every view and that only runs while a connection
opens is not reported dead. Three tests were added:
`the_model_names_the_stores_traits_and_views`,
`the_model_refuses_a_document_that_is_not_a_blastfile`, and
`the_audit_reads_the_blastfile_reader_and_not_the_retired_one` (a retired-shape
document is refused by the one parser — the reachability pin). 61 tests pass.

## 3. Acceptance evidence (final tree)

```console
$ timeout 1800 make test-critical ; echo EXIT=$?
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # critical Rust
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 3.843s
OK
EXIT=0

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/critical/test_check_contract.py python/tests/critical/test_fold_literals_and_atomic.py \
    python/tests/test_fold.py python/tests/test_pytest_plugin.py -q
........................................................................ [ 39%]
........................................................................ [ 78%]
sssssssssssssssssssssssssssssssssssssss                                  [100%]
144 passed, 39 skipped in 3.49s

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_project_manifest.py -q          # unskipped, no DB
.......                                                                  [100%]
7 passed in 0.08s

$ grep -rnE 'tomllib|from_toml_str' python/blastit --include='*.py'
(no output; exit 1)

$ uv run --no-project tools/critical_guard.py --base fa6dc39 --node MB.21.4.7
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.4.7

$ timeout 5400 make check > /tmp/opencode/make-check-attempt3b.log ; echo EXIT=$?
EXIT=0
$ grep -cE 'test result: ok' /tmp/opencode/make-check-attempt3b.log
166
$ grep -E 'test result: FAILED|make: \*\*\*' /tmp/opencode/make-check-attempt3b.log
(no output)
```

### The 39 cases that can only run with `DATABASE_URL`

`python/tests/test_pytest_plugin.py` is `pytestmark = skipif(not DATABASE_URL,
"needs a real PostgreSQL: set DATABASE_URL or BLASTIT_TEST_PG_URL")`. On this
host both variables are **unset**; PostgreSQL listens on 5432 and 5433, and both
refuse without a password (`psql -w … → fe_sendauth: no password supplied`), so
the suite skips as a whole. No credential was guessed and no `.env` was read.

Their fixtures now **parse**, proved without a database:

```console
$ # a copy of python/tests/project whose tests/ is one no-DB stub, so `check`
$ # reads the fixture and runs a suite without a PostgreSQL:
$ PYTHONPATH=…/python DATABASE_URL='postgresql://u:p@127.0.0.1:5432/app' \
    uv run --no-project --with pytest==9.2.0 python -m blastit check --json
{
  "schema_version": 1,
  "ok": false,
  "requests": 0,
  "statements": 0,
  "routes": {"total": 13, "exercised": 0},
  "proxies": {"db": {"statements": 0, "denied": 0}},
  …
}
EXIT=1        # no observations from the stub suite — not exit 2: the Blastfile parsed
```

and the seven unskipped tests in `python/tests/test_project_manifest.py` pin the
fixture and every string the skipped suite asserts:

* the fixture is a `Blastfile` the one reader accepts (13 views, one `db` store,
  no `blastit.toml` beside it);
* `dumps_blastfile` round-trips the fixture's whole model exactly
  (`again == parsed`);
* `repaired_manifest` on a synthetic report writes `query`, `transactions`,
  `egress any`, `trait connection { … }` + `use connection` (including
  `query "SET TIME ZONE 'UTC'"`) and a `fallback` view, and the reader returns
  exactly those fields;
* the pinned `unused` list is the fixture's own grant: `note-delete`'s grant is
  exactly `STALE_GRANT` (`… DESC LIMIT 1`), the fixture has exactly two
  `LIMIT 1`s and no `LIMIT 21`;
* the pinned `unused` list and totals **render through the real audit** with the
  two request shapes the suite makes (no PostgreSQL involved): `note-delete`'s
  `unused` is `[STALE_GRANT]`, `note-create`'s is its body `SELECT`,
  `totals["unused"] == 2` and `totals["missing"] == 1` — a fixture that said
  `LIMIT 21` fails here rather than behind a CI skip;
* `_summary` renders the three hints the skipped suite asserts — `needs
  transactions (add \`transactions\` to its postgres block)`, `has no egress any`,
  the `trait connection { … }` + `use connection` hint — and the
  `select notes_note(id)` / `delete notes_note` snippet, and no retired spelling
  and no global-group line;
* `_finish` renders the exact strict sentence the skipped suite asserts
  (`… 0 route(s) opening connections without egress any …`) and sets
  `TESTS_FAILED`.

These seven tests open no database and do **not** stand in for the 39 live
sessions: they prove the strings and the fixture those sessions read — exactly
what the local skip used to hide — while CI's `pytest-plugin` job runs the
sessions themselves.

## 4. What is deliberately left, and the red window it opens

`pytest python/tests` in CI is **red until `MB.21.4.4` lands** — CI sets
`DATABASE_URL`, so the 39 cases run, but three suites still hand the cutover
plugin a retired document:

* **`python/tests/test_live_server.py`** uses `python/tests/live_project/`
  (line 180) and `python/tests/packaged_project/` (line 155);
* **`python/tests/live_project/blastit.toml`** and
  **`python/tests/packaged_project/blastit.toml`** are still the retired
  document, so the plugin's `blastitfile_model` read raises
  `UsageError: blastit: the Blastfile does not parse: …`;
* **`python/tests/test_check.py`** was already red at the base (`fix.to` is
  `view catalog postgres` since `MB.21.4.3`) and writes its own `blastit.toml`.

All three are `MB.21.4.4`'s (`python/tests/**`, "the remaining python/tests
fixtures"). This leaf did not translate them because the node scopes its
translation to the four suites it names; translating them is the first thing 4.4
does, and it closes the window. `make check` never runs any of them, so this
tree is green on every gate the acceptance command names.

## Follow-ups

- **The CI red window above** (4.4): `test_live_server.py`, `live_project/` and
  `packaged_project/`, plus `test_check.py`.
- **`TableGrants::to_toml`** (the report's `table_grants.toml`) is the last TOML
  writer on a default path; the plugin no longer prints it, but the report still
  carries the field. `MB.21.4.5`'s sweep re-points it (or drops the field in
  favour of `table_grants.tables`); `blastit-sqlpolicy` is outside this node.
- **The report's `unused_global` section** is always empty now (a `Blastfile` has
  no global groups, §7.1). The plugin documented that and stopped printing it;
  removing the key from the document is `MB.21.4.5`'s Rust sweep, adjacent to
  `MB.21.5.1`'s page.
- `MB.21.4.5`: `Manifest::from_toml_str`, `parse.rs`, `edit.rs`'s retired patcher
  and the `toml` dependency are unreachable from Python, `blastit check` and
  `blastit dev --learn` alike.

## Files

Changed: `crates/blastit-py/src/{audit,lib}.rs`,
`python/blastit/{fold,pytest_plugin,_check_pytest}.py`,
`python/tests/{test_fold,test_pytest_plugin,test_plugin_helpers}.py`,
`python/tests/critical/{test_fold_literals_and_atomic,test_check_contract}.py`,
`python/tests/project/views.py` (one docstring).
Added: `python/tests/project/Blastfile`, `python/tests/test_project_manifest.py`.
Deleted: `python/tests/project/blastit.toml`. Outside the globs: one line in
`docs/knowledge/LESSONS.md`. No `Cargo.toml`/`Cargo.lock`, no `plan/` frontmatter,
no docs page.
