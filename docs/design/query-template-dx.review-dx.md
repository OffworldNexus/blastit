# `query` template validation — DX review (human developer experience)

**Scope.** Fresh-context `dx` critique of the four-page diff for MB.34.1 (`docs/reference/blastfile.md` §Grants + the `query` table row, `docs/local.md` §Editors step 2, `docs/ci.md` §What it fails on, `docs/troubleshooting.md` §`blastit check` fails), judged against the MB.34 epic contract, the MB.34.1 acceptance questions ("which part is validated / where the squiggle lands / what `$?` and `...` expand to / what the Quick Fix rewrites"), and the DX-first rules (≤3 steps, zero `sudo`, lean prose, one canonical way). Constraint 1 holds: **zero new fenced blocks** in the diff — every addition is a table row, bullet or inline code. Local and CI step counts do not grow (local: install extension → open file → optional format-on-save; CI: one `blastit check`, one re-run after a fix; deploy: no new step, but see M2 for a missing deploy surface). The `line:col` arithmetic is arithmetically possible throughout (8-space indent ⇒ `query` at col 9, `'` at 15, interior at 16), but two samples cannot be reconstructed by the reader (m1) and the empty-entry site is one column past the last character (m9).

## BLOCKER

### B1 — `docs/reference/blastfile.md:275-276` — `(row), ...` says the wrong thing about the wire

> `* (row), ...` repeats identical rows: after `VALUES` it reaches PostgreSQL as the one row written.

Its two sibling bullets both describe the **wire** expansion (`$?` → `$1, $2, …`; `IN ($?, ...)` → `IN ($1, $2, …)`), so this bullet is read the same way: an `INSERT … VALUES ($?, …), ...` matching N identical rows reaches PostgreSQL as **one** row. That contradicts "repeats identical rows" and would silently turn a bulk insert of 100 rows into 1 — a data bug the docs would have blessed. The reader cannot tell whether `...` covers N rows on the wire or is only a parse-time collapse used to validate the grant.

**Rewrite (reference:275-276):**
> `* (row), ...` covers a `VALUES` list of any length whose rows are identical: Blastit parses it as the one row written, and N rows still reach PostgreSQL as N rows. One grant covers every N.

If the intended wire behaviour is in fact "one row", say so explicitly and say why a repeated `...` token exists at all — otherwise MB.34.2 cannot design it.

## MAJOR

### M1 — `docs/reference/blastfile.md:258` — `…` (U+2026) in the canonical syntax row now collides with the `...` token

> `| query 'SELECT …' | one exact PostgreSQL statement, written in Blastit's `$?` template; repeat the directive for more |`

The page uses `…` as its metavariable (`:35`, `:120`, `:622`). The new grammar gives **three ASCII periods** the opposite meaning: a token that is "refused anywhere else" (`:277-280`). A developer copying the documented row shape types `SELECT ...`, gets `Blastfile:9:31`, and has no way to see the difference in most fonts. "written in Blastit's `$?` template" is also a coined term that appears nowhere else, and a statement with no values is not in any template.

**Rewrite (reference:258):**
> `| query 'SELECT id FROM products WHERE price > $?' | one exact PostgreSQL statement; `$?` binds one value, and the ellipsis is `...` (three ASCII dots) only in the two forms below; repeat the directive for more |`

### M2 — `docs/reference/blastfile.md:263-266` — never says *which part* is validated, and omits the deploy surface

> One `query` is one PostgreSQL statement, parsed as the SQL it claims to be … in `blastit fmt --check`, `blastit check` and the editor alike — never a runtime `sql.denied`.

Three of the four questions MB.34.1 must answer are here, but a reader still cannot say **which part** of the entry is parsed: the whole `query` directive, the string with `$?` untouched, or the string with `$?` replaced by `$1…`? A `$?` is not valid PostgreSQL input, so "parsed as the SQL it claims to be" is literally false unless expansion is stated — and it is stated only on the troubleshooting page (`troubleshooting.md:40`). Deployment is also absent from the surface list, so a deployer cannot learn that production refuses the file too.

**Rewrite (reference:263-266):**
> One `query` is one PostgreSQL statement. Blastit parses the text inside the string with each `$?` replaced by `$1`, `$2`, … (and `IN ($?, ...)`, `(row), ...` collapsed), and demands exactly one statement: a typo, a missing `FROM` or a stray `)` is a `Blastfile:line:col` diagnostic at that token in `blastit check`, the editor and the production boot alike — never a runtime `sql.denied`.

(Drop `blastit fmt --check` here; see M3.)

### M3 — `docs/reference/blastfile.md:265` — `blastit fmt --check` is claimed as a validator but defined elsewhere as canonicality-only

`:586` defines it as "change nothing; name every file that is not canonical and exit non-zero", and `:605` as "its own CI step" for formatting. A developer who reads both concludes `fmt --check` catches SQL typos; the epic's acceptance tests (`cargo test -p blastit-manifest`, `--lib lsp`, `critical_lsp`) prove only `check` and the LSP. Either drop the surface or state the added clause.

**Rewrite:** remove "`blastit fmt --check`, " from reference:265. If fmt really refuses a non-loading file, add one clause to `:586`: "a file that does not load is named by its `line:col` too".

### M4 — `docs/reference/blastfile.md:267` — "trusted as written and runs without further checking" contradicts the new parse

> An exact `query` is trusted as written and runs without further checking.

Directly after "parsed as the SQL it claims to be", a reader reads this as "no checks at all", undoing the paragraph above. The intended meaning is narrower: no table/column analysis, no `sqlpolicy::analyze`.

**Rewrite:** "An exact `query` is matched as written and gets no table or column analysis. Read it before pasting: …"

### M5 — `docs/reference/blastfile.md:272-276` — three bullets for a three-row table, and the `$?` bullet misreads

> `* $?` is one value: it reaches PostgreSQL as `$1`, `$2`, … in statement order.

"One value … `$1`, `$2`, …" reads as if one `$?` becomes several placeholders; the reader has to infer "the first `$?` is `$1`, the next `$2`". The page is table-driven everywhere else and the node invites a table row instead of prose.

**Rewrite (replace reference:272-276 with one table, no fence):**
> | Template | Reaches PostgreSQL as |
> |---|---|
> | `$?` | the next `$n`: the first `$?` is `$1`, the second `$2` |
> | `IN ($?, ...)` | `IN ($1, $2, …, $n)`; any length, one grant |
> | `(row), ...` | N identical `VALUES` rows; parsed as the one row written |

Six prose lines become three, and the answer to "what does `$?` expand to" is scannable.

### M6 — `docs/reference/blastfile.md:281-284` — the literal rule appears twice in two voices, and its only fix does not exist for common literals

> `* A statement that inlines a literal cannot be learned: bind the value (`$?`).`
> `* The only literals a committed `query` may hold are the two protocol setup statements, … any other literal is refused at its own `line:col`, telling you to bind it with `$?`.`

The first bullet reads permissive ("not learnable — fine, I'll write it myself"); the second refuses it. That is exactly the whack-a-mole loop the DX-first rules exist to prevent. Worse, the promised remedy is impossible for a literal that is not a bindable value: `interval '1 day'`, `'2026-01-01'::date`, `LIKE 'x%'`, `date_trunc('day', …)`. A developer with `query "SELECT * FROM t WHERE created_at > now() - interval '1 day'"` is told to bind `$?` where the app sends a literal, and no page says the code must change instead.

**Rewrite (merge into one bullet, reference:281-284):**
> A `query` may hold no literal: a value cannot be learned and is refused at its own `line:col`. Bind it (`$?`) in the query and in the code that sends it. The only exceptions are the two setup statements, `SET TIME ZONE 'UTC'` and `set_config(…)` (see [Connection setup](#connection-setup)); `interval '…'`, `'…'::date` and a `LIKE` pattern are literals — bind them, or the statement cannot be granted.

If a non-bindable literal genuinely has no legal spelling, the epic must record that as a design decision (0.3.0 gap) rather than leave the page promising a fix.

### M7 — `docs/local.md:149-154` — step 2 becomes a 91-word two-sentence run; the Quick Fix's output and safety are ambiguous

> `The Quick Fix **Parameterize SQL for Blastfile ($?)** rewrites a statement holding a literal or `$1…$n` into `$?`/`IN ($?, ...)`, hover over `$?` or `...` says what it binds, and the extension starts `blastit lsp` itself, from `PATH` or through `uv run`.`

Three independent clauses comma-spliced into one 47-word sentence, on top of a 44-word first sentence. The node asked for one or two sentences and this is two only by punctuation. It does not answer "what the Quick Fix rewrites": `$1…$n` becomes `$?` or `IN ($?, ...)`? And read literally, "a statement holding a literal" includes `SET TIME ZONE 'UTC'` — one click on the connection trait would rewrite the page's only exempt statement into `SET TIME ZONE $?`, which is refused. `...` also does not "bind".

**Rewrite (local.md:149-154):**
> 2. Open `./Blastfile`: colours, bracket matching, an outline, and a squiggle for each syntax error, in the words `blastit check` prints, at the same `line:col` counted in the `Blastfile` — inside a `query`, on the SQL token itself. The Quick Fix **Parameterize SQL for Blastfile ($?)** rewrites a pasted literal into `$?` and a `$1…$n` list into `IN ($?, ...)`, skipping the two setup statements; hover over `$?` or `...` says what it binds or repeats. The extension starts `blastit lsp` itself, from `PATH` or through `uv run`.

### M8 — `docs/troubleshooting.md:40` — symptom is inverted, and the `psql -c` advice cannot be followed

> `| a `query` squiggle I cannot find | it is validated after `$?` expansion, so count `IN ($?, ...)` as one value; copy the SQL into `psql -c` to see PostgreSQL's own error |`

"A squiggle I cannot find" describes a squiggle that isn't there, not the case this row serves (a squiggle whose position looks wrong). "Copy the SQL into `psql -c`" cannot work: `$?` is not a psql placeholder, and `IN ($?, ...)` errors in psql before the developer ever sees the error they came for. This page is also the only place the "after `$?` expansion" fact appears (M2).

**Rewrite (troubleshooting.md:40):**
> `| a `query` squiggle on SQL that looks valid | it is validated after `$?` expansion, so `IN ($?, ...)` counts as one value; replace each `$?` with a value and run the statement in `psql` to read PostgreSQL's own error |`

### M9 — `docs/troubleshooting.md:46` — the exit-`2` row sends the developer on a step that `check` now performs

> `| exit `2` | the `Blastfile` or the configuration did not parse; run `blastit dev` once to see the message |`

After `ci.md:40-42`, `check` itself prints `Blastfile:line:col` on stderr. Telling the reader to boot `dev` to see a message they already have is one wasted step and contradicts the new paragraph.

**Rewrite (troubleshooting.md:46):** `| the file does not load | the `Blastfile:line:col` line on stderr names the token: fix it and rerun. Exit `2` is "could not start", not a finding |`

### M10 — `docs/ci.md:40-42` — correct fact, wrong place and awkward register

> `An unparseable `query` stops the run before the audit: the file does not load, so `check` exits `2` with `Blastfile:line:col` on stderr and prints no document, `--json` included. Fix the token it names; no `fix.action` is proposed for it.`

Placed *after* the failure table, so the scanning reader — the normal path on this page — never sees the one failure that produces no table row. "stops the run before the audit" and "the file does not load" say the same thing twice, and "prints no document, `--json` included" is hard to parse: does `--json` print nothing (yes) or a JSON error (no)? A script that `jq`s stdout gets empty input, and the sentence should say so. "no `fix.action` is proposed" is `--json` jargon in the human half of the page.

**Rewrite — move to the first paragraph under `## What it fails on` (ci.md:20-22), replacing lines 40-42:**
> A `query` that does not parse is not a finding: the file does not load. `check` exits `2`, writes one `Blastfile:line:col: <sentence>` line to stderr, and leaves stdout empty — `--json` included, so a script parsing stdout gets no document. Fix the token it names; `--json` carries no `fix.action` for it.

Three lines stay three, but they land where the reader looks and say exactly what a script sees.

## MINOR

### m1 — `docs/reference/blastfile.md:277-280` — the sample messages cannot be reconstructed from the text

All three are arithmetically possible, but only if you guess the statement: `9:24` requires `query 'SELECT 1; SELECT 2'`; `9:31` requires `query 'SELECT id FROM ...'` (`SELECT id FROM ` is exactly 15 chars from the interior at 16, so `...` lands on 31); `9:16` matches both `query 'SELCT …'` and an empty `query ''`. Name the input inline before each sentence, e.g. `query 'SELECT 1; SELECT 2'` → ``…:9:24: a `query` is one statement; this `;` starts a second``. Positive: the `:245` example's indent (8) and interior (16) are consistent with every number shown.

### m2 — `docs/reference/blastfile.md:277` — "an empty entry" is not the page's noun

The message itself says `query`. Rewrite "and so is an empty entry or a second statement" → "and so is an empty `query` or a second statement".

### m3 — `docs/reference/blastfile.md:284` — quote the message instead of paraphrasing the remedy

The contract requires "the sentence the CLI prints" for the inline literal too. Rewrite "telling you to bind it with `$?`" → `Blastfile:9:16: this `query` holds a literal; bind the value with `$?`` (inline code, no fence).

### m4 — `docs/reference/blastfile.md:263` — the named examples and the quoted messages disagree

"a typo, a missing `FROM` or a stray `)`" are not the three cases the messages show (empty, second statement, stray `...`). Either make the prose list match the messages, or add one sentence: "the messages below are the ones you will meet most".

### m5 — `docs/local.md:151` — the Quick Fix is never given an invocation

A reader who has the squiggle does not necessarily know Zed's code-action key. Add four words: "The Quick Fix **Parameterize SQL for Blastfile ($?)** (the code-action key) rewrites …".

### m6 — `docs/local.md:150` — "at the same line and column" can be read as a position inside the string

"inside a `query`, on the SQL token itself" plus "line and column" invites "line/column of the SQL inside the string". Folded into M7's rewrite ("counted in the `Blastfile`"); keep it if M7 is edited differently.

### m7 — `docs/ci.md:42` — `fix.action` is agent vocabulary in a human paragraph

Covered by M10's rewrite; if M10 is rejected, at least write "`--json` proposes no `fix.action` to apply".

### m8 — `docs/reference/blastfile.md:283` — the `#traits` link does not show `set_config(…)`

`## Traits` (`:341`) shows only `SET TIME ZONE 'UTC'`. The section with both spellings is `### Connection setup` (`:301-320`). Point the exemption link at `#connection-setup`.

### m9 — `docs/reference/blastfile.md:278` — confirm the empty-entry site

For `query ''` at 8-space indent, the opening quote is 15 and the closing quote 16; column 16 is therefore one past the last character of an empty interior. That is defensible ("the string"), but the contract says "at the offending token" and an empty string has none. Pin the site in MB.34.2 as the opening quote (15) or the interior start (16), and keep the sample consistent with that choice.

## Answers the four pages must give (audit)

| Question | Answerable? | Where |
|---|---|---|
| Which part of a `query` is validated | **No** — requires the troubleshooting page and still omits `$?` expansion | M2 |
| Where the squiggle appears | **Almost** — "on the SQL token itself" is right, but `line:col` basis and the whole-string vs token distinction need one clause | M7/m6 |
| What `$?`, `IN ($?, ...)`, `(row), ...` expand to | `$?` and `IN` yes; `(row), ...` **no** | B1, M5 |
| What the Quick Fix rewrites | **No** — `$?`/`IN ($?, ...)` target ambiguous, exempt statements unaddressed | M7 |

VERDICT: CHANGES
