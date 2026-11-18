---
lens: dx
reviewer: critic (fresh context)
targets:
  - docs/reference/blastfile.md
  - docs/local.md
  - docs/troubleshooting.md
design:
  - docs/design/blastfile-dsl.md §13.4.3, §13.4.4, §13.4.6
  - docs/design/blastfile-lsp-zed.md §7.6, §7A
---

# DX review: `query` template pages (consistency fix)

**Verdict: PASS** — the 1 BLOCKER and 3 MAJOR are resolved; 5 MINOR items are open, deferred or partial, none blocking.

Quote-check performed first, against the authoritative sources: all seven refusal
sentences on `docs/reference/blastfile.md:298-304` match `blastfile-dsl.md` §13.4.3
byte for byte; both hover blockquotes (`:286-292`) are byte-identical to the constants in
`blastfile-lsp-zed.md` §7.6 (verified with a whitespace-normalised diff); the two
literal-keeping statements (`:308-309`) are exactly `KNOWN_STATIC_SETUP_SQL`
(`crates/blastit-sqlpolicy/src/literals.rs:41`); the Quick Fix description in
`docs/local.md:156-158` matches §7A.2 + §13.4.6 (a literal or a hand-written `$1…$n`,
never the exempt setup forms); the `Site` numbers `9:16`, `11:26`, `13:31` match the
design's own 8-space fixtures. Step count on `docs/local.md` is still 3, zero `sudo`, no
new env var, no new command. Those parts pass.

---

## Resolution check

- **B1 confirmed** — `blastfile.md:306-307` now states only that the message is not printed; the site lives at `:275-278` and in the `Site` column, and the contradictory "no position" claim is gone.
- **M1 confirmed** — `blastfile.md:258` is the accepted one-clause cell; the false `IN`-list implication and the duplicated shapes are gone.
- **M2 confirmed** — `local.md:151-158` is 91 words (was 132) with no false "wrong SQL token" binary; each clause now names something the reader acts on. Leftover MINOR: "— the [grants](reference/blastfile.md#grants) shapes" is elliptical; "matching the shapes under [grants]" would read cleanly.
- **M3 confirmed** — `troubleshooting.md:40` leads with the parse case and is 70 words (was 95); the `Syntax` sentence stays because the node brief requires it, so the duplicate is mandated rather than drift. The `psql` recipe is now `troubleshooting.md:79` under `## Database errors`, where its symptom is real.
- **m1 confirmed** — `blastfile.md:294` reads "Each refusal prints its site in front of the sentence:".
- **m4 confirmed in part** — `:302` is now "a `query` of 65 537 characters" with "its first character" as the site; the `1 025th` spelling at `:303` survives as MINOR, no veto.
- **m2 not applied — accepted** — `IN ($1, $2, $n)` is MB.34.1's accepted cell and §13.4.2's C2 records it; re-opening it would buy no DX.
- **m3 not applied — accepted** — same accepted cell ("one row needs its own `query`"); it still reads oddly, but it is a MINOR on an accepted row.
- **m5 not applied — accepted as deferred** — the exit `2` row (`troubleshooting.md:46`) still says "did not load"; it remains a MINOR owned by MB.34.1, not this change.

No BLOCKER or MAJOR remains; verdict is PASS.

---

## BLOCKER

### B1 — `docs/reference/blastfile.md:306-307` claims a parse error has no position (resolved)

> A parse error reports no position, and its message is not printed either: it
> quotes the word it stopped at, which can be a value.

**Why it is wrong.** `blastfile-dsl.md` §13.4.4 pins `Syntax`'s offset to `0`, the
statement's first character, and R33 forbids `Position::NONE` for a `query` parsed from a
document. It sits directly under a refusal table whose `Syntax` row gives the site `9:16`
(`:301`) and 28 lines below its own bullet at `:275-278` ("at the statement's first token"), and
`docs/troubleshooting.md:40` says it a third time ("marked at its first token"). The design
did not narrow this promise; the page invented a narrower one. A developer who reads
`:306` stops looking for the squiggle, or reports the editor as broken — the exact
audience this bullet exists for. The second half (the message is never printed) is correct
and is the security point; keep it.

**Replacement.**

> A parse error is marked at the statement's first token, not at the word that failed.
> Its message is not printed either: it quotes that word, which can be a value.

---

## MAJOR

### M1 — `docs/reference/blastfile.md:258` teaches a false implication and repeats the table below (resolved)

> one exact PostgreSQL statement; outside an `IN`/`NOT IN` list, `$?` stands for one
> value, and `...` appears only in the two list forms below; repeat the directive for more

**Why it is wrong.** "outside an `IN`/`NOT IN` list, `$?` stands for one value" tells the
reader that `$?` changes meaning inside a list. It does not: §13.4.2 row 1 assigns one
value per `$?` everywhere, including `IN ($?, ...)`. The cell is also 41 words in a table
whose other cells are single digits, and it paraphrases the three-row expansion table six lines
below — the only unique information is "repeat the directive for more".

**Replacement.**

> one exact PostgreSQL statement, matched as a template (three shapes below); repeat the
> directive for more

### M2 — `docs/local.md:151-160`: step 2 is a 132-word feature tour (resolved - now 91 words)

> 2. Open `./Blastfile`: colours (SQL inside a `query '…'` only: a `query "…"` is
>    one string), bracket matching, an outline, and a squiggle on each syntax error. …

**Why it is wrong.** 132 words, 5x step 1 and 3x step 3, in a numbered install step. It
carries six unrelated behaviours, the squiggle's column arithmetic (troubleshooting and the
reference now own that), and a claim the design does not support: "on the SQL token that is
wrong, or on the statement's first token when the statement does not parse" — `Empty` marks
the closing quote and `TooLong` marks the first token of a statement that parses fine
(§13.4.3). A developer installing the extension needs: what they get, how to trigger the
one action, and that the server starts itself.

**Replacement** (keeps the actionable parts, drops the arithmetic and the false binary):

> 2. Open `./Blastfile`: colours (SQL inside a `query '…'` only: a `query "…"` is one
>    string), bracket matching, an outline, and a squiggle at the `line:col` `blastit
>    check` prints, inside the `query` string. Quick Fix **Parameterize SQL for Blastfile
>    ($?)** turns a pasted literal or `$1…$n` into `$?` ([grants](reference/blastfile.md#grants));
>    hovering `$?` or `...` explains each. The extension starts `blastit lsp` itself, from
>    `PATH` or through `uv run`.

### M3 — `docs/troubleshooting.md:40`: 95-word cell duplicates the refusal table and answers a symptom it does not have (resolved - now 70 words, recipe moved)

> a `query` squiggle I cannot place | the column counts from the line's first character …
> with ``the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` ``.
> For PostgreSQL's own error, replace each `$?` with a value of that column's type, drop the
> trailing `, ...`, and run the copy with `psql -c`

**Why it is wrong.** Every neighbouring row in this table runs 12-25 words; this one is 95,
so the table stops scanning. It re-quotes the `Syntax` sentence that the reference refusal
table now owns (`:301`) — two copies, and only the hover constants are `include_str!`-pinned,
so this one will drift. It then ends with a `psql` recipe for a "PostgreSQL's own error"
that this symptom never mentions: the reader with an unplaceable squiggle has no such error
in front of them.

**Replacement** (move the `psql` recipe to `## Database errors`, or delete it):

> a `query` squiggle I cannot place | the column counts from the line's first character (a
> `query` at 8 spaces puts its SQL at 16) and sits on the token you wrote, not the expanded
> text; a statement that does not parse is marked at its first token — see the
> [refusal table](reference/blastfile.md#grants)

---

## MINOR

### m1 — `docs/reference/blastfile.md:294`: "adds its site to the sentence"

> Each refusal adds its site to the sentence:

The CLI *prefixes* `<file>:<line>:<col>:`; nothing is added to the sentence, and "site" is
design vocabulary a page reader has not met. The `9:16`/`11:26`/`13:31` values are the
design's line-9, 8-space fixtures, not anything in the reader's file, so they also need a
label.

**Replacement.** "Each refusal prints its site in front of the sentence; the example sites
are an 8-space-indented `query` on line 9:" — or rename the column to **Example site**.

### m2 — `docs/reference/blastfile.md:270`: `IN ($1, $2, $n)` reads as exactly three parameters

> | `IN ($?)`, `IN ($?, ...)` | `IN ($1, $2, $n)` — one `$n` per value, at any length; …

§13.4.2's wire form is `IN ($1, $2, …, $n)`; dropping the ellipsis while the cell says "at
any length" contradicts it on the same line.

**Replacement.**

```text
| `IN ($?)`, `IN ($?, ...)` | `IN ($1, …, $n)` — one `$n` per value, at any length; `NOT IN` too: one grant covers every list |
```

### m3 — `docs/reference/blastfile.md:271`: "one row needs its own `query`" sends the reader hunting

> two or more identical `VALUES` rows, any count — one row needs its own `query`; every
> copy still reaches PostgreSQL

Every statement is already its own `query`; the sentence means "write one row without
`...`". "identical" is also a matching requirement, not a spelling rule, and the cell does
not say so.

**Replacement.** "two or more identical `VALUES` rows, any count — for a single row write
`VALUES ($?, $?)` with no `...`; every copy still reaches PostgreSQL"

### m4 — `docs/reference/blastfile.md:302-303`: internal typography in the "Written" column

> a decoded statement of 65 537 characters … | … | a 1 025th `$?`

"decoded statement" is the LSP's internal text; the reader has a `query`. And `65 537` /
`1 025th` (design typography) sit next to `65536` in the message column, so the page spells
the cap two ways.

**Replacement.** "a `query` of 65 537 characters" and "a 1025th `$?`" (messages unchanged —
they are design-verbatim).

### m5 — `docs/troubleshooting.md:46`: exit `2` narrowed to a load failure

> exit `2` | the `Blastfile` or the configuration did not load: the refusal is already on
> stderr, one line per problem — fix what it names

`blastit check` exits `2` when it *could not start* (`docs/reference/cli.md`: "`2` it could
not start"), which also covers a missing `pytest` and a broker that never became ready
(`crates/blastit/src/check.rs:338`, `:791`). A developer whose check exits `2` for a missing
dependency is sent to the wrong file. (This row is a diff addition inside the reviewed
section, though not the row named in the brief.)

**Replacement.**

```text
| exit `2` | Blastit could not start — a `Blastfile` or configuration problem, or a dependency it could not launch: the refusal is already on stderr; fix what it names |
```

---

## Checked and passing (no finding)

- `docs/reference/blastfile.md:641-642` — the Limits cross-reference is correct: exactly
  three `query` caps (`MAX_TEMPLATE_CHARS`, `MAX_TEMPLATE_PARAMS`, `MAX_TEMPLATE_PARSES`),
  and all three are present under Grants. The clause was short and duplicated no budget.
  (MINOR at most: "three caps" would read better as "the three `query` budgets", but not
  worth a finding on its own.)
- `docs/reference/blastfile.md:284-292` — hover paragraphs verbatim; the intro correctly
  scopes them "inside a `query`".
- `docs/reference/blastfile.md:314-315` — the `…` refusal is stated with its fix, and is not
  one of the seven kinds, so its absence from the table is right.
- `docs/local.md:151` — "colours only inside `query '…'`" is true of the grammar
  (`blastfile-lsp-zed.md` §9.2a decision 5).
- Zero `sudo`, no env var, and no extra command added by the diff on any page.
