# DX review — `check --learn` and the Single-Pass Promise

Lens: `dx`. Read: `docs/design/learn-single-pass.md`, `docs/VISION.md`,
`docs/ci.md`, `docs/local.md`, `docs/reference/manifest.md`,
`docs/reference/cli.md`, `docs/deploy/index.md`, plus `check.rs`,
`audit_v1.rs`, `edit.rs`, `model.rs` at `872f523`.

## Step count (the record's own §1 claim, checked)

| Env | Steps today | After (a) | Zero `sudo` |
|---|---|---|---|
| Local | 3: `check --learn` → `git diff` → `check` | same 3, **+1 full suite run per refusal round** (fold / comment / literal) | yes, unchanged |
| CI | 1: `check --json` (`ci.md:162`, read-only) | same 1 | yes, unchanged |
| Deploy | 1 manual checklist item (`deploy/index.md:111`) | same, unchanged | production needs root, by design |

No new step is added on the happy path. The friction this record adds is inside
step 1 (write ergonomics) and inside the refusal rounds.

---

## BLOCKER

### B1 — The human `learn:` line asserts a write that did not happen, and the record pins it unchanged

`check.rs:991` computes `clean = requests > 0 && refused.is_empty() && failure.is_none()`;
`check.rs:1006` prints `learn_line(...)` on stdout **regardless of `clean`** — i.e.
`learn: 2 routes, 38 queries added → …` when `would_write` was false and nothing
was written. Only the JSON document is honest (`R7`: "`wrote` … reflects the
actual write").

§5.5 then states: "`learn()`'s gates, exit codes (`CLEAN`/`FINDINGS`/`SUITE_FAILURE`),
and the `learn:` line's wording (`check.rs:957-1033`)" do **not** change, and
"`queries()` already sums every grant's entries, so the message becomes truthful".
It does not: the message is already false on every refusal, and (a) adds two
routine refusal sources — unparameterised statements (more grants per run) and
`R10`'s comment rule. The developer reads "added", runs `git diff` (empty), and
is told nothing on stdout.

Fix (in scope, one line): pass `wrote`/`clean` into `learn_line` and print
`learn: 2 queries proposed, nothing written (1 statement refused) → …` when the
plan was blocked; keep "added" only when `wrote`. Add it to `R7`.

### B2 — `learn_route` contradicts `R3`; a stale `Routed` name blocks the whole write

`R3`: "`Routed { route }` naming a route the manifest does not declare is
skipped, never invented." §10: "A `Routed` name the manifest does not declare is
skipped, not invented (R3)."

§5.3 returns it unguarded:

```rust
ObsTarget::Routed { route } => Some(route.clone()),
```

`Plan::build` then fails with `EditError::MissingRoute` (`edit.rs:571`; message at
`:270`: "a db grant names route \"x\", which is neither declared nor proposed; add
the route first"), `learn()` writes nothing (`check.rs:987`) and exits `1`. The
developer is shown an error naming a route they never typed, after a full suite
run — and §6.3's `R3` oracle ("skipped, no `MissingRoute` error") cannot pass
against this code.

Fix: `ObsTarget::Routed { route } => manifest.route(route).map(|r| r.name().to_string())`.
The trigger is real: a route renamed during the run, or `plan_learn_json` fed
observations older than the manifest.

---

## MAJOR

### M1 — The fold becomes the common path: unreviewable diffs, and one comment blocks every grant

`O4` states the fold ("folds a multi-line `queries` list onto one line by
design") but §5 only rules on the comment half. Two user-visible consequences are
left unruled, and (a) makes both routine (every existing route, not just new
ones):

* `queries = [ … 38 entries … ]` on one line — `edit.rs:864-889` splices
  `toml_string_array(&patch.grant.all)`. `ci.md:134` and `local.md:85` tell the
  developer to "review `git diff` before committing"; a 4 KB single-line diff
  cannot be reviewed, and `VISION.md` §Guiding Principle 2 ("read `blastit.toml`
  in 30 seconds") is gone.
* `R10`'s `CommentedQueries` is raised from `patch_grant`, so `apply_edits`
  returns `Err` and **no route is patched**: one hand-written comment inside one
  multi-line list blocks grants for the whole app, and the remedy text is "run
  `--learn` again" (another full suite).

Fix: when the array spans lines, append the fresh entries as new lines before the
closing `]` instead of folding; no comment is then ever in the splice span and
`R10` is unreachable for the common case. Keep folding only for one-line arrays.

### M2 — The all-or-nothing refusal gate is undocumented on every page the record claims to serve

`R7`/§5.5 keep "one refused statement still blocks the whole write". The pages
say the opposite:

* `local.md:104`: "A statement that inlines a value is refused, not written" —
  per-statement, true of `dev --learn` (`O5`), false of `check --learn`.
* `ci.md:132-134`: "refuses a statement that inlines a value: parameterise it
  first" — silent on "and then nothing at all is written".

Today few developers hit it (`check --learn` only learned new routes). After (a)
it is the primary path, and the record's own §3 condemns exactly this class ("a
concession paragraph is a defect with a changelog entry"). Fix: one sentence in
`ci.md` and `local.md` that also distinguishes the two commands — `dev` writes
the grantable subset and exits `1`; `check` writes nothing. Otherwise the same
sentence is true of one command and false of the other.

### M3 — `R9` claims a boot guarantee the editor does not implement

`R9`: "the resulting document boots (`Manifest::parse_and_validate`)." `verify()`
(`edit.rs:976-984`) calls `Manifest::from_toml_str`, which is `parse::parse` only
(`model.rs:610`); validation (`model.rs:618-628`) is never run on the edited text.

Concrete failure: a route that can never reach the app — `spa = true` with an
`app` set — may not declare a `[route.<proxy>]` grant (`manifest.md:144-147`, a
boot error). Such a route can have a listener **today** through a global `groups`
grant (`manifest.md:314`), so its statements are observable; `learn` then writes
`[route.site.db]` (`edit.rs:811`, `append_grant_table`) and the next `blastit dev`
refuses to boot on a line `--learn` authored. That is the "green learn, red boot"
whack-a-mole the project forbids.

Fix: `verify()` also calls `manifest.validate_default()` and maps a failure to a
named `EditError`; make that the `R9` oracle (§6.3's current oracle is
`apply_edits` then `evaluate`, which boots nothing).

### M4 — Mismatch 4's safety premise is a checklist, not a gate; the ruling's own condition (i) is already met

§7.2 justifies warning-only `allow_raw` because "every shipping path runs
`check`". What was verified is `docs/deploy/index.md:111`, an unchecked `- [ ]`
box — and no deploy preset runs `check` at all (`grep -rn "blastit check"
docs/deploy/` finds only that line and a prose reference on `:56`). DigitalOcean
App Platform is the one-click path the ruling itself names as the risk; there is
no pipeline there to run the checklist.

So condition (i) — "any supported deployment path builds an image and runs
`blastit` with no `check` in the pipeline" — is true, and by the ruling's own rule
the minimal fix (Stage-0 **error** in `Production` only, leaving `dev`'s warning
and `check`'s finding intact) is due, not deferred. DX side: `allow_raw` has "no
request-time meaning at all", yet `manifest.md:104-106` ("`raw` also needs
`allow_raw = true`") reads like a gate. Fix: close (i) in this record, schedule
the production error, and add one sentence that `allow_raw` is an acknowledgement
for review, not a runtime switch.

### M5 — Mismatch 3 leaves two live defects with no interim signal, one of them silent

Ruling: "today the failure is a refused-but-valid configuration (annoying) or an
accepted-but-dead one (silent), never an unsound one." Both are DX-broken, and
the record that condemns "a page that describes a capability the product does not
have" (§3) leaves `manifest.md:20-22` saying exactly that for 0.2.0:

* `app` beside all-`spa` static routes is **accepted and dead** — while the same
  route shape **refuses** `[route.ingress]`, `prefork` and grants with "nothing
  could ever run it" (`manifest.md:144-147`). The developer is told a satellite is
  dead but not that `app` is; requests `404` (`manifest.md:135-139`).
* `BLASTIT_APP` beside a forwarding (`spa = false`) static route is **refused**
  with a false sentence ("every route is static, so no route can reach an app"),
  with no documented workaround.

Deferring the code is defensible; deferring *every* signal is not. Minimum for
0.2.0: emit a boot warning for an `app` no route can reach (the suggested
`no_route_reaches_the_app` predicate), and either land leaf 8 in-release or ship a
one-line correction of `manifest.md:20-22`. Note leaf 8's blast radius:
`manifest.md:22`, `:144-147` and the `BLASTIT_APP` message must move together, or
the page certifies the asymmetry from the other side.

### M6 — Exit codes and dry-run contents are unspecified, so reviewing costs a write or a second suite

`R7`/`R8` are silent on the code a `--learn` run returns; §5.5 says only "exit
codes … unchanged". Enumerated from the code, and needed by any script or agent:
successful write `0`; fixed point `0`; any refusal or zero requests `1`
(`check.rs:991-1011`); manifest error `2`; suite failure `3`. Say it.

Worse, `ci.md:121` and `cli.md:50` sell `--dry-run` as "print the proposal", while
`learn_line` prints counts only (`check.rs:1022-1026`). After (a) the proposal *is*
grant text on existing routes, so reviewing before writing requires either writing
the manifest or a second full suite run. Fix: print each proposed statement to
stderr under `--dry-run` (stdout stays one JSON document), or drop "print the
proposal" from both pages.

---

## MINOR

### m1 — Mismatch 6 is honest but leaves a silent no-op with no signal

`manifest.md:132` already says "`*` stops at `/`", so the book is right. Leaf 7
adds `**`; nothing adds a signal for an `immutable` pattern that matches no file
under `root` — which is the stated defect ("costs the developer a debugging
session and gives no signal"). Add the eight-word interim clause to
`manifest.md`'s `immutable` row in leaf 6 ("one pattern per depth until `**`
lands") or a boot warning in leaf 7.

### m2 — The headline `learn:` line leads with the number that did not change

§6.1 shows `learn: 0 routes, 2 queries added`; `check.rs:1028` also prints
"1 routes" (`:1366` asserts it). After (a) the common case is `0 routes`. Suggest
`learn: 2 queries added to 1 route → …` (drop the routes count at zero); keep the
JSON keys exactly as `R8` pins them.

### m3 — `R10`'s error must be 1-based and match the house wording

§5.4 scans `assignment + 1 .. stop` (0-based `lines` indices) and prints
"line 13"; the file's other errors read `route "products"'s db grant …`
(`edit.rs:230-300`). Specify `line + 1` and the house phrasing, otherwise the
developer is sent one line off — in a message whose whole job is to point at a
line.

### m4 — §1's Local row overpromises "no findings"

"one suite run produces a `blastit.toml` diff the next `check` accepts with **no
findings**" — `--learn` only removes `missing_grant`; `dead_grant`,
`unexercised_route`, `proxy_unused` and `raw_ingress` survive (`ci.md:20-33`).
Say "no `missing_grant`", or the table's own promise becomes a support ticket.

### m5 — Leaf 6's doc list is conditional, which is how the last concession survived

"`docs/local.md` **if** its `learn:` example shows a route-only write". Make the
four pages explicit — `ci.md` (all-or-nothing gate, the new grant case),
`local.md` (same, plus the `dev`/`check` gate difference), `manifest.md:366-374`,
`cli.md:49` — and fold B1/M2's sentences into that leaf.

---

## Verdict on the three rulings

| Ruling | Safe? | Honest? | Note |
|---|---|---|---|
| 3 | no | no | two live defects, one silent, page still false (M5) |
| 4 | no | mostly | safety rests on an unchecked box; condition (i) already true (M4) |
| 6 | yes | yes | accurate page, no signal; needs the 8-word interim clause (m1) |

---

## Round 2 — re-review after the reshape

Re-read the same file at `872f523` plus `ci.md`, `local.md`,
`deploy/index.md:111`, `reference/{cli,manifest}.md`, and `check.rs`'s
`learn()`/`learn_line`/`emit_hints`. Round 1 stands as written; nothing below
reopens a resolved finding.

### Round-1 findings, one line each

| # | Status | Evidence |
|---|---|---|
| B1 | **RESOLVED** | §5.5's "One line changes" table (`added` / `proposed (dry-run)` / `proposed (not written)`), R13, §6.3's R13 oracle, leaf 2. Note §5.5 does not name the signature: `learn_line` needs `wrote`/`failure`, not just `dry_run`. |
| B2 | **RESOLVED** | §5.3 guards the arm (`manifest.routes().any(...).then(...)`); R3; §6.3's R3 oracle (`queries() == 0`, no `MissingRoute`). |
| M1 | **RESOLVED** | §5.4 extends by insertion; R10; leaf 1 lands first. Both consequences (4 KB line, comment inside the splice span) are gone for the multi-line shape. New spec contradictions: N1. |
| M2 | **NOT RESOLVED** (downgraded) | ci.md gets the gate sentence (§5.6); `local.md` does not, and §5.6 asserts "`docs/local.md` needs no change". `local.md:83` is the first place `check --learn` appears and `:104`'s "refused, not written" sits inside the `dev` paragraph. Downgrade reason: R13's "(not written)" line now signals it at the moment. |
| M3 | **NOT RESOLVED** | R9 is honest now, but swaps the guarantee for "the *acceptance test* additionally proves the result passes `Manifest::parse_and_validate`" — no §6.3 oracle and no §11 row names that test (R1/R2's oracle is still `apply_edits` then `evaluate`). The red-boot shape is live: `manifest.md:143-147` forbids `[route.<proxy>]` on `spa = true`, `:313-314` gives that route a listener through a **global** group, so its statements are observable and `learn` writes the forbidden table. |
| M4 | **RESOLVED** | §7.2 + R16 (Stage-0 `Production` error) + leaf 9 + ADR-0011, independent of `deploy/index.md:111`, which is still the only `check` in `docs/deploy/`. Doc residue: N4. |
| M5 | **NOT RESOLVED** (downgraded) | §7.1 now schedules the interim signal (warning + corrected `BLASTIT_APP` message) and the `manifest.md:20-22` correction in leaf 8 — but leaf 8 is "*(parallel, not blocking)*", so 0.2.0 can still ship the accepted-but-dead `app` beside a page that calls it a boot error, which §3 names "the worst defect class we ship". |
| M6 | **NOT RESOLVED** | Exit codes are stated (§5.6); `--dry-run`'s "print the proposal" (`ci.md:121`, `cli.md:49`) is untouched and no leaf prints statements. The middle command of `ci.md:120-123` still reveals two integers. |
| m1 | **RESOLVED** | §7.3 adds "`**` crosses `/`" and "a pattern that matches no file is not reported" to the `immutable` row. Path unassigned: N4. |
| m2 | **NOT RESOLVED** | §5.5's table and §6.1 keep `learn: 0 routes, 2 queries added`; the count that never changes still leads the line. |
| m3 | **RESOLVED** | Moot: the multi-line path no longer errors, so no line number is printed. The rebuild fallback has no comment policy (N1). |
| m4 | **RESOLVED** | §1 now reads "no findings *that `--learn` can write* (transaction control is the one named exception, §7.4)". |
| m5 | **NOT RESOLVED** | Leaf 6 is explicit but *narrower*: "`docs/ci.md` — the sentences quoted in §5.6, verbatim (`docs/local.md` needs none)". `cli.md:49`, `local.md`'s gate sentence and `manifest.md`'s "Writing one" paragraph have no leaf. |

### Step count, re-counted

| Env | Steps | Change from round 1 | Zero `sudo` |
|---|---|---|---|
| Local, autocommit app | 3: `check --learn` → `git diff` → `check` (`local.md:83-87`) | none | yes |
| Local, non-autocommit (psycopg3 default) | 4: +1 hand edit, `transaction_control = true` in one `[route.db]` | new step §7.4 admits; §1's exception covers the *claim*, no page carries the *step* | yes |
| Local, one refused statement | 5: +1 bind the value, +1 re-run `--learn` | unchanged rule, documented in ci.md only | yes |
| CI | 1: `check --json` (`ci.md:162`) | none | yes |
| Deploy | 1 box (`deploy/index.md:111`) | none: R16 adds no step to a manifest `check` passed, and a key removed afterwards now names route + key instead of serving raw | production is root by design |

No new happy-path step; §1's table is accurate.

### §5.6 against the pages as they read

Quote targets verified verbatim: `ci.md:116-117` ("`--learn` discovers routes and
statements") and `ci.md:132-133` ("writes exact `queries` (never table grants)");
`missing_grant`'s row is `ci.md:25`; the hints really do go to stderr
(`emit_hints`, `check.rs:971`). Exit-code sentence matches `ci.md:98` except its
new parenthetical (N2). `local.md` is the mismatch (MAJOR-3).

### Open findings

#### MAJOR

**MAJOR-1 (was M3) — R9's guarantee is an assertion with no oracle, and the red-boot shape is unbuilt.** §6.3's R1/R2 oracle stops at `evaluate`; nothing in §6.3 or §11 boots the edited text, so "the *acceptance test* additionally proves … `parse_and_validate`" is unenforced. Minimal fix: name the test in §6.3 (`apply_edits` → `Manifest::parse_and_validate` → `Ok`) and add the `spa = true` + global `group` fixture to leaf 5, or make `verify()` validate and map the failure to a named `EditError`. Otherwise the one manifest shape that reaches it (`spa = true`, a global db group, statements observed) learns a grant that refuses the next boot.

**MAJOR-2 (was M6) — `--dry-run` still lies on two user pages, and the record declines to fix it.** `ci.md:121` and `cli.md:49` say "print the proposal"; `learn_line` prints counts and §5.5 freezes that. After (a) the proposal *is* grant text on existing routes, so the page's middle command reveals nothing. Fix: print each proposed statement to stderr under `--dry-run` (stdout stays one JSON document), or strike "print the proposal" from both pages in leaf 6.

**MAJOR-3 (new) — §5.6's "`local.md` needs no change" contradicts §7.4.** §7.4 admits the one residue and says "The CI page must say it (§5.6)" — but `local.md:80-88` is the page that introduces `check --learn`, and its `:87` output is `blastit check: … clean`, which a non-autocommit psycopg3 app (the default §7.4 itself names) can never reach: the `BEGIN` statement stays `missing_grant`. `local.md` never mentions `transaction_control`. Same one sentence as ci.md, same words, added to the `local.md` block.

#### MINOR

* **N1 — R10 and §5.4 disagree about the bytes they promise not to touch.** §5.4 step 2 appends a comma to the last element line when it lacks one, while R10 and §6.3 promise "every existing line (entries, … trailing comma style) is byte-identical" and "entry lines and their quoting are byte-identical". The oracle as written cannot pass. Also, the rebuild fallback (closing bracket sharing a line with an element) "moves existing text" with no stated comment policy — the exact silent-comment loss §5.4 exists to prevent. Say "element lines may gain a trailing comma" and "the rebuild fallback refuses when it would drop a comment".
* **M2 — the all-or-nothing gate is still only on the CI page** (§5.6). Downgraded from MAJOR; one clause in `local.md:105` ("`dev` writes what it can; `check` writes nothing") closes it, and it is the `dev`/`check` difference round 1 asked for.
* **M5 — leaf 8 is "not blocking".** The interim warning and the `manifest.md:20-22` correction should be a blocking half of leaf 8 (it is prose plus a warning), or the record should say 0.2.0 ships a page it knows is false.
* **m2 — the `learn:` line still leads with `0 routes`.** `learn: 2 queries added to 1 route → …` (drop routes at zero); `R8` keys are untouched by wording.
* **m5 — leaf 6 is ci.md only.** Add `docs/reference/manifest.md` (§7.3's `immutable` row, R16's production sentence at `:104-106`) and `cli.md:49`, or N4 below recurs every milestone.
* **N2 — §5.6's exit-code clause contradicts R6/R13.** "`1` findings (a refusal or a 'not written' plan included)" — but a fixed point (R6) prints "(not written)" *and* exits `0`. R13 deliberately separates the two non-writing outcomes; the sentence leaf 6 copies verbatim collapses them. Say "`0` clean, including a run with nothing left to learn; `1` a refusal".
* **N3 — the transaction sentence is both incomplete and duplicative.** It names "`BEGIN`, `COMMIT` or `SAVEPOINT`" while `transaction_label` also reports `ROLLBACK` and `RELEASE` (`audit_v1.rs:778-791`) — a bare `ROLLBACK` is what a failing test leaves behind. And the write-gate sentence repeats `ci.md:133-134`'s existing "parameterise it first". One sentence each.
* **N4 — three doc edits have no leaf path.** Leaf 7 omits `docs/reference/manifest.md` (its own docs half); leaf 9 omits R16's user-facing sentence (`manifest.md:104-106`); nothing owns `cli.md`. Leaf 6 owns exactly one page.

Verdict: the decision itself is sound and the two round-1 BLOCKERs are gone; what
remains is three user-page promises the record leaves untrue (MAJOR-1..3) and
bookkeeping that would have caught them (N4, m5).

---

# Round 3 — verdict

**FAIL.** Re-read the whole record at round 3 plus `ci.md:98,116-134`, `cli.md:49`,
`local.md:80-105`, `check.rs:980-1033`, `edit.rs:788-890`, `docs/decisions/`.
Nothing below reopens a resolved finding. 4 MAJOR, 9 MINOR, 0 BLOCKER.

## Order this must be fixed in

`§5.4`'s comma rule (MAJOR-4) and `§5.5`'s exit row (MAJOR-5) are one-line
corrections; `QueriesShape` (MAJOR-6) and the R9 oracle (MAJOR-7) need a sentence
each. All four are cheaper now than after leaf 1/2 land.

## MAJOR

**MAJOR-4 (new, §5.4 step 3) — the "ends with `,`" test is comment-blind; a legal
last element line produces invalid TOML or mutates the author's comment.**
Step 3: "if `last` does not end with `,` … append one to it. This is the **only**
edit to an existing line". But an element line is defined as any line whose first
non-space byte is not `#`/`]` — so `    "SELECT 2" # recent` *is* the last element
line. It does not end with `,`, so the comma is appended before the newline:
`"SELECT 2" # recent,` — the comma lands **inside the comment**, the array element
never gains its separator, and `verify()` (`edit.rs:976-1000`) then fails on
unparseable TOML. The developer gets a bare parse error from `apply_edits`, after
a full suite run, with no route, no proxy and no `QueriesShape` remedy — the exact
"error you cannot act on" §5.4's refusal exists to avoid. The `"SELECT 2", # recent`
spelling is worse in the other direction: `ends_with(',')` is false, so the same
append mutates the comment text to `# recent,` while R10 promises "every existing
line is byte-identical except … that one line gains a `,`". Fix: strip a trailing
`#`-comment (string-aware, the way `trailing_comment` at `:875` already does)
before testing and before appending, and state in R10 that a trailing comment is
preserved verbatim.

**MAJOR-5 (new, §5.5 table) — the four-state table's `--dry-run` row is false
against the code it says does not change.** Row 2 is "`any` | `false` | `0` |
`--dry-run`". `check.rs:991` computes `clean = plan.requests > 0 &&
plan.refused.is_empty() && failure.is_none()` and returns `FINDINGS` otherwise
(`:1010`), independently of `dry_run`. So `check --learn --dry-run` on a suite
with one unparameterised statement prints
`{"proposed":{"routes":0,"queries":0},"wrote":false}` and **exits `1`** — which the
table assigns to row 3, and whose key (`0,0`/`false`) row 1 also claims with exit
`0`. The table therefore cannot "disambiguate the four states", and §5.6 copies
the consequence onto `ci.md` verbatim. Fix: row 2 is "`--dry-run`, plan clean →
`0`"; a blocked dry run is row 3, and say so, or a CI job that gates on
`--dry-run` fails for a reason the page denies.

**MAJOR-6 (new, §5.4 step 2) — `QueriesShape` recapitulates M1's cost and no page
mentions it.** The refusal fires on a plausible hand-format (`queries = [\n
"a",\n "b"]`, which `taplo`-style formatters emit when the closing bracket shares
a line); `patch_grant`'s `Err` aborts `apply_edits`, so **one** such list blocks
every grant in the app (R15's precedent), and the remedy text ends "and run
`--learn` again" — a second full suite. That is round 1's M1 ("one hand-written
comment blocks grants for the whole app") with a comma instead of a comment, and
`§5.6`/leaf 6 give it no sentence, so the developer meets it as an unexplained
failure at the end of step 1. Fix: one clause in `ci.md`'s write-gate paragraph
("a `queries` list whose `]` shares a line with its last entry is refused — put
the bracket on its own line"), and add "nothing was written" to the
`QueriesShape` message, which today names the route but not the proxy it carries
nor the fate of the other routes.

**MAJOR-7 (round-2 MAJOR-1, still open) — R9's oracle is named but cannot fail on
the shape that breaks.** §13.1 says "accepted → … `parse_and_validate` is an §6.3
oracle"; §6.3's R1/R2 row now boots the edited text. But the only manifest that
reaches the red-boot path is `spa = true` **plus a global `group`** grant (its
route has a listener and observable statements, and `manifest.md:143-147` forbids
`[route.<proxy>]` on it) — and no fixture, leaf 5 task or §6.3 row contains that
shape. The happy-path assertion passes while `learn` still authors a table the
next boot refuses. Fix: name the fixture in §6.3's R1/R2 oracle row (or make
`verify()` validate and map it to a named `EditError`), as round 2 asked.

## MINOR (surviving round-2 items, with the section that claims otherwise)

* **N1** — R10 now states the comma exception, but §6.3's R10 oracle still reads
  "a multi-line list with … a missing trailing comma → the comment line, the entry
  lines and their quoting are byte-identical". With a missing comma one entry line
  is not byte-identical, so the oracle cannot pass. Say "the last element line may
  gain a trailing comma".
* **MAJOR-2 half (was M6)** — §5.6 chooses the `cli.md:49` row and then asserts
  "so `ci.md`'s 'print the proposal' phrase is not left standing", but supplies no
  replacement text and leaf 6 copies only "the sentences quoted in §5.6". The
  phrase it must kill is the console comment at `ci.md:123`
  (`--dry-run    # print the proposal, write nothing`). Quote it.
* **N2** — §5.6's new last row, "`1` with the reason on stderr when it wrote
  nothing", is false for `--dry-run` (writes nothing, exits `0`) and is the
  sentence leaf 6 copies verbatim. Round 2's fix ("`0` clean, including a run with
  nothing left to learn; `1` a refusal") was not taken.
* **M2** — `local.md` still has no `dev`/`check` gate difference; §5.6's `local.md`
  paragraph is transaction-control only, and `local.md:105`'s "refused, not
  written" sits in the `dev` paragraph.
* **M5** — leaf 8 is still "*(parallel, not blocking)*", so 0.2.0 may ship the
  accepted-but-dead `app` beside `manifest.md:20-22`, which §3 calls the worst
  defect class we ship.
* **N4** — R16's user-facing sentence ("refused without the acknowledgement",
  §7.2) is still unowned: leaf 9's paths have no `docs/`, and leaf 6's
  `manifest.md` item is the transaction sentence only.
* **C (new, §5.2)** — the rewritten block cites `R25`, `R29`, `R24`, `R33`; the
  document defines `R1..R21` (and `R27` in §3/§8/§9). A cheap implementer cannot
  resolve them. Renumber to the R1..R21 set or add the missing rows.
* **D (new, §12 leaf 8)** — "step 1 (interim, **no behaviour change**): … and an
  `app` no route can reach is a warning". Emitting a new `manifest_warning` event
  is a behaviour change, and the leaf's acceptance ("agree") cannot be false while
  the code is unchanged. Drop the claim.
* **E (new, R21)** — R21's prose ("every route is `[route.php]`, or
  `[route.static]` with `spa = true`") omits the `[route.static]` + `[route.php]`
  (non-`spa`) shape that §7.1's predicate rules unreachable (`_ => true`) and that
  §7.1's own acceptance enumerates. A test written from R21 alone misses one of
  the five shapes.

## Step count, round 3

| Env | Steps | Change |
|---|---|---|
| Local, autocommit | 3 | none |
| Local, non-autocommit | 4 (+`transaction_control`) | none — `local.md` now carries it |
| Local, one refused statement | 5 | unchanged rule, still `ci.md`-only |
| Local, `QueriesShape` shape | 6 (+reformat, +re-run suite) | **new** (MAJOR-6) |
| CI | 1 | none |
| Deploy | 1 box | none |

Zero `sudo` everywhere; production stays root-by-design.
