# `query` template validation — goldfish review round 2, `ai-ergonomics` lens

Scope: the current text of `docs/reference/blastfile.md` (the `query` row at :258, the template table :264-271, the bullets :273-313), `docs/local.md` `## Editors` step 2 (:149-155), `docs/ci.md` `## What it fails on` first paragraph (:22-26) and `docs/troubleshooting.md` (:40, :46), judged as an LLM agent that must write, verify and repair `query` grants in a 50-route `Blastfile` with no human in the loop; checked against `normalize.rs:41-105`, `literals.rs:41-63`, `validate.rs:253-304`, `check.rs:104-141`, `cli.rs:601-653` and the plan nodes `MB.34.2`, `MB.34.3`, `MB.34.4`.

## Round-1 resolution

| Round-1 finding | Gone? | Evidence in the current text |
|---|---|---|
| BLOCKER 1 — template fed raw to `psql`; invites inlining a literal | **yes** (copy repaired) | `troubleshooting.md:40` "replace each `$?` with a value, drop the `...`, and run the copy with `psql -c`" — no `$?`/`...` reaches psql. Residual wording is NEW MINOR 4; the caret half is NEW MAJOR 1 |
| MAJOR 2 — parse order after expansion unstated | **yes** | `blastfile.md:275` "One `query` is one PostgreSQL statement, parsed after those expansions" |
| MAJOR 3 — "trusted as written and runs without further checking" | **yes** | string absent; `blastfile.md:279` "An exact `query` is matched as written and gets no table or column analysis" (wording nit = NEW MINOR 5) |
| MAJOR 4 — `set_config(…)` looser than the allowlist | **yes** | `blastfile.md:293-294` "Only two statements may keep a quoted literal: `SET TIME ZONE 'UTC'` and `SELECT set_config('TimeZone', $?, false)`"; matches `literals.rs:41-44` + `validate.rs:276` exactly |
| MAJOR 5 — "literal" vs "quoted literal" | **yes** | `blastfile.md:295-298` "Any other quoted string … A number, `true`, `NULL` and a quoted identifier are ordinary text"; matches `literals.rs:46-63` |
| MAJOR 6 — hand-written `$1…$n` unstated | **no** | `blastfile.md:273-274` now says "`$1…$n` is accepted too: Blastit numbers it from one in the order written" — the acceptance is right, the mechanism is not (NEW MAJOR 3) |
| MAJOR 7 — exit `2` / `--json` contradiction | **yes** | `ci.md:22-26` "prints nothing on stdout: `--json` included, and an empty stdout is a refusal, never a clean audit. It adds no `kind` and no `fix.action`" |
| MAJOR 8 — `blastit fmt --check` claimed as the validator | **yes** | parse bullet now names only "`blastit check`, the editor and the production boot" (`blastfile.md:276-277`) |
| MAJOR 9 — the row's example was the refused `…` | **partly** | `blastfile.md:258` is a real statement; the `…`-in-a-`query` refusal is still undocumented (NEW MINOR 2) |
| MINOR 1 — three examples all at line 9 | **yes** | lines 9 / 11 / 13 at `blastfile.md:289-291`; but the column on the `;` example is wrong (NEW MAJOR 2) |
| MINOR 2 (round-1 numbering) — `Blastfile:line:col` is not the printed string | **no** (was not in the hand-over list) | `ci.md:24`, `blastfile.md:289-291` (NEW MINOR 1) |
| MINOR 3 — `(row), ...` metasyntax | **yes** | `blastfile.md:271` "`($?, $?), ...`"; `…` survives in two other template spellings (NEW MINOR 2) |
| MINOR 4 / 5 — two exit-`2` instructions; "template" phrasing | **yes** | `troubleshooting.md:46` "the refusal is already on stderr, one line per problem"; `blastfile.md:258` "one exact PostgreSQL statement; `$?` binds one value" (residual = NEW MINOR 3) |

## Hard constraints

* **No new fences.** Counts are `34 / 16 / 18 / 0`, byte-identical to `HEAD` for all four pages.
* **One read.** An agent can write `$?`, `IN ($?, ...)` and `($?, $?), ...` correctly from the template table alone, and `local.md:142` plus `blastfile.md:310-313` kill the `unrestricted` temptation. It cannot yet be certain *where* the caret will be (MAJOR 1) or whether `$n` is renumbered (MAJOR 3).
* **Implementable.** Two claims cannot be honoured alongside `MB.34.2`/`MB.34.3` as written: the `$1…$n` renumbering (MAJOR 3) and the `;` example's column (MAJOR 2). The rest matches the crate code I read.

## New findings

### MAJOR 1 — the troubleshooting row re-imposes the expanded-text caret the other two pages deny

`docs/troubleshooting.md:40`

> `| a `query` squiggle I cannot place | it is validated after `$?` expansion, so `IN ($?, ...)` is one value; replace each `$?` with a value, drop the `...`, and run the copy with `psql -c` to read PostgreSQL's own error |`

`docs/local.md:149-150` promises "a squiggle … at the same `line:col` counted in the `Blastfile` — inside a `query`, on the SQL token itself", and `MB.34.4` exists to map the validator's inner offset back to the raw text (`MB.34.2:39-46`, `MB.34.4:30-45`). The row explains an unplaceable squiggle by expansion, i.e. it tells the agent the caret may sit in text it did not write — round-1 BLOCKER 1's second half, repaired in `psql` but not in the explanation. The agent's next move is to hunt the column in the expanded form, fail, and edit the query (or inline the value). Suggested row, keeping the one-line shape:

> `| a `query` squiggle I cannot place | the caret is on the token you wrote: `$?`/`...` are expanded only to parse and the offset is mapped back. If that token looks right, the sentence names the form it refuses — empty, a second `;` statement, or `...` outside the two list forms. For PostgreSQL's own error, run a copy with each `$?` replaced by a value of that column's type and the trailing `, ...` dropped |`

### MAJOR 2 — the `;` refusal points at a different token than the protected test pins

`docs/reference/blastfile.md:290`

> `| `query 'SELECT 1; SELECT 2'` | ``Blastfile:11:24: a `query` is one statement; this `;` starts a second`` |`

On this layout the `;` is column 24 and the second statement's `S` is column 26. `MB.34.3:87` pins the offset as "the second statement's first character", and `MB.34.2:34-36` says the parse decides `stmts.len() == 1` (so the offset is validator-chosen, not `pg_query`'s). Either the implementation lands on `26` and contradicts this table, or the protected test's expectation is wrong — and in both cases the sentence "this `;`" disagrees with a caret on `SELECT`. The empty row above it is the same class of problem: `Blastfile:9:15` is the opening quote, while `MB.34.4:33-34` fixes the origin at "the first character *inside* the opening quote" (column 16 for an empty string), so the two rows disagree about which origin a table that reads as one set uses. Fix: quote the sentences, drop the columns (see MINOR 1), or align both to the design — "`26`: a `query` is one statement; this one starts a second".

### MAJOR 3 — `$1…$n` is described as renumbered, but the code and the design discard the numbering

`docs/reference/blastfile.md:273-274`

> `* `$1…$n` is accepted too: Blastit numbers it from one in the order written, so a pasted statement needs no edit.`

`normalize.rs:56-60` rewrites "every bind placeholder `$1`, `$23`, … in code becomes `$?`: the numbering carries no meaning", and `MB.34.2:28` / `MB.34.3:34` both say "`$1…$n` pass through unchanged". An implementer who follows this sentence renumbers (so `$2, $1` becomes `$1, $2`, and a lone `$3` becomes `$1`); one who follows the design does not. The *user-visible* promise ("a pasted statement needs no edit") is correct and worth keeping — only the mechanism is wrong, and it also leaves the round-1 "do not mix `$?` and `$n` in one entry" gap open. Fix: "`$1…$n` written by hand is accepted as it stands: the numbering is discarded when the grant is matched, so `$1` and `$?` are the same grant."

### MINOR 1 — the quoted refusal lines are not lines `blastit check` prints

`docs/reference/blastfile.md:289-291`, `docs/ci.md:24` ("at `Blastfile:line:col`"). `check` prints one `eprintln!` of `blastit check: <path>: {errors}` (`check.rs:118`, `:137-146`), and `ManifestErrors`' `Display` prefixes the path **once** for several problems (`error.rs:1650-1665`); each grant refusal also carries `{location}: queries[{rule}]` (`error.rs:419-460`, kept by `MB.34.4:55`), i.e. `route "catalog" (index 0): queries[0]`. So the real lines are of the shape `blastit check: ./Blastfile: 9:15: route "catalog" (index 0): queries[0]: …` and `11:24: …` without the path. An agent anchoring on `^Blastfile:\d+:\d+: ` matches nothing. Fix: quote only the sentence in the table (heading "Refused with (the sentence)") and put the prefix shape once in prose, as `blastfile.md:630` already does with `<file>:<line>:<col>`.

### MINOR 2 — `…` is still used as a template spelling, and the `…` refusal is still undocumented

`docs/reference/blastfile.md:270` "`IN ($1, $2, …, $n)`", `:273` "`$1…$n` is accepted too", `docs/local.md:153` "`IN ($1, …)`". U+2026 inside a `query` is not metasyntax to Blastit: it is the redaction marker and `validate.rs:269-274` refuses it (`RedactedQueryPlaceholder`), with no page saying so — the second half of round-1 MAJOR 9. The reference's own new bullet now literally advertises a spelling (`$1…$n`) that a copy into a `query` turns into that refusal. Fix: `$1`–`$n` in prose (or `$1` to `$n`), and add one bullet after the literal rule: "A `…` in a `query` is refused: it is what redaction writes and the value it replaced is gone. Grant the exact statement, with `$?` where the values bind."

### MINOR 3 — "fix the line it names" does not fit configuration failures

`docs/troubleshooting.md:46`

> `| exit `2` | the `Blastfile` or the configuration did not load: the refusal is already on stderr, one line per problem — fix the line it names |`

The row covers both halves of the exit code. A configuration refusal (`BLASTIT_APP: the WSGI callable … is required`, an unset store variable, a `BLASTIT_*` that does not parse — rows :10-27) names a variable or a knob, not a line. Change the tail to "fix what it names" (or split `Blastfile` and configuration into two rows).

### MINOR 4 — "with a value" and "is one value" hand the agent a type trap

`docs/troubleshooting.md:40`. "`IN ($?, ...)` is one value" should be "expands to one placeholder" (`MB.34.2:29`: `IN ($1)`); the reference calls the same thing a placeholder (`:269`). "replace each `$?` with a value" does not say of which type: for `WHERE email = $?` an agent that substitutes `1` gets `operator does not exist: text = integer` from psql — an error that belongs to the substituted value, not to the grant — and the obvious "fix" is to cast or rewrite a correct query. Say "a value of that column's own type (`0`, `'x'`, `true`)".

### MINOR 5 — "matched as written" contradicts the rest of the same bullet

`docs/reference/blastfile.md:279-282`

> `* An exact `query` is matched as written and gets no table or column analysis. … comments and whitespace are ignored.`

`normalize.rs:50-70` collapses whitespace, replaces comments, maps `$n` to `$?` and collapses `IN` lists and `VALUES` rows; `troubleshooting.md:78` says exactly that ("`$1` compares as `$?`, whitespace collapses; a different column list misses"). "Matched as written" tells an agent the literal text must match. Fix: "matched after comments and whitespace are folded, so the same statement with different spacing is one grant".

### MINOR 6 — pre-existing, outside the diff: `blastit fmt --check` is still called a PHP gate that preflights

`docs/reference/blastfile.md:535-537`

> `For a PHP app the gate is `blastit fmt --check`: the manifest, `root` and `libphp` preflight runs, but 0.2.0 has no request audit…`

`run_fmt` (`cli.rs:601-653`) does `blastfile::read` + `blastfile::fmt` and nothing else: no lowering, no `check_queries`, no `root`/`libphp` preflight. This is the same false green round-1 MAJOR 8 removed 260 lines above, in the one paragraph a PHP agent will use because `check` cannot audit it. Not part of this diff; worth fixing while the page is open.

VERDICT: CHANGES
