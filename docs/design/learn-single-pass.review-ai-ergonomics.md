---
lens: ai-ergonomics
document: docs/design/learn-single-pass.md
status: findings
---

# AI-ergonomics review — `learn-single-pass`

Scope: an LLM agent asked to populate `blastit.toml` for a Flask/Django app,
using only the interfaces this doc promises (§1 Agent row, §5.2/§5.4 text, §6
acceptance). Checked against `docs/reference/manifest.md`, `docs/ci.md`,
`docs/local.md`, and the cited code.

Counts: **BLOCKER 3 · MAJOR 5 · MINOR 2**

---

## BLOCKER

### B1 — `--learn --json` cannot report *why* nothing was written; §6.1's own oracle cannot tell success from a block

§1 promises `--learn --json` → apply → `check --json` "terminates in one pass".
R8 pins the document to `{"proposed":{"routes":N,"queries":M},"wrote":bool}` and
refusal detail goes only to stderr (`check.rs:972-974`). So the same stdout
bytes mean four different things:

- dry run, proposal non-empty → `{"proposed":{"routes":0,"queries":3},"wrote":false}`
- refusal-blocked (`would_write` false), proposal non-empty → **identical**
- `write_atomic` failure → `{…,"wrote":false}`
- fixed point / zero-request suite → `{"proposed":{"routes":0,"queries":0},"wrote":false}`
  (§6.1 prints exactly this as the success case)

An agent parsing stdout (the documented agent interface: ci.md "`--json` writes
one document to stdout and nothing else") therefore cannot terminate, and if it
polls `check --learn --json` until `wrote:false` it terminates on a blocked run.
Exit codes help only partially (`clean = requests > 0 && refused.is_empty() &&
failure.is_none()`, `check.rs:991`): they do not separate a refusal from a failed
write, and neither this doc nor ci.md's exit-code table states that protocol.
§6.1's "fixed point (R6)" assertion is consequently not an oracle.

Suggestion: R8 must be reopened. Add `"refused":[{"route","proxy","sql"}...]`
and `"hints":[...]` (or at minimum `"blocked": true`), state in ci.md that
`wrote:false` alone is not success, and add a §6.1 twin: one refused statement +
three grantable ones → assert a distinguishable document. Without that an agent
can verify "zero grants forgotten" only by counting, never by cause.

### B2 — the repaired transaction-control hint is still an instruction that does not boot

§5.2 emits:

> `transaction control is not granted automatically: set transaction_control = true on [route.{route}.{proxy}]`

and R11/§6.3 assert the string contains `[route.<name>.<proxy>]`. Routes are
`[[route]]` array-of-tables; `[route.notes.db]` is TOML-legal but parses as an
unknown key `notes` **inside the route element**, not as a grant (verified with
`tomllib`), and `crates/blastit-manifest/src/parse.rs` states "unknown keys are
errors". An agent pasting the hint therefore gets a parse error — the exact
failure O3 identified, re-spelled. The cited `docs/reference/manifest.md:239` is
in fact `[group.session.db]` + `transaction_control = true`; the route example
below it uses `[route.db]` (current element). §6.3's R11 oracle asserts the same
wrong substring, so the test certifies the defect.

Suggestion: emit a snippet, not a coordinate — "on route `{route}`, add
`transaction_control = true` to its `[route.db]` table" — or propose the group
form the reference actually shows (`[group.tx.db]` + `groups = ["tx"]`). Oracle:
parse the emitted snippet into a `Manifest` and assert `transaction_control()`,
never substring-match.

### B3 — a tx-heavy app is never clean after `--learn`; the machine channel offers only `observe`, so `unrestricted = true` is the shortest green path

R1 moves `is_tx` statements into a hint, but `evaluate` still reports them:
`(transaction_label(…), Fix::observe())` (`audit_v1.rs:677-682`). For any driver
that sends `BEGIN`/`COMMIT` (Django, SQLAlchemy with explicit transactions), the
next `check --json` carries a `missing_grant` with `fix.action = "observe"` —
contradicting §1 "the next `check` accepts with **no findings**" and §6.1
"clean" (whose fixture has no tx statement, so it cannot catch this).
`fix.action = observe` names no manifest edit; `transaction_control` appears
only in the learn hint, which is stderr-only and discarded by an agent reading
JSON. `Allowlist::unrestricted()` allows everything, so `unrestricted = true` is
the one edit that turns the finding green. That is the strongest `unrestricted`
temptation in the document, and §10 ("`unrestricted` is never approached") does
not consider it.

Suggestion: make the tx finding carry `fix.action = "set_transaction_control"`
with `fix.to` naming the route's grant table (mirroring the hint), surface an
`unrestricted` line in `check` so the loop cannot "fix" the audit that way, and
give §6.1 a `BEGIN` statement.

---

## MAJOR

### M1 — §5.4's error text is ambiguous and its remedy is a bare flag

> `route "products": the queries list has a comment on line 13; put the list on one line (or move the comment above it) and run --learn again`

Four defects for an automated applier: (i) `run --learn again` is not a command —
the command is `blastit check --learn`; an agent may run `--learn` and fail.
(ii) `line 13` names no file and no anchor (manifest line? line inside the
route block? the line shifts as the agent edits). (iii) the message omits
`proxy` although `EditError::CommentedQueries { route, proxy, line }` carries it
and one route can declare several proxy grant tables. (iv) it says nothing about
whether anything was written (nothing was), so the agent must re-run to discover
state, and it reports only the *first* offending list — comments in three routes
cost three full suite runs.

Suggestion: emit `route "products" [route.db], blastit.toml:13: the queries list
has a comment; nothing was written. Put the list on one line, or move the
comment above it, then run blastit check --learn.` Collect every offending list in one
error (or reuse B1's JSON `refused` channel). Consider appending to the
multi-line array instead of refusing at all: the fold is a formatting decision
the tool makes and the agent then has to undo.

### M2 — refusals carry no location and block the whole run

§5.2 refuses with `format!("{}: a redacted statement", normalized)` — no route,
no proxy, no observation target — and R7 keeps the all-or-nothing gate. An agent
with 37 grantable statements and one string literal in the app gets `wrote:false`,
a stderr line whose only locator is SQL text it must grep the suite log for, and
no partial result. That is precisely the state that makes hand-editing 37 grants
or `unrestricted = true` attractive.

Suggestion: refusal entries `{route, proxy, sql, reason}` in stdout JSON (B1),
and print `learn: 37 grantable, 1 refused, nothing written` so the gate and its
cause are visible. The stricter gate is fine; the blindness is not.

### M3 — the docs still teach the second writer this design rejects

§3 rejects `dev --learn` for agents because "there is no click-through at all",
but §12 leaf 6 updates `docs/local.md` only "if its `learn:` example shows a
route-only write". `docs/local.md`'s "Fill the manifest in one pass" section
currently shows `check --learn` **and** `dev --learn` under that one heading,
with the served-server path as the no-suite fallback. Nothing in §6 asserts the
page stops presenting `dev --learn` as a grant writer, so the "no second `dev`
session" claim in §1 is untested.

Suggestion: leaf 6 becomes unconditional — local.md: `check --learn` is the only
path that fills grants; `dev --learn` is for projects with no suite and never for
CI. Add a `make docs-test` anchor asserting the sentence.

### M4 — `learn_route` does not implement R3

R3: "`Routed { route }` naming a route the manifest does not declare is skipped,
never invented". §5.3 returns `Some(route.clone())` for **any** `Routed`,
ignoring `manifest` and `plan`; only the `Unrouted` arm consults them. A stale or
renamed route name from the bridge (`_blastit.plan_learn` does not resolve
targets, O2) then groups a grant for a route with no table in the manifest, and
`apply_edits` fails or invents a route — an error an agent cannot map back to a
fix. §6.3's R3 oracle ("skipped, no `MissingRoute` error") will fail against the
code shown, so the acceptance as written is not satisfiable.

Suggestion: `ObsTarget::Routed { route } => manifest.route_by_name(route)
.map(|r| r.name().to_string())`, or state explicitly that an undeclared
`Routed` is a startup error and drop the "skipped" claim.

### M5 — §6.1's diff assertions are not mechanisable

`git diff --stat` cannot show "two added entries and the existing `SELECT 1`
still present"; that requirement is prose only ("The `git diff` must show
**two** added entries"), and the "clean" assertion is the pretty line
`blastit check: 2 requests, 3 statements, 1/1 routes, clean`, not `--json`. An
agent cannot assert any of it without inventing a TOML parser step the doc does
not give.

Suggestion: acceptance commands, e.g. `uv run blastit check --json | jq -e
'.ok and (.findings|length==0)'`, then `uv run python -c 'import tomllib;
d=tomllib.load(open("blastit.toml","rb")); q=d["route"][0]["db"]["queries"];
assert q==["SELECT 1","SELECT 2","SELECT 3"]'`, then `git diff --exit-code`.

---

## MINOR

### N1 — two JSON dialects, one versioned

R8 blesses a document with "no `schema_version`" while `check --json` opens with
`"schema_version": 1` (ci.md). An agent that validates CLI documents uniformly
must special-case `--learn`, and there is no way to detect a future shape change.

### N2 — §1's Agent row implies a manual apply step

`` `--learn --json` → apply → `check --json` `` — `--learn` writes the manifest
(§2 F1), so "apply" does not exist; as written it suggests the agent should
hand-apply a patch, which §9 rejects. Rewrite as "`--learn --json` (writes) →
`check --json` (`ok:true`)".

---

# Round 2 — ai-ergonomics (reshaped record)

Re-read: the whole document at its current `In review` state, plus
`crates/blastit/src/{check.rs,audit_v1.rs}` and
`crates/blastit-manifest/src/edit.rs` at the same commit. Round 1 is unchanged
above.

## Round-1 verdicts (one line each)

| # | Verdict | Why |
|---|---|---|
| B1 | **NOT RESOLVED** (narrowed) | The rule exists in R8 but is *wrong* (see 2 below), is not written into `ci.md` (§5.6 enumerates leaf 6's sentences and omits it), and the third cause — zero requests — still emits no stderr line at all. |
| B2 | **RESOLVED** | O3 + R11 + §5.2 emit `[route.<proxy>]` and name the route in prose; §6.3's R11 oracle asserts `[route.db]` and the absence of `[route.<name>.`. |
| B3 | **NOT RESOLVED** | §7.4/R14 now *admit* the residue, but `evaluate` still emits `Fix::observe()` for an `is_tx` denial and no oracle asserts what the post-write `check` says (3 below). |
| M1 | **RESOLVED** | §5.4 replaces the fold-and-refuse path with element-line insertion (R10); the ambiguous message is gone. One narrow case returns (8 below). |
| M2 | **NOT RESOLVED** | §5.2 still `format!("{}: a redacted statement", normalized)` — no route, no proxy, no target — and it is now the *only* carrier of cause (4 below). |
| M3 | **NOT RESOLVED** | §5.6 says `docs/local.md` needs no change; `docs/local.md:94-96` still shows `dev --learn` writing grants under the same heading. |
| M4 | **RESOLVED** | §5.3's `Routed` arm now checks `manifest.routes()`, matching R3 and §6.3's oracle. |
| M5 | **NOT RESOLVED** | §6.1 still asserts "two added entries … `SELECT 1` still present" in prose; §6.2's `ok: true` covers only the follow-up `check`. |
| N1 | **NOT RESOLVED** | R8 keeps "no `schema_version`"; the agent must still special-case `--learn`. |
| N2 | **NOT RESOLVED** | §1's Agent row still reads `` `--learn --json` → apply → `check --json` `. |

## Still open (graded)

### 1. **BLOCKER** — a zero-request suite is a silent exit `1`; the agent loop cannot terminate

`learn()`'s `clean` is `plan.requests > 0 && plan.refused.is_empty() && failure.is_none()`
(`check.rs:991`), and `--learn` returns before `emit_report`, so the
`no_observations` finding the doc relies on (`R35`) never reaches the reader in
learn mode. On a suite that hits nothing (the exact state of an app whose
manifest an agent is bootstrapping):

```console
$ uv run blastit check --learn --json ; echo "exit=$?"
{"proposed": {"routes": 0, "queries": 0}, "wrote": false}
exit=1
```

stderr is **empty**: no refusal, no hint, no `failure`, and `learn_line` is
printed only `if clean` (`check.rs:1000-1004`). The agent sees the same bytes as
a fixed point minus the exit code, and `--learn` emits no audit report from
which to learn why. Rerunning is a no-op forever. §5.5's table lumps this with
"refused, zero requests, write failed" — three concurrent remedies behind one
line and one exit code, and under `--json` not even the line.

Fix: print the "not written" line **and** a cause to stderr whenever the run is
not clean (one call site, not one function), naming the zero-request case
(`learn: nothing observed: the suite made 0 requests`). R13's oracle must assert
the *call site* through `check`, not `learn_line` in isolation.

### 2. **MAJOR** — R8's disambiguation rule is false, and §13 (its only home) is empty

> "`wrote == false` with exit `0` can only be a dry run" (R8)

R6 says the fixed point is "a second `check --learn` … proposes `0` routes and
`0` queries, exits `0`", and `clean` sees `requests > 0`, no refusals, no
failure → exit `0`, `wrote: false`. So `wrote:false`+exit `0` is **two** states,
separated only by `proposed.routes + proposed.queries`. The rule must read:
*exit `0` means nothing is left that `--learn` can write; a nonzero
`proposed` count means `--dry-run`.* R8 cites "§13's rejected B1" and §13 is
`(Filled after the goldfish rounds; one row per finding.)` — the rule the whole
agent loop leans on is a parenthesis pointing at an empty section. Write the
rule in R8, in §5.6's leaf-6 list, and in `ci.md`.

### 3. **MAJOR** — the bridge path still drops refusals and reports intent as `wrote`

§5.5 keeps `LearnPlan::json()` "unchanged"; it is
`"wrote": self.would_write(dry_run)` (`audit_v1.rs:1294`). The CLI builds its
own document from the *actual* `wrote` (`check.rs:996-1002`), so the two
`--learn --json` documents of the same run disagree, and R7 ("`wrote` …
reflects the actual write, not the intent") is false for
`_blastit.plan_learn`, which never writes at all and can still say
`"wrote": true`. Worse, `plan_learn_json` prints `emit_hints()` but **never
`plan.refused`** — the bridge has no exit code and no refusal stderr, so R8's
answer ("the exit code and stderr carry the refusal detail") is unavailable
there. §6.2's twin asserts only `proposed.queries`, so it certifies nothing.
Fix: `json()` takes the written flag, and `plan_learn_json` emits the same
`learn: refused` lines as the CLI; give §6.2's twin a refused case.

### 4. **MAJOR** — refusals are now the sole cause channel and are locationless

With R8 declining to extend the document, §5.2's `format!("{}: a redacted
statement", normalized)` is all an agent gets for a blocked write: SQL text it
must grep the suite log for, no route, no proxy. With 37 grantable statements
and one inlined value, the answer is `wrote:false`, one bare line, and the
whole manifest unchanged — the state that makes hand-editing or
`unrestricted = true` attractive. The stricter gate is fine; the blindness is
not. Emit `learn: refused route "notes" [route.db]: <sql>: <reason>` (the
observation carries the target, §5.2 already has `route`) and print
`learn: 37 grantable, 1 refused, nothing written`.

### 5. **MAJOR** — the tx finding still offers only `observe`, so `unrestricted` stays the shortest green edit

`audit_v1.rs:675-682` still yields `(transaction_label(…), Fix::observe())`, and
§7.4 embraces that ("the next `check` names the remaining statement with
`fix.action: observe`"). For a Django/psycopg3 app the documented loop is
"`check --json` until `ok`", and the one JSON field that would name the remedy
names nothing; `transaction_control = true` exists only in a stderr hint from a
previous process and in `ci.md` prose (leaf 6, not yet written). VISION §3
forbids reaching `unrestricted = true`, and §10's "never approached" argument is
about `plan_learn`'s de-dup, not about the agent's edit. R14's oracle
(`would_write`, `queries()==1`, `hints==1`) never inspects the *next*
`check` document, so "the loop terminates in one hand edit" is untested.
Fix: `Fix::set_transaction_control` carrying `to: "[route.db].transaction_control"`
on route "notes", and an oracle that runs `evaluate` after the write and
asserts that `fix.to` string.

### 6. **MAJOR** — `docs/local.md` still teaches the rejected second writer

§5.6: "`docs/local.md` needs no change: its `learn:` examples already show
queries being added". Its example at `:94-96` *is* `blastit dev --learn` writing
`3 queries added`, under the same "Fill the manifest in one pass" heading. §1's
"no second `dev` session" therefore has no page and no test behind it. Leaf 6
should be unconditional: `check --learn` fills grants; `dev --learn` is for
projects with no suite and never for CI.

### 7. **MAJOR** — §6.1's headline assertions are still prose

"The `git diff` must show **two** added entries and the existing `SELECT 1`
still present" cannot be asserted by `git diff`; the doc gives an agent no
command that checks the resulting list. §6.1 should end with:

```console
$ uv run blastit check --json | jq -e '.ok and (.findings|length == 0)'
$ uv run python -c 'import tomllib,sys; q=tomllib.load(open("blastit.toml","rb"))["route"][0]["db"]["queries"]; sys.exit(q!=["SELECT 1","SELECT 2","SELECT 3"])'
$ git diff --exit-code
```

### 8. **MINOR** — the one list shape §5.4 exempts still loses comments silently

"When the closing bracket shares a line with an element (a list like
`queries = ["a",\n "b"]`) … the whole list is rebuilt in the *same* shape". A
trailing comment on the `queries = ["a", # why` line is preserved only on the
one-line path (`edit.rs:873-886`), so the rebuild drops it and `verify` (model
comparison) cannot see it — O4's defect surviving in the shape R10 doesn't
cover. Preserve the comment or refuse by name; do not splice silently.

### 9. **MINOR** — the hint is still a coordinate, not a block

"set transaction_control = true in [route.db] on route \"notes\"" is correct
TOML *in context*, but an agent that appends `[route.db]` at end of file binds
it to the **last** `[[route]]`, not `notes`. Say it as the block to edit
("inside `[[route]] name = \"notes\"`") or emit the three-line snippet.

### 10. **MINOR** — two JSON dialects (N1) and the phantom `apply` (N2)

Unchanged from round 1; both are cheap: add `"schema_version": 1` to the learn
document, and rewrite §1's Agent row as `` `--learn --json` (writes) →
`check --json` (`ok: true`) ``.

## The loop, walked end to end

| stdout (exit) | stderr | Meaning | Stop? |
|---|---|---|---|
| `{0,N}, true` (0) | hints maybe | wrote | **yes** → `check --json` |
| `{0,0}, false` (0) | — | fixed point (R6) | **yes** (tx residue) |
| `{0,N>0}, false` (0) | — | `--dry-run` | yes (re-run without it) |
| `{0,N}, false` (1) | `learn: refused …` | refusal | no → parameterise, rerun |
| `{0,N}, false` (1) | `learn: <path>: …` | write failed | no → fix perms/FS |
| `{0,0}, false` (1) | **empty** | zero-request suite | **no — nothing to act on (1)** |

Stop conditions are unambiguous for rows 1-5 only, and row 5 is a stderr
*shape*, not a documented field. All refusals appear in one run (`BTreeSet`), so
one extra iteration fixes them all — achievable, once (4) gives them a route.

Transaction app: the first `--learn` writes the grantable queries and exits `0`
(R14, consistent with `clean` ignoring hints), so the write completes in the
first pass; the loop then stops on rows 1-3 and the agent must make one edit
that no JSON field names (4). It therefore terminates in one extra *hand edit*,
which is what §7.4 claims — but the claim is enforced by prose, not by an
oracle, and the `unrestricted = true` shortcut remains one field on the same
finding. Add the `fix.to` oracle and row 6's diagnostic, and the loop is
unambiguous end to end; today it is not.
