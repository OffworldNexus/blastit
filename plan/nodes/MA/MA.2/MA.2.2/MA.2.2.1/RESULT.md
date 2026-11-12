# MA.2.2.1 — Manifest 0.1.1 schema, reserved-key guard and boot warnings

## Summary

Implemented the `blastit.toml` 0.1.1 schema in `blastit-manifest`:

- `Manifest::app` / `Manifest::preload` (string or array, order kept, duplicates
  dropped), with `version`/`app`/`preload` reserved inside any table and
  `groups` reserved everywhere except top level and `[[route]]` elements. The
  failure is the exact documented line
  `<key> after a [table] is not top-level; move it above the first table` and it
  is reported alone (a scoping mistake makes every other reading of the file
  untrustworthy).
- `[route.ingress]` → `IngressPolicy` (`body` required, `json|form|none|raw`;
  `allow_raw` accepted only on `raw`, otherwise `UnknownKey`), plus
  `Route::with_ingress` / `Route::ingress`.
- `Route::is_unrestricted_catch_all`, true for `path = ".*"`, `path = ['.*']`
  and `path = ['^.*$']`, false when `methods` restrict the route.
- `Manifest::validate` (unchanged signature) now also refuses an `app` without
  `:`, every `queries` entry holding `…` (`RedactedQueryPlaceholder`) and every
  entry inlining a quoted literal outside `KNOWN_STATIC_SETUP_SQL`
  (`LiteralQuery`). Grant text is normalised first, so the wire spelling
  `SELECT set_config('TimeZone', $1, false)` is exempt like its `$?` form.
  Neither message prints the entry: the position (`queries[N]`) and the fix are
  named instead, so no value-bearing literal can be echoed, logged, or survive
  into an allowlist (`R24`).
- `ManifestWarning::{RawIngress, EgressRoute}`, one per route, in route order.
- `Manifest::is_query_covered_by_tables_or_groups(route_index, proxy,
  normalized_sql)`: the route's own `queries` are excluded (an entry must not be
  its own coverage) while its `tables`, and the queries/tables/`unrestricted` of
  its global and route groups, are consulted; `$?` is rewritten for the
  analysis parser with quoted regions left alone.

New suites: `tests/critical_manifest_011.rs` (protected, 15 tests, positive and
negative twins for every rule) and `tests/docs_fences.rs` (standard tier, 3
tests: the exact 6 complete `title="blastit.toml"` fences parse and validate,
`count == 6`, plus an extractor test and an invalid-regex canary).

Existing standard tests updated where the accepted 0.1.1 behaviour changed them,
never weakened: the route-key list now names `ingress`, and the §6 fixture's
`egress = true` produces the documented `EgressRoute` warning.

`crates/blastit-py/src/audit.rs` (added to this node's `paths`) had 13 failing
unit tests whose fixtures encoded the pre-0.1.1 rule. They were migrated
mechanically, with every assertion intact: the `SETUP` group's connection-setup
grant and its observations/assertions now use `SET TIME ZONE 'UTC'` (a
`KNOWN_STATIC_SETUP_SQL` statement, still matched exactly, still aggregated
across aliases and reformattings), and `CONNECTION`'s unused-grant control is
now the parameterised
`SELECT "never_runs"."id" FROM "never_runs" WHERE "never_runs"."id" = $?`
(still the one `unused_global` entry). The two `connection_setup` examples in
the module docs were renamed to the same legal statement. The `$1` spelling of
`set_config` already validated through normalisation.

## Fixture migration (`crates/blastit-py/src/audit.rs`)

R24 makes both pre-0.1.1 fixtures ungrantable by design ("A statement that
inlines a literal cannot be granted — bind the value instead"), so they were
migrated rather than exempted:

1. `SETUP`'s `[group.connection.db] queries` and every `SET application_name =
   'app'` observation/assertion in the module → `SET TIME ZONE 'UTC'`, which is
   one of the two `KNOWN_STATIC_SETUP_SQL` entries. `ungranted_in`/`granted_in`
   counts, the alias aggregation and the `wire_sql` echo still assert the same
   behaviour.
2. `CONNECTION`'s control grant `"SELECT 'never runs'"` → the parameterised
   `SELECT "never_runs"."id" FROM "never_runs" WHERE "never_runs"."id" = $?`,
   and its `unused_global` assertion; it still proves that a grant no request
   ran is the only `unused_global` entry while the setup grant is "used".

## Review Resolution

**Finding (MAJOR, attempt 1):** `src/parse.rs`'s `walk_table` applied the
reserved-key guard to table **names** as well as to fields, so documented
manifests aborted with the misleading `<key> after a [table] is not top-level`
message: `[proxy.app]`, `[proxy.version]`, `[proxy.groups]`, `[group.app.db]`,
`[proxy.db.output] app = "…"` and `[route.db.tables] groups = { … }`.

**Fix:** the guard now looks at **field** positions only. `check_top_level_keys`
enters the three container tables by name (`proxy`, `group`, `route`) and then
checks the tables inside them: `check_fields` for a body of schema keys
(`[proxy.<name>]`, `[route.ingress]`, a `<grant>.tables.<table>`'s operations)
and `check_grant` for a grant body (`queries`/`tables`/…, descending to the
author's table names). The container keys themselves are never checked: proxy
names, group names, the variable names in `input`/`output` and the table names
under `tables` all belong to the author. A `[[route]]` key is either a route key
(a field, `groups` included) or a proxy name; a reserved key there stays a
misplaced top-level key unless it names a *declared* proxy and its value is a
table, i.e. a grant — so `[proxy.app]` + `[route.app]` boots while `version = 1`
or `app = "app:app"` below a `[[route]]` still aborts. The message text,
`ManifestError::NotTopLevel` and every pre-existing negative case are unchanged.

**Test (RED → GREEN):** `tests/critical_manifest_011.rs` gains
`author_chosen_names_may_repeat_a_top_level_key`: one manifest whose proxy names
(`app`, `groups`, `version`), group name (`app`), output variable (`app` under
`[proxy.db.output]`) and table name (`groups`) are exactly the four reserved
keys, which parses, validates with zero warnings, and exposes the expected model
(proxy `app`, group `app` granting `db`/`groups`/`version`, the `[route.app]`
grant, `tables.table("groups")`). Its denied twin (`app = "app:app"` under a
`[[route]]` with no `[proxy.app]`) still fails with `NotTopLevel("app")`. Run
against attempt 1's `walk_table`, the very same test fails with seven
`NotTopLevel` errors (`app`, `app`, `app`, `groups`, `version`, `app`,
`groups`) — the reviewer's bug, caught before the fix and gone after it.

## Evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_manifest_011 --locked
running 15 tests
test author_chosen_names_may_repeat_a_top_level_key ... ok
test a_reserved_key_after_a_table_names_the_key_and_the_fix ... ok
test groups_stays_top_level_at_the_top_and_inside_a_route ... ok
test every_body_codec_parses ... ok
test every_literal_form_in_a_grant_is_refused ... ok
test a_statement_is_covered_by_tables_or_by_groups_but_not_by_itself ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

# RED: attempt 1's `walk_table` restored temporarily, same suite, same file
---- author_chosen_names_may_repeat_a_top_level_key stdout ----
thread 'author_chosen_names_may_repeat_a_top_level_key' panicked at
crates/blastit-manifest/tests/critical_manifest_011.rs:24:37:
manifest should parse: ManifestErrors([NotTopLevel { key: "app" },
NotTopLevel { key: "app" }, NotTopLevel { key: "app" },
NotTopLevel { key: "groups" }, NotTopLevel { key: "version" },
NotTopLevel { key: "app" }, NotTopLevel { key: "groups" }])
test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out

$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked
test only_a_complete_blastit_fence_is_collected ... ok
test an_invalid_regex_is_refused ... ok
test every_documented_manifest_parses_and_validates ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
unittests: 9 passed; tests/critical_manifest_011: 15 passed; tests/docs_fences: 3 passed;
tests/grants: 13 passed; tests/parse: 33 passed; tests/routing: 11 passed;
tests/uid: 6 passed; tests/validate: 29 passed; Doc-tests: 3 passed
(all `test result: ok`, 0 failed, 0 ignored; 122 total)

$ CARGO_NET_OFFLINE=true cargo test -p blastit-py --locked
test audit::tests::broken_observations_are_errors_that_name_where ... ok

test result: ok. 57 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s

$ cargo fmt --all -- --check
fmt: clean (exit 0)

$ CARGO_NET_OFFLINE=true make check
uv run --no-project tools/guard.py
uv run --no-project tools/critical_guard.py --base HEAD
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo test -p blastit-ingress --test critical_ingress
cargo test -p blastit-manifest --test critical_manifest_011
cargo test -p blastit-sqlpolicy --test critical_literals
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
$ echo $?
0
# 63x "test result: ok", 1612 tests passed, 0 failed, 0 ignored
```

Before this change every documentation fence and every new positive case was
impossible: `app`, `preload` and `[route.ingress]` were unknown keys and the
catch-all helper did not exist. The negative halves now fail closed: reserved
keys, `allow_raw` on a structured codec, `…` and `'secret'` all refuse the boot
with the documented message, and the reserved-key guard is narrow enough that
the names the documentation lets the author choose (`[proxy.app]`,
`[group.app.db]`, an `app` output variable, a `groups` table) boot.


## Files

- `crates/blastit-manifest/src/error.rs`
- `crates/blastit-manifest/src/lib.rs`
- `crates/blastit-manifest/src/model.rs`
- `crates/blastit-manifest/src/parse.rs`
- `crates/blastit-manifest/src/validate.rs`
- `crates/blastit-manifest/tests/critical_manifest_011.rs` (new, protected)
- `crates/blastit-manifest/tests/docs_fences.rs` (new, standard)
- `crates/blastit-manifest/tests/parse.rs` (route-key list)
- `crates/blastit-manifest/tests/validate.rs` (§6 `egress` warning)
- `crates/blastit-py/src/audit.rs` (test fixtures + module examples migrated to 0.1.1)

## Follow-ups

- `crates/blastit-py/src/audit.rs` is a 0.1.0 audit engine that `MA.2.3.4` will
  rewrite around `CheckObsV1`; its `connection_setup` fixtures now grant
  `SET TIME ZONE 'UTC'` because a literal-bearing grant is unrepresentable in
  0.1.1. When the engine is rewritten, keep that invariant: a setup grant can
  only be one of `KNOWN_STATIC_SETUP_SQL`.
- `MA.2.2.x` (config/CLI) should read `Manifest::app`/`preload` through the
  documented precedence (`BLASTIT_APP`/`BLASTIT_PRELOAD` win, an explicitly set
  empty variable still wins) and keep `crates/blastit/src/config.rs`'s existing
  `must be \`module:attribute\`` message for `BLASTIT_APP`; the manifest side now
  checks its own `app` the same way, so a bad value fails at boot from either
  source.
- `docs/reference/manifest.md` still shows `SELECT … FROM django_session …`; that
  `…` is now a validation error, and `MA.3` already owns replacing it with a
  concrete column list.
- `blastit-sqlpolicy` has a private `$?`→`$1` `parse_form` used by `--learn`;
  `blastit-manifest` re-implements a minimal version for the `dead_grant`
  coverage check. If `sqlpolicy` ever exports it, `parameterised` in `model.rs`
  should be deleted in favour of it.
- `crates/blastit-manifest/src/edit.rs` (`MA.2.2.2`) writes `[route.<proxy>]`
  blocks: a proxy named `app`, `version` or `preload` is legal only together with
  its `[proxy.<name>]` declaration, because the reserved-key guard recognises a
  route key as a proxy name when that proxy is declared and the value is a
  table. Always emit the declaration before the grant, and keep letting a bare
  `app = "app:app"` under a `[[route]]` fail as a misplaced top-level key.
