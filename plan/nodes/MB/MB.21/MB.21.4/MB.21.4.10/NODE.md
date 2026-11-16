---
id: MB.21.4.10
title: "Delete the last retired spellings from user-facing sentences and give the report's table scope a `blastfile` key"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.4.5.6]
paths:
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/**"
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/host.rs
  - "crates/blastit/tests/**"
  - "crates/blastit-view/src/php.rs"
  - "crates/blastit-view/tests/**"
  - "crates/blastit-sqlpolicy/src/tables.rs"
  - "crates/blastit-sqlpolicy/tests/**"
  - "crates/blastit-sqlpolicy/tests/critical_policy_surface.rs"
  - "crates/blastit-py/src/**"
  - python/blastit/pytest_plugin.py
  - python/blastit/fold.py
  - python/tests/test_fold.py
  - python/tests/test_pytest_plugin.py
  - docs/reference/blastfile.md
  - "docs/decisions/ADR-0012-blastfile-dsl.md"
attempts: 3
max_attempts: 4
escalation: none
created: "2026-11-16T18:58"
updated: "2026-11-16T21:41"
---

# Goal

Delete the last **user-facing** sentences that still tell a developer to write the retired manifest
format, and make the report's table-scope value a pasteable `Blastfile` fragment under a `.blastfile`
key.

Owner rule (2026-11-16, amending `ADR-0012` §14): **zero `.toml` field names may survive anywhere in
Blastit's CLI or JSON output.** The v2 manifest is a `Blastfile`, so no schema key, field, function
name or sentence may carry the retired extension. Where the retired name is only a *record* (a
`done`/`superseded` plan brief, a `LOG.md` entry, `ADR-0012`'s own decision 14) or a *guard*
(`MB.21.5.2`'s retired-name test, `critical_check_engine.rs:1074`), it stays.

The producer is real, not a no-op: `TableGrants::to_toml` still renders TOML and the report still
publishes it under `"toml"`.

## The renderer (`crates/blastit-sqlpolicy/src/tables.rs:504`)

`pub fn to_toml(&self, prefix: &str) -> String` writes `[route.db.tables]` plus
`x = { select = ["id", "name"] }`. It becomes **`pub fn to_blastfile(&self, locator: &str) -> String`**
and emits the directives a view's `postgres` block accepts (`docs/design/blastfile-dsl.md` §6.1),
so the value is valid `Blastfile` when pasted:

| Grant | Directive |
|---|---|
| `Columns::List({"id","name"})` on `select` | `select x(id, name)` |
| `Columns::All` | `select x` (no parentheses) |
| `insert` / `update` | `insert x(col)`, `update x(id)` |
| `delete = true` | `delete x` |
| `functions = ["unaccent"]` | `functions unaccent` |
| lock-only row that cannot name a column | no `update` line, plus the existing `# … name one here` advice kept as a `#` comment |
| empty grant | `""` |

`locator` is the Blastfile node the fragment belongs in — `view <name> postgres` (unnamed store),
`view <name> postgres <store>`, or `trait <name>` — and is rendered as **one leading comment line**
`# <locator>` (Blastfile `#` trivia), so the pasted value says where it goes. No `route.db` string
survives anywhere. Rename the private helpers with it (`columns_toml`, `key_toml`, `string_toml`): a
function that emits Blastfile must not be named for the format it no longer writes.

## The report (`crates/blastit-py/src/audit.rs`)

`:366` (`covering`) and `:1552-1553` (`table_grants_json`) insert `"toml": to_toml(prefix)` /
`to_toml(&format!("route.{proxy}"))`. The key becomes **`blastfile`**, and each caller passes a real
locator (`view {name} postgres…`); the seven assertions at `:2071`, `:2076`, `:2097`, `:2102`,
`:2118`, `:2123`, `:3494` and the doc comments at `:110`, `:326` follow. `python/blastit/fold.py:564`
already passes a store locator (`postgres`, `postgres analytics`) — it becomes
`view {route_name} postgres…`, and `fold.py` reads only `tables`/`functions`/`unanalysable`, so its
behaviour is unchanged. `python/blastit/pytest_plugin.py:292`'s docstring names `TableGrants::to_toml`
and calls the key "the retired snippet": both sentences are corrected (docstring only; its
`_table_snippet` renderer already emits these directives and does not change).

Keys that do not move: `route`, `proxy`, `schema_version: 1` (`ADR-0003`, `R22`).

## The sentences, none reachable by `make check` today

| Where | Today | Must name instead |
|---|---|---|
| `crates/blastit-manifest/src/validate.rs:556` | `dead("[route.ingress]")` | the view's `body` directive (`body json`, `body raw unvalidated`, …) from `IngressPolicy { body, allow_raw }` |
| `crates/blastit-manifest/src/validate.rs:562` | `dead(format!("[route.{proxy}]"))` | the store block: `postgres { … }` / `postgres <name> { … }` |
| `crates/blastit-manifest/src/validate.rs` (`SPA_NEVER_REACHES_FRONT_CONTROLLER`, both `why` values, `dead("spa = true", …)`, `dead("prefork = true")`) | `spa = true`, `prefork = true`, `the manifest declares no app` | `fallback "<file>"` (the directive that replaced `spa`), `prefork`, the file's `app` |
| `crates/blastit/src/audit_v1.rs:1528` | `set transaction_control = true on [route.{}]` | `add \`transactions\` to \`view <name>\`'s \`postgres\` block (or to a trait it \`use\`s)` |
| `crates/blastit/src/check.rs:457` | `declare [proxy.<name>] with protocol = "postgresql"` | `declare a \`postgres\` store: \`postgres $DATABASE_URL\`` |
| `crates/blastit/src/check.rs:492` | `name them in [proxy.{}]` | `name them in that store's block: \`postgres { user $DB_USER; password $DB_PASSWORD }\`` |
| `crates/blastit/src/stage0.rs:1424` | `route "app": [route.php] workers = …` | nothing: delete `php_worker_budget` with its call site (see Notes) |
| `crates/blastit/src/host.rs:2064` | claim label `route "app": [route.php] entry` | a label naming `entrypoint php` |
| `crates/blastit-view/src/php.rs:100` | `entry … must be an absolute path ([route.php] entry)` | `entry … must be an absolute path (the path \`entrypoint php\` resolved to)` |

# Context

- Design: `docs/design/blastfile-dsl.md` §6.1 (grant grammar), §12.3 (`workers` removed,
  `PhpPolicy.workers` permanently `None`), §13.3 (the `--json` keys `route`/`proxy`/
  `schema_version: 1` stay), §15 (erasure); `ADR-0012` decisions 1–15; `ADR-0003` (the report and
  its atomic write).
- **Measured on this branch at `7ba6d23`**, and re-measured with `MB.21.4.5.6`'s uncommitted work in
  the tree (it touches none of these files):

  ```console
  $ grep -rnI -E 'to_toml|"toml"|\.toml"' crates/blastit-py crates/blastit-sqlpolicy python/blastit \
      python/tests/test_fold.py python/tests/test_pytest_plugin.py | grep -v __pycache__
  crates/blastit-sqlpolicy/src/tables.rs:170,504          the doc link and `pub fn to_toml`
  crates/blastit-sqlpolicy/src/tables.rs:968..1316        its unit tests, all passing "route.db"
  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:740,761   PROTECTED, see below
  crates/blastit-sqlpolicy/tests/policy_module_tables.rs:412,434,461,472
  crates/blastit-py/src/audit.rs:110,326,366,1552,1553    producers + JSON examples
  crates/blastit-py/src/audit.rs:2071,2076,2097,2102,2118,2123,3494   assertions
  python/blastit/pytest_plugin.py:292                     docstring
  python/tests/test_fold.py:178,268,303,323,338,470       stubs
  python/tests/test_pytest_plugin.py:1115                 the pinned TOML literal
  $ grep -rn 'toml' crates/blastit/src/audit_v1.rs        (no output — the CLI report is clean)
  $ grep -rn 'toml' python/tests/critical/test_check_contract.py
  :512, :695  only `pyproject.toml` — the protected contract does not pin the key, so it is NOT edited
  ```

  `python/tests/runnable_docs/harness.py:3555` is **not** a field: it is
  `BLOCK_CHECKS = {"yaml": …, "toml": _static_toml, "sql": …}`, the markup-language → validator map
  (`docs/{ci,getting-started,index,local}.md` still open ```` ```toml ```` fences). Renaming that key
  would stop every TOML fence being parsed and cannot become `"blastfile"`, because no page opens a
  ```` ```blastfile ```` fence (`ADR-0012` §12). It stays, and `harness.py` is not in `paths`.
- **`crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:735-770` is protected and calls the
  renamed function.** It is authorized by its own explicit path, and the two tests
  (`a_covering_grant_writes_the_lock_it_can_name`, `a_lock_without_a_column_is_reported_not_widened`)
  keep every structural claim: a lock names a column it reads, a lock never widens to all columns, an
  unnamable lock invents no `update` entry and says why. Only the spelling each assertion looks for
  changes (`update = ["id"]` → `update t(id)`). `RESULT.md` quotes the diff.
- `stage0::php_worker_budget` cannot fire: `PhpPolicy.workers` is `None` for every `Blastfile` (§12.3),
  so `route.php()?.workers?` never matches. Deleting it (and its call site, and the test doc at
  `stage0.rs:5463`) is the fix, not re-wording it.
- **Fixtures that deliberately prove the retired text is an ordinary syntax error are legitimate and
  must not be "cleaned"**: `crates/blastit-manifest/tests/blastfile_errors.rs`, the retired fixtures in
  `crates/blastit-py/src/audit.rs` (`:3630`, `:3638`), `blastfile_discovery.rs`, and
  `crates/blastit/tests/critical_check_engine.rs:1074` (a **guard**: nothing like `[route.` may reach
  the `--json` document; it must keep holding, unmodified).
- `docs/reference/blastfile.md` is measured clean: it documents `--json`'s findings (`route`, `proxy`,
  `fix.to`) and never the plugin report's `table_grants`, so no documented example is in step with this
  value. The report field is **undocumented** — that gap is `MB.21.5.1`'s, recorded in `RESULT.md`; the
  page gets no new rows here.
- Comment-only mentions (≈115 across 32 files) are not this leaf's work: list them in `MB.21.4`'s
  `RESULT.md`. The exceptions are the doc comments this leaf's own edits would leave lying —
  `error.rs`'s `StaticDeadDeclaration` list, `lib.rs`/`pytest_plugin.py`'s schema sentences,
  `stage0.rs:5463` — which are corrected because they describe text changed here.

# Acceptance

```console
# 1. No retired spelling left in a code string of the named files (comment-only lines excluded).
$ grep -rnE '\[(route|proxy|group)\.[a-z<>{}]|transaction_control = true|spa = true|prefork = true' \
    crates/blastit-manifest/src/validate.rs crates/blastit/src/audit_v1.rs crates/blastit/src/check.rs \
    crates/blastit/src/stage0.rs crates/blastit/src/host.rs crates/blastit-view/src/php.rs \
    crates/blastit-sqlpolicy/src/tables.rs | grep -vE '^[^:]+:[0-9]+: *(//|\*)'
(no output)

# 2. The owner's rule on the renderer's surface: no function, key or rendered heading.
$ grep -rnI -E 'to_toml|"toml"|\.toml"' crates/blastit-py crates/blastit-sqlpolicy python/blastit \
    python/tests/test_fold.py python/tests/test_pytest_plugin.py
(no output at all)

#    Build artefacts (`python/blastit/_blastit*.so`, `__pycache__/**`) match a bare grep: use `-I` and
#    never "fix" or delete them. `pyproject.toml` is a real file path and is untouched.

# 3. The value is directives under a `.blastfile` key.
$ grep -rn '"blastfile"' crates/blastit-py/src/audit.rs
(two producers)
$ grep -rn 'select notes_note(id, owner_id)' crates/blastit-sqlpolicy/src/tables.rs \
    crates/blastit-py/src/audit.rs crates/blastit-sqlpolicy/tests/ python/tests/test_pytest_plugin.py
(at least one line in each of the four — the same directive literal everywhere)

# 4. Tests (the Makefile's RUST_ENV/PYO3_PYTHON is what `make rs-test` sets).
$ timeout 180 cargo test -p blastit-sqlpolicy --locked
$ cargo test -p blastit-py
$ cargo test -p blastit --lib audit_v1:: check:: stage0:: host::
$ cargo test -p blastit-view --lib php
$ cargo test -p blastit-manifest --test static_mounts
(all exit 0)

# 5. Gates.
$ uv run --no-project tools/critical_guard.py --base <dispatch base> --node MB.21.4.10
(clean: `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` is authorized by its own path,
 and quoted in RESULT.md)
$ make check
$ make test-critical
$ make docs-check
$ uv run --no-project tools/plan.py validate
(all exit 0)
```

Per-string test mapping (a test asserting only `is_err()` is not enough):

| New text | Test asserting it |
|---|---|
| `body …` dead declaration | `crates/blastit-manifest/tests/static_mounts.rs`: accepted twin (a view that reaches an app keeps `body`/`prefork`) + denied twin (an app-less mount refuses with the `body` spelling) |
| `postgres { … }` / `postgres <name> { … }` dead declaration | same file: denied twin asserting the store-block spelling and that no `[route.` survives |
| `add \`transactions\` to \`view …\`'s \`postgres\` block` | `crates/blastit/src/audit_v1.rs` `mod tests`: a `--learn` plan whose hint names `transactions`/`view` and no `[route.` |
| `postgres $DATABASE_URL` (pytest-django, no store) | `crates/blastit/src/check.rs`: update `a_pytest_django_project_without_a_postgresql_proxy_is_a_start_failure`, keep its `expect_err` denial |
| `postgres { user $…; password $… }` (no input variables) | `crates/blastit/src/check.rs`: the missing-variable denial asserts the new sentence |
| `entrypoint php` budget (deleted) | `crates/blastit/src/stage0.rs`: `a_php_boot_with_no_workers_cap_passes_any_budget` stays; the keyword's own refusal is the grammar's test and is not weakened |
| claim label | `crates/blastit/src/host.rs`: the provenance test asserts the new label in the refusal |
| `entry … (the path \`entrypoint php\` resolved to)` | `crates/blastit-view/src/php.rs` tests: strengthen the relative-entry case from `is_err()` to the exact text; no `[route.` |
| `select notes_note(id, owner_id)` | `tables.rs` unit tests (empty grant → `""`, `*` → no parentheses, `delete`, `functions` first, `# <locator>` header, lock advice comment), `policy_module_tables.rs`, `critical_policy_surface.rs`, `audit.rs` JSON tests, `test_pytest_plugin.py:1115`, `test_fold.py` stubs |

# Notes

- **Order.** (1) `tables.rs`: `to_toml` → `to_blastfile` emitting the fragment, with its unit tests and
  the protected surface's spelling. (2) `blastit-py`: `"blastfile"` + locators + JSON examples + the
  seven assertions; `fold.py`'s and `pytest_plugin.py`'s sentences. (3) the six sentence re-spellings
  with their twins. (4) the `ADR-0012` line. (5) the gates.
- **One rendering, decided here**, because the report value and the plugin's `_table_snippet` print the
  same directives. Lines, in order: `# <locator>` (see below); `functions a, b` when the grant calls
  one beyond the safe set; then for every table in sorted order `select <t>(<cols>)`, `insert`,
  `update`, `delete <t>`; `select <t>` with no parentheses for `Columns::All`; absent operations emit
  nothing; no directives at all → `""` (the locator is not emitted alone). A lock-only row keeps its
  advice as a `#` comment above that table's lines and still omits `update`. `_table_snippet` (the
  terminal hint, `MB.21.4.7`'s) prints the directive lines without the header — that is the only
  allowed difference, and the directive lines themselves must be character-identical.
- **ADR-0012**, `## Decision`, gains one numbered line: the v2 manifest is a `Blastfile`, so no schema
  key, field, function or sentence carries the retired extension; the report's table-scope field is
  `table_grants.blastfile`, which **supersedes that single field name in `ADR-0003`** — the field's
  meaning, the directive syntax inside it and `ADR-0003`'s atomic-write/redaction rules are unchanged.
  `docs/decisions/README.md`'s `ADR-0003` row stays as it is (a record of what was decided then).
- `docs/reference/blastfile.md` gets **no new rows**: measured, it never names `table_grants`. If the
  implementer finds a mention, it becomes `table_grants.blastfile` with a `Blastfile` value; nothing
  else on the page changes (`MB.21.5.1` owns docs prose).
- **Do not touch** `python/tests/runnable_docs/harness.py` (its `"toml"` is a fence-language key, not a
  field), the deliberate retired-format fixtures listed in `# Context`, or `test_check_contract.py`.
- Protected files: only `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs` changes, authorized
  by its explicit `paths` entry; `RESULT.md` quotes the diff and names which claims were kept. If any
  other `critical_*` file must change, stop and record it in `RESULT.md` for `lead` first.
- No `Cargo.toml`/`Cargo.lock` edit (`MB.21.4.5.6` owns the dependency removal). Do not run `cargo
  update`, and do not delete `python/blastit/_blastit*.so`.
- Leave the ≈115 comment-only mentions alone: list them in `MB.21.4`'s `RESULT.md`. Follow-ups go there
  too — including the undocumented report field — each naming the sibling that owns it.
