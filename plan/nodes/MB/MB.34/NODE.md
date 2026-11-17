---
id: MB.34
title: "SQL + $?/... template validation with exact inner line:col spans, Quick Fixes and highlighting across blastit-sqlpolicy, blastit-manifest, blastit lsp and the Zed grammar"
kind: epic
status: ready
priority: 1
depends_on: [MB.31]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:17"
---

# Goal

Every `query '…'` in a `Blastfile` is **validated as the PostgreSQL statement it claims to
be**, with the red squiggle on the exact `line:col` of the typo — inside the string, not at
`0:0` — and the developer is offered a one-click Quick Fix that turns a pasted statement into
Blastit's parameter-template form. Hover explains what `$?` and `...` bind to. The editor's
grammar colours the SQL inside the string, so a `query` reads as SQL and not as one grey run.

Owner mandates (`0.2.0`, 2026-11-17):

> *"could the lsp and highlighting also cover the SQL inside the various statements in the
> Blastfile? … and by covering SQL I mean of course including the parameters replacing and so
> forth"*

Today `blastit check` and `blastit lsp` check a `query` entry for two things only
(`crates/blastit-manifest/src/validate.rs::check_queries`): the redacted `…`, and a quoted
literal outside `KNOWN_STATIC_SETUP_SQL`. **Neither runs the statement through
`pg_query::parse`**, both report `Position::NONE`, and a `$?` template is never even rewritten
into SQL PostgreSQL can parse. A grant that is syntactically impossible — a missing `FROM`, a
stray `)`, `SELCT`, an empty string, two statements in one entry — is accepted by every gate
and fails at runtime as `sql.denied`, which is the worst possible place to learn it.

The five surfaces, in DX-first order:

1. **User docs** (`MB.34.1`) — what is validated, where the squiggle lands, what the Quick Fix
   does, and what `$?`/`...` expand to. Before any code.
2. **Design** (`MB.34.2`) — the template rewrite, the exact span contract, the `codeAction`
   shape, the hover contract and the grammar boundary, with `security`, `implementability`,
   `testability` and `consistency` critic reviews.
3. **`blastit-sqlpolicy`** (`MB.34.3`) — one deterministic template validator: rewrite
   `$?` → `$1, $2, …`, `IN ($?, ...)` → `IN ($1)`, `VALUES (…), ...` → one row, tracking a
   decoded-character → byte offset so every refusal carries an offset into the *original*
   query text; parse with `pg_query::parse`; require PostgreSQL 18 syntax and exactly one
   statement (an empty or `;`-joined entry is refused).
4. **`blastit-manifest`** (`MB.34.4`) — the CST already knows the string token's `Position` and
   the raw text; carry both into the model and map `Position + inner offset` to the exact
   `(line, col)` in the `Blastfile`, then emit the refusals with that site.
5. **Editor surfaces** (`MB.34.5`, `MB.34.6`) — `textDocument/codeAction` with the
   `"Parameterize SQL for Blastfile ($?)"` Quick Fix, `$?`/`...` hover, and the tree-sitter SQL
   tokenization plus a re-pinned Zed grammar rev.

`0.2.0` scope. `depends_on: [MB.31]` because `MB.31.4` re-pins the same grammar files and the
same `extension.toml` rev.

# Context

- **The validator's home.** `crates/blastit-sqlpolicy` already owns `lexer::lex` (the same
  scanner `normalize` and `has_quoted_literal` use) and depends on `pg_query = "6.2.1"` with
  `serde_json`; `crates/blastit-manifest` does **not** depend on `pg_query` and must not start:
  the manifest calls a function the crate already exposes.
- **The stored text is normalised, the source text is not.**
  `crates/blastit-manifest/src/blastfile/lower.rs:625-631` runs
  `blastit_sqlpolicy::normalize` before pushing into `PgGrant`, and dedupes identical results.
  Validation must therefore run on the **raw decoded** string (offsets in the source file) and
  keep the dedupe: the first writable site wins. Normalising first and then mapping offsets back
  through whitespace collapse and comment removal is the trap this epic must not fall into.
- **Where the position comes from.** `lower.rs` has the CST and the source; `Cst::strings`
  returns the token text including its quotes, and `lower.rs:184-219` already turns a span into
  a 1-based `Position` counted in `char`s. The raw spelling has no escapes; the `"…"` spelling
  has four (`crates/blastit-manifest/src/error.rs` `InvalidEscape`), so a `"…"` query needs the
  decoded→source map too — pin it with a multi-line and an escaped-fixture test.
- **Why the existing two refusals have no site.** `ManifestError::RedactedQueryPlaceholder` and
  `ManifestError::LiteralQuery` (`crates/blastit-manifest/src/error.rs:419-460`) carry only
  `location`, `proxy` and `rule`; `error.rs:1403` `position()` cannot return one. The LSP then
  draws them at `(0,0)` (`crates/blastit/src/lsp/mod.rs:813`). They must gain `at: Position`, and
  `crates/blastit-manifest/tests/critical_manifest_011.rs:625,640,663,683,709` constructs them —
  that file is **protected**: list it, add `at` only, delete no assertion.
- **The grammar to colour.** `editors/tree-sitter-blastfile/grammar.js:125` is
  `raw_string: /'[^']*'/` and `_value` (`:77`) admits it for `query`; every `'…'` is one leaf.
  Highlighting the interior needs a `sql_string` node built from ordinary rules (no external C
  scanner, no second SQL extension), and `editors/zed/extension.toml` pins
  `rev = "053b56e…"`, so `src/parser.c` and the new `rev` are two commits (`LESSONS.md`, the
  `MB.22.4` → `MB.22.5` two-step).
- **The Zed protocol surface is additive.** `crates/blastit/src/lsp/mod.rs` advertises
  `capabilities()` at `:743`; `codeActionProvider` is not there, and §7's hover is a static word
  table (`hover.rs`) whose lookup never sees `$?` or `...` (`word_at` walks word characters).
  `critical_lsp.rs` (1880 lines, protected) proves the ten existing messages; it grows cases and
  loses none.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-sqlpolicy
$ cargo test -p blastit-manifest
$ cargo test -p blastit --lib lsp
$ cargo test -p blastit --test critical_lsp
$ make zed-check
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
```

and, pasted into the epic `RESULT.md`, one live transcript over a `Blastfile` holding
`query 'SELCT id FROM products WHERE price > $?'` on line 9, column 9, opened in Zed:

- one squiggle whose range starts at the `SELCT`, not at `(0,0)`; the message is the CLI's
  sentence; `blastit check` prints the same `9:16` (the `S` of `SELCT`);
- the Quick Fix `"Parameterize SQL for Blastfile ($?)"` turns
  `query 'SELECT id FROM products WHERE email = ''a@b.com'''`-shaped input (a quoted literal in
  a `"…"` string, the real spelling) into `$?` and the second `blastit check` is clean;
- hover over `$?` and over `...` shows the binding/collapse paragraphs.

`grep -rn 'Position::NONE' crates/blastit-manifest/src/validate.rs` must no longer be reachable
from `check_queries` for a parsed document (programmatic grants keep it by the stated boundary),
and `grep -rn 'pg_query' crates/blastit-manifest/Cargo.toml` stays empty.

# Notes

- **Order is causal, not cosmetic.** `MB.34.1` fixes the sentences a developer reads; `MB.34.2`
  turns them into the span/rewrite/`codeAction` contract; `MB.34.3` builds the engine; `MB.34.4`
  wires positions; `MB.34.5` and `MB.34.6` are path-disjoint editor leaves after that; `MB.34.7`
  proves the documented commands verbatim.
- **No second SQL policy.** The template validator answers one question: *is this a single,
  parseable PostgreSQL 18 statement once Blastit's placeholders are expanded?* It must not
  reject `COPY … PROGRAM` or `pg_read_file` — an exact `query` is trusted as written
  (`docs/reference/blastfile.md`), and `blastit_sqlpolicy::analyze` stays the table-grant gate.
- **No pytest-in-CI shortcut.** The three leaves that touch Rust each bundle their own tests;
  none may leave tests for a later "write tests" node.
- **`editors/zed` shares the cargo registry lock.** If another node is compiling, run
  `make zed-check` after it finishes; the tree-sitter regeneration is CPU-bound and offline.
