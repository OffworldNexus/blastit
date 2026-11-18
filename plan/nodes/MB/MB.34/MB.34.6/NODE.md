---
id: MB.34.6
title: "Tree-sitter: tokenize query SQL (keywords, $?/$n, ...), capture it in highlights.scm, regenerate parser.c and re-pin the Zed grammar rev"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.34.2, MB.34.4]
paths:
  - "editors/tree-sitter-blastfile/grammar.js"
  - "editors/tree-sitter-blastfile/highlights.scm"
  - "editors/tree-sitter-blastfile/src/parser.c"
  - "editors/tree-sitter-blastfile/src/grammar.json"
  - "editors/tree-sitter-blastfile/src/node-types.json"
  - "editors/tree-sitter-blastfile/test/corpus/**"
  - editors/zed/languages/blastfile/highlights.scm
  - editors/zed/extension.toml
  - scripts/check_zed_extension.py
  - scripts/test_check_zed_extension.py
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T04:39"
---

# Goal

Colour the SQL *inside* a `query` string in Zed, with no external C scanner and no second Zed
extension, and keep the pinned grammar installable.

1. **`editors/tree-sitter-blastfile/grammar.js`.** Introduce `sql_string` for the value of a
   `query` directive, tokenizing its interior per `MB.34.2`:
   `sql_keyword` (the SQL keyword set in the design, case-insensitive), `sql_parameter`
   (`$?` and `$1…$n`), `sql_ellipsis` (`...`), `sql_number`, `sql_word`, `sql_operator`, and a
   one-character `sql_other` fallback. `raw_string` stays for every other `'…'`, including
   `regex '…'`; `query "…"` keeps its escaped-string node with the same children.
   The grammar may be looser than `blastit-manifest` but **never stricter**: any text a
   `raw_string` accepts must parse with no `ERROR`/`MISSING`.
2. **Corpus** (`editors/tree-sitter-blastfile/test/corpus/query-sql.txt`, new): at minimum
   - `query 'SELECT id FROM products WHERE price > $?'`
   - `query 'SELECT id FROM products WHERE id IN ($?, ...)'`
   - `query 'UPDATE t SET a = $1 WHERE b = $12'`
   - `query 'INSERT INTO t (a, b) VALUES ($?, $?), ...'`
   - a **lowercase** and a **mixed-case** statement (`select … From …`), proving keyword
     matching is case-insensitive;
   - `query 'SELECT count(*), x FROM t -- note'` (operator soup plus a SQL comment), and the
     `"…"` spelling `query "SELECT count(*), x->>'k' FROM t"` (a JSON operator; `'` is legal in
     a `"…"` string, and is the exact inline-literal shape the Quick Fix handles);
   - `query 'select "Order" from "orders" where total >= 10.5'` (quoted identifier, number);
   - a statement holding `%`, `::`, `<>`, `||`, `IS NOT NULL`, and a tab/newline, each parsed
     with no `ERROR`;
   - `query regex '…'`-adjacent shapes stay `raw_string` (one case).
3. **`editors/zed/languages/blastfile/highlights.scm`.** Map the new nodes onto captures the
   bundled themes actually style (the file's own header lists them): `sql_keyword` → `@keyword`,
   `sql_parameter` → `@variable.special`/`@variable` (or the existing `$IDENT` pair),
   `sql_ellipsis` → `@variable.special`/`@variable`, `sql_number` → `@number`, `sql_word` →
   `@property` or no capture (decide against a theme), `sql_operator` → `@operator`,
   `sql_string` itself → `@string.special`. Never capture the whole string twice; the interior
   captures replace the single grey run.
4. **Regeneration + re-pin, two commits.** `src/parser.c`, `src/grammar.json` and
   `src/node-types.json` are regenerated with the pinned `tree-sitter 0.25.10` and committed
   **first** (commit A); `editors/zed/extension.toml`'s `[grammars.blastfile] rev` is set to
   commit A in a **second** commit (commit B), because `make zed-check` proves
   `git cat-file -e <rev>:editors/tree-sitter-blastfile/src/parser.c`.
5. **The editor guard grows.** `scripts/check_zed_extension.py` gains properties for
   `sql_string`/`sql_parameter`/`sql_ellipsis` in the grammar and their captures in
   `highlights.scm`; `scripts/test_check_zed_extension.py` asserts each new property name and
   has one negative twin (remove the capture → red). Update the count assertion in the same
   commit.

# Context

- **Grammar today.** `raw_string: /'[^']*'/` (`grammar.js:125`), `_value` admits it (`:77`),
  `keyword` already lists `query` (`:117`), `regex_raw_string: seq('regex', alias($.raw_string,
  $.regex_body))` (`:129`). `MB.31.4` (earlier) already regenerated `parser.c` and re-pinned the
  rev for `immutable regex`; rebase your regeneration on top, do not revert it.
- **Why no external scanner.** An external C scanner means `src/scanner.c`, a WASI build step
  and a grammar that no longer compiles from `grammar.js` alone; the design (`MB.34.2`) chose
  ordinary rules plus a one-char fallback. A grammar that can fail to parse a legal `Blastfile`
  is worse than no highlighting, which is why the fallback and the "never stricter" corpus case
  are acceptance items.
- **Case-insensitive keywords in tree-sitter.** Use the mechanism the generator supports (a
  `choice` of spellings, or a regex with the supported flag); prove it with the lowercase corpus
  case rather than asserting it.
- **Pins that move together.** `src/grammar.json`, `src/node-types.json`, `src/parser.c`;
  `src/tree_sitter/*.h` are gitignored and must stay out of the commit. The generator host fact
  (`/tmp/opencode/bin/tree-sitter`, 0.25.10) and the two-commit rule are in `LESSONS.md`.
- **Not this leaf.** No Rust, no manifest validator, no LSP handler, no docs-site JS
  (`MB.32.1` owns `docs/javascripts/blastfile-highlight.js` and gets the same vocabulary as a
  brief update), no `docs/**`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ /tmp/opencode/bin/tree-sitter --version                    # 0.25.10
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter generate
$ git diff --exit-code -- editors/tree-sitter-blastfile/src  # no drift after generate
$ /tmp/opencode/bin/tree-sitter test                         # corpus, new cases included
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_zed_extension.py
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project \
    scripts/test_check_zed_extension.py
$ make zed-check
$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1)
$ test ${#REV} -eq 40 && git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ git cat-file -e "$REV:editors/zed/extension.toml"
```

`tree-sitter parse` on each corpus input prints no `ERROR`/`MISSING`. The corpus's
`query 'SELECT …'` case must show a `sql_string` node with at least one `sql_keyword` and one
`sql_parameter` child (paste the tree). Paste the two commit hashes and the two-commit split in
`RESULT.md`.

# Notes

- **Capture the interior, not the wrapper.** A `(sql_string) @string.special` plus child
  captures is correct; a second `(raw_string) @string.special` inside would paint over the
  keywords in some themes.
- **`...` is not an operator.** It needs its own node so the hover/LSP and the highlighter agree;
  three dots inside `IN (…)` are one token, never three operators.
- **Do not add a `highlights` test for a capture that no bundled theme styles**; the file's
  header names the fourteen verified captures — stay inside them or explain a new one.
- The Zed extension's wasm build shares the cargo registry lock: run `make zed-check` when no
  other node is compiling.
