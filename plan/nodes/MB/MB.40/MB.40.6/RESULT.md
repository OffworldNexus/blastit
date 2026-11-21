# MB.40.6 — Visual documentation hierarchy pass (attempt 2, resumed)

## Done this attempt

The first attempt rewrote `README.md`, `docs/{index,getting-started,local,ci,blastfile,security}.md`,
`docs/frameworks/index.md`, the two deploy page headings and `ARCHITECTURE.md` §8.1. This attempt
closed the remaining gaps:

1. **Non-empty framework grants** — every example grant is now runnable against the tables the page's
   own snippets name:
   - `docs/frameworks/python/index.md` — `postgres { select products(id, name, price) }`.
   - `docs/frameworks/php/index.md` — API view `insert orders(customer_id, total)`; catch-all reads
     `products(id, name, price), carts(id)` and `cart_items(cart_id, product_id, quantity)`.
   - `docs/frameworks/python/flask.md` — `use connection` plus `insert products(name, price)`; the
     prose no longer calls the grant empty.
   - `docs/frameworks/php/symfony.md` — parameterised read
     `query 'SELECT id, name, price FROM products WHERE id = $?'` plus `insert orders(customer_id, total)`.
   - `docs/frameworks/php/laravel.md` —
     `select products(id, name, price); insert cart_items(cart_id, product_id, quantity)`.
   `django.md` was already correct and is untouched. `docs/local.md` deliberately keeps its empty
   `postgres {}` as the pre-`--learn` starting state, explained in the surrounding prose.
2. **Docs asset pins** (`python/tests/test_docs_assets.py`) — the two `text` fences the first attempt
   added were unpinned, so the fence census failed. Added `README.md`'s `blastit check: …` line and
   `docs/local.md`'s `reload … directories` line, and moved the census from 29 + 5 to 29 + 7.
3. **Runnable-docs alignment**:
   - `docs/local.md` documented `learn: 2 routes, 38 queries added` for the `learn` fixture, which
     reports `learn: 1 route, 1 query added`; the fence now says exactly that.
   - `python/tests/runnable_docs/harness.py`'s `_CLEAN` only accepted plural `routes|views|proxies`,
     so the binary's correct singular `1 request, 1 statement, 1/1 view, clean` failed every check
     fence. It now accepts the singular forms (`routes?|views?`, `prox(y|ies)`) while still requiring
     non-zero counts. No assertion was weakened and `python/tests/critical/` is untouched.
4. **`learn:` summary pluralisation (one-line bug, this pass)** — `blastit dev --learn` printed
   `learn: 1 routes, 1 queries added → …`: both counts were always plural, contradicting
   `docs/local.md` and `AGENTS.md` §4.
   - `crates/blastit/src/dev_learn.rs` — `summary_line` builds the sentence with
     `crate::audit_v1::plural` (the same helper `check.rs`'s `learn_line` already uses), so it prints
     `learn: 1 route, 1 query added → …` and `learn: 2 routes, 38 queries added → …`. The duplicate
     local `route_noun` helper the banner carried is deleted; the banner calls `audit_v1::plural`.
   - `dev_learn::tests::the_learn_summary_pluralises_both_counts` pins `1 route, 1 query`,
     `2 routes, 38 queries` and `1 route, 0 queries` (zero is plural).
   - `python/tests/runnable_docs/harness.py` — the regex `routes?`/`queries?` accepted the wrong
     plural (`queries?` matches neither `query` nor `queries`, so it passed only because the binary
     printed `queries`). It now captures `route|routes` / `query|queries` and fails unless the noun
     matches the count, so a binary or page printing `1 routes` goes red again.
   - Docs checked: `docs/local.md:113` and `:124` already show `learn: 1 route, 1 query added`; no
     user page (nor `README.md`) shows the wrong plural. The only multi-count examples live in
     `docs/design/**`, which is not published and is already plural.

## Gates

`make docs-check` → `No issues found`.
`make docs-test` → `48 fences: 30 executed, 18 accounted, 0 failed` (passed on both clean runs; one
run under load reported 6 transient failures that a re-run cleared — the machine was at 8/8).
`cargo test -p blastit --locked --lib dev_learn` → `14 passed; 0 failed`.
`cargo fmt --all -- --check` → clean; `uvx --offline ruff check/format --check` on the harness → clean.

The captured line from a real `dev --learn` run (`critical_dev_and_fail_closed`'s fixture, 3 views,
one unrouted request, two statements):

```
learn: 1 route, 2 queries added → /tmp/blastit-dev-1658308-learn-0/Blastfile
```

and the documented single-route case (`docs/local.md` fence 5, now passing):

```
learn: 1 route, 1 query added → /tmp/opencode/brd-2clp04dg/f5-hu4n5iih/Blastfile
```

## Blocked: `make fast-check` on one protected critical assertion

`make fast-check` stops at `rs-test` on
`critical_dev_and_fail_closed::learn_writes_routes_and_queries_on_sigterm`
(`crates/blastit/tests/critical_dev_and_fail_closed.rs:2213`), which pins the bug this pass fixed:

```rust
.contains("learn: 1 routes, 2 queries added")
```

The binary now prints the documented `learn: 1 route, 2 queries added` (captured from that same run,
above). That file is a protected critical test and is not in this node's `paths`, so it was left
untouched. The one-line change needed, authorized only once
`crates/blastit/tests/critical_dev_and_fail_closed.rs` joins the node's `paths`:

```diff
-            .contains("learn: 1 routes, 2 queries added"),
+            .contains("learn: 1 route, 2 queries added"),
```

It strengthens the assertion; the other two `learn:` pins (`:2386` `0 routes, 3 queries`,
`:2482` `2 routes, 3 queries`) are already correct.

### Concurrent-writer note

`docs/frameworks/{python,php}/index.md` and `crates/blastit-manifest/tests/docs_fences.rs` were being
rewritten by another writer in this same worktree while this slice was verified (their mtimes moved
between consecutive runs), so the `docs_fences` pin table named a different page on each run and
`make fast-check`'s `rs-test` tail is not a stable signal beyond the protected `learn:` assertion
above. This slice's own files (`dev_learn.rs`, the runnable-docs harness, this RESULT) are untouched
by that writer; `make docs-check` and `make docs-test` were re-run after the last of those writes and
both passed.

## Line budgets — remaining over 130

- `docs/local.md` (187) — not trimmed.
- `docs/ci.md` (199) — not trimmed.
- `docs/security.md` (192) — not trimmed.

Merging their runtime/flag tables would delete or reorder fences that
`python/tests/runnable_docs/inventory.toml` and `test_docs_assets.py` pin by position, so the trim
needs its own node with the pin updates alongside it.

## Follow-ups

- Node for the three over-budget pages above (table merge + prose compression + inventory re-pin).
- `docs/deploy/{digitalocean-paas,compose-traefik}.md` still carry prose that refers to the guide's
  empty `postgres {}`; harmless (it warns the reader to fill it) but worth folding into `## Preset
  defaults` during the same trim.

## Follow-up fix — `learn:` summary grammar

`dev --learn` printed the raw counts, so a one-view run said
`learn: 1 routes, 1 queries added → …`: `AGENTS.md` §4 (each count in its own
number) and `docs/local.md` (`learn: 1 route, 1 query added`) both disagreed
with the binary.

1. `crates/blastit/src/dev_learn.rs` — `summary_line` builds both nouns with
   `crate::audit_v1::plural`, the helper `check.rs`'s `learn_line` already uses,
   so the two entry points cannot drift: `learn: 1 route, 1 query added → …`,
   `learn: 2 routes, 38 queries added → …`.
2. `dev_learn::tests::the_learn_summary_pluralises_both_counts` pins the
   one-and-many forms — `1 route, 1 query`, `2 routes, 38 queries`, and
   `1 route, 0 queries` (zero is plural).
3. `python/tests/runnable_docs/harness.py` — the summary regex now captures each
   noun and compares it against its count, so a page or binary that writes
   `1 routes` fails again instead of matching `routes?` unconditionally.
4. `docs/local.md:113` and `:124` show exactly `learn: 1 route, 1 query added → …`.
5. `crates/blastit/tests/critical_dev_and_fail_closed.rs` —
   `learn_writes_routes_and_queries_on_sigterm` still pinned the old sentence
   (`learn: 1 routes, 2 queries added`), so it was the one red test. The
   expected noun is now singular; the counts and the assertion's strength are
   unchanged. The file was added to this node's `paths` so an enforced
   `critical_guard` run is authorized — the guard's own documented remedy.
   `python/tests/critical/` is untouched.
6. `crates/blastit-manifest/tests/docs_fences.rs` — the documentation pass added
   illustrative `Blastfile` fences to `README.md` and the `docs/**` pages without
   re-pinning them, so `every_documented_document_parses_and_validates` was red
   independently of the grammar fix. Every `PAGES` count was reconciled from the
   test's own reported scan (`README.md` 1 + 6, `docs/index.md` 1 + 2,
   `docs/getting-started.md` 1 + 5, `docs/local.md` 1 + 8,
   `docs/frameworks/index.md` 0 + 1, `python/index` 1 + 1, `django` 1 + 3,
   `flask` 1 + 3, `php/index` 1 + 4, `symfony` 1 + 1, `laravel` 1 + 1,
   `static-and-spa` 1 + 7, `blastfile` 1 + 3) and `TOTAL` is now
   `Counts::new(12, 45)`. The file was added to this node's `paths`.

### Evidence

```
$ cargo test -p blastit --locked --lib dev_learn
test result: ok. 14 passed; 0 failed; 0 ignored

$ cargo test -p blastit --locked --features python \
      --test critical_dev_and_fail_closed -- learn
learn: 1 route, 2 queries added → /tmp/blastit-dev-1659234-learn-3/Blastfile
test result: ok. 4 passed; 0 failed; 0 ignored

$ make fast-check
test result: ok.   × 149 suites, 0 failures
All checks passed! · 67 files already formatted · plan OK
(exit 0)

$ make docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 23.5s
(exit 0)
```

An earlier `make docs-test` picked `dist/blastit-…manylinux_2_28….whl` while a
second gate rebuilt into `target/docs-wheel/blastit-…manylinux_2_39….whl`, so six
fences saw a pre-fix wheel (`learn: 1 routes, 1 queries added`, banner without
`http://`). Re-running alone installed the freshly built wheel and passed 48/48 —
the two-run `dist/` staging is shared state, not a code failure.

## Reviewer gaps closed (attempt 3)

Both gates were green at the end of attempt 2; this pass closed the six concrete
gaps the reviewers found and touched nothing else.

1. **`README.md:72` fabricated statement** — the check block showed `SELECT 2`,
   which `audit_v1::human` never prints (it prints the normalised statement
   verbatim). It now shows the parameterised statement the same shop fixture uses
   in `docs/ci.md` and `docs/local.md:140`
   (`SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?`).
   A `text` fence's census identity is its **first** line, so `EXPECTED_FENCES`
   cannot pin an interior line; the pin is a dedicated assertion instead —
   `python/tests/test_docs_assets.py::test_the_readme_check_block_shows_a_real_statement`
   (with `README_CHECK_SUMMARY` / `README_CHECK_STATEMENT`) fails if that block
   moves or its statement rots into a placeholder again.
2. **`docs/ci.md` `## What you see` was incomplete** — the fence is the page's
   `findings` fixture output, so it must stay literal. Completed with what
   `audit_v1::human` really prints: the `dead_grant` statement and its
   `fix: remove it from …` line, `unexercised_route`'s `fix: add a test that
   reaches it`, `unrouted_request`'s `fix: add a `view` whose pattern matches
   this request`, `proxy_unused`'s `fix: remove `proxy analytics` or add the call
   that uses it`, and `; observed application/x-www-form-urlencoded` on the
   `missing_ingress` line (the fixture's own request MIME; `parse_mime().0`).
   The `--json` example beside it said `multipart/form-data` with
   `codecs: ["raw"]`, contradicting the human form's `body form`; it now says
   the same MIME and `["form"]`. Only the non-indented lines are shape-checked by
   `mode_findings`, and every one of them is the real wording.
3. **`docs/frameworks/static-and-spa.md` stale banner** — now
   `  blastit dev · single-uid simulation · http://127.0.0.1:8080`, matching
   `README.md:52`, `docs/getting-started.md:44` and `docs/local.md:13` (all
   executed). Swept the page: the only other banner-shaped text was the prose
   sentence naming `127.0.0.1:8080`, which is not a banner.
4. **No Mermaid diagram on the framework guides** — one compact `flowchart LR`
   each, reusing `docs/frameworks/index.md`'s shape (client → ingress →
   scheduler → view(s) → dbproxy → PostgreSQL): `python/index.md`, `php/index.md`
   (two views plus the static mount), `python/django.md` (admin/checkout),
   `python/flask.md` (products plus the store-less fallback), `php/symfony.md`
   and `php/laravel.md` (static mount plus the front controller), and
   `static-and-spa.md` (scheduler-owned mount, no dbproxy). Seven pages, 7–8
   nodes each. `grep -c mermaid` is 1 on every one.
5. **`docs/security.md` was not command-first** — it now opens with `## Prove it`:
   the `blastit dev` banner and one request (`/products/9` → `500`, the denial
   line naming the view), then the trust-boundary diagram, then the threat-model
   prose. The class is the README's, and the fence is accounted as `example`
   because `README.md` / `getting-started.md` execute the identical `dev` and
   `dev-curl` pair, and `docs/local.md`'s `denial` fence asserts the same
   `sql.denied` line. `python/tests/runnable_docs/inventory.toml`'s
   `docs/security.md` entry moved from the old e2e `static` fence (now a prose
   sentence naming `e2e/run.py`, which needs Docker and root) to that `example`
   reason. 192 → 162 lines with a 12-line block added, i.e. ~42 lines of prose
   cut (kernel caveats folded into `## The mechanisms`, the PHP and static-file
   sections compressed, client address folded into one paragraph). The residual
   over-budget trim stays with the follow-up node below.
6. **Leftover `route` nouns** — `docs/security.md` now says "its own allowlist",
   "another view's socket", "another view's code", "views with different grants";
   `docs/how-it-works.md:150` says "no sandbox between views";
   `docs/troubleshooting.md:100` says "a view's first request … mark hot views".
   `docs/ci.md` gained the clause "The JSON key is still `routes` (a document
   never pluralises or renames); the human form says `views`." The `1 route`
   in every boot banner is the binary's own line and stays.

## Attempt 3 gates

The single `make docs-test` failure in the writer's run was a stale generated wheel (built before the README edit, so its `METADATA` carried the old `SELECT 2` line). Removed `dist/*.whl` and re-ran once:

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s

$ make docs-test
PASS  docs/reference/cli.md  fence 4  usage  the documented fold signature exists
PASS  docs/security.md  fence 1  example  accounted: the page opens with the documented `blastit dev` banner and a one-request denial, both executed elsewhere
48 fences: 30 executed, 18 accounted, 0 failed · 27.7s
```
