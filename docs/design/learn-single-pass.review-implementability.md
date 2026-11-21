# `learn-single-pass` — implementability review

Lens: cheap-model buildability. Every finding cites the section under review and
the code line it has to compile against. Verified against `872f523`.

Counts: **2 BLOCKER, 7 MAJOR, 6 MINOR.**

---

## BLOCKER

### B1 — `[route.<route>.<proxy>]` is not a manifest key; R11 ships the failure O3 claims to fix

§3/§5.2 (`learn-single-pass.md:189-193`), R11 (`:148`), §6.3 (`:342`).

The hint becomes
`… set transaction_control = true on [route.notes.db]`. No such table exists:

* Route grants are keys **inside** the route table. `parse.rs:527-663` walks the
  `[[route]]` element's fields and treats each table-valued key as
  `[route.<proxy>]`, which is why `docs/reference/manifest.md:242-246` writes
  `[[route]] … [route.db]` and `docs/ci.md:67` publishes
  `"to": "[route.db].queries"`.
* TOML resolves `[route.notes.db]` as a **nested key `notes` of the last
  `[[route]]` element** (verified with `tomllib`: it yields
  `{'route': [{'name': 'notes', …, 'notes': {'db': …}}]}`). `parse.rs:640-657`
  then sees an undeclared proxy named `notes` and pushes an `UnresolvedGrant` →
  the boot refuses. That is *exactly* the "agent pastes the line and the manifest
  fails to boot" failure O3 (`:78-85`) attributes to the current text, except the
  current text (`[route.db]`, `audit_v1.rs:1367-1370`) is correct.

The citation in O3 is also wrong: `docs/reference/manifest.md:239` is
`transaction_control = true` inside the `[group.session.db]` example at `:238`.
The design read a **group** key (`[group.<name>.<proxy>]`, which *is* real) and
generalised it to routes.

Consequence for a cheap implementer: §5.2 and §6.3's R11 oracle agree with each
other and disagree with `parse.rs`, `manifest.md`, `ci.md` and
`critical_manifest_edit.rs`. Implementing the spec verbatim ships a hint that
corrupts a manifest; implementing it correctly contradicts R11. This is a forced
decision, not a detail.

Fix: keep `[route.<proxy>]` and put the *route name* in prose, e.g.
`set transaction_control = true on the [route.db] table inside route "notes"`.
The real O3 defect (the hint never names the route, so a paste lands on whichever
`[[route]]` is last) is still fixed, and R11/§6.3 change with it.

### B2 — the §5.3 `learn_route` `Routed` arm never checks the route exists, so it fails R3 and its own oracle

§5.3 (`:222-238`), R3 (`:140`), §6.3 (`:335`), R2 (`:139`).

```rust
ObsTarget::Routed { route } => Some(route.clone()),
```

returns a name unconditionally. `plan_learn` then groups
`("nosuchroute", "db")`, the de-dup at `:1398-1417` finds no index so keeps every
query, and `apply_edits` hits `route_index(manifest, …) == None` →
`EditError::MissingRoute` (`edit.rs:570-575`). So:

* R3 ("skipped, never invented", §10 `:569-570`) is not implemented by the
  pasted seam;
* §6.3's R3 oracle (`Routed { route: "nosuchroute" }` → skipped, **no
  `MissingRoute`**) fails against the code as pasted;
* `evaluate` handles the identical input by yielding `None`
  (`audit_v1.rs:637-643`), so the two sides disagree — the very divergence R2
  exists to forbid.

The implementer must invent the predicate. Give it: return `Some` only when
`manifest.routes().iter().any(|r| r.name() == route)`, and say so in the helper's
doc comment (it currently claims the Routed arm is authoritative).

---

## MAJOR

### M1 — §5.4's scan starts one line too late: the opening line's comment is still silently dropped

§5.4 (`:248-260`), R10 (`:147`).

`patch_grant` computes `comment` only in the `one_line` branch
(`edit.rs:868-876`); for a multi-line list `comment` is `String::new()` and the
replacement at `:877-887` splices from `assignment`. So

```toml
queries = [  # reviewed by @ana, do not delete
    'SELECT 1',
]
```

loses `# reviewed by @ana, do not delete` **today**, and still loses it after the
specified fix because the scan is `assignment + 1 .. stop`. R10 says "never
silently folded without the comment"; the one-line-before hole is the same class
as the interior-comment hole the rule targets.

Fix: also test `trailing_comment(lines[assignment])` when `!one_line` and refuse
(or carry the comment across). Say which in §5.4 — the two choices produce
different bytes and different tests.

### M2 — §5.4 does not specify the `line` value, and its "exact rather than heuristic" claim is false

§5.4 (`:246-260`).

* `EditError::CommentedQueries { route, proxy, line }` — the Display example says
  "line 13", but nothing says whether `line` is the 0-based index into the
  `Vec<String>` (what the code has) or the 1-based document line. The
  implementer must guess, and the new `edit_branches.rs` test will pin the guess.
* "inside a multi-line TOML array no element can start a line with `#`" is false
  for multi-line strings: `queries = [ """ … \n # x \n … """ ]` is legal TOML and
  the `#` line is string content. `scan_lines` already records this as
  `info.in_string_at_start` (`edit.rs:818-826`, `:1016-1021`) — say "a line whose
  first non-space byte is `#` and which is not inside a multi-line string", and
  name the field, or the error misdiagnoses a legal (if ungrantable) list.

### M3 — an existing route with an inline grant turns `check --learn` into a hard failure

§5.5 (`:262-265`), O4 (`:87-93`), §12 leaves 1/2.

`apply_edits` sets `inline: existing.is_some()` (`edit.rs:602`); `patch_grant`
then returns `EditError::InlineGrant` when the route has no `[route.<proxy>]`
header (`:804-810`). `db = { queries = ["SELECT 1"] }` inside `[[route]]` is
legal (`parse.rs:640-662`; refusal tested at `critical_manifest_edit.rs:510-519`)
and has no header. Today (a) is `dev --learn`'s path only; after (a) it is
`check --learn`'s, and the run writes *neither routes nor grants*
(`check.rs:979-988` treats any `EditError` as failure). Nothing in §4, §5.5 or §6
rules on it and nothing tests it.

Decide and write it down: skip the route (and say so on stderr) or fail the run.
Either way add the case to §6.3.

### M4 — R7 and §5.5 require opposite things of `wrote`, and the "fix" breaks a protected test

R7 (`:144`), §5.5 (`:271-273`), §11 (`:584`).

`LearnPlan::json()` returns `would_write(dry_run)` — intent
(`audit_v1.rs:1293-1299`). `check.rs:995-1001` builds its own document with the
*actual* `wrote`. So R7 ("reflects the actual write, not the intent") is already
true for the CLI without touching `json()`, while `json()` stays intent-based for
the bridge (`plan_learn_json`, `:1446-1457`), which never writes at all.

An implementer reading R7 as a change request will edit `json()`; that flips
`document["wrote"] == true` in `critical_check_engine.rs`'s existing assertions
(`:1130-1140`, `:1178-1190`) — a protected file §11 says must stay green.

Reword R7 to "the CLI document's `wrote` (already `check.rs:995-1001`) is the
actual write; `LearnPlan::json()` stays `would_write`", or drop the sentence.

### M5 — §6.1/§6.2 land only in a DATABASE_URL-gated file, so leaf 4's acceptance command passes vacuously

§6.1 (`:286-319`), §6.2 (`:321-328`), §12 leaf 4 (`:602`).

`python/tests/test_check.py` is `pytestmark = pytest.mark.skipif(not DATABASE_URL,
…)` (`:46-52`) and its own docstring says "`make check` does not run this file"
(`:14-18`). Leaf 4's acceptance is `uv run pytest python/tests/test_check.py` —
without PostgreSQL that exits `0` with every test skipped, so the leaf cannot
prove itself and a cheap implementer will report a green run it never had.

Either put the fixture in the offline tier (a stub pytest / mock upstream as
`python/tests/critical/test_check_contract.py` does) or make the leaf's
acceptance `DATABASE_URL=… uv run pytest …` plus an explicit "skipped means not
done" note. §6.1's transcript also cannot be produced by that file (it needs a
real `psycopg` connection).

### M6 — leaf 3 has no writable test path; §6.2's bridge twin has no home

§12 leaf 3 (`:601`), §6.2 (`:326-328`), §11 (`:581`).

The leaf's paths are `crates/blastit/src/audit_v1.rs` **and
`crates/blastit-py/tests/critical_check_schema.rs` (existing test,
unmodified)**. A protected file cannot host the "new test: raw `Unrouted` bytes
for a routed path propose `0` routes", and §6.2's `_blastit.plan_learn` twin
cannot go there either. The only remaining slot is `audit_v1.rs`'s `#[cfg(test)]`
module, which never calls the bridge — i.e. the leaf would claim bridge parity
with a test that does not exercise the bridge.

Name the file: a new unprotected `crates/blastit-py/tests/learn_parity.rs`, or
`python/tests/test_bridge_learn.py`, and add it to the leaf's paths. (The bridge
needs no code change: `blastit-py/src/check.rs:38-44` and `lib.rs:326-333` both
forward to `plan_learn_json`.)

### M7 — leaf 8 asks for a second predicate that duplicates `Manifest::can_boot_without_app()`, and its own path list is conditional

§7.1 (`:393-422`), §12 leaf 8 (`:606`).

`no_route_reaches_the_app` (`:400-406`) computes, route by route, "this route is
its own executor". `Manifest::can_boot_without_app()` already exists for that
question and `app_rule` uses it (`stage0.rs:303-323`); the design does not say
whether the new predicate replaces it, wraps it, or lives beside it — yet leaf 8
lists `crates/blastit-manifest/src/model.rs` **"if the predicate lives there"**,
which is a decision in a paths field. Also `static_app_refusal` is called by
`check` (`stage0.rs:313-314`), so leaf 8 silently changes `check`'s exit for
`BLASTIT_APP`-less all-static manifests, and no acceptance row covers it.

Pin: predicate location, whether `can_boot_without_app` is re-expressed in terms
of it or vice versa, and a `check`-path acceptance.

---

## MINOR

### m1 — §6.1's JSON line is not what the code prints
`serde_json`'s `to_string()` is compact (`audit_v1.rs:1294-1298`,
`check.rs:995-1001`): `{"proposed":{"routes":0,"queries":0},"wrote":false}`. The
pasted line has spaces. Cosmetic in a transcript, fatal if a docs fence compares
bytes.

### m2 — R9 attributes `parse_and_validate` to `apply_edits`, which never calls it
`edit.rs:361-374` calls `Manifest::from_toml_str` + `verify` and stops; the boot
check lives in the caller's test (`critical_check_schema.rs:294-297`). R9's "the
resulting document boots (`Manifest::parse_and_validate`)" is a caller duty, not
a postcondition — an implementer may add the call to `apply_edits` and change its
error surface.

### m3 — `plan_learn`'s doc comments are left false
`audit_v1.rs:1312-1319` ("Only `ObsTarget::Unrouted` statements are mapped") and
the module doc `:34-37`. §5.5 enumerates what does not change; these do, and the
cheap implementer will follow §5.5.

### m4 — §7.3 pastes a non-Rust placeholder
`:485` `Some(b'*') => { /* unchanged */ }`. Replace with the real branch
(`static_asset.rs:218-220`) or the leaf is a copy-and-hope.

### m5 — §6.3's R5 `tables` oracle does not name a statement the grant covers
"a manifest using `tables`, a `group`, and `unrestricted = true` → `queries() ==
0` in all three". `is_query_covered_by_tables_or_groups` decides table coverage by
analysing the statement (`model.rs:886-931` via `Allowlist`), so the fixture's
statement matters. State it (a `SELECT` on a granted table/column), or the
implementer guesses and the row goes red.

### m6 — leaf 6's `docs/local.md` entry is a conditional inside a paths field
`:604` "`docs/local.md` **if** its `learn:` example shows a route-only write".
`docs/local.md:84` is `learn: 2 routes, 38 queries added` and the runnable-docs
harness checks it for *shape* only, so the answer is "no change" — decide it here
rather than delegating a judgement to the implementer.

---

## Verified, no finding

* **Compiles against the real types.** `ObsTarget::{Routed{route},
  Unrouted{method,path}}` matches `blastit-dbproxy/src/learn.rs:84-97`;
  `RoutingTable::route(&str,&str) -> Option<Match>` with `Match{route: usize}` is
  `routing.rs:43-49,122-136`; `Manifest::route(usize) -> Option<&Route>` is
  `model.rs:700-702`; `RoutePlan{by_path: BTreeMap<String,String>}` is
  `infer_routes.rs:41-48`; `ProxyName`/`Grant`/`RoutingTable` are already imported
  at `audit_v1.rs:60`. `&String → &str` coercion on `method`/`path` is fine, and
  let-chains are already used (`routing.rs:124`, MSRV 1.88).
* **Determinism preserved.** `paths` is a `BTreeSet` (`:1335`), `grouped` a
  `BTreeMap<(String,String), BTreeSet<String>>` (`:1358`), `hints` a `BTreeSet`;
  the added `Routed` names do not reorder `infer`'s output.
* **No existing protected test breaks** (checked, not assumed):
  `GOLDEN_MANIFEST` routes only `/` (`critical_check_engine.rs:254-264`), so the
  §5.1 guard leaves `/orders/42/` and `/notes/` in `paths` and the golden counts
  (`routes==1, queries==2`, `:297-312`, `:1140-1170`) and the tx-hint test
  (`:1186-1213`) are unchanged; `critical_check_schema.rs:249-298` and
  `test_check_contract.py:1080-1100` both learn a genuinely new route
  (`/orders/42/`, `/notes`); `edit_branches.rs:79-107` and `:113-135` carry no
  interior comment, so the §5.4 refusal does not touch them; the `learn` fixture
  only exercises unrouted paths, so `mode_learn_git`/`learn_sequence` are
  unaffected.
* **Sizing.** Every blocking leaf is well under 600 changed lines: leaf 1 ≈ 40 +
  one branch test, leaf 2 ≈ 60 + unit tests, leaves 4/5 test-only, leaf 6 docs.
  Leaf 2 is the only one touching a non-test source file.
* **Licences/deps.** No new crate, no new dependency, no SPDX question.

---

# Round 2 (after the reshape)

Re-read at the same tree. Open after round 2: **0 BLOCKER, 6 MAJOR, 9 MINOR.**
Round-1 rows first, then the new material compiled against `edit.rs`,
`static_asset.rs`, the lint config and the nine leaves.

## Round-1 findings, one line each

| # | Status | Where |
|---|---|---|
| B1 | **RESOLVED** | §5.2 keeps `[route.{}]` filled with the proxy and puts the route in the sentence (`on route "{route}"`); R11 and §6.3's oracle (no `[route.<name>.`) now agree with `parse.rs`. |
| B2 | **RESOLVED** | §5.3's `Routed` arm is `manifest.routes().iter().any(…).then(|| route.clone())`. Compiles: `&str == &String` needs `str: PartialEq<String>`, which std has; R3's oracle is satisfiable. |
| M1 | **RESOLVED** | The insertion path never reads or rewrites `lines[assignment]`, so a comment on the opening line survives by construction — the `comment`/`replacement` splice that dropped it is gone. |
| M2 | **RESOLVED (obsolete)** | No comment scan and no `EditError::CommentedQueries` remain; the 0-/1-based `line` question and the multi-line-string claim are moot. New comment risk in N1. |
| M3 | **RESOLVED** | §5.4 + R15 now decide it: the *whole* write is refused with `InlineGrant`'s actionable sentence; §11 gives it a home in `edit_branches.rs`; §6.3's R1 row stays satisfiable. |
| M4 | **NOT RESOLVED** (MINOR) | R7 still says "`wrote` … reflects the actual write", which §5.5 (`json()` unchanged) and §11 (`critical_check_engine.rs:1100-1215` unmodified) now contradict. Reword R7; §4 is read before §5.5. |
| M5 | **RESOLVED** | §6.1 names the offline protected case and the PG twin as *separate* homes; leaf 4's acceptance is `pytest python/tests/critical -q`, which cannot pass vacuously. |
| M6 | **RESOLVED** | `pub mod audit_v1` (`lib.rs:34`) makes `plan_learn_json` reachable from `crates/blastit/tests/` or from the module's own `cfg(test)`. Path bookkeeping left in N8. |
| M7 | **NOT RESOLVED** (MAJOR, sharper) | Paths are unconditional now, but §7.1 still says nothing about `can_boot_without_app`. Worse: its predicate says a non-`spa` static route *reaches* the app, so `app_rule`'s second half (`!can_boot_without_app() && app.is_none()` → require app) demands no app for exactly that manifest. The specified fix reproduces the asymmetry it removes. |
| m1 | **NOT RESOLVED** (MINOR) | §6.1:403 still prints `{"proposed": {"routes": 0…}` with spaces; `json()` is `to_string()` (compact). Harmless only because `docs/design/` is outside `make docs-test`. |
| m2 | **RESOLVED** | R9 now separates the postcondition (`verify`) from the caller's `parse_and_validate` oracle. |
| m3 | **RESOLVED** | Leaf 2 names `audit_v1.rs:1312-1324` and `:34-37`. |
| m4 | **RESOLVED** | §7.3 is a complete Rust function; no `/* unchanged */`. |
| m5 | **NOT RESOLVED** (MINOR) | §6.3's R5 row still names no statement, and `is_query_covered_by_tables_or_groups` decides coverage by analysing it. Name a `SELECT` on a granted table/column. |
| m6 | **RESOLVED** | §5.6 and leaf 6 both say `docs/local.md` needs no change; the conditional is gone. |

## §5.4 against `patch_grant` (what the spec does not say)

The anchor is right: `stop` is the first index *after* the closing bracket, so
`stop - 1` **is** the bracket line in every multi-line shape, including
`queries = ['a',\n 'b',\n]` (`depth_end` of `]` is 0). Splicing fresh lines at
`stop - 1..stop - 1` orders them after the existing entries, which is the order
`expected_routes` builds (`all = written + fresh`) — so `verify` passes. What is
missing:

* **N1 (MAJOR, new)** — `was_empty` and "element line" are undefined. An empty
  multi-line list (`queries = [\n]`) parses, sets `was_empty = true`
  (`edit.rs:600`), is *not* `one_line` (`depth_end == 1`), and reaches the new
  branch; step 2 then appends a comma to `lines[stop - 2]`, which is
  `queries = [`, giving `queries = [,` → `Manifest::from_toml_str` fails →
  `EditError::Verification { section: "the document syntax" }`: a legal manifest
  is refused with a message about the wrong thing. A comment-only line before
  `]` (`# keep sorted`) is the same bug in silent form: the comma lands inside
  the comment, so an existing line changes while `verify` (model-only) passes.
  §6.3's R10 fixture is "comment + missing trailing comma", not these two.
* **N2 (MAJOR, new)** — step 2 contradicts R10 and its own oracle. Adding the
  comma *rewrites an existing element line*, so "every existing line … byte-
  identical" and "a missing trailing comma" cannot both hold. Say "modulo the
  one comma step 2 adds" in R10 and §6.3, or normalise the comma on insert.
* **N3 (MAJOR, new)** — the fallback ("the closing bracket shares a line with
  an element") has no predicate. `depth_start`/`depth_end` cannot tell `]` from
  ` "b"]` — both are `(1, 0)` — so an implementer testing the fields it has
  inserts *before* the last element, reorders `all`, and `verify` refuses the
  write (`"the routes"`). The test has to be textual: the bytes before `]` on
  that line are blank.
* **N4 (MINOR, new)** — the branch must be written as a *replacement* for
  `edit.rs:869-891`: `had_newline`, `comment` and `replacement` become unused
  there, and `make rs-lint` is `clippy … -D warnings`. The doc never says
  `had_newline` is dead in the new path (it is: new lines always end `\n` and
  the bracket line is untouched, so the EOF-no-newline shape needs nothing).
* **N5 (MINOR, new)** — step 3 says "`toml_basic_string(query)` … the same
  quoting `toml_string_array` uses". It is not: `toml_string_array` →
  `toml_string` prefers a **literal** `'…'` (`edit.rs:1422-1436`). Following the
  text writes `"SELECT …"` lines beside `'SELECT …'` entries — the mixed-style
  diff §5.4 exists to prevent.

## §7.3 DP against the real code and the lint set

Signature, anchoring and `*`-stops-at-`/` all match the current
`glob(&[u8], &[u8])` (`static_asset.rs:212-226`), so `matches` is untouched; both
recurrences are the standard wildcard DP and are correct against the old
semantics (including `*` matching empty, and `**` ≡ one `*` today because each
star may match empty). No lint hazard: `clippy --workspace … -D warnings` with
only `unwrap_used`/`expect_used` at `warn` (`Cargo.toml:14-19`) — the
`for j in 1..width` bodies index with `j - 1`, which keeps
`needless_range_loop` quiet, and there is no `unwrap`/constant index.
`.then(|| …)` in §5.3 is also fine (the lint does not fire; the workspace
already uses it, e.g. `blastit-sqlpolicy/src/normalize.rs:629`).
One suggestion, not a finding: `tokens()` allocates per `matches` call on every
static answer; tokenising once in `ImmutableGlob::new` (pattern ceiling 256)
removes it, at the cost of a `Vec<Tok>` field.

## §12 leaves: ≤ 600 lines with their own writable tests?

Yes for 1, 2, 4, 6, 7, 9 and for 8 *as two steps*. Leaf 1 has `edit.rs` +
unprotected `edit_branches.rs`; leaf 2 has `audit_v1.rs` + `check.rs` (the
`learn_line` signature change and `check.rs:1366`'s assertion are in the leaf,
unprotected). Exceptions, all new:

* **N6 (MAJOR, new)** — R16/§7.2/leaf 9 say "in `ExecutionMode::Production`".
  No such type exists (`grep ExecutionMode` → nothing; `cli.rs:39-46` has
  `Command::{Serve,Dev,Check}`, `config.rs:218` has `Mode::{Root,Capabilities}`).
  R16 must name the `Command::Serve` boot and the site: after
  `Manifest::parse_and_validate` in the Serve path (`stage0.rs:1827`), **not**
  inside `parse_and_validate`, which `dev` (`:2103`) and `check` also call —
  otherwise `dev` stops warning and `check` starts refusing, contradicting R16's
  own three-way statement.
* **N7 (MAJOR, new)** — leaf 8 step 1 says "an `app` no route can reach is a
  **warning**"; §7.1 says "both a dead declaration → **refused**". Two
  behaviours, two exit codes, one leaf. Pick one (refused, with step 1 as the
  message fix) and give mismatch 3 an `R` id — it has none, so §11's "every
  requirement mapped" does not cover it.
* **N8 (MINOR, new)** — leaf 3's acceptance says "test in the new Rust suite",
  its paths list only `crates/blastit/src/audit_v1.rs`, and leaf 5 *creates* that
  suite: two leaves on one new file. Put the bridge twin in `audit_v1.rs`'s
  `cfg(test)` (same module, no protected file) or add the path to leaf 3.
* **N9 (MINOR, new)** — leaf 5 claims "R1-R16 oracles". R15 lives in
  `edit_branches.rs` (§11), R16 in a new stage0 critical file (§11), R9 is
  already proven by `critical_check_engine.rs:330-334`, and **R12 has no oracle
  anywhere in §11**. Restate the leaf as R1-R8, R11, R14 and either add an R12
  row (§10's no-privilege claim is otherwise untested) or delete R12.
* **N10 (MINOR, new)** — leaf 9's paths include the ADR but say "lead owns
  `docs/decisions/`": a decision inside a paths field, the m6 class again.
* **N11 (MINOR, new)** — §10 calls the wire verdict `LearnVerdict::Query`;
  `plan_learn` consumes `ObsVerdict` (`blastit-dbproxy/src/learn.rs:109`).
  `LearnVerdict` is `blastit-sqlpolicy`'s (`learn.rs:174`) — a cheap
  implementer can import the wrong one.

Sizing note: leaf 5 is the only one with real headroom risk — the
`http_line`/`unrouted_sql`/`observations` builders live in the protected
`critical_check_engine.rs` and cannot be imported, so ~120 lines of fixtures are
re-derived before the first of ten tests. Splitting it (plan-level R1-R5,
CLI- and bridge-level R6-R14) is cheap insurance.

# Round 3 — verdict: FAIL

Re-read at the same tree, after the §5.4 six-step rewrite, R10/R16/R21, §7.2/§7.3 and the ten-leaf plan.
Only surviving round-2 items and new defects. Opening counts: **2 BLOCKER, 4 MAJOR, 14 MINOR.**

## (a) Round-2 findings still NOT resolved

| # | Status | Evidence |
|---|---|---|
| M4 (R7) | **NOT RESOLVED** | R7 (`:147`) still says the document's `wrote` "reflects the actual write, not the intent", while §5.5 makes `_blastit.plan_learn` pass `would_write(dry_run)` (`:382-385`). It is no longer only wording — see B1. |
| N7 (leaf 8) | **NOT RESOLVED** | leaf 8 step 1 still ships "an `app` no route can reach is a **warning**" (`:1025`) while R21 (`:161`) and §7.1 require a refusal by `blastit` **and** `check`. Two behaviours in one leaf, and step 1's warning is an outcome no requirement asks for. |
| N4 (dead locals) | **NOT RESOLVED** | §5.4 replaces `edit.rs:869-891` but never says `had_newline` (`:864`) and `comment` (`:868-876`, always `String::new()` past the `one_line` early return) become unused. `make rs-lint` is `clippy … -D warnings`: the leaf must delete them, and the doc does not say so. |
| N5 (quoting) | **NOT RESOLVED** | step 5 (`:328-329`) still renders `toml_basic_string(query)` and calls it "the same quoting `toml_string_array` uses". `toml_string_array` → `toml_string` prefers a **literal** `'…'` (`edit.rs:1407-1431`). Following the text writes `"SELECT …"` beside the author's `'SELECT …'` — the mixed diff §5.4 exists to prevent. |
| N8 (leaf 3 test home) | **NOT RESOLVED** | leaf 3 (`:1020`) still says "test in the new Rust suite", which leaf 5 *creates* (`:1022`) and which is in neither leaf's `paths`. |
| N9 (leaf 5 scope, R12) | **NOT RESOLVED** | leaf 5 still claims "§6.3 oracles (R1-R20)" (`:1022`); **R12 has no oracle** anywhere in §11, and R21 — new — appears in neither §11 nor §6.3, so "every requirement mapped" is still false. |
| N10 (ADR in `paths`) | **NOT RESOLVED** | leaves 9 and 10 still list the ADR and say "lead owns `docs/decisions/`" (`:1026-1027`): a decision inside a paths field. |
| N11 (`LearnVerdict`) | **NOT RESOLVED** | §10 (`:968`) still says "Every entry is a `LearnVerdict::Query { normalized }`". §5.2 is right (`ObsVerdict`); `LearnVerdict` is `blastit-sqlpolicy`'s (`dbproxy/src/learn.rs:99-109`). |
| m1 (JSON spacing) | **NOT RESOLVED** | §6.1 (`:508`) still prints `{"proposed": {"routes": 0, …}}`; `json!().to_string()` is compact. |
| m5 (R5 oracle) | **NOT RESOLVED** | §6.3's R5 row (`:554`) still names no statement, and `is_query_covered_by_tables_or_groups` decides coverage by analysing it. |

M7 (leaf 8 / `can_boot_without_app`) is resolved: §7.1 and R21 pin `is_all_static`, `can_boot_without_app` and `app_rule`'s second half as untouched and name the `check` call site (`:652-661`). N6 is closed below under a new number because the *site* got worse.

## (b) New defects

### B1 — `json(wrote)` silently inverts a protected test that §11 forbids editing

§5.5 (`:369-379`), R20 (`:160`), §11 (`:998`).

`LearnPlan::json` is `pub fn json(&self, dry_run: bool)` today (`audit_v1.rs:1293-1299`); the redesign keeps the arity and changes the meaning. Every call site compiles, so the compiler is silent:

* `audit_v1.rs:1456` — `Ok(plan.json(dry_run))`: must become `plan.json(plan.would_write(dry_run))`, or the bridge reports `wrote: false` for a writable plan. §5.5 says this in prose only.
* `crates/blastit/tests/critical_check_engine.rs:1183-1184` — `plan.json(false)` then `assert_eq!(document["wrote"], true)` on a **writable** plan. Under the new signature that is `wrote: false`: the test fails, and it is **protected** while §11 (`:998`) says `critical_check_engine.rs:1100-1215` "must stay green unmodified" and leaf 3's `paths` (`:1020`) do not include it.

A cheap implementer has three wrong outs: keep `dry_run` semantics (breaks R20), edit the protected file without `critical_guard --node`, or teach `json` both meanings. Pick one and write it: either keep `json(dry_run)` and add `json_wrote(wrote)` (no protected edit), or put `critical_check_engine.rs` in leaf 3's paths **and** in §11's "add-only, authorised" list with the one line that changes.

### B2 — R18 makes a different protected test red, and §11 cites the wrong one

R18 (`:158`), §7.4 (`:885-890`), §11 (`:1002`), §12 leaf 10 (`:1027`).

The tx branch is `audit_v1.rs:677-681`. The test that pins its current `observe` is `critical_check_engine.rs:509-511`:

```rust
if sql.contains('\u{2026}') || sql == "SAVEPOINT" {
    assert_eq!(fix.action, "observe", "{sql}");
    assert_eq!(fix.to, None, "{sql} edits no table");
```

`set_transaction_control` + `to: Some("[route.db].transaction_control")` fails **both** assertions. §11 sends R18's oracle to "`critical_check_engine.rs:~1500`" — that is `audit_v1.rs:1504`/`:1525` (the unprotected unit module); the protected conflict is at `:476-527` and is *modified by necessity*, not add-only. Leaf 10's paths are `audit_v1.rs`, `ADR-0004`, `docs/ci.md`. So the leaf either fails `make rs-test-critical` or touches a protected file outside its paths. Fix: list `critical_check_engine.rs` in leaf 10's paths, in §11's protected row, and name `:509-511` as the two lines that change (add-only cannot cover this).

### B3 — §5.4 step 3 appends the comma after the newline, which R10 forbids

§5.4 step 3 (`:319-321`), R10 (`:150`).

`render` splits with `source.split_inclusive('\n')` (`edit.rs:717`), so every line in `lines` ends in `\n`. "If `last` does not end with `,`, append one to it" is `lines[last].push(',')` → `'SELECT 1'\n,` — valid TOML, wrong bytes. R10 says "that one line gains a `,` **before its newline**", and §6.3's R10 oracle ("the document parses") passes either way, so no test catches the divergence. Say `insert(len - newline_len, ",")` or `trim_end_matches('\n')` + `,` + terminator, and add the byte assertion to R10's oracle.

### B4 — the element-line predicate counts a lone `[` as an element, so `was_empty` never saves the empty list

§5.4 steps 3-4 (`:317-327`), R10 (`:150`).

`is_queries_assignment` accepts `queries =` with nothing after the `=` (`edit.rs:1260-1265`), so this legal manifest reaches the new branch:

```toml
queries =
[
]
```

`stop - 1` is `]` ✓, but the element scan `assignment + 1 .. stop - 1` finds one line whose first non-space byte is `[` — neither `#` nor `]` — so `last` is the `[` line, `was_empty == true` is ignored (step 4 keys on "no element line"), and step 3 appends a comma: `[\n,` → `Manifest::from_toml_str` fails → `EditError::Verification { section: "the document syntax" }`. A legal manifest is refused with a message about the wrong thing — the exact N1 class the empty case was added to close. Key step 4 on `patch.grant.was_empty` first, and exclude a line whose only content is `[` (or `[`+comment) from "element line".

### M1 — §7.2's "the site `app_rule` already occupies" is two sites, one of them `dev`

§7.2 (`:709-712`), R16 (`:156`), leaf 9 (`:1026`).

`app_rule` is called at `stage0.rs:1869` (the boot) **and** `stage0.rs:2142` (`dev_fail`, the dev boot), and `app_rule` itself calls `static_app_refusal` (`:316`). An implementer who puts the raw gate "where `app_rule` is" gates `dev` too, contradicting R16's "`blastit dev` keeps the warning" and breaking `critical_manifest_011.rs:459` (the `RawIngress` warning) and stage0's dev tests. Stage 1 is also dev-reachable (`stage1.rs:240` passes `dev.as_ref()`), so the re-read in R16 needs its own `dev.is_none()` at `:262`. Name the site as `stage0.rs:1869`, say explicitly "not inside `app_rule`", and add "`dev.is_none()` in both points" to R16.

### M2 — §7.3's `Tok` cannot derive `Debug`, and deleting the `glob` call leaves dead code

§7.3 (`:761-782`).

* `#[derive(Clone, Copy)] enum Tok` (`:764-768`) inside `#[derive(Clone, Debug)] pub struct ImmutableGlob` → E0277: `StaticRoot` derives `Debug` (`static_asset.rs:241`) and holds `Vec<ImmutableGlob>` (`:258`). Add `Debug`.
* `matches` stops calling `glob` (`:200-202`), and `fn glob` (`:207-226`) is private with exactly that one caller → `dead_code` under `-D warnings`. The leaf must delete `glob`, and no acceptance row says so.

**Does the field swap break a caller? No.** `pattern` is private; only `matches` reads it (`static_asset.rs:201`); construction is only `ImmutableGlob::new` (`:324`, `filter_map` over `policy.immutable`); the only external use is `pattern.matches(&file.url_path)` (`static_response.rs:186`) — the closure binding is an `&ImmutableGlob`, not the field. Both test files use `new`/`matches` (`critical_static_resolve.rs:766-781`). Swapping to `tokens: Vec<Tok>` is source-compatible.

### m1 — leaf 7's test home is a protected file, unnamed and unauthorised

leaf 7 (`:1024`) says "+ its critical suite". That suite is `crates/blastit-scheduler/tests/critical_static_resolve.rs` (`:750-782`), which leaves 4/5 explicitly authorise via `critical_guard.py --node`. Name the file and the guard flag, as leaves 4/5 do.

### m2 — `edit_branches.rs:501` asserts the fold's EOF shape and goes red

`a_replaced_list_does_not_add_a_newline_the_file_did_not_have` builds a multi-line list at EOF with no trailing newline and asserts `edited.ends_with("']")` (`:515-518`) — the folded, single-line marker. The extend path leaves the author's `]` as the last line, so it ends `"]"`, not `"']"`. Leaf 1 owns the file, but §5.4/R10/§6.3 name only "the fold test" (`:77`); this second test is a silent casualty and a cheap implementer may "fix" it by restoring the fold.

### m3 — leaves 8/9/10 are not independent, though §12 says they are

§12 (`:1030`) "Leafs 7-10 are independent of the learn contract and of each other." They share files: 8 and 9 both edit `stage0.rs`, `stage1.rs`, `check.rs`; 10 edits `audit_v1.rs`, which leaf 2 rewrites; 6 and 10 both edit `docs/ci.md`. Parallel implementers will conflict. Either serialise 8→9→10 or say "one branch each, merged in order".

### m4 — R18's `to` is written as a literal placeholder

R18 (`:158`) gives `to: "[route.<proxy>].transaction_control"`; the code interpolates the real proxy (`Fix::add_query` at `audit_v1.rs:692`, `ci.md`'s `"[route.db].queries"`), and §6.3 (`:562`) expects `[route.db].transaction_control`. Copying R18 verbatim writes the string `<proxy>`. Write `format!("[route.{proxy}].transaction_control")` in R18.

## §12 leaves: ≤ 600 changed lines, with a writable test?

| # | ≤ 600? | Writable test? |
|---|---|---|
| 1 | yes (~120) | yes — `edit_branches.rs` (unprotected), incl. m2 |
| 2 | yes (~200) | yes — `audit_v1`'s `cfg(test)` + `check.rs` (both unprotected) |
| 3 | yes (~80) | **no as written** — N8: the named suite is leaf 5's new file |
| 4 | yes (test-only) | yes — protected add-only + the PG twin, both in `paths` |
| 5 | yes, thin (~400) | yes — new file it creates; 14 oracles with fixtures re-derived from the protected `critical_check_engine.rs` builders |
| 6 | yes (docs) | yes — `make docs-check`, `make docs-test`; `docs/reference/manifest.md` is outside `docs_fences.rs`'s `FILES`, so no fence churn |
| 7 | yes (~150) | **no as written** — m1: protected suite unnamed/unauthorised |
| 8 | borderline (~450-600) | yes — but two steps, five source files, a 5×3×2 matrix, and m3's overlap with 9 |
| 9 | yes (~150) | yes — new `critical_*.rs`; N10's ADR ownership |
| 10 | yes (~80) | **no as written** — B2: the failing test is protected and outside `paths` |

No licence/SPDX question: no new crate, no new dependency.

# Round 3 — verdict: FAIL

- **BLOCKER B1** — `json(dry_run)` → `json(wrote)` is silent at the type level and fails the protected `critical_check_engine.rs:1183-1184`, which §11 forbids editing and leaf 3 does not own.
- **BLOCKER B2** — R18 fails the protected `critical_check_engine.rs:509-511` (`observe`, `fix.to == None`); §11 cites `:~1500`, the wrong file, and leaf 10's paths cannot repair it.
- **MAJOR B3** — §5.4 step 3 appends the comma *after* the newline (`split_inclusive('\n')`), contradicting R10's "before its newline"; no oracle compares bytes.
- **MAJOR B4** — the element-line predicate counts a lone `[` as an element, so `queries =\n[\n]` gets a comma after `[` and is refused with `Verification` instead of using `was_empty`.
- **MAJOR M1** — §7.2's "site `app_rule` occupies" is `stage0.rs:1869` *and* `:2142` (dev); placing the gate in `app_rule` or an ungated stage 1 breaks R16's dev-warns rule and `critical_manifest_011.rs:459`. (Round-2 N6, regressed.)
- **MAJOR (surviving N7)** — leaf 8 step 1 still warns for an unreachable `app` where R21 requires a refusal.
- **MINOR M2** — §7.3's `Tok` needs `Debug` (E0277 through `StaticRoot: Debug`) and `fn glob` becomes dead code under `-D warnings`; the field swap itself breaks **no** caller.
- **MINOR** — leaf 3 (N8), leaf 7 (m1) and leaf 10 (B2) have no writable, authorised test path; leaves 8/9/10 and 2/10 and 6/10 share files while §12 calls them independent (m3); `edit_branches.rs:501` goes red unnamed (m2); R18's `to` is a literal placeholder (m4).
- **MINOR (surviving)** — M4/R7 wording, N4 dead locals, N5 quoting, N9/R12+R21 unmapped, N10 ADR in paths, N11 `LearnVerdict`, m1 JSON spacing, m5 R5 oracle.

# Round 4 — confirmation

**Question: are round-3 B1 and B2 closed? Both closed.**

* **B1 closed.** §5.5 (`:402-409`) names `audit_v1.rs:1456` (the only non-test
  call site — `grep '\.json('` finds `audit_v1.rs:1456` plus four in the test
  file), names `critical_check_engine.rs:1183`, gives the surviving form
  `plan.json(plan.would_write(false))`, and leaf 3 (`:1063`) plus §11 (`:1034`)
  put that protected file in `paths` under `critical_guard.py --node`
  (`tools/critical_guard.py:262` takes `--node`). The known failure is now an
  authorised edit, not a silent one.
* **B2 closed.** §11 (`:1040`) and leaf 10 (`:1070`) name
  `critical_check_engine.rs:509-511`; `:510-511` are the two asserts
  (`action == "observe"`, `to == None`), so the protected conflict now has a
  file, a line range and a guard flag. Nothing claims the file is untouched.

**Verdict: PASS** — both round-3 BLOCKERs are closed. Three new MAJORs keep the
doc from acceptance.

1. **MAJOR** — §5.4 step 4 (`:326-330`) tests the raw text after the first `[`
   for a quote, so a quoted trailing comment (`queries = [  # "SELECT 2" is
   denied by design`) makes the assignment an "element line"; step 4 then writes
   `queries = [,  # …` — invalid TOML, the exact `was_empty` failure step 5
   exists to prevent, and step 4's own claim that it "excludes `[ # …`" is
   false. Fix: run `trailing_comment` first and test the remaining text (or key
   step 4 on `patch.grant.was_empty`); add the shape to leaf 1's "empty" case.
2. **MAJOR** — §5.5 (`:404`) "leaves **two** stale call sites" is wrong:
   `crates/blastit/tests/critical_check_engine.rs` has four `plan.json(false)`
   calls (`:1111`, `:1141`, `:1169`, `:1183`). Only `:1183` (`wrote == true`,
   `:1184`) is told to change, so `:1112`, `:1142` and `:1170`
   (`document["wrote"] == false`) become tautologies that pass whatever
   `would_write` decides. Pass `plan.would_write(false)` at all four, as §5.5
   already prescribes for `:1183`.
3. **MAJOR** — R18's named diff (`:509-511`, "**one** assertion", leaf 10 "no
   other line of the protected file changes") cannot express the change:
   redacted statements keep `observe` (`audit_v1.rs:694-696`, the non-`Query`
   fallthrough), so the guard at `:509`
   (`if sql.contains('\u{2026}') || sql == "SAVEPOINT"`) must split into a third
   arm, adding an `else if` outside the cited range. Following "one assertion"
   literally makes the redacted finding assert `set_transaction_control`, which
   R18's own tx-only scope forbids.
4. **MINOR** — §7.3 (`:878-880`) misattributes the field: `pattern` is
   `ImmutableGlob`'s (`static_asset.rs:176-177`); `StaticRoot` holds
   `immutable: Vec<ImmutableGlob>` (`:255-258`). With `glob` deleted, §13.5
   (`:1163`) still says "leaf 7 updates `glob`'s comment (`:205`)", while the
   comment that actually goes stale is `ImmutableGlob`'s own (`:171-174`, "`*` …
   is the only metacharacter"); leaf 7 must name that one.
5. **MINOR (premise)** — leaf 7 (`:1065`) still reads "+ its critical suite":
   `crates/blastit-scheduler/tests/critical_static_resolve.rs` and its guard flag
   are unnamed, and §11 (`:1048`) "Three protected files are touched" omits it.
   Only leaves 3 and 10 name their protected file; leaf 9's
   (`crates/blastit-manifest/tests/critical_manifest_011.rs`) appears only as
   "unmodified" in §11's R16 row.
