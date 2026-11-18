# Result MB.34.2

Status: done (attempt 5/5). Designer node: four decisions only, no Rust, the four user
pages untouched.

Base: `release/0.2.0` working tree, `docs/reference/blastfile.md` as MB.34.1 left it.

## What was written (paths in `paths` only)

| File | Change | Lines |
|---|---|---|
| `docs/design/blastfile-dsl.md` | §3's MB.34.2 requirement table (R31–R36), §4.4's four new rows, §4.5's two caps, **§13.4** (the template: what is validated, the expansion table, the refusal set, the `pg_query` finding, the span contract, `parameterize`), §13.5 (which leaf builds what), §18.1 (three sentences gain a site), §20's six test rows | +297 |
| `docs/design/blastfile-lsp-zed.md` | §0.2 (the amendment map), §0.1's non-goals, §2.2/§2.3 (`textDocument/codeAction`, `codeActionProvider`), **§4.6** (`Lines::offset`, P7), §4.2/§5.3 (the two variants leave the `(0,0)` lists), §7.3's closing note, §7.5 (H7–H8), **§7.6** (the two SQL tokens), **§7A** (the Quick Fix, C1–C8), **§9.2a** (`sql_string`, measured), §9.3/§9.5 (rows and corpus), §10.1/§10.2 (four rules, no new capture name), **§13** (R60–R68, the test delta, the two editor leaves) | +454 |
| `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` | the status line and a dated **Amendment 2026-11-18** (three decisions, consequences, rejected alternatives) | +71 |

Nothing outside those three paths was edited (`ADR-0003` is **unchanged**). `docs/reference/blastfile.md`, `docs/local.md`,
`docs/ci.md` and `docs/troubleshooting.md` are MB.34.1's and already carry every sentence
this design quotes (`docs/reference/blastfile.md:277,286-292,298-304`).

## The four decisions

### 1. The total deterministic template rewrite (verbatim, `blastfile-dsl.md` §13.4.2)

| # | Written (in code, outside quotes and comments) | Expanded | Rule |
|---|---|---|---|
| 1 | `$?` | the next `$N` | `N = 1, 2, 3, …` in statement order, counting **only** `$?` |
| 2 | `$1` … `$n` (digits) | copied unchanged | PostgreSQL's own numbering is legal as written, so nothing is renumbered |
| 3 | `IN ($?, ...)` | `IN ($1)` **for the parse** | the `, ...` is **deleted** and the surviving placeholder keeps its number; a one-element list is valid SQL, so the parse needs no list length. The **wire** form is the page's — `IN ($1, $2, …, $n)`, one `$n` per value at any length; the app numbers its own parameters, and the grant only has to parse |
| 4 | `NOT IN ($?, ...)` | `NOT IN ($1)` **for the parse** | row 3's rule; the reference promises `NOT IN` too |
| 5 | `(<row>), ...` after the keyword `VALUES` | `(<row>)` **for the parse** | the `, ...` is **deleted**; every repeated row still reaches PostgreSQL at run time, as the page says, because the rows are the app's and the `...` is Blastit's |
| 6 | an item that is exactly `...` in any other position (a longer run of dots included) | — | refusal `StrayEllipsis` at its first `.` |
| 7 | nothing outside space, comments and one trailing `;` | — | refusal `Empty` at offset `0` (`query ';'` is no statement) |
| 8 | a second statement (a code `;` that is not the text's last code character) | — | refusal `MultipleStatements` |
| 9 | the expanded text does not parse | — | refusal `Syntax` at the **first non-trivia code character** (`0` when the text opens with code), fixed sentence (§13.4.3); pg_query's message is never rendered (§13.4.4) |
| 10 | a decoded statement over `MAX_TEMPLATE_CHARS`, or more than `MAX_TEMPLATE_PARAMS` `$?` | — | refusals `TooLong` / `TooManyParameters` at `0` / at the offending `$?` |

Total and deterministic: the output is the input's characters in order with exactly two
deletions and the numbering — no re-spacing, no case folding, no comment removal, no map
iteration, no locale. Numbering is `u32` with `checked_add`; output ≤ `input + 5 × (input /
2)`, reserved as `4 × MAX_TEMPLATE_CHARS + 8`. Caps: `MAX_TEMPLATE_CHARS = 65_536`,
`MAX_TEMPLATE_PARAMS = 1_024`. It is a **parse, not a policy**: `COPY … PROGRAM`,
`pg_read_file`, `dblink` stay legal here. `stmts.len() == 1` after `pg_query::parse`; one
trailing `;` legal.

### 2. The refusal set (verbatim, `blastfile-dsl.md` §13.4.3)

| `kind` | Written | `offset` into the decoded text | The CLI prints |
|---|---|---|---|
| `Empty` | `query ''`, `query '  '`, `query '-- x'`, `query '  ;  '` | `0` | ``this `query` is empty; write the one statement it grants`` |
| `MultipleStatements` | `query 'SELECT 1; SELECT 2'` | the first code character after the first code `;` that is not the text's last code character | ``a `query` is one statement; this one starts a second`` |
| `StrayEllipsis` | `query 'SELECT id FROM ...'` | the first `.` of the three | ```...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row``` |
| `Syntax` | `query 'SELCT id FROM t'`, `query 'SELECT id FROM'`, `query '\nSELECT id FROM'` | `0`, `0`, `1` — the first non-trivia code character, see the `pg_query` finding | ``the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` `` |
| `TooLong` | a decoded statement of 65 537 characters | `0` | ``a `query` is 65536 characters at most; split it, or grant the tables and columns instead`` |
| `TooManyParameters` | a 1 025th `$?` | that `$?` | ``a `query` holds 1024 bind placeholders at most; grant the tables and columns instead`` |
| `TooManyQueries` | the 4 097th distinct entry in one file | that entry's `at` | ``a `Blastfile` validates 4096 `query` entries at most; fold the shared statements into a `trait` that many views `use` `` |

One entry, one sentence (`if … else if … else`): redacted `…` → inline literal → template.
`Syntax` **never renders pg_query's message**: it quotes the offending lexeme, which can be
a value, so it is used to classify the refusal only.

### 3. The `pg_query` error-shape finding (the hard fact)

Read locally on this host (no guessing, no build):

```
$ sed -n '30,40p' ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/pg_query-6.2.1/src/query.rs
33:     let result = unsafe { pg_query_parse_protobuf(input.as_ptr()) };
34:     let parse_result = if !result.error.is_null() {
35:         let message = unsafe { CStr::from_ptr((*result.error).message) }.to_string_lossy().to_string();
36:         Err(Error::Parse(message))
37:     } else {
...
$ sed -n '11,18p' ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/pg_query-6.2.1/libpg_query/pg_query.h
11: 	char* message; // exception message
...
15: 	int cursorpos; // char in query at which exception occurred
...
$ sed -n '92,102p' ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/pg_query-6.2.1/libpg_query/src/pg_query_parse.c
100: 		error->lineno    = error_data->lineno;
101: 		error->cursorpos = error_data->cursorpos;
$ sed -n '41p' ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/pg_query-6.2.1/src/lib.rs
41: mod bindings;
```

- `pg_query 6.2.1` exposes `Error::Parse(String)` (`src/error.rs:11`), and the string is only
  `(*result.error).message` (`src/query.rs:35`, wrapped at `:36`).
- The C struct carries the position (`libpg_query/pg_query.h:15`), filled at
  `libpg_query/src/pg_query_parse.c:101` — but `mod bindings` is private (`src/lib.rs:41`),
  so a downstream crate cannot read it, and no other `pg_query::Error` variant carries a
  position (`Scan`/`Split` wrap the same message-only string).

**Verdict: `cursorpos` is not exposed. `Syntax`'s offset is pinned to the first **non-trivia
code character** of the decoded statement (`0` when it opens with code, `1` for a
leading newline) — never `Position::NONE`, never `0:0`.** The sentence is
fixed and never names the token (pg_query's message quotes the offending lexeme, which can
be a value); the epic's case (`SELCT …` at `9:16`) lands exactly on the `S`. Both pinned
shapes are test items in
`blastfile-dsl.md` §13.4.3/§13.4.4 (`SELCT …` → `0`, `SELECT id FROM` → `0`,
`\nSELECT id FROM` → `1`). If a later `pg_query` exposes the field, only `template.rs`'s `Syntax`
arm moves; the `TemplateError` contract already carries an offset.

### 4. The decoded span contract (`blastfile-dsl.md` §13.4.5)

`QuerySite { at, interior: Range<u32>, escapes: Vec<u32>, raw: String }`, parallel to
`queries()`; origin = the `Position` of the first character **inside** the opening quote
(`position_of(token.start + 1)`); `interior = token.start + 1 .. token.end - 1` (bytes,
closing quote at `.1`); `escapes` = the decoded char indices whose source spelling is one of
the four two-character escapes (empty for `'…'`); inner offset → `Position` by walking
`raw` from `at` (+2 columns for an escape, line+1/col 1 for a raw `'\n'`, +1 otherwise).
1-based chars, matching `error::Position`; a mapping that cannot be computed is a refusal at
the token (the opening quote) and, for a programmatic grant, `Position::NONE` — never
`0:0`. Dedupe: two spellings that normalise alike are one entry with the **first** site; the
duplicate gets no second diagnostic (and, by the same rule, no hover and no Quick Fix).

### 5. The protocol additions (`blastfile-lsp-zed.md` §2.3, §7.6, §7A)

`codeActionProvider: true` (boolean, no kind list) is the only new capability. One Quick Fix,
title `Parameterize SQL for Blastfile ($?)`, `kind: "quickfix"`, one `TextEdit` over the
**whole string interior**, `newText` re-encoded for the spelling; `context.diagnostics`
never read; no `workspace/applyEdit`, no `codeAction/resolve`, no `codeActionKinds`, no new
error code; unknown uri → `[]`, malformed params → `-32602`. Hover resolves `$?` and `...`
**only** inside a `query` interior, with two fixed paragraphs; `word_at` is untouched
everywhere else, so `$IDENT` still answers and `$?` in a comment does not.

### 6. The grammar boundary (`blastfile-lsp-zed.md` §9.2a, §10.2)

`sql_string` for a `query`'s `'…'` interior with `sql_keyword` (case-insensitive, 79 words),
`sql_parameter` (`$?`, `$N`), `sql_ellipsis` (`...`), `sql_number`, `sql_word`,
`sql_operator` and the one-character `sql_other` fallback. Never stricter than `raw_string`:
the interior's rules are subsets of `[^']` whose union is `[^']`. Two mechanisms are
decided here because the alternatives break silently (both measured with the pinned
`tree-sitter 0.25.10` binary, see the probe below): **`query` leaves the shared `keyword`
rule** (with it in, the longest-match lexer always prefers the one-token `raw_string` and
`sql_string` is unreachable) and **every interior token's lexical precedence is above the
extras'** (at precedence 0 a `#` inside the literal is lexed as a `#…` comment and eats the
closing quote: `MISSING "'"`). One consumer: `editors/zed/languages/blastfile/highlights.scm`,
four new rules, **no new capture name** (the guard's fifteen-name allowlist is unchanged);
`sql_number`/`sql_word`/`sql_operator`/`sql_other` are deliberately uncaptured and inherit
`@string.special`. `query "…"` keeps `$.string` (no `sql_string`), and `regex '…'` keeps
`raw_string`.

## The grammar probe (evidence behind §9.2a's two mechanisms)

Scratch grammar in `/tmp/opencode` (a copy of `editors/tree-sitter-blastfile/grammar.js`
with the delta applied), not in the repo:

```
$ cd /tmp/opencode/ts-min && tree-sitter generate && tree-sitter parse h.sql   # 'a#b'
  # sql_operator at prec(0):
  (sql_string (sql_word) (comment) (sql_other))            (MISSING "'" [1,0])
  # sql_operator at prec(9) / prec(2) (the design's mechanism):
  (sql_string (sql_word) (sql_operator) (sql_word))        no ERROR, no MISSING
```

```
$ cd /tmp/opencode/ts-probe && tree-sitter parse probe.Blastfile   # 16 query shapes
  (query_decl (sql_string (sql_keyword) (sql_other) (sql_number)))   query 'SELECT 1'
  (query_decl (string))                                              query "…"
  (query_decl (sql_string (sql_keyword) (sql_other) (sql_parameter) …))  query '… IN ($?, ...)'
  grep -cE 'ERROR|MISSING' → 0
```

The "no false red" loop over the repository's own files (the §9.5 R47 command, run before
the leaf exists):

```
$ for f in $(fixtures/*.Blastfile e2e/*/Blastfile e2e/*/*/Blastfile python/tests/*/Blastfile \
            python/tests/runnable_docs/fixtures/*/Blastfile); do
    test "$(tree-sitter parse "$f" 2>&1 | grep -cE 'ERROR|MISSING' || true)" -eq 0 || echo "$f"
  done
(no output)          # 23 files: 8 fixtures + 6 e2e + 9 python/tests
```

With `'query'` still in `keyword`, the same 16-shape file parsed every `query '…'` as
`(directive (keyword) (raw_string))` and produced **no** `sql_string` node — which is the
measurement behind decision 1 of §9.2a.

## The worked `line:col` example MB.34.4 can turn into a test

Source (a `"…"` string, eight spaces of indent, line 3 — the escape map is what is under
test):

```text
3        query "SELECT \"id\" FROM ..."
```

- decoded: `SELECT "id" FROM ...` — the `...` starts at decoded char index **17**
  (`SELECT ` 7 + `"id"` 4 + ` ` 1 + `FROM` 4 + ` ` 1).
- `QuerySite.at` = the first char inside the quote = line 3, col **16**
  (`query` 9–13, space 14, `"` 15, interior 16).
- the map: decoded indices 7 and 10 came from `\"` (two source columns each), so the walk
  advances `17 + 2 = 19` source columns from col 16.
- **expected: `Position::at(3, 35)`** — `StrayEllipsis` at the first `.`; the squiggle is one
  unit (§4.3 rule 2: `.` is not a word character).
- the two wrong answers a test separates: identity map → `3:33`; byte count (with a
  non-ASCII identifier) → something else again. A second case, multi-line + non-ASCII:
  `WHERE prïx > $? AND ...` on line 7 with its origin at `5:16` → **`7:21`**, where a byte
  count gives `7:22`.

## Critic reports

| Lens | Verdict | Findings | Report |
|---|---|---|---|
| security | CHANGES | 2 BLOCKER, 8 MAJOR, 3 MINOR (two passes, merged) | `docs/design/query-template.review-security.md` |
| implementability | CHANGES | 4 BLOCKER, 6 MAJOR, 7 MINOR (two passes, merged) | `docs/design/query-template.review-implementability.md` |
| testability | CHANGES | 2 BLOCKER, 8 MAJOR, 5 MINOR | `docs/design/query-template.review-testability.md` |
| consistency | CHANGES | 3 BLOCKER, 7 MAJOR, 8 MINOR | `docs/design/query-template.review-consistency.md` |

Every BLOCKER and MAJOR is resolved in the two design documents' dated
`## Review Resolution — the MB.34.2 amendment` sections (one row per finding, with the
section that changed or the leaf that carries a deferred text edit). The resolutions that
changed the contract rather than the wording:

- **The grammar is PostgreSQL 17.7, not 18** (security S7): `pg_query 6.2.1` vendors `PG_VERSION_STR "PostgreSQL 17.7 (libpg_query)"` (`libpg_query/pg_query.h:166`), so §13.4.1/R31 say 17.7 and the epic prose is a follow-up.
- **`Syntax` carries no message** (security S2): pg_query's message quotes the offending
  lexeme, which can be a value, so it is classification only; the sentence is fixed and the
  squiggle is the statement's first token.
- **`parameterize` refuses what `check_learnable` refuses** (security S2), so the Quick Fix
  cannot produce a grant `--learn` would reject.
- **One `QuerySite` per `queries()` entry**, with `at: NONE` for a programmatic grant
  (implementability B1), and `PgGrant`'s `PartialEq` excludes the sites (first-pass B1) so
  `critical_blastfile_parity.rs` stays green.
- **`MAX_TEMPLATE_PARSES = 4 096` / `TooManyQueries`** (security S4), because the per
  statement caps did not bound the number of parses per publish.
- **The parse form vs the wire form of `IN ($?, ...)`** (consistency C2), and **`Position::NONE`
  means "no site", not "no document"** (consistency C1).
- **§1's variant list corrected** to fifteen `Location` variants with the two `query`
  refusals in the positioned arm (consistency C3), and the `_statement` splice for
  `query_decl` written out (implementability B4).

Two findings were **rejected** with reasons: implementability's "`base` shifts the site"
(the `base` is the `UidLayout` uid base, not a line offset — §13.4.5 now says so), and
consistency's "`strictly looser`" (the design says "never stricter, exactly `'[^']*'` here";
the ADR wording now matches). The second-pass implementability report's `regex` corpus case
and `escapes` producer are resolved in the dump above (`§9.5`, `§13.4.5`).

## Acceptance commands

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
```

```console
$ make docs-check
Build started
No issues found
Build finished in 0.45s
```

```console
$ git diff --stat -- docs/design docs/decisions
 docs/decisions/ADR-0013-blastfile-lsp-and-zed.md |  78 ++-
 docs/design/blastfile-dsl.md                     | 522 ++++++++++++++++++++
 docs/design/blastfile-lsp-zed.md                 | 586 +++++++++++++++++++++--
 3 files changed, 1136 insertions(+), 50 deletions(-)
```

`ADR-0003` is **unchanged** from its accepted text (its noted message does not change, so
the node's "only if the message changes" rule forbids the note; the earlier one was
dropped). The `python/tests/test_docs_assets.py` fence-table failure belongs to MB.32.1's
in-flight edit of that file and is not counted here; this node's three commands above are
green.

## Follow-ups (out of scope here; the epic owner applies them)

0. **`MB.34.3`'s brief is stale in two lines** (the lead sent the implementer the correction
   directly on 2026-11-18; this records it for a future reader).
   `plan/nodes/MB/MB.34/MB.34.3/NODE.md:39-40` and `:108-110` say `TemplateError` carries
   pg_query's message ("`Syntax` (pg_query's own message, clipped and escaped to one line)").
   The contract is: **`TemplateError` carries `kind` only** (plus the char offset);
   `Syntax`'s sentence is fixed (`blastfile-dsl.md` §13.4.3) and **pg_query's message is
   never stored, escaped or rendered** (it quotes the offending lexeme, which can be a
   value). For the offset rule, "trivia" is `is_space(c)` **or** a character inside a
   `RegionKind::Comment`; a text that is all trivia is `Empty` at offset `0`.

1. **`MB.34.4`'s brief, two corrections** (its body cannot be edited with `plan.py`; these are
   the exact replacement wordings):
   - line 39, replace “Expose it on `PgGrant` as a read-only accessor (e.g. `query_sites()`),
     defaulting to empty for a grant built by `PgGrant::new` — the model stays constructible
     in tests.” with **“Expose it on `PgGrant` as `query_sites(): &[QuerySite]` with exactly
     one entry per `queries()` entry; a grant built by `PgGrant::new` gets one entry with
     `at: Position::NONE`, `interior: 0..0`, `escapes: []` and `raw` = the stored text — the
     vector is never empty, so `check_queries` can zip.”**
   - line 54, delete “plus pg_query's clipped message for `Syntax`” and write **“and the
     `kind`'s one fixed sentence; `Syntax` carries no message (pg_query's message quotes the
     offending lexeme, which can be a value).”**
2. **`MB.34.5`'s brief, bullet 1 re-word** (M3): replace “offered when the request's `range`
   intersects a `query` string of the tracked document whose current diagnostics include
   `LiteralQuery`, **or** whose text contains a numbered placeholder `$1…$n` … implemented on
   the **manifest side**” with **“offered iff `blastit_sqlpolicy::parameterize(&site.raw)`
   returns `Some`; the server re-derives the site from its own text and never reads
   `context.diagnostics`.”**
3. **`MB.34.6`'s acceptance, the 23-file loop** (T3): add
   `for f in editors/tree-sitter-blastfile/test/fixtures/*.Blastfile e2e/*/Blastfile e2e/*/*/Blastfile python/tests/*/Blastfile python/tests/runnable_docs/fixtures/*/Blastfile; do test "$(tree-sitter parse "$f" 2>&1 | grep -cE 'ERROR|MISSING' || true)" -eq 0 || exit 1; done`
   to its acceptance block, so the "no false red" claim is a command and not a design-time
   probe.
4. **The epic prose says "PostgreSQL 18"** (`plan/nodes/MB/MB.34/NODE.md` and the epic's
   goal); `pg_query 6.2.1` vendors **17.7** (`libpg_query/pg_query.h:166`,
   `pg_config.h:639`). The design says 17.7; the epic owner should re-word the goal.

No path follow-up is needed: `MB.34.3` already lists `literals.rs`, `MB.34.4` `lib.rs`, and
`MB.34.6` `test/corpus/**`. The pages need no follow-up either: `docs/reference/blastfile.md`
now carries all seven refusal rows and "the statement's first token"
(`docs/reference/blastfile.md:277,298-304`). `docs/design/blastfile-lsp-zed.md` §5.4/§5.5
were re-read and need no change.

## Notes

- One writer per document; each edit was section-sized (§13.4 ≈ 300 lines was written in
  three edits), no `*.part` file was left behind.
- Everything in §9.2a was generated and parsed with the pinned `0.25.10` binary on this
  host; the probe lives in `/tmp/opencode/ts-probe` and `/tmp/opencode/ts-min` and is not
  committed.
- `pg_query` was read from the cargo registry cache, never from a doc page or a guess; the
  file paths and line numbers are in the finding above.

## Final verdict

**PASS.** Every BLOCKER and MAJOR from the four lenses is resolved in the design text (or
carried by a named leaf with the reason, and listed in Follow-ups); the four reports are on
disk with their verdicts; the three acceptance commands are green at the stated output. The
expansion table, the refusal set, the `pg_query` file:line finding and the worked `line:col`
examples above are unchanged by the resolutions except where a resolution says so (the
`Syntax` sentence, `TooManyQueries`, P7's measured 20/21/24, and the five protected sites
`(5,34) (5,72) (5,84) (4,23) (5,23)`). The blocker in `python/tests/test_docs_assets.py` is
MB.32.1's in-flight edit of that file, not this node's: the node's three acceptance commands
are green.
