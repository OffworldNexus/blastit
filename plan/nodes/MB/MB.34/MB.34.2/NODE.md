---
id: MB.34.2
title: "Design the template rewrite, the exact decoded-span contract, codeAction/hover and the SQL grammar boundary (ADR-0013 amendment)"
kind: task
status: ready
owner: designer
priority: 1
depends_on: [MB.34.1]
paths:
  - "docs/design/blastfile-lsp-zed.md"
  - "docs/design/blastfile-dsl.md"
  - "docs/decisions/ADR-0013-blastfile-lsp-and-zed.md"
  - "docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:17"
---

# Goal

Turn the accepted sentences of `MB.34.1` into one technical contract every later leaf
implements, and get it through four goldfish critics. Four things are decided here and nowhere
else:

1. **The template rewrite, exactly.** One function in `blastit-sqlpolicy`:
   - `$?` → the next `$N` in statement order; `$1…$n` pass through unchanged;
   - `IN ($?, ...)` → `IN ($1)` (a one-element list is valid SQL, and the list length is
     irrelevant to parsing); `IN ($1, $2, ...)` → `IN ($1, $2)`;
   - `($?, $?), ...` in a `VALUES` list → one row;
   - `...` anywhere else is a refusal with its own sentence (it is not SQL);
   - an empty or whitespace-only entry and a `;`-separated second statement are refusals;
   - `stmts.len() == 1` after `pg_query::parse`; PostgreSQL 18 grammar; the CST's own
     `cursorpos` (1-based) is the failure offset when present, else the first character of the
     statement — **never `0:0`**.
   State the expansion table and the refusal set in `docs/design/blastfile-dsl.md`; state that
   this is a *parse*, not a policy (`COPY … PROGRAM` stays legal here).
2. **The span contract.** The validator returns offsets into the **raw decoded** query text
   (characters, 1-based at the consumer), and `blastit-manifest` maps
   `string-token Position + inner offset` to a `Blastfile` `Position`. Define:
   - which `Position` is the origin (the first character *inside* the opening quote, so an
     offset of `0` is the first SQL character);
   - the decoded→source map for `"…"` (four escapes) and multi-line strings, and that a
     mapping which cannot be computed is a refusal at the token, not at `0:0`;
   - that character counting matches `error::Position` (1-based, chars, not bytes).
3. **The protocol additions.** In `docs/design/blastfile-lsp-zed.md` §7/§8 (new subsection):
   - `initialize` advertises `codeActionProvider: true` (and nothing else new);
   - `textDocument/codeAction` returns **one** Quick Fix
     `{title: "Parameterize SQL for Blastfile ($?)", kind: "quickfix", edit: {changes: {uri:
     [one TextEdit]}}}` when the requested range intersects a `query` string whose diagnostic is
     `LiteralQuery`, or whose text holds `$1…$n`; the edit replaces the **whole string
     interior** (numbering is statement-wide) and the result must parse and contain `$?`;
   - no Quick Fix for `KNOWN_STATIC_SETUP_SQL`, none for a redacted `…`;
   - `context.diagnostics` is a hint, never the source of truth: the server re-derives the
     finding from its own tracked text;
   - hover on `$?` and `...` **only inside a `query` string**, with the two paragraphs the
     reference page now carries; `word_at` stays a word scanner for every other word.
4. **The grammar boundary.** `editors/tree-sitter-blastfile` gains a `sql_string` node for the
   interior of a `query` value: `sql_keyword`, `sql_parameter` (`$?`, `$1…$n`),
   `sql_ellipsis`, `sql_number`, `sql_word`, `sql_operator`, and a one-character `sql_other`
   fallback so the grammar is **never stricter** than `raw_string`. Case-insensitivity and the
   keyword list are decided here with the corpus shape, not in the grammar leaf. Record that
   `editors/zed/languages/blastfile/highlights.scm` is the only consumer (no second Zed
   extension, no external C scanner).

Amend `ADR-0013` with a dated amendment (the accepted text stays as the record): the new span
obligation for `query` refusals and the one new protocol method. Add a note to `ADR-0003` only
if the inline-literal refusal's message changes.

# Context

- **The facts a critic must be able to check in-tree.**
  `lower.rs:625-631` normalises + dedupes; `error.rs:419-460` is the two position-less
  refusals; `error.rs:1403` is `position()`; `validate.rs:271-294` is `check_queries`;
  `lsp/mod.rs:743` is `capabilities()`; `lsp/mod.rs:762-831` is diagnostics/`Position::NONE`
  rendering; `lsp/mod.rs` `word_at`; `grammar.js:77,125`; `extension.toml` `rev`.
- **`pg_query`'s error shape is the design's to pin.** Read `pg_query 6.2.1`'s
  `Error`/`ParseResult` locally (`~/.cargo/registry/src/**/pg_query-6.2.1/src/`) and state
  whether `cursorpos` is exposed; if it is not, the design says what the fallback offset is and
  a test pins it. Do not guess in the brief of a later leaf.
- **The rewrite must be total and deterministic.** Same input, same output, no map iteration,
  no locale. Editing `$?` numbering is `u32`/`usize` arithmetic with a cap tied to the statement
  byte cap; a statement that would exceed it is refused, not wrapped.
- **Critics (fresh context, disk only):** `security` (a `query` is a trust boundary; the
  validator must not become a second allowlist, must not print the statement, and must not let
  a rewrite hide a second statement), `implementability` (the mapping, the dedupe, the two
  string spellings), `testability` (what is a protected test and what is a unit test),
  `consistency` (one refusal sentence per defect, no `0:0`).
- **Do not** write Rust here. Do not edit `docs/reference/**` or `docs/local.md` —
  `MB.34.1` owns them; if the design needs a sentence the page lacks, list it as a follow-up
  for `MB.34.1`'s owner instead of editing.
- **Protected-test constraint to record.** Adding `at: Position` to the two error variants
  forces mechanical edits in `crates/blastit-manifest/tests/critical_manifest_011.rs`; the
  design names that file and says the only permitted change there is the new field, with no
  assertion removed.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make docs-check
$ git diff --stat -- docs/design docs/decisions
```

plus, in `RESULT.md`:

1. the four critic reports with `VERDICT: PASS` (or the findings and their resolution),
2. the expansion table and refusal set, verbatim, as they will appear in the design,
3. the `pg_query` error-shape finding with the file path and line it was read from,
4. one worked `line:col` example (source lines in, `Position` out) that `MB.34.4` can turn into
   a test.

The design is accepted when a cheap implementer can write `MB.34.3` and `MB.34.4` from it
without re-deciding any of the four points above.

# Notes

- **One writer per design doc, incremental `.part` files** (`LESSONS.md`, `MB.3.14`): if the
  doc is large, write sections to `*.part` and assemble once.
- The design must say what happens to **deduped** queries: two spellings that normalise alike
  are one entry; the first site is the one that gets the squiggle, and the design states that a
  duplicate gets no second diagnostic (the manifest is one grant, not two).
- Keep `docs/design/**` and `docs/decisions/**` off the public site (they are excluded); the
  reader-facing contract stays in `MB.34.1`'s pages.
