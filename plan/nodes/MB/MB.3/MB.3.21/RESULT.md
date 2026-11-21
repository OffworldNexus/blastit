# RESULT: MB.3.21 — `--learn` must not drop a route's executor block

## Summary

`expected_routes` in `crates/blastit-manifest/src/edit.rs` rebuilt every patched route with
`Route::new(...).with_ingress(...)`, and `Route::new` starts `static_policy` and `php` at
`None`. The rewriter's postcondition therefore lost both executor blocks.

**Observed pre-fix behaviour (this revision):** an edit that adds a `queries` entry to a route
carrying `[route.static]` or `[route.php]` is **refused, fail-closed**, by `verify` —
`EditError::Verification { section: "the routes" }`, `--learn` exits `1`, and the manifest is
not written. It does not silently delete the block today. The drop is the latent risk: the
moment the postcondition compared less than the whole `Route` (or a caller bypassed `verify`),
the rebuilt route would have no block to compare with and the author's `[route.static]` /
`[route.php]` keys would be gone while the tool reported success.

**Fix:** carry the route's executor state into the rebuilt model exactly as `ingress` already
was: `.with_static(route.static_policy().cloned()).with_php(route.php().cloned())`. All ten
`Route` fields now round-trip through the editor's postcondition, so an executor block the
writer cannot reproduce refuses the edit instead of vanishing from the author's file.

Coverage at two levels:

* `crates/blastit-manifest/tests/critical_manifest_edit_executor.rs` (new, protected): a
  php+static manifest survives `apply_edits` + `write_atomic` byte-for-byte outside the one
  `queries` line; a bare route gains no block; an inserted route does not disturb an executor
  route; malformed executor state is refused before an edit.
* `crates/blastit/tests/critical_learn_executor_blocks.rs` (new, protected, **user-visible**):
  a real `blastit dev --learn` (Dev harness + `MockPgServer`) whose route carries
  `[route.static] root = "public"` and no `[route.db]`; the ungranted `SELECT 1` is learned on
  `SIGTERM`, and the static block's raw lines must be byte-identical afterwards. This is the
  write path `supervise.rs`'s `finish` runs (`dev_learn::LearnRun::finish` → `apply_edits` →
  `write_atomic`). `MB.3.50` later seeded the fixture with `[route.db] queries =
  ['SELECT 42']`, so the same case now also proves the learned query **extends** an existing
  list instead of creating the table.
* `crates/blastit/src/dev_learn.rs` unit test: the same php+static manifest through
  `LearnRun::finish` directly, plus the refused-statement and malformed-manifest twins (both
  write nothing).

## Acceptance command of record

`NODE.md`'s acceptance command names a test target that does not exist. The lead's LOG entry
`REVIEW FAIL (attempt 1)` (`2026-11-15T06:43`, MAJOR 1) is the correction of record, and its
"never the stale one" is why the target is not repeated here. The command actually run, and
the one that guards this data, is:

```console
$ cargo test -p blastit-manifest --test critical_manifest_edit_executor --locked
```

## Evidence

### Round trip, before → after (`apply_edits`, the same fixture as the critical test)

Before (author's file):

```toml
[[route]]
name = "app"
path = ['.*']

# Only fingerprinted output is cached for a year.
[route.static]
root = "public"
index = "index.html"
spa = false
immutable = ["/assets/*", "/build/*"]

# The front controller; 4 is a cap, the budget is derived.
[route.php]
entry = "public/index.php"
workers = 4

[route.db]
queries = ['SELECT "shop_product"."id" FROM "shop_product"']
```

After (one learned query, everything else the same byte):

```toml
[[route]]
name = "app"
path = ['.*']

# Only fingerprinted output is cached for a year.
[route.static]
root = "public"
index = "index.html"
spa = false
immutable = ["/assets/*", "/build/*"]

# The front controller; 4 is a cap, the budget is derived.
[route.php]
entry = "public/index.php"
workers = 4

[route.db]
queries = ['SELECT "shop_product"."id" FROM "shop_product"', 'SELECT "shop_product"."sku" FROM "shop_product" WHERE "shop_product"."id" = $?']
```

The test proves this mechanically: `without_queries(before) == without_queries(after)`, plus
`block(after, "[route.static]") == block(before, "[route.static]")` and the same for
`[route.php]`, plus the reparsed model's `static_policy()`/`php()` (including `index`, `spa`,
`immutable`, `workers`) and a second application returning the output unchanged.

### User-visible level: `dev --learn` on a static-block route

`crates/blastit/tests/critical_learn_executor_blocks.rs` drives the shipped binary:
`blastit dev --learn` over a WSGI route with

```toml
[[route]]
name = "sql"
path = ['/sql']

# Built assets are served by the responder; every other path is the view's.
[route.static]
root = "public"
index = "index.html"
```

`GET /sql` misses `public/sql`, reaches the view, and the view's ungranted `SELECT 1` is
recorded through dbproxy; `SIGTERM` makes the supervisor call `LearnRun::finish`. The test
asserts the write exits `0`, the manifest contains the learned `[route.db] queries =
['SELECT 1']`, the `[route.static]` raw lines are byte-identical to the fixture's, and the
reparsed route still carries `root = "public"` and the grant. It also proves the statement
crossed the wire (`MockPgServer::startups`/`statements`), so the learned grant is a real
observation. It needs the interpreter-backed dev build
(`#![cfg(feature = "python")]`, like `critical_dev_and_fail_closed.rs`); the protected tier
always builds with `--features python` (`make rs-test-critical`).

Green:

```text
test dev_learn_writes_the_query_and_keeps_the_static_block_byte_for_byte ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Red runs — the bug restored

`expected_routes` reverted (the two `.with_static`/`.with_php` lines deleted), all three
suites re-run:

```text
$ cargo test -p blastit-manifest --test critical_manifest_edit_executor --locked
test a_learned_query_keeps_php_and_static_blocks_byte_for_byte_on_disk ... FAILED
test an_executor_route_is_edited_only_on_its_queries_line ... FAILED
a route with an executor block is editable: Verification { section: "the routes" }
test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
$ cargo test -p blastit --features python --test critical_learn_executor_blocks --locked
thread 'dev_learn_writes_the_query_and_keeps_the_static_block_byte_for_byte' panicked:
assertion `left == right` failed: the learn write exits cleanly
stderr:
learn: the edit would rewrite the routes, which it does not own; the manifest was left untouched
  left: Some(1)
 right: Some(0)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
$ cargo test -p blastit --lib dev_learn::tests::a_learned_query_keeps --locked
test dev_learn::tests::a_learned_query_keeps_the_route_s_executor_blocks_on_disk ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 195 filtered out
```

The source was restored afterwards and every test below was re-run green.

### Commands

```console
$ cargo test -p blastit-manifest --test critical_manifest_edit_executor --locked
test result: ok. 5 passed; 0 failed; 0 ignored
$ cargo test -p blastit --features python --test critical_learn_executor_blocks --locked
test result: ok. 1 passed; 0 failed; 0 ignored
$ cargo test -p blastit --lib dev_learn::tests::a_learned_query_keeps --locked
test result: ok. 1 passed; 0 failed; 0 ignored; 195 filtered out
```

```console
$ cargo test -p blastit-manifest --locked
test result: ok. 40 passed (lib); 5 / 17 / 23 / 9 / 15 / 5 / 19 / 13 / 20 / 34 / 11 / 6 / 29 / 5
passed, 0 failed
```

```console
$ cargo test -p blastit --locked
test result: ok. 195 passed; 0 failed; 1 ignored (lib)
test result: ok. 31 passed (smoke)
test result: ok. 19 / 19 / 11 / 11 / 5 / 4 / 3 / 1 / 0 passed, 0 failed (integration targets;
  critical_learn_executor_blocks is 0 under the default build by its `python` cfg gate)
```

```console
$ uv run --no-project tools/plan.py validate
plan OK
$ make critical-skip-check
protected critical tests hold no skip construct
$ uv run --no-project tools/critical_guard.py --base 59dd162 --node MB.3.21
critical-guard: 59dd162..working tree, 5 changed file(s)
node MB.3.21: paths [... 'crates/blastit/tests/**' ...]
  no change to a protected critical test
$ make check > /tmp/opencode/mb321-make-check-final2.log 2>&1; echo "EXIT=$?"
EXIT=0
```

`make check` ran both new critical files through `rs-test-critical`
(`cargo test -p blastit-manifest --test critical_manifest_edit_executor`,
`cargo test -p blastit --features python --test critical_learn_executor_blocks`) and clippy
with `-D warnings` on both touched crates.

## Notes on the two minor findings

* The malformed-executor case in `critical_manifest_edit_executor.rs` now asserts only
  `apply_edits` → `EditError::Source`, and says why that is the honest level: `apply_edits`
  is the unit that would have produced text, `--learn` calls `write_atomic` only on its `Ok`
  output, and a document that does not parse never becomes a manifest, so no writer is
  reached. The writer-level refusals (a refused statement, a malformed manifest) are driven
  through `LearnRun::finish` in the `dev_learn` unit test, where the manifest on disk is
  asserted unchanged.
* The node's "reports success" premise is replaced above by the observed exit `1` refusal;
  the data-loss risk and why the round trip (not the exit code) is the guard are stated.

## Files

- `crates/blastit-manifest/src/edit.rs` — carry `static_policy` and `php` into the
  postcondition; `apply_edits` doc states the promise.
- `crates/blastit-manifest/tests/critical_manifest_edit_executor.rs` — **new** protected
  round-trip suite (5 tests).
- `crates/blastit/tests/critical_learn_executor_blocks.rs` — **new** protected user-visible
  `dev --learn` suite (Dev + `MockPgServer` + WSGI app + `[route.static]`).
- `crates/blastit/src/dev_learn.rs` — unit test on `LearnRun::finish` (php+static on disk;
  refused-statement and malformed-manifest twins).

## Follow-ups

- `check --learn`'s plan proposes only new routes and their grants (`audit_v1::plan_learn`
  builds `grouped` solely from `ObsTarget::Unrouted`), so a missing grant on a *declared*
  route is an `add_query` finding the operator applies; the patch path that reaches
  `expected_routes` is `dev --learn`, now covered at process level.
- Worth a `docs/knowledge/LESSONS.md` line (out of this node's `paths`): a postcondition-based
  rewriter built with `Route::new(...)` must chain every field the constructor defaults, or
  the next field added to the model is silently outside the postcondition.
