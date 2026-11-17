---
id: MB.34.4
title: "blastit-manifest: carry each query's raw text and string Position, validate the template, and put every query refusal at its exact inner line:col"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.34.3]
paths:
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/blastfile/lower.rs"
  - "crates/blastit-manifest/tests/validate.rs"
  - "crates/blastit-manifest/tests/critical_manifest_011.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:17"
---

# Goal

Make the `Blastfile` model carry enough to point at a `query`'s SQL, and make the three refusals
land on the exact `(line, col)` inside the string. This is the leaf that turns
`MB.34.3`'s offsets into editor squiggles; it changes no sentence the CLI already prints, except
to add a site where there was none.

1. **Lower records the site and the raw text.** In `crates/blastit-manifest/src/blastfile/lower.rs`
   the `NodeKind::Query` arm (`:620-632`) already decodes the string (`raw_value`/`str_value`),
   normalises it and dedupes. Alongside `queries: Vec<String>`, record for each unique entry:
   - the `Position` of the **first character inside the opening quote** (so offset `0` is the
     first SQL character), and
   - the **raw decoded** statement text (before `normalize`), and
   - for a `"…"` string only, the decoded→source character map needed to turn an inner offset
     into a source offset; a `'…'` string needs no map.
   Expose it on `PgGrant` as a read-only accessor (e.g. `query_sites()`), defaulting to empty
   for a grant built by `PgGrant::new` — the model stays constructible in tests.
2. **`check_queries` validates.** `crates/blastit-manifest/src/validate.rs:271-294`:
   - keep the redacted-`…` and inline-literal refusals and their exemption
     (`KNOWN_STATIC_SETUP_SQL`), but compute `at` from the site + the offset the lexer gives for
     the `…` or the quoted literal;
   - then run `blastit_sqlpolicy::check_template(raw)` and push a new positional refusal for
     `Empty`, `MultipleStatements`, `StrayEllipsis`, `Syntax`;
   - never `Position::NONE` for a query parsed from a `Blastfile`; a programmatic grant
     (`PgGrant::new`, no site recorded) keeps `NONE`, and one test pins that boundary so the
     "never `0:0`" promise is about documents, not about values nobody wrote.
3. **New/updated error variants** in `crates/blastit-manifest/src/error.rs`:
   - `RedactedQueryPlaceholder { location, proxy, rule, at }` and
     `LiteralQuery { location, proxy, rule, at }` gain `at`;
   - one new variant for the parse refusals carrying `location`, `proxy`, `rule`, `at` and the
     `kind`'s one fixed sentence (plus pg_query's clipped message for `Syntax`);
   - `position()` (`error.rs:1403`) returns the `at` for all of them;
   - messages stay one line, never echo the statement, keep `{location}: queries[{rule}] …`.

# Context

- **Why the raw text and not the normalised one.** `normalize` collapses whitespace, replaces
  comments and rewrites `IN` lists (`normalize.rs:107+`), so its offsets are not the file's.
  Validating the normalised text and mapping back is the defect this leaf exists to avoid.
- **`Position` arithmetic.** `error::Position` is 1-based line and 1-based `char` column
  (`error.rs:58-90`); `lower.rs:184-219` already has the offset→`Position` walker for one file.
  Reuse that logic instead of a second implementation, or move it to one helper both call.
- **Protected files.** `crates/blastit-manifest/tests/critical_manifest_011.rs:625,640,663,683,709`
  constructs `LiteralQuery`/`RedactedQueryPlaceholder`; it is listed in `paths` and the only
  permitted change is adding `at: Position::NONE` (or a real position where the fixture has
  one). No assertion is removed or weakened. `crates/blastit-manifest/tests/validate.rs:615`
  is **not** protected and gains the exact-position cases.
- **What must not change.** `blastit check`'s existing sentences and exit codes; the `--json`
  finding shape (`fix.action` stays the same set — the Quick Fix is an editor feature, not a
  new machine action); warnings and their `sites` order
  (`docs/design/blastfile-lsp-zed.md` §5.4); and `Manifest::from_blastfile_str`'s signature.
- **The LSP needs nothing new here.** `crates/blastit/src/lsp/mod.rs:813-831` already calls
  `error.position()`, so a correct `at` is a correct squiggle. Do not edit `crates/blastit/**`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
$ cargo test -p blastit-manifest --test validate --test critical_manifest_011
$ cargo test -p blastit-manifest --test docs_fences
$ cargo clippy -p blastit-manifest --all-targets -- -D warnings
```

New cases in `tests/validate.rs` (or a focused new non-protected test file) must assert the
exact `line:col` for:

1. a one-line `query 'SELCT id FROM products WHERE price > $?'` — the squiggle on `SELCT`, the
   column computed by hand in the test;
2. a **multi-line** `query 'SELECT id\nFROM products\nWHERE price > $?'` with the typo on line
   three — the position follows the newline, not the token start;
3. a `"…"` string with escapes (`query "SELECT \"name\" FROM t WHERE a = $?"`) — the position is
   the SQL token, not the backslash;
4. a redacted `…` and an inline literal — both now carry a real position (assert not
   `Position::NONE`, and assert the column of the `…`/quote);
5. a duplicate (two spellings that normalise alike) — one entry, one refusal, at the first site;
   the second gets no diagnostic;
6. `crates/blastit-manifest/tests/critical_manifest_011.rs` still compiles and passes with only
   the new field added (paste a diff of exactly those hunks in `RESULT.md`).

Also paste `git diff --stat` and the `grep -n 'Position::NONE' src/validate.rs` result showing
`check_queries` no longer reaches one.

# Notes

- **Model surface is public API.** Adding an accessor is fine; changing `queries()`'s type is
  not (twenty call sites, incl. `blastit-scheduler`, `blastit-py`, tests). Keep the parallel
  vector and document its invariant: `query_sites.len() == queries.len()`, populated by the
  parser, empty for a programmatic grant.
- **Do not add a `pg_query` dependency here.** One new import from `blastit_sqlpolicy` only.
- **Bounded.** Sites are `Position` + a `String` per unique query; the document is capped at
  `MAX_SOURCE_BYTES` and queries dedupe, so the memory is twice the file at worst — state the
  bound in the doc comment and do not add a second cap.
- If `/tmp` scratch or a build collision with another node appears, remember this leaf must not
  run `make check` in parallel with a cargo-heavy node; run the four `cargo test` commands when
  the tree is free.
