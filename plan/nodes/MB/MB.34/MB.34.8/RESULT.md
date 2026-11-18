# Result MB.34.8

`MB.34.2` pinned seven `query` refusals and two hover paragraphs; the pages were silent
about four of them. This node adds them, quoted from the design and nothing else, on three
pages, and changes no fence.

Scope touched: `docs/reference/blastfile.md`, `docs/local.md`, `docs/troubleshooting.md`
only. No `docs/design/**`, no `crates/**`, no `zensical.toml`, no fence.

## What changed

1. `docs/reference/blastfile.md` — the `query` refusal table now carries the design's whole
   set: the three rows it had, plus `Syntax`, `TooLong`, `TooManyParameters` and
   `TooManyQueries` (§13.4.3). `TooManyParameters` was not in the brief's three rows, but
   the node's goal is "state every refusal `blastit check` can print" and §13.4.3's sentence
   for it existed with no page to print it on; the page now carries all seven kinds.
2. `docs/reference/blastfile.md` — the two hover paragraphs, verbatim from
   `blastfile-lsp-zed.md` §7.6 (blockquotes, so no `text` fence is added; the constants are
   one paragraph each and the line wrapping here is the page's, not the string's).
3. `docs/reference/blastfile.md` — "at that token" narrowed to "at the statement's first
   token" (design C4 / Follow-up 5: §13.4.4 pins `Syntax` to offset `0`), and one bullet
   saying why the refused word is not named (S1: it can be a value).
4. `docs/reference/blastfile.md` — one clause in `## Limits` pointing at the three `query`
   caps, so the budget table does not read as exhaustive.
5. `docs/local.md` `## Editors` step 2 — the colour caveat (`"…"` spelling is one string),
   the Quick Fix on a pasted `$1…$n`, the `IN ($1, $2)` shape kept (§13.4.6 cites this
   sentence as the promised shape), 132 → 91 words.
6. `docs/troubleshooting.md` — the `query` row carries the `Syntax` sentence; its `psql`
   recipe moved to `## Database errors` (it answered a symptom that row does not have).

## The rows, clauses and paragraphs (final text)

`docs/reference/blastfile.md`, grant row (was: "outside an `IN`/`NOT IN` list, `$?` stands for
one value…", which implied `$?` changes meaning inside a list — §13.4.2 row 1 says one value
per `$?` everywhere):

```text
| `query 'SELECT id FROM products WHERE price > $?'` | one exact PostgreSQL statement, matched as a template (three shapes below); repeat the directive for more |
```

`docs/reference/blastfile.md`, the two hover paragraphs (§7.6, verbatim):

```text
> `$?` is one bind placeholder: the app sends one value for it, and the wire sees
> `$1, $2, …` in the order the placeholders appear. `IN ($?, ...)` binds any number
> of values with one grant — the list length is not part of the grant.

> `...` is Blastit's variadic marker, not SQL: it is legal only as `IN ($?, ...)`
> (any number of values) or after a `VALUES` row, `($?, $?), ...` (any number of
> rows). `blastit check` refuses it anywhere else.
```

`docs/reference/blastfile.md`, the refusal table (intro was "Each of these is a separate
`query` at this indent"; now true of all seven rows):

```text
Each refusal prints its site in front of the sentence:

| Written | Site | The CLI prints |
|---|---|---|
| `query ''` | `9:16` | ``this `query` is empty; write the one statement it grants`` |
| `query 'SELECT 1; SELECT 2'` | `11:26` | ``a `query` is one statement; this one starts a second`` |
| `query 'SELECT id FROM ...'` | `13:31` | ``...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row`` |
| `query 'SELCT id FROM t'` | `9:16` | ``the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` `` |
| a `query` of 65 537 characters | its first character | ``a `query` is 65536 characters at most; split it, or grant the tables and columns instead`` |
| a 1 025th `$?` | that `$?` | ``a `query` holds 1024 bind placeholders at most; grant the tables and columns instead`` |
| the 4 097th distinct `query` in one file | its own `line:col` | ``a `Blastfile` validates 4096 `query` entries at most; fold the shared statements into a `trait` that many views `use` `` |

* The parse's own message is not printed: it quotes the word it stopped at,
  which can be a value.

* One `query` is one PostgreSQL statement, parsed after those expansions: a single
  trailing `;` is allowed; a typo, a missing `FROM` or a stray `)` is a
  `Blastfile:line:col` diagnostic at the statement's first token, from `blastit
  check`, the editor and the production boot — never a runtime `sql.denied`.
```

`docs/reference/blastfile.md` `## Limits` (one clause added):

```text
file, never to refuse a real one. Each refusal is one line and names the budget; a
`query`'s three caps are under [Grants](#grants).
```

`docs/local.md` `## Editors` step 2:

```text
2. Open `./Blastfile`: colours (SQL inside a `query '…'` only: a `query "…"` is
   one string), bracket matching, an outline, and a squiggle, inside the string,
   at the `line:col` `blastit check` prints. Quick Fix **Parameterize SQL for
   Blastfile ($?)** turns a pasted literal or `$1…$n` into `$?`, and an
   `IN ($1, $2)` list into `IN ($?, ...)` ([grants](reference/blastfile.md#grants)
   has the shapes). Hovering `$?` or `...` explains each. The extension starts
   `blastit lsp` itself, from `PATH` or through `uv run`.
```

`docs/troubleshooting.md` `## blastit check fails`, the `query` row:

```text
| a `query` squiggle I cannot place | a statement that does not parse is marked at its first token, with ``the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` ``; otherwise the column counts from the line's first character (a `query` at 8 spaces puts its SQL at 16) and sits on the token you wrote, not the expanded text |
```

`docs/troubleshooting.md` `## Database errors`, the recipe moved out of that row:

```text
| PostgreSQL rejects a statement the grant allows | run it as written, each `$?` as a value of that column's type and the trailing `, ...` dropped: `psql -c '<statement>'` |
```

## Fence counts, before and after (unchanged)

Measured with `python/tests/runnable_docs/fences.py` (`scan_page`), in the brief's order
(reference fence lines / reference `text` blocks / local fence lines / troubleshooting
fence lines):

| Measure | Before | After |
|---|---|---|
| `docs/reference/blastfile.md` fence-marker lines | 34 | 34 |
| `docs/reference/blastfile.md` `text` blocks (of 17 blocks: 16 `text`, 1 `json`) | 16 | 16 |
| `docs/local.md` fence-marker lines (9 blocks: 6 `console`, 2 `text`, 1 `json`) | 18 | 18 |
| `docs/troubleshooting.md` fence-marker lines | 0 | 0 |
| **public pages, `text` blocks** (the `HIGHLIGHTED_FENCES` + `PLAIN_FENCES` 33) | 33 | 33 |

The hover paragraphs are blockquotes and the refusal additions are table rows, so no
fence was added, moved or reclassified.

## Acceptance (full tails)

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ make doc-tone docs-check
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.38s
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 3.56s
```

```console
$ git diff --stat -- docs
 docs/ci.md                                       |   6 +
 docs/decisions/ADR-0013-blastfile-lsp-and-zed.md |  78 ++-
 docs/design/blastfile-dsl.md                     | 512 ++++++++++++++++++++
 docs/design/blastfile-lsp-zed.md                 | 587 +++++++++++++++++++++--
 docs/local.md                                    |  26 +-
 docs/reference/blastfile.md                      |  65 ++-
 docs/stylesheets/extra.css                       |  53 +-
 docs/troubleshooting.md                          |   4 +-
 8 files changed, 1254 insertions(+), 77 deletions(-)
```

The other `docs/**` rows in that stat are not this node's: they are MB.34.2's design
amendment and the concurrent `docs/javascripts` node's work in the same tree. This node's
own hunks are `docs/local.md`, `docs/reference/blastfile.md`, `docs/troubleshooting.md`;
`docs/local.md`'s diff also carries MB.34.1's uncommitted page work, which was already in
the tree when this node started.

**One transient failure, reported as required.** The first two pytest runs (before and just
after the page edits) failed `test_every_text_block_classifies_as_measured` and
`test_the_plain_fences_stay_plain_with_a_query_line_appended`, because the other node's
in-flight `python/tests/test_docs_assets.py` pinned `text` fences by line number and this
node's 15 inserted lines moved seven of them on `docs/reference/blastfile.md`
(`323,359,373,403,549,610,663` → `338,374,388,418,564,625,679`). That file was never
edited here; the owning node replaced the line-number sets with a page-keyed table, and
the re-run is green at 52 passed. No host flake was claimed.

## Critic report

`docs/design/query-template-pages.review-dx.md`, one fresh-context `critic`, lens `dx`,
three page paths only.

* **First verdict: CHANGES REQUIRED** — 1 BLOCKER, 3 MAJOR, 5 MINOR. Quote-check passed:
  7/7 refusal sentences byte-identical to §13.4.3, both hover paragraphs byte-identical to
  §7.6, the two literal exemptions equal `KNOWN_STATIC_SETUP_SQL`.
* **Second verdict (resolution check): PASS.**

| Finding | Resolution |
|---|---|
| B1 · BLOCKER: the new bullet claimed a parse error "reports no position", contradicting §13.4.4's pinned offset `0`, the bullet above and the table's `9:16` | **accepted.** The bullet now states only what is true of the message: "The parse's own message is not printed: it quotes the word it stopped at, which can be a value." The site stays stated once, at the bullet above and in the table. |
| M1 · MAJOR: the grant-table cell (41 words) implied `$?` changes meaning inside an `IN` list and paraphrased the table below | **accepted,** although the cell is `MB.34.1`'s prose: §13.4.2 row 1 makes it false, so it is now "one exact PostgreSQL statement, matched as a template (three shapes below); repeat the directive for more". |
| M2 · MAJOR: `docs/local.md` step 2 was a 132-word tour whose "wrong SQL token" binary is false for `Empty`/`TooLong` | **accepted.** 91 words; the arithmetic and the false binary are gone (the reference owns both), the actionable parts stay, `IN ($1, $2)` kept because §13.4.6 cites it. |
| M3 · MAJOR: the `query` row was 95 words, re-quoted the reference sentence and ended in an off-symptom `psql` recipe | **accepted in part.** 70 words and the recipe moved to `## Database errors`; the `Syntax` sentence still sits in the row because the node's brief requires it there. |
| m1 · MINOR: "adds its site to the sentence" | **accepted** → "prints its site in front of the sentence". |
| m2 · MINOR: `IN ($1, $2, $n)` vs the wire form `IN ($1, …, $n)` | **deferred -> MB.34.1's owner.** `MB.34.1`'s accepted cell, and no falsehood: it reads "first, second, nth". |
| m3 · MINOR: "one row needs its own `query`" sends the reader hunting | **deferred -> MB.34.1's owner** (same reason as m2). |
| m4 · MINOR: "a decoded statement of" and `1 025th` vs `65536` | **accepted in part.** "a `query` of 65 537 characters"; `1 025th` kept, which is this page's thousands typography ("4 096" in `## Limits`) while the message is design-verbatim and cannot move. |
| m5 · MINOR: exit `2` narrowed to "did not load" | **deferred -> MB.34.1's owner.** The row is `MB.34.1`'s and `docs/reference/cli.md` says "could not start". |
| follow-up MINOR: the "— the grants shapes" fragment after the first fix | **accepted** → "(grants has the shapes)". |

The critic's own check line: "`docs/reference/blastfile.md:641-642` — exactly three `query`
caps (`MAX_TEMPLATE_CHARS`, `MAX_TEMPLATE_PARAMS`, `MAX_TEMPLATE_PARSES`), and all three are
present under Grants"; "`docs/local.md:151-158` 91 words (was 132), no false token claim";
"`docs/troubleshooting.md:40` 70 words (was 95), `psql` recipe now `:79` under `## Database
errors`".

## Follow-ups (out of scope here)

* `docs/reference/blastfile.md`'s template cell `IN ($1, $2, $n)` and the `VALUES` cell's
  "one row needs its own `query`" are `MB.34.1`'s accepted sentences with two MINOR
  readability findings (critic m2, m3). Whoever next owns that section can apply them.
* `docs/troubleshooting.md`'s exit `2` row is `MB.34.1`'s and narrows "could not start" to
  "did not load" (critic m5); `docs/reference/cli.md` is the authority.
* `docs/reference/blastfile.md`'s `## Errors` intro still says "A parse error names the
  token", which is true of the file's own parser and not of a `query`'s (§13.4.4 makes the
  statement's first token the site and forbids printing the message). Left as written
  because the critic passed it and the section's rows are all file-parse rows.
* The hover strings are still prose on the page and constants in `hover.rs`: `MB.34.5`
  implements the hovers and must copy these two paragraphs byte for byte (§7.6 R61).

## Post-review MINORs (reviewer-specified, applied verbatim)

The reviewer passed the node and named three corrections on
`docs/reference/blastfile.md`. All three are applied; nothing else changed, and the last
Follow-up above (`## Errors` still claiming a `query` refusal names view and directive) is
closed by item 2 here.

1. **`:300`, the `StrayEllipsis` cell rendered a misplaced backtick.** The span opened with
   two backticks immediately followed by the sentence's own backtick, so the parser ate the
   sentence's first backtick and the page showed the sentence with one backtick missing.
   The span is now padded with one space at each end (which the renderer strips), and the
   built HTML carries the sentence whole:
   `<td><code>`...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row</code>`
   (read back from `site/reference/blastfile/index.html`). `docs/design/blastfile-dsl.md`
   §13.4.3 carries the same source defect and was **not** edited: the page is this node's.
2. **`:662`, "a whole-file check names the view and the directive" was false for the seven
   `query` refusals.** The sentence now reads:

   ```text
   A parse error names the token; a whole-file check names the view and the directive —
   except a `query` refusal, which prints its site and its sentence ([grants](#grants)).
   ```

3. **`:306`, "it quotes the word it stopped at" was unconditional** while
   `syntax error at end of input` quotes no word. The bullet now reads:

   ```text
   * The parse's own message is not printed: it can quote the word it stopped at,
     which can be a value.
   ```

   This supersedes the wording quoted in the critic table above for B1; the finding's
   resolution (no claim about a missing position) is unchanged.

### Acceptance after the MINORs

```console
$ make doc-tone docs-check
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.78s
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 2.55s
```

Fence counts re-measured and unchanged — 34 (reference fence lines) / 16 (reference `text`
blocks) / 18 (local fence lines) / 0 (troubleshooting fence lines); public pages still 27,
`text` blocks still 33 in total, reference still 17 blocks (16 `text`, 1 `json`), local still
9 (6 `console`, 2 `text`, 1 `json`). No fenced block was added, moved or reclassified.
