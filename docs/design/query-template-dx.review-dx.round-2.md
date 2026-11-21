# MB.34.1 `query` templates — `dx` review, round 2

**Scope:** the current `release-0.2.0` diff to `docs/reference/blastfile.md` (Grants: the `query` row, the two template/refusal tables and the bullets), `docs/local.md` (`## Editors`, step 2), `docs/ci.md` (`## What it fails on`, first paragraph) and `docs/troubleshooting.md` (the `blastit check` fails table), judged against `docs/VISION.md`, `plan/nodes/MB/MB.34/NODE.md`, `plan/nodes/MB/MB.34/MB.34.1/NODE.md` and the shipped `normalize`/`KNOWN_STATIC_SETUP_SQL` behaviour they describe.

## Evidence

- The fence-delimiter count (lines starting with three backticks) on the four pages is **34 / 16 / 18 / 0**, the required counts; the diff adds no fence.
- `make doc-tone` → `doc-tone: ok — 30 user-facing page(s), no banned phrase`.
- `make docs-check` → `uvx --offline zensical build --strict` → `No issues found` (so `#traits`, `#connection-setup` resolve).
- `python/tests/test_docs_assets.py` → `35 passed` (the pinned block inventory is untouched).

## Round-1 resolution

| Round-1 finding | Resolved | Current text |
|---|---|---|
| B1 `(row), ...` implied one row on the wire | yes, with a new overclaim | `blastfile.md:271` — "every copy still reaches PostgreSQL"; but "any number of times" is wrong → **MAJOR-1** |
| M1 ellipsis glyph / coined "template" in the row | yes | `blastfile.md:258` — real statement, no `…` |
| M2 which part is validated, parse order, deploy surface | yes | `blastfile.md:275-277` — "parsed after those expansions … `blastit check`, the editor and the production boot" |
| M3 `blastit fmt --check` claimed as SQL validator | yes | gone from the `query` contract at `258-298`; `536`, `600` and `619` describe `blastit fmt --check` as a formatting/CI step only |
| M4 "trusted as written and runs without further checking" | yes in the reference; one copy left elsewhere | `blastfile.md:279` — "matched as written and gets no table or column analysis"; `local.md:135` still says "trusted as written" → **MINOR-5** |
| M5 three prose bullets | yes | `blastfile.md:267-271` is a table |
| M6 duplicate literal rule / vague fix | yes | one bullet, `blastfile.md:293-298`, both exempt statements named |
| M7 91-word comma splice in local step 2 | structurally yes | `local.md:149-155` is three sentences, but two run ~36 and 39 words → **MINOR-6** |
| M8 symptom inverted; `psql` could not take `$?`/`...` | yes on both round-1 points; the replacement adds an error | `troubleshooting.md:40` — "cannot place"; the copy is repaired before `psql -c`; "is one value" is wrong → **MAJOR-2** |
| M9 exit-`2` row sent the reader to `blastit dev` | yes | `troubleshooting.md:46` — "the refusal is already on stderr" |
| M10 CI paragraph placement / empty stdout / no `kind` | yes | `ci.md:22-26`, before the table, "an empty stdout is a refusal", "no `kind`" |

## Hard-constraint re-check

- **Fences:** 34/16/18/0; no new block.
- **Four answers:** which part is validated — the whole statement after expansion (`blastfile.md:275`); where the squiggle appears — inside the string, on the SQL token, same `line:col` as `check` (`local.md:150-151`); what `$?`, `IN ($?, ...)` and `($?, $?), ...` expand to — `blastfile.md:269-271`; what the Quick Fix rewrites — `local.md:152-153`. All four are answerable once MAJOR-1/2 and MINOR-1 are fixed.
- **Arithmetic** at 8 spaces (`query` = col 9, opening quote = 15, interior = 16): `11:24` is the `;` (`SELECT` 16-21, space 22, `1` 23, `;` 24); `13:31` is the first `.` (`SELECT` 16-21, space 22, `id` 23-24, space 25, `FROM` 26-29, space 30, `.` 31); both are each a separate `query`. `9:15` is the quote, not the interior — see MINOR-3.
- **Tone:** no apology language, no kernel lecture; no `sudo`/privilege step is added anywhere.

## New findings

### MAJOR-1 — `blastfile.md:271`: "repeated any number of times" includes one row, and one row is denied

> | `($?, $?), ...` | that one `VALUES` row repeated any number of times: every copy still reaches PostgreSQL |

`normalize` collapses a run of **two or more** identical rows only (`crates/blastit-sqlpolicy/src/normalize.rs:67-71`; its test at `:1166` keeps `VALUES ($1, $2)` as `VALUES ($?, $?)`). A grant written with `, ...` therefore does **not** match a single-row insert — the common `bulk_create` with one object — and the developer gets the runtime `sql.denied` this page exists to prevent, with no row in Troubleshooting to explain it. "Any number" is read as 1, 2, 3…

Rewrite:

> | `($?, $?), ...` | `VALUES` with two or more identical rows, any count: a single row is its own `query`; every copy still reaches PostgreSQL |

### MAJOR-2 — `troubleshooting.md:40`: "`IN ($?, ...)` is one value" is the opposite of the reference and of the code

> | a `query` squiggle I cannot place | it is validated after `$?` expansion, so `IN ($?, ...)` is one value; replace each `$?` with a value, drop the `...`, and run the copy with `psql -c` to read PostgreSQL's own error |

`IN ($?, ...)` expands to `IN ($1, $2, …, $n)` (`blastfile.md:270`; `normalize.rs:61-64`), "one grant covers every list". Telling a reader it "is one value" contradicts the page the message came from and sends them counting the wrong thing; the row also never says where the squiggle's column starts, which is the actual "cannot place" remedy (a `query` at 8 spaces starts at column 9, the SQL at 16). The `psql -c` advice is otherwise sound.

Rewrite:

> | a `query` squiggle I cannot place | columns count from the line's first character, so a `query` at 8 spaces starts at column 9. It is validated after expansion, where each `$?` becomes its own `$n` and `IN ($?, ...)` becomes a list of any length. To read PostgreSQL's own error, replace each `$?` with a value, drop the `...`, and run the copy with `psql -c` |

### MINOR-1 — `blastfile.md:258`, `:270`: a lone `$?` inside `IN (…)` already covers every list

> one exact PostgreSQL statement; `$?` binds one value and `...` appears only in the two list forms below

`normalize` collapses an `IN` placeholder list "whatever its length", including a single placeholder (`normalize.rs:61-64`, test `:1066`: `WHERE id IN ($1)` → `WHERE id IN ($?, ...)`), and the grant's stored text is normalised (`crates/blastit-manifest/src/blastfile/lower.rs:687`). So a hand-written `IN ($?)` silently permits `IN ($1, $2, $3)`. The page currently implies the `...` is what grants a list. That is an understated permission, not a broken flow, hence MINOR — but the reference should not understate a grant.

Rewrite the row's second half and the table row:

> one exact PostgreSQL statement; outside an `IN`/`NOT IN` list, `$?` binds one value, and `...` appears only in the two list forms below; repeat the directive for more

> | `IN ($?)`, `IN ($?, ...)` | `IN ($1, $2, …, $n)` at any length: one grant covers every list |

### MINOR-2 — `blastfile.md:279-282`: "matched as written" beside "comments and whitespace are ignored"

> An exact `query` is matched as written and gets no table or column analysis. … comments and whitespace are ignored.

Two adjacent clauses disagree about what "as written" means (and `$1`/`$?` are also the same, per `troubleshooting.md:78`). Fold the normalisation into the first clause so the caution still reads.

Rewrite:

> An exact `query` is matched on the text you wrote, with comments, whitespace and `$n` normalised, and gets no table or column analysis. Read it before pasting: `pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file or network capabilities, not ordinary statements.

### MINOR-3 — `blastfile.md:289`: `9:15` anchors the empty string's quote, while the other rows anchor the interior

> | `query ''` | ``Blastfile:9:15: this `query` is empty; write the one statement it grants`` |

At 8 spaces the quote sits at 15 and the (empty) interior begins at 16; the other two rows anchor interior characters (24, 31), and `MB.34/NODE.md` pins the interior convention ("`blastit check` prints the same `9:16` (the `S` of `SELCT`)"). An implementer following `Position + inner offset` will emit 16. Use 16 here; if quote anchoring is intended instead, the reference must say so before MB.34.4 pins the span.

Rewrite:

> | `query ''` | ``Blastfile:9:16: this `query` is empty; write the one statement it grants`` |

### MINOR-4 — `blastfile.md:291`: the `...` refusal names `IN` but not `NOT IN`

> `` `...` is only valid in `IN ($?, ...)` and after a `VALUES` row ``

`normalize` treats `NOT IN` as the same list form (`normalize.rs:61-64`, test `:1070`), so `NOT IN ($?, ...)` is a legal grant shape; the refusal sentence, which the CLI prints verbatim, would tell a correct file it is wrong. Name both, here and in the template cell (MINOR-1's rewrite can stay `IN` if the cell says "and `NOT IN`").

Rewrite:

> `` `...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row ``

### MINOR-5 — `local.md:135`: the old "trusted as written" contract survives

> An exact `query` is trusted as written: paste only a statement you have read.

This is the sentence M4 removed from the reference, now contradicted by `blastfile.md:275` (a typo refuses at load). It still needs its security warning, but "trusted" now implies "unvalidated".

Rewrite:

> An exact `query` runs as written — it is parsed, not analysed: paste only a statement you have read.

### MINOR-6 — `local.md:149-155`: step 2 is three sentences but still two 40-word ones, and "the two setup statements" has no referent here

> … a squiggle for each syntax error, in the words `blastit check` prints, at the same `line:col` counted in the `Blastfile` …

Sentence 1 is ~36 words, sentence 2 is 39; the DX-first rule is short sentences, and an editor page is where a 5 pm reader stops. Split, say where the column starts, and name the exempt statements instead of counting them.

Rewrite:

> Open `./Blastfile`: colours, bracket matching, an outline, and a squiggle on each syntax error, with the words `blastit check` prints and the same `line:col` counted from the line's first character. A `query` gets its squiggle inside the string, on the SQL token. The Quick Fix **Parameterize SQL for Blastfile ($?)** turns a pasted literal into `$?` and an `IN ($1, …)` list into `IN ($?, ...)`; `SET TIME ZONE 'UTC'` and `set_config(…)` keep their literal. Hover over `$?` or `...` to see what each stands for. The extension starts `blastit lsp` itself, from `PATH` or through `uv run`.

### MINOR-7 — `local.md:149`: the editor section omits the SQL colouring the epic promises

> … colours, bracket matching, an outline, and a squiggle …

"Colours" reads as Blastfile syntax highlighting. The owner mandate and `MB.34.6` colour the SQL *inside* each `query` (keywords, `$?`/`$n`, `...`), and this page is the only user-facing place to say so. One clause, no new step.

Rewrite (insert after "on the SQL token."): `` Its SQL is coloured too: keywords, `$?`/`$n` and `...`. ``

### MINOR-8 — `ci.md:22`: "One failure has no row below" claims a uniqueness that is not true

> One failure has no row below: a `query` that does not parse.

A static `root` and a missing `pytest` also have no row (they are exit `2`, described later). Say what this paragraph is about — the parse refusal is not a finding — and make "It" name its subject.

Rewrite:

> A `query` that does not parse is not a finding and has no row below. The file does not load, so `check` exits `2`, writes the refusal to stderr (one line per problem, at `Blastfile:line:col`), and prints nothing on stdout: `--json` included, and an empty stdout is a refusal, never a clean audit. The refusal adds no `kind` and no `fix.action`; edit the line it names and rerun.

VERDICT: CHANGES
