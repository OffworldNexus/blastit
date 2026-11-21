# RESULT — MB.32.1

**STATUS: done** (attempt 1/3; round 2 after the reviewer's three findings — see
[Post-review MINORs](#post-review-minors-round-2))

One offline, dependency-free `Blastfile` highlighter, the AA palette it needs in both schemes, the
`zensical.toml` seam that loads it, and the asset tests that pin the 33-fence classification and the
SQL interior tokens. No fence was added: the `text` inventory is still 33 (12 `text title="Blastfile"`
documents + 17 keyword-led statement lists + the 4 named plain blocks).

## Files touched (all inside `paths`)

| File | Change |
|---|---|
| `docs/javascripts/blastfile-highlight.js` | new — one classic script: classifier, one-pass lexer, Text-node DOM walk |
| `docs/stylesheets/extra.css` | + the 13-class token palette, one rule per class per scheme, beside the existing table rule |
| `zensical.toml` | + `extra_javascript = ["javascripts/blastfile-highlight.js"]` |
| `python/tests/test_docs_assets.py` | + 17 tests: wiring, offline, entry paths, markup, the 33 fences, AA, the SQL fixtures, the DOM walk |

No `docs/**/*.md`, no `crates/**`, no nav, no plan status touched.

## What the lexer does

* **Classification.** Reads `code.textContent` and looks at the **first non-blank, non-`#` line**.
  It is a `Blastfile` when that line opens with a section (`app entrypoint postgres trait view
  static`), a block directive, or a grant word; it is refused when the line matches
  `^\S+:\d+:\d+:` (a diagnostic) or contains `→` / `::=` (grammar). The `span.filename` title is
  never consulted, so `docs/deploy/digitalocean-paas.md` is classified by its `postgres` first
  line, not by `Blastfile: the store block`.
  The test table keys on that same first line (`EXPECTED_FENCES`), never on a line number:
  `MB.34.8` is editing `docs/reference/blastfile.md` in this worktree and moved seven fences
  between the first measurement and the final run, which is exactly the drift the key must absorb.
* **One pass.** `query` opens a SQL context only when it is the directive (a view named `query`, or
  a `query` inside a `#` comment, opens none) and the next significant character is a quote. The SQL
  scanner runs on the string's **interior**, so `--` can never reach the closing quote. `...` is one
  token. `$?` → `.se`, `$1…$n` → `.nv`. Quoted identifiers and inline literals inside SQL
  (`"Order"`, `'UTC'`) are opaque, so `"Order"` is not the keyword `ORDER`.
* **The DOM walk** splits Text nodes only. `<span id="__span-0-N">`, `<a id="__codelineno-0-N">`
  and `code.textContent` are byte-for-byte what `zensical build` wrote, so `content.code.copy` /
  `content.code.select` are unchanged. The `<code>` carries `data-blastfile-highlighted` so a second
  instant-navigation run does nothing.
* **Entry paths** are the two the brief names, verbatim: `document$.subscribe(highlight)` when
  Material's observable exists, `DOMContentLoaded` otherwise.

## Acceptance

### 1. `make docs-check`

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.56s
$ ls -l site/javascripts/
-rw-r--r-- 1 remy remy 17724 Nov 18 01:34 blastfile-highlight.js
$ grep -o 'src="[^"]*blastfile-highlight[^"]*"' site/blastfile/index.html
src="../javascripts/blastfile-highlight.js"
```

### 2. `pytest python/tests/test_docs_assets.py`

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
..................................................                       [100%]
52 passed in 2.66s
```

(35 pre-existing tests + 17 new ones.)

### 3. `cargo test -p blastit-manifest --test docs_fences`

```console
$ cargo test -p blastit-manifest --test docs_fences
running 11 tests
test a_document_on_an_unpinned_page_is_named ... ok
test a_plain_toml_manifest_on_a_pinned_page_is_refused ... ok
test a_pinned_page_that_lost_its_document_is_named ... ok
test a_page_whose_count_drifted_is_named ... ok
test a_document_that_does_not_parse_names_its_page_position ... ok
test an_unclosed_fence_is_refused ... ok
test a_toml_fragment_passes_but_a_retired_complete_document_is_refused ... ok
test an_invalid_regex_is_refused ... ok
test a_valid_document_fence_is_collected_parsed_and_counted ... ok
test the_django_page_static_fragment_still_parses ... ok
test every_documented_document_parses_and_validates ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
```

### 4. `tools/plan.py validate`

```console
$ PATH="$HOME/.local/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
```

### 5. The offline grep (no output)

```console
$ grep -rn 'https\?://\|fetch(\|eval(' docs/javascripts/blastfile-highlight.js
$ echo $?
1
```

The test's own guard is wider (`import`, `require(`, `new Function`, and a URL anywhere), and it also
refuses `innerHTML`/`outerHTML`/`insertAdjacentHTML`/`document.write` assignments.

### 6. `make check`

```console
$ make check
...
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
66 files already formatted
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py \
	python/tests/test_docs_assets.py \
	python/tests/test_release_scripts.py \
	scripts/test_doc_tone.py -q
............................................................................... [ 63%]
.............................................                            [100%]
124 passed, 65 subtests passed in 4.73s
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.26s
scripts/build-wheel.sh -o "/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel"
...
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 30 user-facing page(s), no banned phrase
...
zed-check: OK (42 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
...................
----------------------------------------------------------------------
Ran 19 tests in 2.998s

OK
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The gate printed 5 162 lines and exited 0; the log carries no `***` and no `error[`, so no
target failed. Every stage ran: `guard`, `critical-guard`, `rs-fmt`, `rs-lint`, `rs-doc`,
`rs-test`, `test-critical`, `py-lint`, `docs-unit`, `docs-check`, `docs-test` (wheel),
`zed-check`, `plan-validate`. Both guards passed on this working tree:

```text
uv run --no-project tools/guard.py
guard: OK (15 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 24 changed file(s)
  no change to a protected critical test
```

Two earlier `make check` runs on this branch were red, and neither is called a host flake:

* **The fence table went stale.** `docs-unit` failed on
  `test_every_text_block_classifies_as_measured` with seven `text` fences moving from
  `docs/reference/blastfile.md:323/359/373/403/549/610/663` to `:338/374/388/418/564/625/679`,
  because `MB.34.8` edited that page's prose in this worktree while this node ran. The cause
  was mine — a `(page, line)` key. The table is now keyed on each fence's first non-blank line
  (`EXPECTED_FENCES`) and cannot drift with prose; the runs above are after the re-key.
* **`crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs:518`** panicked once with
  `BrokenPipe` writing a control command during the parallel workspace run (run 2), then passed
  3/3 standalone runs and every `make check` since. It is a pre-existing protected test outside
  this node's `paths`; recorded under Follow-ups rather than touched.

## `zensical.toml` diff (verbatim)

```diff
diff --git a/zensical.toml b/zensical.toml
index 9f230a3..a72af9f 100644
--- a/zensical.toml
+++ b/zensical.toml
@@ -59,6 +59,12 @@ nav = [
 
 extra_css = ["brand/brand.css", "stylesheets/extra.css"]
 
+# The Blastfile highlighter (`MB.32.1`): one classic script, no build step and no
+# network. It loads after the theme bundle on every page, including the pages
+# `navigation.instant` swaps in, and colours only a `Blastfile` fence; a
+# `text` block that is not one is left exactly as rendered.
+extra_javascript = ["javascripts/blastfile-highlight.js"]
+
 # `archive/` and `spikes/` are working material, not pages, and so are the brand
 # files a human works with rather than reads. Everything else under `brand/` is
 # published, because the site serves it: the logo, the favicons, the social
```

## The 33-fence classification (measured, then asserted)

Measured with `python/tests/runnable_docs/fences.py` over the 27 public pages; the verdict is
the shipped lexer's `classify()`, executed in Node. The test table is **this list keyed by the
fence's first non-blank line**, never by a line number: `MB.34.8` is editing
`docs/reference/blastfile.md` in the same worktree and had already moved seven fences between
the first measurement and this run, so a `(page, line)` pin went red on prose alone. The line
numbers below are today's positions, printed for orientation only.

```text
public pages: 27; `text` blocks: 33
HIGHLIGHT  README.md  'app "hello"'  (line 29 today)
HIGHLIGHT  docs/blastfile.md  'app "shop"'  (line 6 today)
HIGHLIGHT  docs/ci.md  'trait connection {'  (line 96 today)
HIGHLIGHT  docs/deploy/digitalocean-paas.md  'postgres {'  (line 69 today)
HIGHLIGHT  docs/frameworks/php/index.md  'app "shop"'  (line 8 today)
plain      docs/frameworks/php/index.md  'sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+'  (line 46 today)
HIGHLIGHT  docs/frameworks/php/laravel.md  'app "shop"'  (line 8 today)
HIGHLIGHT  docs/frameworks/php/symfony.md  'app "shop"'  (line 8 today)
HIGHLIGHT  docs/frameworks/python/django.md  'app "myshop"'  (line 8 today)
HIGHLIGHT  docs/frameworks/python/flask.md  'app "shop"'  (line 15 today)
HIGHLIGHT  docs/frameworks/python/index.md  'app "shop"'  (line 8 today)
HIGHLIGHT  docs/frameworks/static-and-spa.md  'static "/" {'  (line 7 today)
HIGHLIGHT  docs/getting-started.md  'app "hello"'  (line 29 today)
plain      docs/how-it-works.md  '/run/blastit/views/<uid>/ 0700, owned by the view uid'  (line 103 today)
HIGHLIGHT  docs/index.md  'app "myshop"'  (line 20 today)
HIGHLIGHT  docs/local.md  'app "shop"'  (line 57 today)
HIGHLIGHT  docs/local.md  "view products regex '/products/.*' {"  (line 129 today)
plain      docs/reference/blastfile.md  'file → app? entrypoint? section*'  (line 17 today)
HIGHLIGHT  docs/reference/blastfile.md  'static "/" {'  (line 64 today)
HIGHLIGHT  docs/reference/blastfile.md  'app "shop"'  (line 71 today)
HIGHLIGHT  docs/reference/blastfile.md  'entrypoint php "public/index.php"'  (line 82 today)
HIGHLIGHT  docs/reference/blastfile.md  'app "shop"'  (line 89 today)
HIGHLIGHT  docs/reference/blastfile.md  'postgres $DATABASE_URL # the default store, one libpq URL'  (line 113 today)
HIGHLIGHT  docs/reference/blastfile.md  'view products GET "/products", "/products/{id:int}" {'  (line 161 today)
HIGHLIGHT  docs/reference/blastfile.md  'view cart {'  (line 175 today)
HIGHLIGHT  docs/reference/blastfile.md  'view catalog GET "/products/{id:int}" {'  (line 237 today)
HIGHLIGHT  docs/reference/blastfile.md  'trait connection {'  (line 338 today)
HIGHLIGHT  docs/reference/blastfile.md  'trait session {'  (line 374 today)
HIGHLIGHT  docs/reference/blastfile.md  'view products GET "/products" {'  (line 388 today)
HIGHLIGHT  docs/reference/blastfile.md  'static "/" {'  (line 418 today)
HIGHLIGHT  docs/reference/blastfile.md  'app "shop"'  (line 564 today)
HIGHLIGHT  docs/reference/blastfile.md  'view products GET "/products/{id:int}" {'  (line 625 today)
plain      docs/reference/blastfile.md  'Blastfile:3:9: `=` is not Blastfile syntax; write the value after the keyword'  (line 679 today)
highlighted 29, plain 4, total 33
```

`docs/deploy/digitalocean-paas.md` is `HIGHLIGHT` by its `postgres` first line even though its
title is `Blastfile: the store block`: the classifier never reads the title.

`test_a_prose_edit_above_the_fences_keeps_every_identity` re-parses the reference page with a
paragraph prepended (in memory; the page is never written), proves the fence lines moved, and
asserts the identity sequence and every verdict are unchanged.

## Contrast table (computed from the hex values in `extra.css`)

The code surface is `--md-code-bg-color`, which `docs/brand/brand.css` maps to `--blastit-surface`
(`#fffcf5` light, `#1b232c` dark). WCAG AA for text is 4.5:1; every token clears it in its own
scheme, and the test fails with the ratio it computed.

| token | role | default on `#fffcf5` | slate on `#1b232c` |
|---|---|---|---|
| `.k` | keyword (sections, `use`, grants, SQL keywords) | `#223846` 11.90:1 | `#f3c997` 10.29:1 |
| `.kd` | block directive (`body`, `root`, `immutable`, store slots) | `#81371c` 8.21:1 | `#eb9a6c` 7.08:1 |
| `.kc` | HTTP method (`GET`, `POST`, …) | `#9b4723` 6.17:1 | `#ff9ecb` 8.32:1 |
| `.kt` | runtime (`python`, `php`), `:type`, trait/store names | `#5a2f70` 9.88:1 | `#d7a8e8` 8.05:1 |
| `.nf` | a view's name | `#3f3a36` 10.96:1 | `#a8c7dd` 8.98:1 |
| `.nv` | `$IDENT`, `$1…$n`, `...`, a route parameter name | `#2f5670` 7.63:1 | `#8fd6b4` 9.40:1 |
| `.s1` | `'…'` raw string (SQL, regex) | `#2a6b4f` 6.18:1 | `#b8e0a0` 10.74:1 |
| `.s2` | `"…"` string, a route pattern | `#1f5c63` 7.40:1 | `#a9d3d8` 9.82:1 |
| `.se` | escape, `$?` | `#7a2f6b` 8.38:1 | `#e0b0d8` 8.59:1 |
| `.c1` | `#` and SQL `--` comment (italic) | `#56616b` 6.18:1 | `#a5b0b8` 7.18:1 |
| `.p` | `{ } ( ) , ;` | `#56616b` 6.18:1 | `#b9c6cf` 9.10:1 |
| `.mi` | SQL number | `#8a5a00` 5.78:1 | `#f0c07a` 9.47:1 |
| `.o` | SQL operator (`= < > <> || :: -> ->> …`) | `#3f3a36` 10.96:1 | `#cbd5dc` 10.65:1 |

**Minimum: 5.78:1** (`.mi` on the light surface). Brand tokens are reused where they already clear
the floor (`--blastit-primary`, `--blastit-rust`, `--blastit-rust-deep`, `--blastit-muted`,
`--blastit-buff`, `--blastit-accent`, `--blastit-umber`); the hues the nuthatch palette lacks
(strings, escapes, numbers, the dark method pink) are new values, and each is a row above.

## Mutations: every guard was shown to fail

Everything above is green; a red run appears **only** in this section, by construction. Each
row is one plausible defect applied to the shipped files, the named test run, the output
recorded, and the file restored (the site copy still `cmp`s equal afterwards).

### 1. drop the `extra_javascript` entry → `test_the_highlighter_is_wired_and_published`

```text
python/tests/test_docs_assets.py:1705: AssertionError
E       AssertionError: zensical.toml must name the asset in `extra_javascript`, or no page ever loads it (found [])
E       assert [] == ['javascripts...highlight.js']
E         
E         Right contains one more item: 'javascripts/blastfile-highlight.js'
E         Use -v to get more diff
1 failed, 51 deselected in 0.21s
```

### 2. leave `site/` with a stale copy → `test_the_highlighter_is_wired_and_published`

```text
python/tests/test_docs_assets.py:1716: AssertionError
E           AssertionError: site/javascripts/blastfile-highlight.js is stale: rebuild the site (`make docs-check`)
E           assert b'/*\n * Blas... : this);\n\n' == b'/*\n * Blas...is : this);\n'
E             
E             Use -v to get more diff
1 failed, 51 deselected in 0.15s
```

### 3. put a URL in the asset → `test_the_highlighter_is_one_offline_script`

```text
python/tests/test_docs_assets.py:1648: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses a URL: the site is built and served offline, so the asset must be self-contained
E       assert not ['a URL']
1 failed, 51 deselected in 0.14s
```

### 4. drop the instant-navigation call → `test_the_highlighter_runs_on_first_load_and_instant_navigation`

```text
python/tests/test_docs_assets.py:1659: AssertionError
E       AssertionError: no `document$.subscribe(highlight)` call: an instant-navigation page swap would render every Blastfile plain
E       assert None
1 failed, 51 deselected in 0.12s
```

### 5. never set the idempotence marker → `test_the_highlighter_runs_on_first_load_and_instant_navigation`

```text
python/tests/test_docs_assets.py:1674: AssertionError
E       AssertionError: nothing sets the idempotence marker, so a second run re-wraps the spans
E       assert 'setAttribute(MARK' in '/*\n * Blastfile highlighting for the documentation site (MB.32.1).\n *\n * Why hand-written: Pygments has no `Blastf...nt.addEventListener("DOMContentLoaded", highlight);\n  }\n})(typeof globalThis !== "undefined" ? globalThis : this);\n'
1 failed, 51 deselected in 0.09s
```

### 6. rebuild `code.innerHTML` → `test_the_highlighter_never_rebuilds_the_markup`

```text
python/tests/test_docs_assets.py:1692: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses an innerHTML assignment: rebuilding the markup drops the line anchors and the copy button's text
E       assert not ['an innerHTML assignment']
1 failed, 51 deselected in 0.12s
```

### 7. classify any first line as a Blastfile → `test_every_text_block_classifies_as_measured`

```text
python/tests/test_docs_assets.py:1751: AssertionError
E       AssertionError: docs/frameworks/php/index.md:46: expected ('sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+', False), classified ('sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+', True)
E         the Blastfile highlighter is progressive enhancement: fix the classifier, not the page
E       assert not ["docs/frameworks/php/index.md:46: expected ('sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+', False), classified ('sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+', True)"]
1 failed, 51 deselected in 0.24s
```

### 8. decide per line, not per block → `test_the_plain_fences_stay_plain_with_a_query_line_appended`

```text
python/tests/test_docs_assets.py:2069: AssertionError
E       AssertionError: a plain `text` block that gains a `query` line is still not a Blastfile: ['docs/frameworks/php/index.md:46', 'docs/how-it-works.md:103']
E         the Blastfile highlighter is progressive enhancement: fix the classifier, not the page
E       assert not ['docs/frameworks/php/index.md:46', 'docs/how-it-works.md:103']
1 failed, 51 deselected in 0.26s
```

### 9. a grey keyword below AA → `test_every_token_colour_meets_aa_in_both_schemes`

```text
python/tests/test_docs_assets.py:1826: AssertionError
E       AssertionError: WCAG AA needs 4.5:1 on the code surface; below that are: default .k #8a8a8a is 3.37:1 on #fffcf5
E       assert not ['default .k #8a8a8a is 3.37:1 on #fffcf5']
1 failed, 51 deselected in 0.07s
```

### 10. drop `...` from the SQL interior → `test_the_sql_inside_a_query_string_is_tokenized`

```text
python/tests/test_docs_assets.py:1982: AssertionError
E           AssertionError: uppercase keywords, an IN list and the variadic marker: the SQL a `query` string carries: one statement, bound values, no inline literal
E           assert [('view', 'k'...y', 'k'), ...] == [('view', 'k'...y', 'k'), ...]
E             
E             At index 12 diff: ("'", 's1') != ('...', 'nv')
E             Right contains one more item: ('}', 'p')
E             Use -v to get more diff
1 failed, 51 deselected in 0.28s
```

### 11. fork the keyword set (`UNION` gone) → `test_the_sql_keyword_set_is_one_set`

```text
python/tests/test_docs_assets.py:1846: AssertionError
E       AssertionError: the SQL keyword set changed: missing ['UNION'], extra []
E       assert ('ALL', 'AND'...N', 'BY', ...) == ('ALL', 'AND'...N', 'BY', ...)
E         
E         At index 42 diff: 'UPDATE' != 'UNION'
E         Right contains one more item: 'WITH'
E         Use -v to get more diff
1 failed, 51 deselected in 0.08s
```

### 12. let `--` read past the closing quote → `test_a_query_comment_never_swallows_the_closing_quote`

```text
python/tests/test_docs_assets.py:1989: AssertionError
E       AssertionError: the SQL a `query` string carries: one statement, bound values, no inline literal
E       assert [('query', 'k...), ('}', 'p')] == [('query', 'k...), ('}', 'p')]
E         
E         At index 3 diff: ("-- note'", 'c1') != ('-- note', 'c1')
E         Use -v to get more diff
1 failed, 51 deselected in 0.28s
```

### 13. stop honouring `\"` escapes → `test_an_escaped_query_finds_its_tokens_and_ends_on_its_quote`

```text
python/tests/test_docs_assets.py:2004: AssertionError
E       AssertionError: the SQL a `query` string carries: one statement, bound values, no inline literal
E       assert [('query', 'k...', 's2'), ...] == [('query', 'k...M', 'k'), ...]
E         
E         At index 3 diff: ('\\', 'se') != ('\\"', 'se')
E         Right contains 2 more items, first extra item: ('$?', 'se')
E         Use -v to get more diff
1 failed, 51 deselected in 0.24s
```

### 14. drop the plain runs while splitting text → `test_the_dom_walk_keeps_every_anchor_and_every_byte`

```text
python/tests/test_docs_assets.py:2092: AssertionError
E       AssertionError: `code.textContent` changed, so the copy button would read different text: 'app "shop"\n\nview products GET "/products/{id:int}" {\n    postgres { select products(id, name, price) }\n}\n' became 'app"shop"\nviewproductsGET"/products/{id:int}"{postgres{select(,,)}}'
E       assert 'app"shop"\nv...{select(,,)}}' == 'app "shop"\n...price) }\n}\n'
E         
E         - app "shop"
E         ?    -
E         + app"shop"
1 failed, 51 deselected in 0.25s
```

### 15. key a fence on its whole body instead of its first line → `test_a_prose_edit_above_the_fences_keeps_every_identity`

```text
python/tests/test_docs_assets.py:2158: AssertionError
E       AssertionError: a prose edit changed a fence identity: the key must be the fence's own first line, and nothing positional
E       assert ("file → app?...ABASE }', ...) == ('file → app?...bpq URL', ...)
E         
E         At index 0 diff: "file → app? entrypoint? section* section → app | entrypoint | store | trait | view | static block → '{' statement* '}' # a statement ends at a newline, ';', or this '}' app → 'app' STRING entrypoint → 'entrypoint' ( 'python' STRING block? | 'php' STRING ) store → 'postgres' ( ENV | block | NAME ( ENV | block ) ) trait → 'trait' NAME block view → 'view' NAME ( METHOD ( ',' METHOD )* )? ( PATTERN ( ',' PATTERN )* ) block | 'view' NAME block # multi-endpoint: METHOD[, …] PATTERN block? inside static → 'static' STRING block" != 'file → app? entrypoint? section*'
E         Use -v to get more diff
1 failed, 51 deselected in 0.29s
```

## Anti-vacuity notes

* The classification verdicts come from the lexer's own `classify()`, executed in Node; the Python
  side only pins *which* `(page, line)` must be highlighted and which must stay plain. A 34th
  `text` fence fails with its path and line.
* The SQL fixtures are driven through `tokenize()`, the shipped function; the expectation table only
  records the contract. `joined(answer) == fence` on every fixture proves no byte was lost or
  duplicated.
* The keyword set is declared once (`SQL_KEYWORDS = lookup([...])`) and the test asserts the
  declaration count is 1 before comparing the words, so a forked list fails as "declared 2 times".
* The DOM test builds the exact rendered shape (`<code><span id="__span-0-N"><a
  id="__codelineno-0-N"></a>line\n</span>`), and its stub's `innerHTML` setter throws: a
  string-rebuilding implementation cannot pass even if it leaves the ids alone.

## Follow-ups

* **Intermittent, pre-existing, outside `paths`:**
  `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs:518`
  (`learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route`) panicked once with
  `BrokenPipe` writing a control command under the parallel `cargo test --workspace` of run 2, then
  passed three standalone runs and two full `make check` runs. It is a protected critical test and
  this node does not open it; a follow-up owner should give the harness a load-independent budget
  (the panic is a 2 s socket write, not a policy decision).
* `docs/knowledge/LESSONS.md` (outside this node's `paths`): with
  `pymdownx.highlight.line_spans = "__span"` and `anchor_linenums = true`, each line's text lives in
  its own `<span id="__span-…">` whose first child is an **empty** `<a id="__codelineno-…">`; any
  highlighter must walk Text nodes instead of re-emitting the block.
* The lexer is ~570 lines with ~150 of comments/JSDoc and the two vocabulary tables; the tokenizer
  itself is ~120. Worth a reviewer's eye in `MB.32.2` only if a page reads slower than expected.

## Post-review MINORs (round 2)

Three reviewer findings, fixed and re-proven. Everything above still holds: no page changed, no
fence was added (still 33 `text` blocks, 29 highlighted + 4 plain), no plan status touched.

### 1. [MAJOR] the palette was scheme-scoped only → scoped through the lexer's marker

`[data-md-color-scheme="…"] .highlight .X` matched **every** Pygments token on the site, so a
`python`, `console` or `json` fence was recoloured too — the epic's non-negotiable 3 in reverse.
Every rule now goes through the attribute the lexer sets after `classify()` returns true:

```diff
-[data-md-color-scheme="default"] .highlight .k { color: #223846; }
+[data-md-color-scheme="default"] .highlight code[data-blastfile-highlighted] .k { color: #223846; }
```

The same edit applies to all 13 classes in both schemes (26 rules; `git diff docs/stylesheets/extra.css`
shows nothing else but the comment that now explains the marker). The test's regex is widened to
require the `code[data-blastfile-highlighted]` step, and a second assertion refuses the old shape
outright:

```text
E AssertionError: 1 palette rule(s) are scheme-scoped only, so they would recolour every
  `console`, `python` and `json` fence on the site: each rule needs
  `code[data-blastfile-highlighted]` between `.highlight` and the token class
```

**Proof that a non-Blastfile fence is untouched.** `make docs-check` rebuilt the site; in the built
HTML no `<code>` carries the marker, and the fence wrappers are still language-specific:

```text
site/frameworks/python/flask/index.html: <code>   1 `language-python` block,  0 markers in the page
site/getting-started/index.html:         <code>   3 `language-console` blocks, 0 markers in the page
site/blastfile/index.html:               <code>   1 `language-text` block,    0 markers in the page
```

The attribute is runtime state, so the static HTML proves the *selector* cannot match anywhere yet;
the lexer is what decides which fence gets it. Driving the real asset over one block of each kind —
a faithful mini-DOM plus the asset's own `.highlight.language-text` selector — gives:

```text
python fence     marker=false textContent unchanged=true classes=["k"]
Blastfile fence  marker=true  textContent unchanged=true classes=["k","s2","nf","p"]
```

The `python` fence keeps the build's own `<span class="k">` and no rule of this palette can match it
(`code[data-blastfile-highlighted]` is absent), while the `Blastfile` block is marked and its spans
(`k`, `s2`, `nf`, `p`) do match. `MB.32.2` still owns the pixels.

**AA recomputed after the scoping** — same hexes, parsed out of the 26 scoped rules:

| token | default on `#fffcf5` | slate on `#1b232c` |
|---|---|---|
| `.k` | `#223846` 11.90:1 | `#f3c997` 10.29:1 |
| `.kd` | `#81371c` 8.21:1 | `#eb9a6c` 7.08:1 |
| `.kc` | `#9b4723` 6.17:1 | `#ff9ecb` 8.32:1 |
| `.kt` | `#5a2f70` 9.88:1 | `#d7a8e8` 8.05:1 |
| `.nf` | `#3f3a36` 10.96:1 | `#a8c7dd` 8.98:1 |
| `.nv` | `#2f5670` 7.63:1 | `#8fd6b4` 9.40:1 |
| `.s1` | `#2a6b4f` 6.18:1 | `#b8e0a0` 10.74:1 |
| `.s2` | `#1f5c63` 7.40:1 | `#a9d3d8` 9.82:1 |
| `.se` | `#7a2f6b` 8.38:1 | `#e0b0d8` 8.59:1 |
| `.c1` | `#56616b` 6.18:1 | `#a5b0b8` 7.18:1 |
| `.p` | `#56616b` 6.18:1 | `#b9c6cf` 9.10:1 |
| `.mi` | `#8a5a00` 5.78:1 | `#f0c07a` 9.47:1 |
| `.o` | `#3f3a36` 10.96:1 | `#cbd5dc` 10.65:1 |

Minimum **5.78:1**, unchanged, every token still above the 4.5:1 floor in its own scheme.

### 2. The count message now names the surplus `path:line`

`problems` are computed before the count assert, and the count failure names each surplus
`page:line` plus its key, and each missing table entry. Simulated by dropping one table row
(mutation 21):

```text
E AssertionError: the public pages carry 33 `text` blocks, not 32: a fence was added or removed,
  and this task ships none. Surplus ['README.md:29 \'app "hello"\''], missing none
```

### 3. The offline guard covers the other network APIs

`XMLHttpRequest`, `WebSocket`, `navigator.sendBeacon(`, and a mid-line `import(` are now forbidden
alongside the URL, `fetch(`, statement-level `import`, `require(`, `eval(` and `new Function`. The
shipped asset is clean; mutations 17–20 each go red naming the API.

### Mutations for the three findings (each applied, run, restored)

### 16. scope one rule by scheme only again → `test_every_token_colour_meets_aa_in_both_schemes`

```text
python/tests/test_docs_assets.py:1799: AssertionError
E       AssertionError: 1 palette rule(s) are scheme-scoped only, so they would recolour every `console`, `python` and `json` fence on the site: each rule needs `code[data-blastfile-highlighted]` between `.highlight` and the token class
E       assert not ['[data-md-color-scheme="default"] .highlight .']
1 failed, 51 deselected in 0.25s
```

### 17. open a WebSocket → `test_the_highlighter_is_one_offline_script`

```text
python/tests/test_docs_assets.py:1652: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses WebSocket: the site is built and served offline, so the asset must be self-contained
E       assert not ['WebSocket']
1 failed, 51 deselected in 0.08s
```

### 18. call XMLHttpRequest → `test_the_highlighter_is_one_offline_script`

```text
python/tests/test_docs_assets.py:1652: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses XMLHttpRequest: the site is built and served offline, so the asset must be self-contained
E       assert not ['XMLHttpRequest']
1 failed, 51 deselected in 0.06s
```

### 19. call navigator.sendBeacon → `test_the_highlighter_is_one_offline_script`

```text
python/tests/test_docs_assets.py:1652: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses sendBeacon: the site is built and served offline, so the asset must be self-contained
E       assert not ['sendBeacon']
1 failed, 51 deselected in 0.11s
```

### 20. dynamic import() mid-line → `test_the_highlighter_is_one_offline_script`

```text
python/tests/test_docs_assets.py:1652: AssertionError
E       AssertionError: docs/javascripts/blastfile-highlight.js uses a dynamic import: the site is built and served offline, so the asset must be self-contained
E       assert not ['a dynamic import']
1 failed, 51 deselected in 0.12s
```

### 21. drop one table row (an unlisted fence) → `test_every_text_block_classifies_as_measured`

```text
python/tests/test_docs_assets.py:1761: AssertionError
E       AssertionError: the public pages carry 33 `text` blocks, not 32: a fence was added or removed, and this task ships none. Surplus ['README.md:29 \'app "hello"\''], missing none
E       assert 33 == 32
E        +  where 33 = len([('README.md', 29, 'app "hello"', 'app "hello"\n\nentrypoint python "hello:application"\n\nview hello GET "/*" {}'), (...hp84-pdo_pgsql                # Alpine\nsudo dnf install php-embedded php-pgsql                 # Fedora / RHEL'), ...])
1 failed, 51 deselected in 0.41s
```

### Re-run on this round

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.31s
$ cmp site/javascripts/blastfile-highlight.js docs/javascripts/blastfile-highlight.js
identical (exit 0)

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 2.30s
```

`cargo test -p blastit-manifest --test docs_fences` → `11 passed`; `tools/plan.py validate` →
`plan OK`; the offline grep → no output (exit 1).

### `make check` (final)

```console
$ make check
...
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
66 files already formatted
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py \
	python/tests/test_docs_assets.py \
	python/tests/test_release_scripts.py \
	scripts/test_doc_tone.py -q
............................................................................... [ 63%]
.............................................                            [100%]
124 passed, 65 subtests passed in 19.50s
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.36s
scripts/build-wheel.sh -o "/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel"
...
zed-check: OK (42 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
...................
----------------------------------------------------------------------
Ran 19 tests in 18.447s

OK
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

5 352 lines, exit 0. One earlier attempt in this round failed in `rs-doc`, not in any file this
node touches: `cargo test --workspace`'s `blastit-view` doctest printed
`error[E0463]: can't find crate for pyo3`. The cause is in the worktree, not the code — two other
cargo builds of this shared `target/` were running at that moment (`ps` showed three `cargo`
processes with three distinct parent shells, one linking `target/debug/build/blastit-py/...`), and a
build-script output directory was being replaced while the doctest linked against it. The re-run
above, with the same concurrent builds but scheduled after the link, is green; nothing was changed
in `crates/**` by this node.
