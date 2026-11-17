---
id: MB.31.4
title: "Hover and tree-sitter teach bare immutable and regex: regenerate parser.c, then re-pin the Zed grammar rev"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.31.2]
paths:
  - crates/blastit/src/lsp/hover.rs
  - "editors/tree-sitter-blastfile/grammar.js"
  - "editors/tree-sitter-blastfile/src/parser.c"
  - "editors/tree-sitter-blastfile/src/grammar.json"
  - "editors/tree-sitter-blastfile/src/node-types.json"
  - "editors/tree-sitter-blastfile/test/corpus/sections.txt"
  - "editors/tree-sitter-blastfile/highlights.scm"
  - editors/zed/extension.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:33"
updated: "2026-11-17T12:33"
---

# Goal

Make the editor surfaces tell the truth about `immutable`, and keep the pinned Zed grammar
installable.

1. **`blastit lsp` hover** (`crates/blastit/src/lsp/hover.rs:120-124`) stops teaching the
   staircase. New sentence: bare `immutable` caches every exact file; a list takes the
   **same** patterns as a `view` (a trailing `/*` crosses `/` at any depth, `regex '…'` is
   raw); an `index` or `fallback` answer is always `no-cache`. The table's own test requires
   every key to appear in `docs/reference/blastfile.md`, which `MB.31.1` already rewrote.
2. **`editors/tree-sitter-blastfile`** parses the same language: `immutable regex '…'` must
   colour as a pattern rather than fall back to a loose `name` + `raw_string` shape, with a
   corpus case for it, for a list, and for bare `immutable`. `grammar.js` may be looser than
   `blastit-manifest` but **never stricter**.
3. **`src/parser.c` is regenerated** with the pinned `tree-sitter-cli 0.25.10` and committed
   **first** (commit A).
4. **`editors/zed/extension.toml` `[grammars.blastfile] rev` is re-pinned to commit A** in a
   **second** commit (commit B) — the `MB.22.4` → `MB.22.5` two-step recorded in
   `docs/knowledge/LESSONS.md`, because `make zed-check` proves
   `git cat-file -e <rev>:editors/tree-sitter-blastfile/src/parser.c`.

# Context

- **The hover row, verbatim today:** `` "`immutable \"/assets/*\"` adds `Cache-Control:
  public, max-age=31536000, immutable`; `*` stops at `/`, so a nested tree needs one glob per
  depth." `` It is one of `HOVER_TABLE`'s rows, ≤600 bytes each, and
  `crates/blastit/src/lsp/hover.rs:229-251` asserts the table is sorted, bounded (≤48) and
  every key is present in the reference page.
- **Grammar state:** `keyword` already lists `immutable` (`grammar.js:117`), and
  `regex_raw_string: seq('regex', alias($.raw_string, $.regex_body))` exists (`:130-132`),
  but `_value` (`:86-96`) does not include it, so `immutable regex '/static/…'` degrades to
  `name` + `raw_string` and the `(regex_raw_string (regex_body) @string.regex)` capture never
  fires. `_values` is optional, so bare `immutable` already parses (corpus:
  `test/corpus/sections.txt:23`, `trailing-comma-newline.txt:6`).
- **Generator host fact:** the pinned binary is `tree-sitter 0.25.10`
  (`package.json` `devDependencies.tree-sitter-cli`), fetched from the upstream release and
  run as `/tmp/opencode/bin/tree-sitter`; `npx` ships no native binary in this environment
  (`MB.22.4`'s `RESULT.md`). ABI 14 stays (no `tree-sitter.json`).
- **Pins that must move together:** `src/grammar.json`, `src/node-types.json` and
  `src/parser.c` are generated; `src/tree_sitter/{alloc,array}.h` are gitignored and must stay
  out of the commit. `scripts/check_zed_extension.py` (42 properties) and
  `scripts/test_check_zed_extension.py` (19 negative cases) run in `make zed-check`; the
  regeneration gate (`git diff --exit-code -- src` after `tree-sitter generate`) is
  `MB.26.9`'s, so run it by hand here and paste both tails.
- **Not this leaf:** no Rust manifest change (`MB.31.2`), no scheduler change (`MB.31.3`), no
  page prose beyond the hover row (the reference page is `MB.31.1`'s). If the hover test
  needs a reference-page sentence that is missing, report it rather than editing `docs/`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit --lib lsp
$ cargo test -p blastit --test critical_lsp
$ make zed-check
$ /tmp/opencode/bin/tree-sitter --version            # 0.25.10
$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter generate
$ git diff --exit-code -- editors/tree-sitter-blastfile/src   # no drift after generate
$ /tmp/opencode/bin/tree-sitter test                 # corpus, including the new cases
$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1)
$ test ${#REV} -eq 40 && git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ git cat-file -e "$REV:editors/zed/extension.toml"
$ git rev-parse "$REV"                              # commit A, the regeneration commit
```

Corpus cases to add: `immutable` bare; `immutable "/static/*"`;
`immutable "/a/*", "/b/{id:int}"`; `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`
(must contain a `regex_raw_string` node, no `ERROR`/`MISSING`). One negative is not needed —
the grammar is a colourist.

# Notes

- **Two commits, in this order.** Commit A: `grammar.js`, regenerated `src/**`,
  corpus, hover row. Commit B: `extension.toml` `rev = <commit A>` only. A single commit
  cannot satisfy `zed-check`: the rev it names would not contain the new `parser.c`.
- Resolve the `_value` overlap the way `table_grant` does (`prec(1, …)`), not by deleting
  `name` from `_value`: a keyword is legal wherever a name is (`grammar.js` §9.2), and a
  stricter grammar is a wrong squiggle in a file `blastit` accepts.
- The hover row must stay one sentence of user-facing value: no `REGEX_SIZE_LIMIT`, no
  two-phase-compile explanation, ≤600 bytes.
