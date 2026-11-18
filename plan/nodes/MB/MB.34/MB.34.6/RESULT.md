# RESULT — MB.34.6

**STATUS: done.** The SQL inside a `query '…'` is tokenized by the grammar, captured by
`highlights.scm` with no new capture name, and the regenerated `parser.c`/`grammar.json`/
`node-types.json` are in the tree. `editors/zed/extension.toml`'s `rev` is **still the old
commit** by design: the lead makes commit A (the regenerated parser) and then sets `rev` to
A in commit B — the two-step `LESSONS.md` records, and the same one `MB.31.4` left open.

Generator: `/tmp/opencode/bin/tree-sitter`, `tree-sitter 0.25.10
(da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)`, i.e. exactly `package.json`'s pin. No other
version was run.

## What landed

| File | Change |
|---|---|
| `editors/tree-sitter-blastfile/grammar.js` | the `ci()` helper (one `[Ss][Ee]…` class per keyword); `query_decl: seq('query', optional(choice($.sql_string, $.string)))` spliced first into `_statement` (the `optional` is §13's regression fix); `sql_string` over the eight interior rules; `sql_keyword` (79 `ci()` spellings), `sql_parameter` (`$?`, `$n`), `sql_ellipsis`, `sql_number`, `sql_word`, `sql_operator`, `sql_other`; **`'query'` removed from `keyword`** (§9.2a decision 1: kept in, the one-token `raw_string` always won and `sql_string` was unreachable). |
| `src/parser.c`, `src/grammar.json`, `src/node-types.json` | regenerated with the pin; `node-types.json` now carries `sql_string` (children: `sql_ellipsis`, `sql_keyword`, `sql_number`, `sql_operator`, `sql_other`, `sql_parameter`, `sql_word`), `sql_keyword`, `sql_parameter`, `sql_ellipsis`. `src/tree_sitter/*.h` stays gitignored. |
| `test/corpus/query-sql.txt` **(new, 11 cases)** | `SELECT … price > $?`; `IN ($?, ...)`; `UPDATE t SET a = $1 WHERE b = $12`; `VALUES ($?, $?), ...`; a lowercase statement; a mixed-case one (`select Count(*) From … Where Price Is Not Null`); `SELECT count(*), payload->>name FROM t -- note` beside `query "SELECT count(*), x->>'k' FROM t"`; `select "Order" from "orders" where total >= 10.5`; `%`, `::`, `<>`, `\|\|`, `IS NOT NULL` across a real tab and newline; `query ''`; and `immutable regex '…'` next to a `query 'select 1'`. |
| `test/corpus/query-hash.txt` **(new)** | `query 'SELECT tags #> $? '` and `query 'SELECT 1 # x'`: the `#` is an `sql_operator`, not a `comment` extra, and the closing quote is found (§9.2a decision 4). |
| `test/corpus/query-spelling.txt` **(new)** | `query "SET TIME ZONE 'UTC'"` stays `(string)`; `immutable regex '…'` keeps `regex_raw_string`/`regex_body`; `sql_string` appears only under `query_decl`. |
| `test/corpus/contextual-keywords.txt` | one case added: `functions query`, `select query`, `insert query`, `update query`, `delete query` in one grant block — a bare `query` is a legal function name and a legal granted table (§13's regression). |
| `test/corpus/{env-and-query,multi-line-raw-string,sections}.txt` | expected trees re-recorded (`tree-sitter test --update`), because a `query '…'` value is now `(query_decl (sql_string …))` instead of `(directive (keyword) (raw_string))`. No source line changed. |
| `editors/zed/languages/blastfile/highlights.scm` | `(query_decl "query" @keyword)`; `(sql_string) @string.special`; `(sql_keyword) @keyword`; `(sql_parameter) @variable.special @variable`; `(sql_ellipsis) @variable.special @variable`. **No new capture name**: `sql_word`, `sql_number`, `sql_operator`, `sql_other` are deliberately uncaptured and inherit `@string.special` from the wrapper (§10.1). |
| `editors/tree-sitter-blastfile/highlights.scm` | the same five captures, in this copy's comment style, and the stale `'…'` note corrected (a `query` value is `sql_string` now, not `raw_string`). See §12: this file was added to the node's paths as a follow-up. |
| `scripts/check_zed_extension.py` | five new properties — `grammar: sql_string` (all nine rules present, `query` out of `keyword`, `query_decl`'s **optional** value as a source pattern (§14), `node-types.json` regenerated, `query-sql.txt` present), `grammar: fixture count` (≥ 23 discovered), `grammar: fixtures parse` (the §9.2a "no false red" loop over every in-repo `Blastfile`), `highlights: sql captures`, and `highlights: copies agree` (§12) — plus `--require-parse` (§13, finding 2). 42 → **47** properties. |
| `scripts/test_check_zed_extension.py` | the 47-name list and the count, the 23 `Blastfile`s copied into the temp tree, six negative twins (§ below, §12, §14), `assert_red` asserting the complete `FAIL` set (§13, finding 3), and `--require-parse` wherever the pin exists (§13). 19 → **26** tests. |

## The two commits (the lead's two steps)

I hold no commit rights here, so the working tree carries the regenerated artefacts and
`rev` still points at the last commit that contains a `parser.c` (`053b56e`, `MB.22.4`).

```
A  git add editors/tree-sitter-blastfile/{grammar.js,src/parser.c,src/grammar.json,src/node-types.json} \
         editors/tree-sitter-blastfile/test/corpus editors/zed/languages/blastfile/highlights.scm \
         scripts/check_zed_extension.py scripts/test_check_zed_extension.py
   git commit -m "[MB.34.6] Colour the SQL inside a query string"
B  (edit editors/zed/extension.toml: rev = <A>, the full 40-hex sha)
   git commit -m "[MB.34.6] Re-pin the Zed grammar rev"
```

Why two: `make zed-check` proves `git cat-file -e <rev>:…/src/parser.c`, and Zed shallow-
fetches exactly that commit; a `rev` can only name a commit that already exists. **`rev` must
not be set before A exists** — the value cannot be known until A is made. Commit B fixes
`MB.31.4`'s stale pin at the same time (its commit `c2ab3e5` says "The Zed grammar rev stays
un-pinned until a commit exists to point at").

## Acceptance

Each block is the command and its real output tail. Full transcripts:
`/tmp/opencode/mb346-accept.log`, `/tmp/opencode/mb346-accept2.log`.

### 1. `uv run --no-project tools/plan.py validate`

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
```

### 2. The pinned generator

```console
$ /tmp/opencode/bin/tree-sitter --version
tree-sitter 0.25.10 (da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)
```

### 3. `tree-sitter generate`, and no drift on a second run

```console
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter generate
Warning: No `tree-sitter.json` file found in your grammar, this file is required to generate with ABI 15. Using ABI version 14 instead.
This file can be set up with `tree-sitter init`. For more information, see https://tree-sitter.github.io/tree-sitter/cli/init.
exit=0
$ sha256sum src/parser.c src/grammar.json src/node-types.json   # before a second generate
$ /tmp/opencode/bin/tree-sitter generate
$ sha256sum src/parser.c src/grammar.json src/node-types.json   # after it
diff-exit=0
c4ca32353e84ab00040e3db9c8ca75bf9abd5655c6c573f3547f4379afaa4444  src/parser.c
08e6b770c6e5f34be2b9699557325345a3fb0c5a2bbd50ce976bdd85a634e06e  src/grammar.json
468e6808a1e3139980311b5274cc68c2b7641d4cb88dceb9c831ba1b9d546650  src/node-types.json
```

(The ABI-14 warning is pre-existing: the grammar ships no `tree-sitter.json`. `MB.22.4`'s
record has the same line.)

### 4. `git diff --exit-code -- editors/tree-sitter-blastfile/src`

This is the gate §9.4 runs *after* the artefacts are committed, so in this uncommitted tree
it exits 1 and names exactly the three regenerated files — the intended state before commit
A. The property it tests (a fresh `generate` produces no change) is the hash diff above.

```console
$ git diff --exit-code -- editors/tree-sitter-blastfile/src
diff --git a/editors/tree-sitter-blastfile/src/grammar.json b/editors/tree-sitter-blastfile/src/grammar.json
index fc7d478..5d27eef 100644
…
exit=1
$ git status --porcelain -- editors/tree-sitter-blastfile/src
 M editors/tree-sitter-blastfile/src/grammar.json
 M editors/tree-sitter-blastfile/src/node-types.json
 M editors/tree-sitter-blastfile/src/parser.c
```

### 5. `tree-sitter test`

(Attempt 1 had 33 cases; §13.4 has the current 34-case run after the corpus case the
reviewer asked for.)

```console
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter test
…
  query-hash:
     19. ✓ a hash operator does not start a Blastfile comment inside the literal
     20. ✓ a hash with no operator around it still ends at the closing quote
  query-spelling:
     21. ✓ the double-quoted spelling stays a string, and a regex keeps its own nodes
     22. ✓ sql_string appears only under query_decl, never under a directive or a regex
  query-sql:
     23. ✓ a bind placeholder after an operator in a SELECT
     24. ✓ an ellipsis keeps the statement open in an IN list
     25. ✓ a numbered bind parameter, `$1` and `$12`
     26. ✓ a multi-row INSERT is a trailing ellipsis after a value tuple
     27. ✓ a lowercase statement proves keyword matching is case-insensitive
     28. ✓ a mixed-case statement colours every spelling of the same keyword
     29. ✓ an operator soup statement with a SQL comment and a quoted JSON operator
     30. ✓ a quoted identifier, a double-quoted table and a decimal number
     31. ✓ a statement holding sort, cast and concatenation operators across a tab and a newline
     32. ✓ an empty query string has no interior tokens
     33. ✓ a regex next to a query keeps its regex_body, and other raw strings stay raw_string

Total parses: 33; successful parses: 33; failed parses: 0; success percentage: 100.00%; average speed: 7874 bytes/ms
exit=0
```

### 6. `scripts/check_zed_extension.py`

(This and §8 ran before the follow-up in §12 added a forty-seventh property; §12 has the
current run.)

```console
$ uv run --no-project scripts/check_zed_extension.py
…
ok   grammar: generator pin
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
…
ok   docs: settings fence
zed-check: OK (46 properties checked)
exit=0
```

### 7. `scripts/test_check_zed_extension.py` (offline)

(Attempt 1 had 23 tests; §13.4 has the current 25.)

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
.......................
----------------------------------------------------------------------
Ran 23 tests in 13.946s

OK
exit=0
```

The four new negative twins, each red only on its own property:

| Twin | Mutation | Red on |
|---|---|---|
| `test_missing_sql_capture_is_red` | delete `(sql_keyword) @keyword` | `highlights: sql captures` |
| `test_query_back_in_the_keyword_rule_is_red` | put `'query',` back in `keyword` | `grammar: sql_string` |
| `test_a_stale_node_types_json_is_red` | rename the `sql_keyword` entry in `node-types.json` | `grammar: sql_string` |
| `test_a_broken_blastfile_is_red` | append `}` to `test/fixtures/app.Blastfile` | `grammar: fixtures parse` |

The last one is the required "break a Blastfile → red" twin. It asks the guard whether it
could parse at all (a `note` line) and `skipTest`s otherwise, so a host without the pinned
binary does not fail the twin for the wrong reason; on this host it ran and went red.

### 8. `make zed-check`

```console
$ make zed-check
uv run --no-project scripts/check_zed_extension.py
…
zed-check: OK (46 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
.......................
----------------------------------------------------------------------
Ran 23 tests in 7.538s

OK
exit=0
```

### 9. The `rev` still points at a commit that contains both files

```console
$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1)
$ test ${#REV} -eq 40 && git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ git cat-file -e "$REV:editors/zed/extension.toml"
REV=053b56e1770f81a5856e17a0b8b68004d9e67069 (40 chars)
parser.c-at-rev exit=0
extension.toml-at-rev exit=0
```

This passes today because `053b56e` is a real commit holding both paths, but that commit's
`parser.c` predates `sql_string`: the extension would install a grammar with no `query_decl`.
Commit A + commit B is what makes this `rev` point at the new parser.

### 10. The twenty-three `Blastfile`s, zero `ERROR`/`MISSING`

```console
$ for f in editors/tree-sitter-blastfile/test/fixtures/*.Blastfile \
           $(git ls-files 'e2e/*Blastfile' 'python/tests/*Blastfile'); do
    tree-sitter parse "$f" | grep -cE 'ERROR|MISSING' || true
  done
parsed=23 files with ERROR/MISSING=0
```

Eight `test/fixtures/*.Blastfile`, six under `e2e/`, nine under `python/tests/` — the design's
list (§9.2a, R47). The loop is now the `grammar: fixtures parse` property, so it runs on
every `make zed-check` (and it parses the fifteen `crates/blastit-manifest/tests/fixtures/
*.blastfile` too when run by hand: 38/38 clean; they are left out of the guard because a
future error-case fixture there is *meant* to be unparseable). Its companion property,
`grammar: fixture count`, is what stops the loop from passing with a glob that matched
nothing.

The no-binary path was exercised too, by pointing a copy's `package.json` at `0.25.9` so
the pin matches nothing on this host: the property holds, and the `note` names what it
wanted, so a CI runner without the dev tool is not red and not silent.

```console
$ scripts/check_zed_extension.py --root /tmp/opencode/mb346-guardprobe   # pin edited to 0.25.9
ok   grammar: sql_string
note grammar: fixtures parse: skipped, no tree-sitter 0.25.9 in $BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH
ok   grammar: fixtures parse
```

### 11. The corpus case's tree, and the captures it produces

```console
$ tree-sitter parse /tmp/opencode/mb346-tree.Blastfile     # query 'SELECT id FROM products WHERE price > $?'
(source_file [0, 0] - [5, 0]
  (view_decl [0, 0] - [4, 1]
    (name [0, 5] - [0, 13])
    (method [0, 14] - [0, 17])
    (string [0, 18] - [0, 29])
    (block [0, 30] - [4, 1]
      (grant [1, 4] - [3, 5]
        (block [1, 13] - [3, 5]
          (query_decl [2, 8] - [2, 56]
            (sql_string [2, 14] - [2, 56]
              (sql_keyword [2, 15] - [2, 21])
              (sql_other [2, 21] - [2, 22])
              (sql_word [2, 22] - [2, 24])
              (sql_other [2, 24] - [2, 25])
              (sql_keyword [2, 25] - [2, 29])
              (sql_other [2, 29] - [2, 30])
              (sql_word [2, 30] - [2, 38])
              (sql_other [2, 38] - [2, 39])
              (sql_keyword [2, 39] - [2, 44])
              (sql_other [2, 44] - [2, 45])
              (sql_word [2, 45] - [2, 50])
              (sql_other [2, 50] - [2, 51])
              (sql_operator [2, 51] - [2, 52])
              (sql_other [2, 52] - [2, 53])
              (sql_parameter [2, 53] - [2, 55]))))))))
```

```console
$ tree-sitter query ../zed/languages/blastfile/highlights.scm test/fixtures/view.Blastfile
    capture: 0 - keyword, start: (20, 8), end: (20, 13), text: `query`
    capture: 7 - string.special, start: (20, 14), end: (20, 56), text: `'SELECT count(*) FROM carts WHERE id = $?'`
    capture: 0 - keyword, start: (20, 15), end: (20, 21), text: `SELECT`
    capture: 0 - keyword, start: (20, 22), end: (20, 27), text: `count`
    capture: 0 - keyword, start: (20, 31), end: (20, 35), text: `FROM`
    capture: 0 - keyword, start: (20, 42), end: (20, 47), text: `WHERE`
    capture: 9 - variable.special, start: (20, 53), end: (20, 55), text: `$?`
    capture: 10 - variable, start: (20, 53), end: (20, 55), text: `$?`
```

The wrapper is captured once (no `(raw_string)` inside to paint over it), the keywords are
`@keyword`, `$?` gets `$IDENT`'s right-to-left pair, and the spaces/operators inherit the
wrapper's `@string.special` — the grey run is gone.

## Never stricter: the argument, and the four shapes it was checked against

`sql_string`'s language is `' [^']* '`: every interior rule is a subset of `[^']` and
`sql_other` (`token.immediate(prec(1, /[^']/))`) is all of it, so the interior's union is
`[^']` exactly and the closing quote can only be the rule's own literal. The corpus is the
regression, not the argument (§9.2a decision 2). Contextual-keyword shapes were re-parsed:

```console
$ tree-sitter parse /tmp/opencode/mb346-ctx.Blastfile   # postgres query $DATABASE_URL / trait query / view query "/x"
  (store_decl [0, 0] - [0, 28] …
  (trait_decl [2, 0] - [6, 1] …
  (view_decl [8, 0] - [12, 1] …
          (ERROR [10, 15] - [10, 24] …      # `select query(id)`, pre-existing (§9.3), unchanged here
```

## 12. Follow-up: the grammar package's copy, and the guard that pins it

The lead added `editors/tree-sitter-blastfile/highlights.scm` to this node's paths. It is the
copy that travels with the grammar package, and it still promised that `'…'` colours `query`
— true until `query` left the shared `keyword` rule. It now carries the same five patterns as
Zed's copy, in its own comment style, and the `'…'` note is corrected:

```scheme
; `'...'` -- byte-verbatim: every value except a `query`'s, plus a regex body
; (which has its own node above). A `query '...'` is `sql_string` below, because
; `query` left the shared `keyword` rule and took its value with it.
(raw_string) @string.special

; `query` is a rule of its own now, so its word is captured by parent node.
(query_decl "query" @keyword)

; The SQL inside `query '...'`: the wrapper first, then the two tokens the
; reference page explains. …
(sql_string) @string.special
(sql_keyword) @keyword
(sql_parameter) @variable.special @variable
(sql_ellipsis) @variable.special @variable
```

A second copy is exactly how two files drift, so the guard now pins them: **`highlights:
copies agree`** requires every pattern in the grammar package's copy to appear in Zed's copy
(the grammar copy is the smaller file by design, so the rule is one-directional), comparing
whitespace-normalised, comment-stripped pattern lines. Its negative twin is
`test_a_grammar_copy_capture_zed_lacks_is_red`: appending `(sql_number) @number` to the
grammar copy reddens that property and nothing else.

### 12.1 `uv run --no-project scripts/check_zed_extension.py` (full)

```console
$ uv run --no-project scripts/check_zed_extension.py
ok   extension: keys
ok   extension: id
ok   extension: schema_version
ok   extension: version
ok   extension: description
ok   grammars: key
ok   grammars: repository
ok   grammars: rev
ok   grammars: path
ok   language_servers: languages
ok   language_servers: language_ids
ok   language_servers: no name
ok   config: name
ok   config: grammar
ok   config: path_suffixes
ok   config: line_comments
ok   config: tab_size
ok   config: hard_tabs
ok   grammar: files
ok   grammar: export
ok   grammar: name
ok   grammar: no scanner
ok   grammar: no externals
ok   grammar: generator pin
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
exit=0
```

### 12.2 `UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py` (full)

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
........................
----------------------------------------------------------------------
Ran 24 tests in 7.386s

OK
exit=0
```

### 12.3 `tree-sitter test` (full tail)

```console
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter test
  regex-pattern:
     26. ✓ a regex pattern is its own node, not a raw string
     27. ✓ a string pattern list stays one string per item
  sections:
     28. ✓ the reference page's complete example parses as six sections
     29. ✓ bare immutable caches every exact file the mount serves
     30. ✓ an immutable list is one pattern per item, the same spellings a view takes
     31. ✓ an immutable regex is its own node, not a name and a raw string
  trailing-comma-newline:
     32. ✓ a newline after a trailing comma continues the list
     33. ✓ a wrapped function grant continues after the comma

Total parses: 33; successful parses: 33; failed parses: 0; success percentage: 100.00%; average speed: 8160 bytes/ms
exit=0
```

### 12.4 `make zed-check` (full tail)

```console
$ make zed-check
uv run --no-project scripts/check_zed_extension.py
…
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
…
ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
........................
----------------------------------------------------------------------
Ran 24 tests in 10.002s

OK
exit=0
```

(The `…` lines are the same `ok` lines as §12.1, in the same order; the full transcript is
`/tmp/opencode/mb346-followup.log`.)

### 12.5 The two copies render the same colours

```console
$ cd editors/tree-sitter-blastfile
$ tree-sitter query highlights.scm test/fixtures/view.Blastfile
    capture: 4 - keyword, start: (20, 8), end: (20, 13), text: `query`
    capture: 3 - string.special, start: (20, 14), end: (20, 56), text: `'SELECT count(*) FROM carts WHERE id = $?'`
    capture: 4 - keyword, start: (20, 15), end: (20, 21), text: `SELECT`
    capture: 5 - variable.special, start: (20, 53), end: (20, 55), text: `$?`
    capture: 6 - variable, start: (20, 53), end: (20, 55), text: `$?`
$ tree-sitter query ../zed/languages/blastfile/highlights.scm test/fixtures/view.Blastfile
    capture: 0 - keyword, start: (20, 8), end: (20, 13), text: `query`
    capture: 7 - string.special, start: (20, 14), end: (20, 56), text: `'SELECT count(*) FROM carts WHERE id = $?'`
    capture: 0 - keyword, start: (20, 15), end: (20, 21), text: `SELECT`
    capture: 9 - variable.special, start: (20, 53), end: (20, 55), text: `$?`
    capture: 10 - variable, start: (20, 53), end: (20, 55), text: `$?`
```

Same capture names, same ranges, same text — only the pattern indices differ, because Zed's
copy is the larger file. `plan.py validate` stays `plan OK`, `editors/zed/extension.toml` is
untouched (`rev` is still `053b56e`, for the lead's two-step commit).

## 13. Attempt 2: the `query`-as-name regression, and two guard weaknesses

The reviewer confirmed the gates and the generated parser, and found one real regression plus
two holes in the guard's own evidence. All three are fixed here; `editors/zed/extension.toml`
is still untouched.

### 13.1 [MAJOR] `query` is a legal name, and `query_decl` now says so

`query` left `keyword` (§9.2a decision 1), and with a **required** value the only production
that could start with that token was `query_decl`, so a bare `query` in a value position had
nowhere to go: `functions query` and `select|insert|update|delete query` — all canonical
output of `blastit fmt`, and error-free under `MB.22.4`'s grammar — became `ERROR` nodes.

```js
query_decl: $ => seq('query', optional(choice($.sql_string, $.string))),
```

A valueless `query` is refused by the manifest (`any_string`, `parser.rs:1335`), so accepting
it is looseness — the LSP owns that squiggle — while the five accepted files lose their red.
The corpus case is `a bare `query` is a legal function name and a legal granted table`
(`test/corpus/contextual-keywords.txt`), which records the tree the parser now produces: no
`ERROR`, with `functions query` colouring as `(mode)` + `(query_decl)` and the four grants as
`(directive (keyword))` + `(query_decl)`. That is a colour artefact of the precedence `mode`
and `table_grant` already had, not a red, and the corpus pins it so a later change is visible.

**What this does not fix:** `select query(id)` — a column list after a table named `query` —
still produces one `ERROR` over `(id)`. Before this fix the same file erred over `query(id)`;
the table name is now accepted and the column list is not, because the parser still prefers
`directive` + `query_decl` over `table_grant` when it sees `query`. Design §9.3 records it as
pre-existing strictness with its own fix (one more `query` alternative under `_table`), and
that is a separate row this finding did not ask for.

### 13.2 [MINOR] `--require-parse`: the loop can no longer pass without running

With no pinned binary the property printed a `note` and an `ok`, so a CI without the dev tool
reported 47 green while the 23-file loop had never run. The skip is now an explicit opt-out:

```
--require-parse   fail instead of skipping when the pinned tree-sitter is missing, so
                  §9.2a's 23-file loop cannot pass without running (design §9.4)
```

Without the flag nothing changes (CI and a laptop with no `tree-sitter` keep their green
`make check`). With it, *both* "no binary" and "the binary is there but cannot load the
grammar" are failures, and the message names the fix (`install the pin (design §9.4) or drop
--require-parse`). The self-tests pass the flag whenever this host really has the pin
(`pinned_parse()` probes the same candidate list and version pin), so every case here now
proves the loop ran, and `test_the_repository_passes` asserts no `note` appears in a green
baseline. `scripts/test_check_zed_extension.py` adds
`test_a_missing_pin_is_red_under_require_parse`, which points the temp pin at `0.0.0` and
requires exactly `{"grammar: fixtures parse"}` — so the flag is proven, not just documented.

One thing this found: a `note` in the guard's output cannot be read as "no binary" — it also
appears when the binary exists and the grammar does not load (the unexported-symbol twin).
That is why the probe lives in the tests and asks `--version`, and why
`test_unexported_grammar_is_red` now expects **two** reds: the missing export and the loop
that cannot run because of it.

Both modes, end to end (a copy whose `package.json` pin is `0.0.0`, so nothing matches here):

```console
$ scripts/check_zed_extension.py --root /tmp/opencode/probe            # fallback: note + ok
note grammar: fixtures parse: skipped, no tree-sitter 0.0.0 in $BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH
ok   grammar: fixtures parse
rc=0
$ scripts/check_zed_extension.py --root /tmp/opencode/probe --require-parse
FAIL grammar: fixtures parse: no tree-sitter 0.0.0 in $BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH; install the pin (design §9.4) or drop --require-parse
```

### 13.3 [MINOR] Every twin proves its complete red set

`assert_red` asserted exit 1 plus "the property name appears somewhere", so a case could claim
one property was proven while three others went red beside it. It now compares the **set** of
`FAIL <name>:` lines against the set the case names:

- `assert_red("grammars: rev")` — one red.
- `assert_red("highlights: sql captures", "highlights: copies agree")` — two, because deleting
  a capture from Zed's copy also leaves the grammar package's copy claiming it (§12).
- `assert_red("grammar: export", "grammar: fixtures parse")` — two, per §13.2.
- `assert_red("grammar: fixtures parse")` for the broken-`Blastfile` twin, etc.

The names are matched against the known list rather than split on `:`, because a property name
contains a colon itself (`grammars: rev`) and a message may contain another.

### 13.4 Acceptance (attempt 2)

```console
$ /tmp/opencode/bin/tree-sitter --version
tree-sitter 0.25.10 (da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)

$ cd editors/tree-sitter-blastfile
$ sha256sum src/parser.c src/grammar.json src/node-types.json
09b44773958077ad6a8d6ed4b74ac9ca028066883a8cb6a39eb812fac8a56726  src/parser.c
891fa4a15c56a60eb58abd8ada0e7200221241923eb5df47a04768a66c4a9cc7  src/grammar.json
6fa008ca7e8d75b99180f50c3753457701c3ce0bce6d6b882aa891871977a1c1  src/node-types.json
$ /tmp/opencode/bin/tree-sitter generate
Warning: No `tree-sitter.json` file found in your grammar, this file is required to generate with ABI 15. Using ABI version 14 instead.
This file can be set up with `tree-sitter init`. For more information, see https://tree-sitter.github.io/tree-sitter/cli/init.
$ sha256sum src/parser.c src/grammar.json src/node-types.json
09b44773958077ad6a8d6ed4b74ac9ca028066883a8cb6a39eb812fac8a56726  src/parser.c
891fa4a15c56a60eb58abd8ada0e7200221241923eb5df47a04768a66c4a9cc7  src/grammar.json
6fa008ca7e8d75b99180f50c3753457701c3ce0bce6d6b882aa891871977a1c1  src/node-types.json
```

(The ABI-14 warning is pre-existing; the hashes are identical before and after, so the
committed artefacts are exactly what the pin produces.)

```console
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter test
  contextual-keywords:
      1. ✓ a keyword is legal as a view name
      2. ✓ a keyword is legal as a trait, view and store name
      3. ✓ a bare `query` is a legal function name and a legal granted table
  custom-method:
      4. ✓ a custom method is one token, hyphen included
      5. ✓ a method list takes several custom and standard methods
  env-and-query:
      6. ✓ an environment variable, a double-quoted query and a bind placeholder
  multi-line-raw-string:
      7. ✓ a raw string spans lines and is byte-verbatim
  one-line-block:
      8. ✓ a closing brace ends the statement, so a one-line block needs no semicolon
      9. ✓ a one-line endpoint block and a one-line grant block
  qualified-function:
     10. ✓ a pg_catalog-qualified function parses inside a named grant
     11. ✓ a bare function name is one value
  query-hash:
     12. ✓ a hash operator does not start a Blastfile comment inside the literal
     13. ✓ a hash with no operator around it still ends at the closing quote
  query-spelling:
     14. ✓ the double-quoted spelling stays a string, and a regex keeps its own nodes
     15. ✓ sql_string appears only under query_decl, never under a directive or a regex
  query-sql:
     16. ✓ a bind placeholder after an operator in a SELECT
     17. ✓ an ellipsis keeps the statement open in an IN list
     18. ✓ a numbered bind parameter, `$1` and `$12`
     19. ✓ a multi-row INSERT is a trailing ellipsis after a value tuple
     20. ✓ a lowercase statement proves keyword matching is case-insensitive
     21. ✓ a mixed-case statement colours every spelling of the same keyword
     22. ✓ an operator soup statement with a SQL comment and a quoted JSON operator
     23. ✓ a quoted identifier, a double-quoted table and a decimal number
     24. ✓ a statement holding sort, cast and concatenation operators across a tab and a newline
     25. ✓ an empty query string has no interior tokens
     26. ✓ a regex next to a query keeps its regex_body, and other raw strings stay raw_string
  regex-pattern:
     27. ✓ a regex pattern is its own node, not a raw string
     28. ✓ a string pattern list stays one string per item
  sections:
     29. ✓ the reference page's complete example parses as six sections
     30. ✓ bare immutable caches every exact file the mount serves
     31. ✓ an immutable list is one pattern per item, the same spellings a view takes
     32. ✓ an immutable regex is its own node, not a name and a raw string
  trailing-comma-newline:
     33. ✓ a newline after a trailing comma continues the list
     34. ✓ a wrapped function grant continues after the comma

Total parses: 34; successful parses: 34; failed parses: 0; success percentage: 100.00%; average speed: 8774 bytes/ms
exit=0
```

```console
$ uv run --no-project scripts/check_zed_extension.py --require-parse
ok   extension: keys
ok   extension: id
ok   extension: schema_version
ok   extension: version
ok   extension: description
ok   grammars: key
ok   grammars: repository
ok   grammars: rev
ok   grammars: path
ok   language_servers: languages
ok   language_servers: language_ids
ok   language_servers: no name
ok   config: name
ok   config: grammar
ok   config: path_suffixes
ok   config: line_comments
ok   config: tab_size
ok   config: hard_tabs
ok   grammar: files
ok   grammar: export
ok   grammar: name
ok   grammar: no scanner
ok   grammar: no externals
ok   grammar: generator pin
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
exit=0
```

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
.........................
----------------------------------------------------------------------
Ran 25 tests in 4.933s

OK
exit=0
```

```console
$ make zed-check
uv run --no-project scripts/check_zed_extension.py
ok   extension: keys
…
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
…
ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
.........................
----------------------------------------------------------------------
Ran 25 tests in 4.827s

OK
exit=0
```

(`make zed-check` does not pass `--require-parse`, because the Makefile is not in this node's
paths; the self-tests it runs do pass it, so the loop is still enforced there. The `…` lines
are the same `ok` lines as above, in the same order; full transcript
`/tmp/opencode/mb346-attempt2.log`.)

```console
$ 23-file loop over test/fixtures/*.Blastfile + e2e/*Blastfile + python/tests/*Blastfile
parsed=23 files with ERROR/MISSING=0
```

The regression itself, and the row that stays:

```console
$ cd editors/tree-sitter-blastfile                                                   # functions query, select query, insert query, update query, delete query
$ /tmp/opencode/bin/tree-sitter parse /tmp/opencode/mb346-query-name.Blastfile
(source_file [0, 0] - [9, 0]
  (view_decl [0, 0] - [8, 1]
    (name [0, 5] - [0, 6])
    (string [0, 7] - [0, 11])
    (block [0, 12] - [8, 1]
      (grant [1, 4] - [7, 5]
        (block [1, 13] - [7, 5]
          (mode [2, 8] - [2, 17]
            (keyword [2, 8] - [2, 17]))
          (query_decl [2, 18] - [2, 23])
          (directive [3, 8] - [3, 14]
            (keyword [3, 8] - [3, 14]))
          (query_decl [3, 15] - [3, 20])
          (directive [4, 8] - [4, 14]
            (keyword [4, 8] - [4, 14]))
          (query_decl [4, 15] - [4, 20])
          (directive [5, 8] - [5, 14]
            (keyword [5, 8] - [5, 14]))
          (query_decl [5, 15] - [5, 20])
          (directive [6, 8] - [6, 14]
            (keyword [6, 8] - [6, 14]))
          (query_decl [6, 15] - [6, 20]))))))
exit=0
```

```console
$ /tmp/opencode/bin/tree-sitter parse /tmp/opencode/mb346-query-id.Blastfile     # select query(id)
          (directive [2, 8] - [2, 14]
            (keyword [2, 8] - [2, 14]))
          (query_decl [2, 15] - [2, 20])
          (ERROR [2, 20] - [2, 24]
            (sql_word [2, 21] - [2, 23]))
/tmp/opencode/mb346-query-id.Blastfile	Parse:    0.04 ms	  1548 bytes/ms	(ERROR [2, 20] - [2, 24])
```

```console
$ git status --porcelain -- editors scripts
 M editors/tree-sitter-blastfile/grammar.js
 M editors/tree-sitter-blastfile/highlights.scm
 M editors/tree-sitter-blastfile/src/grammar.json
 M editors/tree-sitter-blastfile/src/node-types.json
 M editors/tree-sitter-blastfile/src/parser.c
 M editors/tree-sitter-blastfile/test/corpus/contextual-keywords.txt
 M editors/tree-sitter-blastfile/test/corpus/env-and-query.txt
 M editors/tree-sitter-blastfile/test/corpus/multi-line-raw-string.txt
 M editors/tree-sitter-blastfile/test/corpus/sections.txt
 M editors/zed/languages/blastfile/highlights.scm
 M scripts/check_zed_extension.py
 M scripts/test_check_zed_extension.py
?? editors/tree-sitter-blastfile/test/corpus/query-hash.txt
?? editors/tree-sitter-blastfile/test/corpus/query-spelling.txt
?? editors/tree-sitter-blastfile/test/corpus/query-sql.txt
```

`uv run --no-project tools/plan.py validate` → `plan OK`. `editors/zed/extension.toml` is
absent from that list: `rev` is still `053b56e`, for the lead's two-step commit (§ "The two
commits").

## 14. Follow-up: `query_decl`'s shape is pinned in the source, not only in the corpus

Reviewer verdict: PASS, with one MINOR worth taking now. `grammar: sql_string` described the
`sql_string` rules and the regenerated node types, but said nothing about `query_decl`, so
`query_decl: seq('query', choice($.sql_string, $.string))` — the required-value form that
turned five `blastit fmt`-accepted files (`functions query`, `select|insert|update|delete
query`) into `ERROR` nodes (§13.1) — could come back with the guard green on any host without
the pinned generator, because only the binary-dependent 23-file loop and `tree-sitter test`
would notice.

`check_sql_grammar` now reads the shape out of `grammar.js` and adds it to the same
`grammar: sql_string` property:

```python
if (
    re.search(
        r"query_decl:\s*\$\s*=>\s*seq\('query',\s*optional\(", source
    )
    is None
):
    missing.append(
        "query_decl: seq('query', optional(choice($.sql_string, $.string)))"
        " (a required value turns `functions query` into an ERROR)"
    )
```

Its negative twin is `test_a_required_query_value_is_red`, which rewrites exactly that line
back to the required-value form and requires **one** red, `grammar: sql_string` — the
`parser.c` and the corpus are untouched by the rewrite, so nothing else may fire. Property
count stays **47** (the clause lives inside an existing property); the test count goes
24 → **26** across §12–§14.

### 14.1 The hole, demonstrated on a host with no generator

A copy of the tree with the pin set to `0.0.0` (so nothing on this host matches) and
`optional(` dropped. This is the CI shape: no `--require-parse`, so the loop is a note, and
the new source pin is what catches it:

```console
$ scripts/check_zed_extension.py --root /tmp/opencode/mb346-nobin
FAIL grammar: sql_string: the `query` SQL grammar is incomplete: query_decl: seq('query', optional(choice($.sql_string, $.string))) (a required value turns `functions query` into an ERROR) (§9.2a: `query` must leave `keyword`, or `sql_string` never fires)
zed-check: 1 of 47 properties failed; the extension would not install (design §8, §10, §12.4)
note grammar: fixtures parse: skipped, no tree-sitter 0.0.0 in $BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH
ok   grammar: fixtures parse
```

The same copy with the real `grammar.js` stays green with the loop skipped, so the pin is not
a false positive on a machine without the dev tool:

```console
$ scripts/check_zed_extension.py --root /tmp/opencode/mb346-nobin      # real grammar.js
ok   grammar: sql_string
note grammar: fixtures parse: skipped, no tree-sitter 0.0.0 in $BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH
ok   grammar: fixtures parse
zed-check: OK (47 properties checked)
```

### 14.2 `uv run --no-project scripts/check_zed_extension.py --require-parse` (full)

```console
$ uv run --no-project scripts/check_zed_extension.py --require-parse
ok   extension: keys
ok   extension: id
ok   extension: schema_version
ok   extension: version
ok   extension: description
ok   grammars: key
ok   grammars: repository
ok   grammars: rev
ok   grammars: path
ok   language_servers: languages
ok   language_servers: language_ids
ok   language_servers: no name
ok   config: name
ok   config: grammar
ok   config: path_suffixes
ok   config: line_comments
ok   config: tab_size
ok   config: hard_tabs
ok   grammar: files
ok   grammar: export
ok   grammar: name
ok   grammar: no scanner
ok   grammar: no externals
ok   grammar: generator pin
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
exit=0
```

### 14.3 `UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py` (full)

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
..........................
----------------------------------------------------------------------
Ran 26 tests in 5.979s

OK
exit=0
```

### 14.4 `make zed-check` (full)

```console
$ make zed-check
uv run --no-project scripts/check_zed_extension.py
ok   extension: keys
ok   extension: id
ok   extension: schema_version
ok   extension: version
ok   extension: description
ok   grammars: key
ok   grammars: repository
ok   grammars: rev
ok   grammars: path
ok   language_servers: languages
ok   language_servers: language_ids
ok   language_servers: no name
ok   config: name
ok   config: grammar
ok   config: path_suffixes
ok   config: line_comments
ok   config: tab_size
ok   config: hard_tabs
ok   grammar: files
ok   grammar: export
ok   grammar: name
ok   grammar: no scanner
ok   grammar: no externals
ok   grammar: generator pin
ok   grammar: sql_string
ok   grammar: fixture count
ok   grammar: fixtures parse
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
..........................
----------------------------------------------------------------------
Ran 26 tests in 5.854s

OK
exit=0
```

`uv run --no-project tools/plan.py validate` → `plan OK`. `editors/zed/extension.toml` is
untouched (`rev` still `053b56e`, the lead's two-step commit). Full transcript:
`/tmp/opencode/mb346-sqlshape.log`.

### 14.5 The other MINOR is not this node's

CI never installs the pinned `tree-sitter` and never passes `--require-parse` to
`scripts/check_zed_extension.py`; `make zed-check` in the `Makefile` and the jobs in
`.github/workflows/ci.yml` belong to **MB.35.6**. This node pins the properties and gives the
flag; making CI provide the binary and pass it is MB.35.6's change, and it is listed here so it
travels with the follow-up list.

## Follow-ups (out of this node's paths)

1. **`docs/knowledge/LESSONS.md` deserves one line** (not my path): *the `tree-sitter` CLI
   caches a compiled grammar by grammar **name** in `~/.cache/tree-sitter/lib`, so a
   doctored `parser.c` compiled anywhere poisons every later `parse` with `undefined symbol:
   tree_sitter_blastfile`; set `TREE_SITTER_LIBDIR` to a directory keyed by the parser's
   content.* The guard does exactly that (`parse_cache`), which is also why its self-test may
   edit `parser.c` without breaking the repository's `tree-sitter test`. I removed the one
   poisoned entry this host had accumulated (`~/.cache/tree-sitter/lib/blastfile.so`).
2. **`editors/tree-sitter-blastfile/highlights.scm`** — **done in §12** now that the lead put
   it in this node's paths: it carries the same five SQL patterns as Zed's copy and no longer
   claims `'…'` covers a `query`; `highlights: copies agree` keeps the two from drifting.
3. **`select query(id)`** — a *column list* after a table named `query` — is still an
   `ERROR` (`(id)`, no longer the whole `query(id)`), while the bare forms `select query`,
   `insert query`, `update query`, `delete query` and `functions query` parse cleanly since
   §13.1. The parser still prefers `directive` + `query_decl` over `table_grant` when it sees
   `query`, so the design's §9.3 fix (one more `query` alternative under `_table`, plus a
   corpus case) is what closes it; this node's finding did not ask for that row.
4. **`docs/javascripts/blastfile-highlight.js`** (`MB.32.1`'s file) needs the same vocabulary
   (`sql_keyword`/`sql_parameter`/`sql_ellipsis`) for the docs-site highlighter to match.
