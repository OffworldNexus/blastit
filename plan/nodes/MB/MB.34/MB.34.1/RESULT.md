# Result MB.34.1

Status: done, attempt 2/3. Doc-writer wrote the DX-first sentences for `query`
template validation before any code. Two fresh-context `critic` lenses, two rounds;
a fresh-context reviewer failed attempt 1 on five findings, fixed in attempt 2.

Base: `5f29398` (branch `release/0.2.0`).

## Pages touched (the four in `paths`)

| Page | Change |
|---|---|
| `docs/reference/blastfile.md` | `query` row of the Grants table, two new tables (template expansions, the three refusals) and the query bullets |
| `docs/local.md` | `## Editors` step 2; one clause in `## A denial` that still called a `query` "trusted as written" |
| `docs/ci.md` | first paragraph under `## What it fails on`: an unparseable `query` is exit `2`, not a finding |
| `docs/troubleshooting.md` | the `query` squiggle row; the `exit 2` row no longer sends the reader to `blastit dev` |

**Zero new fenced blocks.** Delimiter counts are unchanged from `HEAD`:
`34 / 16 / 18 / 0` for the four pages. Everything added is a table row, a bullet
or inline code.

## The four contract sentences (`git diff`)

```diff
-| `query 'SELECT …'` | one exact statement; repeat the directive for more |
+| `query 'SELECT id FROM products WHERE price > $?'` | one exact PostgreSQL statement; `$?` stands for one value, and `...` appears only in the two list forms below; repeat the directive for more |
+
+Matching reads the string as a template. Three shapes, and the statement
+PostgreSQL receives:
+
+| Template | PostgreSQL receives |
+|---|---|
+| `$?` | the next `$n`: `$1`, `$2`, … in statement order |
+| `IN ($?)`, `IN ($?, ...)` | `IN ($1, $2, …, $n)` at any length — `NOT IN` too: one grant covers every list |
+| `($?, $?), ...` | two or more identical `VALUES` rows, any count — one row needs its own `query`; every copy still reaches PostgreSQL |
+
+* `$1` to `$n` written by hand is accepted as it stands: PostgreSQL parses it, and
+  matching drops the numbering, so `$1` and `$?` are one grant.
+* One `query` is one PostgreSQL statement, parsed after those expansions: a typo, a
+  missing `FROM` or a stray `)` is a `Blastfile:line:col` diagnostic at that token,
+  from `blastit check`, the editor and the production boot — never a runtime
+  `sql.denied`.
...
+Each of these is a separate `query` at this indent; the refusal adds its site to
+the sentence:
+
+| Written | Site | The CLI prints |
+|---|---|---|
+| `query ''` | `9:16` | ``this `query` is empty; write the one statement it grants`` |
+| `query 'SELECT 1; SELECT 2'` | `11:26` | ``a `query` is one statement; this one starts a second`` |
+| `query 'SELECT id FROM ...'` | `13:31` | ``...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row`` |
+
+* Only two statements may keep a quoted literal: `SET TIME ZONE 'UTC'` and
+  `SELECT set_config('TimeZone', $?, false)` ([traits](#traits), [connection
+  setup](#connection-setup)). Any other quoted string — `'x'`, `E'x'`, a
+  dollar-quoted block — is refused at its own `line:col`, because a value cannot be
+  learned: bind it (`$?`) in the query and in the code that sends it. A number,
+  `true`, `NULL` and a quoted identifier are ordinary text.
+* A `…` in a `query` is refused: it is what redaction writes, and the value it
+  replaced is gone. Grant the exact statement, with `$?` where the values bind.
```

Answerable from the pages alone: which part is validated (the statement, parsed
after the three expansions, one statement only); where the squiggle lands (inside
the string, on the SQL token, same `line:col` counted from the line start, read by
`blastit check` and the editor); what `$?` / `IN ($?, ...)` / `($?, $?), ...`
expand to (the table); what the Quick Fix rewrites (`local.md` step 2).

## Critic reports

| Round | Lens | Verdict | Findings | Report |
|---|---|---|---|---|
| 1 | `dx` | CHANGES | 1 BLOCKER, 10 MAJOR, 9 MINOR | `docs/design/query-template-dx.review-dx.md` |
| 1 | `ai-ergonomics` | CHANGES | 1 BLOCKER, 8 MAJOR, 5 MINOR | `docs/design/query-template-ai-ergonomics.review-ai-ergonomics.md` |
| 2 | `dx` | CHANGES | 0 BLOCKER, 2 MAJOR, 8 MINOR | `docs/design/query-template-dx.review-dx.round-2.md` |
| 2 | `ai-ergonomics` | CHANGES | 0 BLOCKER, 3 MAJOR, 6 MINOR | `docs/design/query-template-ai-ergonomics.review-ai-ergonomics.round-2.md` |

Every BLOCKER and MAJOR was resolved in the pages (no deferral); round 2 was run
because the sentences changed substantially.

Final verdict: PASS — the two round-2 lens reviews ended `VERDICT: CHANGES` and their
findings were resolved by the writer at blastfile.md:258-300 / local.md:135,149-156 /
troubleshooting.md:40; the reviewer then returned `VERDICT: PASS`.

### Round 1 — resolutions

| Finding | Resolution |
|---|---|
| dx B1 `(row), ...` implied a bulk insert collapses to one row | Template table; `($?, $?), ...` now says every copy still reaches PostgreSQL (and, per round 2, that two or more rows are needed) |
| dx M1 `query 'SELECT …'` row used the `…` glyph and a coined "template" term | Row shows a real statement; no `…` |
| dx M2 / ai MAJOR 2 which part is validated, parse order, deploy surface | "parsed after those expansions", surfaces are `blastit check`, the editor and the production boot |
| dx M3 / ai MAJOR 8 `blastit fmt --check` claimed as a SQL validator | Removed: `fmt` only parses and prints, so it must not be sold as the gate |
| dx M4 / ai MAJOR 3 "trusted as written and runs without further checking" | "matched on the text you wrote … gets no table or column analysis" (`local.md`'s twin clause fixed too) |
| dx M5 three prose bullets for three shapes | One three-row table |
| dx M6 / ai MAJOR 4+5 duplicate literal rule, `set_config(…)` too loose, "literal" vs "quoted literal" | One bullet with the exact two exempt statements; quoted strings only; a number/`true`/`NULL`/`"…"` named as ordinary text |
| dx M7 local step 2 was a 91-word comma splice, exempt statements unspecified | Rewritten as five short sentences; the two exempt statements named |
| dx M8 / ai BLOCKER troubleshooting row inverted, `psql` cannot take `$?`/`...` | Symptom "cannot place"; the copy is repaired (values in, `...` dropped) before `psql -c` |
| dx M9 / ai MINOR 4 exit-`2` row sent the reader to `blastit dev` | "the refusal is already on stderr, one line per problem — fix what it names" |
| dx M10 / ai MAJOR 7 CI paragraph after the table, `--json` register | Moved above the table; says stdout is empty, an empty stdout is a refusal, and no `kind`/`fix.action` is added |
| ai MAJOR 6 `$1…$n` accepted but unstated | Its own bullet: accepted as it stands, numbering dropped by matching |
| ai MAJOR 9 / MINOR 1+2+3 the redacted `…`, example columns, `(row)` metasyntax | `…` refusal documented; sites 9/11/13; `($?, $?), ...` |

### Round 2 — resolutions

| Finding | Resolution |
|---|---|
| dx MAJOR-1 "any number of times" includes one row; `normalize` collapses two or more | Cell: "two or more identical `VALUES` rows, any count — one row needs its own `query`" (verified in `crates/blastit-sqlpolicy/src/normalize.rs:61-71`) |
| dx MAJOR-2 / ai MAJOR 1 "`IN ($?, ...)` is one value" and the caret's origin | Row now says the column counts from the line start (a `query` at 8 spaces puts its SQL at 16) and the caret sits on the token you wrote |
| ai MAJOR 2 `;` refusal pointed at the `;` (24) while `MB.34.3` pins the second statement's first char; empty row used the quote (15) | Sites are `11:26` and `9:16` — both on the interior origin |
| ai MAJOR 3 `$1…$n` described as renumbered | "accepted as it stands: PostgreSQL parses it, and matching drops the numbering" (`normalize.rs:56-60`) |
| dx MINOR-1 a hand-written `IN ($?)` already covers every list | Template cell is `IN ($?)`, `IN ($?, ...)` |
| dx MINOR-2 / ai MINOR 5 "matched as written" beside "whitespace ignored" | "matched on the text you wrote, with comments, whitespace and `$n` folded" |
| dx MINOR-4 `NOT IN` missing from the refusal | Refusal sentence names `IN`/`NOT IN`; the table says `NOT IN` too |
| dx MINOR-6+7 local step 2 length, SQL colouring | Five sentences, and "colours (the SQL inside a `query` too)" |
| dx MINOR-8 CI "one failure has no row below" | "A `query` that does not parse is not a finding and has no row below" |
| ai MINOR 1 `Blastfile:line:col` is not what `check` prints | The table quotes the sentence and carries the site in its own column; no prefix claim |
| ai MINOR 2 `…` as a template spelling, `…` refusal undocumented | Copyable spellings use `$1` to `$n`; the `…` refusal is now a bullet |
| ai MINOR 3 exit-`2` tail did not fit a configuration refusal | "fix what it names" |
| ai MINOR 4 substituting a value needs its type | "a value of that column's type" |

## Deviations from the brief (recorded, not silent)

1. **`local.md` step 2 is five short sentences, not "one or two".** Both `dx`
   rounds flagged the punctuation-only split (two 36–39-word sentences), and the
   non-negotiable rule is short sentences. Every sentence is now ≤ 30 words.
2. **The brief's "count `IN ($?, ...)` as one value" is not in the troubleshooting
   row.** It is false: `normalize` expands `IN ($?, ...)` to `IN ($1, $2, …, $n)`
   "whatever its length". Both lenses raised it independently; the row keeps the
   fact that matters (columns count from the line start; the caret is on the
   written token) and the working `psql -c` recipe.
3. **The exemption links `#traits` and `#connection-setup`.** The brief named
   `#traits` for `set_config`; only `### Connection setup` shows that spelling, so
   both anchors are given (`docs-check` proves both resolve).
4. **One extra bullet: a `…` in a `query` is refused.** It is the shipped
   `RedactedQueryPlaceholder` behaviour, the reference did not say so, and a
   copied `sql.denied` line is redacted by construction.

## Attempt 2 — reviewer findings (five, all resolved)

The attempt-1 reader failed on five findings (one MAJOR, four MINOR). All five are
fixed in the pages; nothing else changed.

| # | Finding | Where it now reads | Resolution |
|---|---|---|---|
| 1 | MAJOR `local.md` told the reader to paste a redacted `sql` full of `…`, which the reference refuses (`RedactedQueryPlaceholder`); the "normalised statement" bullet repeated it | `local.md:120-123`, `local.md:126` | The sentence says to replace every `…` with `$?` (the value is gone) and that the result is what a grant has to contain, safe to paste into a ticket; the bullet says "the normalised statement, each `…` bound as `$?`" |
| 2 | MINOR `blastfile.md:258` "`$?` stands for one value" understated `IN ($?)`, which `normalize` collapses to `IN ($?, ...)` at any length | `blastfile.md:258` | "outside an `IN`/`NOT IN` list, `$?` stands for one value, and `...` appears only in the two list forms below" |
| 3 | MINOR `local.md:155` "Hover over `$?` or `...` says …" was ungrammatical | `local.md:156` | "Hovering over `$?` or `...` says what each stands for." |
| 4 | MINOR `troubleshooting.md:40` "drop the `...`" left a dangling comma (`IN ($1, )`) in the `psql -c` copy | `troubleshooting.md:40` | "drop the trailing `, ...`" |
| 5 | MINOR `RESULT.md` recorded both round-2 lenses as `CHANGES` and never the `VERDICT: PASS` the Goal requires | `RESULT.md:81-83` | The round-2 lens reviews ended `CHANGES`, the writer resolved their findings, and the reviewer then returned `VERDICT: PASS` (wording sharpened again by post-review MINOR 3) |

No promise was reopened: the four contract sentences (`$?`, `IN ($?, ...)`,
`($?, ...)`, the single-statement rule) are unchanged apart from finding 2's
qualifier at `blastfile.md:258`.

## Post-review MINORs (four, applied)

The reviewer returned `VERDICT: PASS` and specified four MINORs; applied as written,
nothing else touched.

| # | Finding | Where it now reads | Resolution |
|---|---|---|---|
| 1 | `blastfile.md:270` spelled the `IN` list with U+2026, the glyph `RedactedQueryPlaceholder` refuses | `blastfile.md:270` | `IN ($1, $2, $n)` — one `$n` per value, at any length; `NOT IN` too: one grant covers every list |
| 2 | `local.md:120-121` did not say to bind the value in the sending code | `local.md:120-121` | "… and it is exactly what a grant has to contain, safe to paste into a ticket; bind it in the code that sends it" |
| 3 | `RESULT.md:81` "Final verdict: PASS" sat beside two lens reports that end `VERDICT: CHANGES` | `RESULT.md:81-83` | The two round-2 lens reviews ended `CHANGES`; the writer resolved their findings; the reviewer then returned `VERDICT: PASS` |
| 4 | The single-statement rule never said a lone trailing `;` is legal | `blastfile.md:275-276` | "a single trailing `;` is allowed" |

Finding 1 named the `IN` cell only; the `$?` row's "`$1`, `$2`, … in statement order"
is prose for "and so on", not an SQL fragment a reader pastes, and is unchanged.

Post-review run, same worktree:

```console
$ uv run --no-project tools/plan.py validate
plan OK
exit=0
```

```console
$ make doc-tone docs-check
doc-tone: ok — 30 user-facing page(s), no banned phrase
Build started
No issues found
Build finished in 1.16s
exit=0
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
...................................                                      [100%]
35 passed in 2.75s
exit=0
```

Fence delimiters unchanged, `34 / 16 / 18 / 0`; `git diff --stat -- docs` is
`64 insertions(+), 19 deletions(-)` over the same four files.

## Acceptance (run in `.worktrees/release-0.2.0`, attempt 2)

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
exit=0
```

```console
$ make doc-tone docs-check
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.28s
exit=0
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
...................................                                      [100%]
35 passed in 0.87s
exit=0
```

```console
$ git diff --stat -- docs
 docs/ci.md                  |  6 ++++++
 docs/local.md               | 26 +++++++++++++++----------
 docs/reference/blastfile.md | 47 +++++++++++++++++++++++++++++++++++++--------
 docs/troubleshooting.md     |  3 ++-
 4 files changed, 63 insertions(+), 19 deletions(-)
```

Fence delimiters per page, current / `HEAD`: `34 / 34`, `16 / 16`, `18 / 18`,
`0 / 0` (`grep -c '^```'`), so no fenced block was added or removed. The trailing
`Exit code: 1` in that check run is `grep -c` reporting zero matches on
`troubleshooting.md`, not a failing command.

## Follow-ups (out of this node's scope)

1. `docs/reference/blastfile.md` §`blastit check` still calls
   `blastit fmt --check` "the gate" for a PHP app, but `run_fmt` only parses and
   prints (`crates/blastit/src/cli.rs:601-653`): the same false green `MB.34.1`
   removed from the `query` contract. It needs its own sentence (a different
   leaf; the `fmt` surface is not `query` validation).
2. ~~`docs/local.md` §`A denial` says the redacted `sql` is "exactly what a grant has
   to contain"~~ — closed in attempt 2 (reviewer finding 1): the clause now says to
   replace every `…` with `$?` before pasting (`docs/local.md:120-126`).
3. `check` prints a positioned refusal as `blastit check: <path>: <line>:<col>: …`
   (`crates/blastit/src/check.rs:104-141` joins the path and the errors with
   `": "`), while `fmt` prints `<path>:<line>:<col>: …`
   (`crates/blastit/src/cli.rs:655-671`). `MB.34.4` should make the two shapes one,
   and this page already documents the `<file>:<line>:<col>` form.
4. `docs/reference/blastfile.md` §Errors could name the exact `check` prefix once
   (3) is decided, so an agent can anchor on the line it will really see.
5. The trailing-`;` sentence says "a single trailing `;`". Code strips repeated
   trailing separators too (`crates/blastit-sqlpolicy/src/normalize.rs:53-55`:
   `SELECT 1;;` → `SELECT 1`), so the sentence is a lower bound; widen it only if
   `MB.34.3` pins the repeated form as documented behaviour.
