# `query` template validation — goldfish review, `ai-ergonomics` lens

Scope: MB.34.1's diff to `docs/reference/blastfile.md` (the `query` row + Grants bullets), `docs/local.md` (`## Editors`, step 2), `docs/ci.md` (`## What it fails on`, new paragraph) and `docs/troubleshooting.md` (`blastit check` fails, new row), judged as an LLM coding agent that must write, verify and repair `query` entries in a 50-route `Blastfile` with no human in the loop. Checked against `docs/VISION.md` §3 (machine-readable verification must make `unrestricted` unnecessary), `plan/nodes/MB/MB.34/NODE.md`, and the behaviour the words promise: `crates/blastit/src/cli.rs::run_fmt`/`run_check`, `crates/blastit-manifest/src/blastfile/print.rs::format_with`, `crates/blastit-manifest/src/validate.rs::check_queries`, `crates/blastit-sqlpolicy/src/{normalize,literals}.rs`. Fence counts are unchanged (34/16/18/0 vs `HEAD`), and no added content needed a fence: the `psql` advice is correctly inline code and the CI paragraph correctly stays prose (its table is keyed by `kind`, and an unloadable file has none).

## Answers to the six questions

1. **One parseable statement?** Almost. The table row and the 263 bullet say it, but *when* the parse happens (after `$?`/`...` expansion) appears only in `troubleshooting.md`, so a top-down reader can believe the literal text `IN ($?, ...)` must be valid PostgreSQL. See MAJOR 2.
2. **Quotable refusals / `line:col` arithmetic?** The arithmetic is sound: 8 spaces put `query` at 9, the string interior at 16; `;` at 24 is `'SELECT 1; …'` and `...` at 31 is `'SELECT $1, $2, ...'`. But the quoted lines are not the shape the CLI prints (`check` prefixes `blastit check: ` and a path; only the first of several lines repeats it), and all three claim line 9, which no single run can produce. See MINOR 1–2.
3. **Expansion unambiguous?** `$?` numbering is clear; what `$1…$n` in a hand-written entry means is not, and `local.md` tells the agent the Quick Fix rewrites exactly that. See MAJOR 6.
4. **Anything inviting `unrestricted` / a pasted literal / a guess?** Yes, once: the troubleshooting row sends the template to `psql`, where it is guaranteed to fail at `$?`/`...`, and the obvious repair is to strip the placeholder and inline a value — the literal the reference then refuses. See BLOCKER 1. The exemption, in contrast, is *loose* rather than tight (`set_config(…)`). See MAJOR 4.
5. **CI exit `2`?** Stated that a document is not printed, but contradicted two sentences later by "no `fix.action` is proposed", and it never says an empty stdout is not a clean audit, nor that no `kind` is added. See MAJOR 7.
6. **Zero new fences?** Satisfied on all four pages; no addition reads as a fence-count change and nothing that should have been a row was written as a fence.

## Findings

### BLOCKER 1 — the `psql` advice feeds a template to PostgreSQL, and the only error it can produce points at the wrong thing

`docs/troubleshooting.md:40`

> `| a `query` squiggle I cannot find | it is validated after `$?` expansion, so count `IN ($?, ...)` as one value; copy the SQL into `psql -c` to see PostgreSQL's own error |`

Failure mode: `psql -c "… IN ($?, ...)"` fails with `syntax error at or near "$"`; replacing `$?` with `$1` fails with `there is no parameter $1`. The agent therefore gets a "PostgreSQL error" that belongs to Blastit's template, not to its statement, and the shortest way to make it go away is to delete the placeholders and inline the value — precisely the literal the reference refuses two pages away (`docs/reference/blastfile.md:281-284`). The row also implies the reported column lives in the *expanded* text, contradicting `local.md:150` ("at the same line and column — inside a `query`, on the SQL token itself") and `blastfile.md:264` ("at that token"), and it never says which of the two is true.

Proposed row (one line, no fence, no new anchor needed):

> `| a `query` squiggle I cannot find | the column is in the text you wrote: `$?` and `...` are expanded only for parsing, so nothing is renumbered under the caret. For PostgreSQL's own error, copy the statement into `psql -c` with every `$?` replaced by `1`, every `IN ($?, ...)` replaced by `IN (1)`, and the trailing `, ...` dropped — a bare `$?` or `...` fails in `psql` on its own |`

### MAJOR 2 — the page never says the parse happens *after* expansion

`docs/reference/blastfile.md:263-266`

> `* One `query` is one PostgreSQL statement, parsed as the SQL it claims to be: a typo, a missing `FROM` or a stray `)` is a `Blastfile:line:col` diagnostic at that token…`

Failure mode: the only sentence on any page that names the order ("validated after `$?` expansion") is in `troubleshooting.md:40`, which this review asks to rewrite. An agent that writes `IN ($?, ...)` and reads 263 will try to make the literal template text parse, and the first thing "fixing" that produces is `IN ($1, $2)` with a hand-counted list.

Add (and use this same sentence to resolve MAJOR 8):

> `* One `query` is one PostgreSQL statement, parsed as the SQL it claims to be once `$?` and `...` are expanded (below), and refused before the audit if it is not: an empty entry, a typo, a missing `FROM` or a stray `)` is a `Blastfile:line:col` diagnostic at that token, in `blastit check` and the editor alike — never a runtime `sql.denied`.`

### MAJOR 3 — "trusted as written and runs without further checking" contradicts the two bullets around it

`docs/reference/blastfile.md:267-268`

> `* An exact `query` is trusted as written and runs without further checking.`

Failure mode: 263 says the statement is parsed and 282-284 says its literals are refused, so "without further checking" is false; "runs" is worse — a `query` is a grant, it is never executed, and an agent that believes it runs may put `SET`/side-effecting statements in it expecting them to be applied. This is the bullet an agent quotes when deciding whether its grant is checked at all.

Proposed: `* An exact `query` is trusted as written once it parses: the grants do not narrow it further.`

### MAJOR 4 — `set_config(…)` is looser than the allowlist and contradicts `ci.md`

`docs/reference/blastfile.md:282-284`

> `* The only literals a committed `query` may hold are the two protocol setup statements, `SET TIME ZONE 'UTC'` and `set_config(…)` ([traits](#traits)); any other literal is refused at its own `line:col`…`

The real allowlist is two exact normalised strings (`crates/blastit-sqlpolicy/src/literals.rs:41`: `SET TIME ZONE 'UTC'`, `SELECT set_config('TimeZone', $?, false)`), and `docs/ci.md:90` already says so ("`add_group` fits only `SET`, `SHOW` or `set_config('TimeZone', …)`; another `set_config(…)` is `observe`"). Failure mode: an agent writes `query "SELECT set_config('search_path', 'public', false)"`, is refused for a literal, and the reference appears to have told it this was exempt — so the next move is to drop the statement or reach for a wider grant. `set_config(…)` also collides with the page's `…` elision, which is a hard refusal inside a `query` (see MAJOR 9).

Proposed:

> `* The only two statements allowed to keep a quoted literal are `SET TIME ZONE 'UTC'` and `SELECT set_config('TimeZone', $?, false)` ([connection setup](#connection-setup)); every other quoted literal is refused at its own `line:col`, and the refusal tells you to bind the value with `$?`. Another `set_config(…)` is not exempt.`

### MAJOR 5 — "literal" means "quoted literal" in the code, and the page says "any literal"

`docs/reference/blastfile.md:281, 284`

> `* A statement that inlines a literal cannot be learned: bind the value (`$?`).`

> `  other literal is refused at its own `line:col`, telling you to bind it with `$?`.`

`has_quoted_literal` (`crates/blastit-sqlpolicy/src/literals.rs:60`) refuses `'…'`, `E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, `$tag$…$tag$` — not numbers, `true`, `NULL`, or quoted identifiers. Failure mode both ways: an agent rewrites a legal `LIMIT 10`/`WHERE active` statement into `LIMIT $?` "to satisfy the literal rule", and an agent that reads "literal" as "quoted string" pastes `interval '1 day'` and gets an unexplained refusal. Neither agent learns the rule that would have prevented it.

Proposed (replaces 281, and the second clause of 284): `* A statement that inlines a quoted literal cannot be learned: bind the value (`$?`). Only quoted strings count — `'…'`, `E'…'`, `$tag$…$tag$`; a number, `true`, `NULL` or a quoted identifier (`"…"`) is ordinary text.`

### MAJOR 6 — a grant holding `$1…$n` is accepted (and renumbered), but no page says so

`docs/reference/blastfile.md:270, 272` and `docs/local.md:152`

> `  the text: comments and whitespace are ignored, `$1` and `$?` are the same, a`

> `* `$?` is one value: it reaches PostgreSQL as `$1`, `$2`, … in statement order.`

> `  Blastfile ($?)** rewrites a statement holding a literal or `$1…$n` into`

`normalize` rewrites every `$1`, `$23`, … to `$?` (`crates/blastit-sqlpolicy/src/normalize.rs:56`), so a pasted `$1…$n` statement is legal input and its numbering is discarded. The only statement of that is buried in the *matching* clause ("`$1` and `$?` are the same"), while `local.md` names `$1…$n` as something a statement may hold. Failure mode: the VISION's Node/asyncpg targets send `$1…$n`, so the agent must decide whether to keep them (and whether `$3` alone, or `$2` before `$1`, is meaningful) — and question 3's "no room to invent `$?` numbering" is not yet met, because mixing `$?` with `$3` in one entry is undefined.

Proposed, appended to the `$?` bullet: ` `$1…$n` written by hand is accepted and renumbered from one in the order the placeholders appear, so `$?` is the only spelling the editor writes; do not mix `$?` and `$n` in one entry.` (If mixing is instead legal, say what it expands to — the page must pick one.)

### MAJOR 7 — `--json` on exit `2`: "no document" and "no `fix.action`" cannot both be true, and empty stdout is not flagged as unsafe to read

`docs/ci.md:40-42` (with `docs/ci.md:102-104`)

> `…so `check` exits `2` with `Blastfile:line:col` on stderr and prints no document, `--json` included. Fix the token it names; no `fix.action` is proposed for it.`

`ci.md:103` already says "exit `2` leaves stdout empty", so "no `fix.action` is proposed" describes a finding object that never exists; an agent reading the pair will look for a `findings[]` entry (and may invent `kind: "unparseable_query"`, since the doc names `kind` values everywhere else and says nothing here). Then the pipeline case: `check --json > audit.json` yields a zero-byte file, and any wrapper that treats an empty document as "nothing to report" marks the repo clean. The sentence also says "Fix the token it names", which does not fit its own "this `query` is empty" message — there is no token.

Proposed:

> `An unparseable `query` stops the run before the audit: the file does not load, so `check` exits `2` and prints the refusal on stderr as `blastit check: <path>:<line>:<col>: <message>`, one line per problem. No document is written on exit `2` — an empty stdout is a refusal, never a clean audit — and this case adds no `kind` and no `fix.action`: edit the line it names and rerun.`

### MAJOR 8 — `blastit fmt --check` does not validate a `query`, so the sentence gives a false green

`docs/reference/blastfile.md:265-266`

> `  that token, in `blastit fmt --check`, `blastit check` and the editor alike —`

`run_fmt` calls `blastfile::read` + `blastfile::fmt`, and `format_with` only parses the CST and re-parses its own output (`crates/blastit-manifest/src/blastfile/print.rs:60-75`); no lowering, no `check_queries`, no `pg_query`. `fmt --check` therefore exits `0`/`1` on a file whose `query` is `SELCT`, and `docs/reference/blastfile.md:605` recommends it as "its own CI step". Failure mode: an agent adds `blastit fmt --check` to its pre-commit loop (the cheap, no-database step), sees green, and discovers the typo only in CI's `check` — the exact "validated where I looked" assumption this epic exists to create.

Either wire the validator into `fmt` (MB.34.4 must then say so, and `fmt` must still format a file whose SQL does not parse — a writer must not be blocked from fixing layout) or drop it from this sentence. Recommended, since a formatter should not run SQL policy: use MAJOR 2's wording, which lists `blastit check` and the editor only.

### MAJOR 9 — the table row's own example is the `…` that `check` refuses, and that refusal is documented nowhere

`docs/reference/blastfile.md:258`

> `| `query 'SELECT …'` | one exact PostgreSQL statement, written in Blastit's `$?` template; repeat the directive for more |`

`validate.rs:270` refuses any `query` containing `…` (`RedactedQueryPlaceholder`: "holds a redacted `…` placeholder … grant the exact statement, with `$?` where the values bind"), and no page mentions it. Failure mode: an agent copies the row (or the `sql` a deny log printed, which is always redacted with `…`) and gets a message about a placeholder it never knowingly wrote, with no documented fix; `local.md`'s Quick Fix is the only escape and it is editor-only. The page also uses `…` as metasyntax at 34-35, 372, 615, so the glyph has three meanings.

Proposed: cell -> `` | `query 'SELECT count(*) FROM products'` | one exact PostgreSQL statement, `$?` where a value is bound; repeat the directive for more | ``, plus one bullet after the literal rule: `* A `…` in a `query` is refused: it is what redaction writes and the value it replaced is gone. Grant the exact statement, with `$?` where the values bind — `--learn` never writes one.`

### MINOR 1 — the three examples read as one run they cannot be, in an order that disagrees with their lead-in

`docs/reference/blastfile.md:277-280`. The lead-in lists `...`, empty, `;`; the examples list empty, `;`, `...`. All three carry `Blastfile:9:`, but one line cannot simultaneously be an empty string and hold a `;` at column 24, and `check` reports all refusals for a file, so an agent comparing its own stderr with these will not find three lines of this shape. Fix: `* An empty entry, a second `;`-separated statement, or `...` anywhere else is refused, each naming its own token (three separate queries, all at this indent):` and give the examples lines `9:16`, `11:24`, `13:31`.

### MINOR 2 — `Blastfile:line:col` is not the string the CLI prints

`docs/reference/blastfile.md:278-280, 284`, `docs/ci.md:41`. Real output is `<path>:<line>:<col>: <message>` (`cli.rs:662-671` for `fmt`) and `blastit check: <path>:<line>:<col>: <message>` for `check` (`check.rs:114-141`), with the path repeated only on the first of several lines. A `^Blastfile:\d+:\d+: ` anchor misses every real run. One sentence in the Errors section fixes it for agents and for the three examples above: `The shape is `<path>:<line>:<col>: <message>`; `check` prefixes `blastit check: `, and when a file has several refusals only the first line repeats the path.` (`blastfile.md:630` already says `<file>:<line>:<col>` — reuse that word). MAJOR 5's `line:col` should then read `Blastfile:line:col` like every other sentence on the page.

### MINOR 3 — `(row), ...` is metasyntax in a language where `row` is a copyable token

`docs/reference/blastfile.md:275-276`. An agent told "`(row), ...` repeats identical rows" can write `VALUES (row), ...` and get a parser error about a column. Show the row as a row: `* `($?, $?), ...` after `VALUES` is any number of copies of that one row: it reaches PostgreSQL as the single row written, so one grant covers a multi-row insert of any length.` — and keep `…` (U+2026) out of every template spelling.

### MINOR 4 — two different instructions for exit `2`

`docs/ci.md:42` ("Fix the token it names") vs `docs/troubleshooting.md:46` ("run `blastit dev` once to see the message"). On a `check` run the message is already on stderr, so the troubleshooting row sends the agent to a second command for output it already has; and "token" does not fit the empty-entry message. Proposed row: `| exit `2` | the `Blastfile` or the configuration did not parse: the refusal is already on stderr, one line per problem, and `blastit dev` stops at the same line |`.

### MINOR 5 — "written in Blastit's `$?` template" implies every `query` needs a placeholder

`docs/reference/blastfile.md:258`. A constant statement (`query 'SELECT count(*) FROM products'`) has no `$?` and is legal; an agent that reads the row as "must be in template form" may add a placeholder that binds nothing, or assume a template is required to grant anything. Proposed cell wording is in MAJOR 9: `` `$?` where a value is bound ``.

VERDICT: CHANGES
