# `blastfile-dsl.md` — consistency review (final acceptance round)

- **Lens:** consistency
- **Document:** `docs/design/blastfile-dsl.md` (status: In review, node MB.21.2), 1872 lines, fifth
  revision. This file replaces the round-4 consistency review; it is a fresh-context check of the
  text as it stands, not a diff of the resolutions. Findings that a later round already fixed are
  **not** repeated here.
- **Compared against:** the accepted pages (`docs/blastfile.md`, `docs/reference/blastfile.md`,
  `docs/reference/cli.md`), `docs/VISION.md`, `ARCHITECTURE.md`, `ADR-0002`, `ADR-0004`,
  `ADR-0006`/`ADR-0011`, `ADR-0012`, `plan/nodes/MB/MB.21/NODE.md` and every leaf under it, and the
  code the cutover must keep behaving (`crates/blastit-manifest/src/{model,validate,edit,routing}.rs`,
  `crates/blastit-scheduler/src/{static_asset,static_response}.rs`, `crates/blastit/src/audit_v1.rs`,
  `crates/blastit-env/src/**`, `crates/blastit-manifest/tests/docs_fences.rs`,
  `python/blastit/fold.py`, `python/tests/runnable_docs/inventory.toml`).
- **Ground truth re-measured, not trusted:** §16.1's fence classification run over both accepted
  Blastfile pages; the `LiteralQuery`/`KNOWN_STATIC_SETUP_SQL` rule in `validate.rs`;
  `valid_method`; `canonical_function`; `MAX_QUOTED_CHARS`; the `StaticOutcome`/`AnswerKind`
  variants; `url_path`'s construction in `static_asset.rs`; every `from_toml_str` call site; MB.21's
  14-row provenance table against §7.1; the `fix.to` strings in `audit_v1.rs`; `fold.py`'s
  `--manifest`; the leaf titles, paths and acceptance text under `plan/nodes/MB/MB.21/**/NODE.md`;
  `ARCHITECTURE.md` §2.4.

## What holds (verified this round)

- **The v1 capability map is complete.** All 14 rows of MB.21's provenance table have a Blastfile
  spelling or a named drop in §7.1, plus the new `app "<name>"` row. The three drops — the `output`
  map, the per-view `workers` cap, the global `groups` attach — are named in §7.5, §12.3 and §7.1
  and recorded in `ADR-0012`'s Consequences.
- **Discovery and the negative path match `ADR-0002`/`ADR-0004`** in shape: `./Blastfile`,
  `BLASTIT_MANIFEST`, no upward search, an empty variable is an error, a foreign body is an ordinary
  syntax error with no second sentence.
- **The `check` contract matches the page and `ADR-0004`:** the finding kinds, the `route`/`proxy`
  keys, `0/1/2/3`, empty stdout on `2`/`3`, the PHP `check` at `2`, and the `--learn --json`
  proposal shape.
- **The `raw` gate matches `ADR-0006`/`ADR-0011`:** one `raw_ingress_refusals`, both stages, `dev`
  warns, `check` reports, production refuses, `body raw unvalidated` acknowledges, `--learn` cannot
  write either form.
- **§9.2's four steps are the page's "Request precedence"**, including `Missing` falling through and
  `Hidden`/`Refused` being terminal.
- **The §13.3 retired `to` column is accurate:** `[route.<proxy>].queries`,
  `[group.connection.<proxy>].queries`, `[group.<name>.<proxy>].queries` and `[route.ingress]` are
  what `audit_v1.rs` emits today (lines 690, 692, 862, 935, 1071).
- **§9.1's `/.well-known` prefix is real**, because `is_dotfile` already exempts `.well-known` at
  position 0 (`static_asset.rs:665`); §6.1's `valid_method` equivalence claim is exact
  (`validate.rs:729`); `canonical_function` does drop `pg_catalog.`; `MAX_QUOTED_CHARS = 120`,
  `REGEX_SIZE_LIMIT`, `REGEX_NEST_LIMIT`, `SpaLost`, `route_lines`, `static_roots` and
  `can_boot_without_app` all exist as named.
- **§16.2's baseline ("nine files") and §16.3's inventory row are true:** `FILES: [&str; 9]` and one
  `[[page]]` for `docs/reference/manifest.md` (inventory line 310).

---

## BLOCKER

### B1 · BLOCKER — §13.3 retires `fold`'s required `--manifest PATH`, reversing `ADR-0002` decision 1 and the accepted CLI reference, and §21 assigns the file to a different leaf than the work

Design §13.3:

> `MB.21.4.3` (a) adds one PyO3 function … (b) **re-points `fold.py` at it and retires its
> `--manifest` flag in favour of discovery**, and (c) keeps its statement policy unchanged

Three ground-truth sources say the opposite:

- `ADR-0002` decision 1: "`blastit-manifest fold` **retains its explicit `--manifest PATH` argument
  because it operates offline on arbitrary log files**".
- `docs/reference/cli.md`: "`--manifest PATH` (**required**) | the `blastit.toml` to extend; **this
  command does not discover it**", with the canonical invocation
  `blastit-manifest fold LOG... --manifest blastit.toml [--write]`.
- `python/blastit/fold.py`: `fold.add_argument("--manifest", required=True…)`.

`ADR-0012` opens with "**Supersedes:** nothing" and only carries `ADR-0002`'s discovery *shape*
across; it records no change to fold. No leaf owns `docs/reference/cli.md`'s fold row: §21's 5.1
work is "rewrite site + `README.md` + `ARCHITECTURE.md` … + `docs/VISION.md`'s mention", not the CLI
contract. And §21's 4.3 row lists no `python/blastit/fold.py` path, while 4.5's paths do — so the
design assigns the re-point to 4.3 and the file to 4.5.

**Failure mode:** after 0.2.0 the documented invocation in the CLI reference is rejected; a bare
`fold` discovers the working directory's `Blastfile`, which for a log copied from another app is the
wrong policy file, and `--write` puts grants in it.

**Suggestion:** keep `--manifest PATH` for fold and state why fold is exempt from discovery (it
names a conforming Blastfile, so this is not a compatibility path), or record a superseding decision
explicitly and add `docs/reference/cli.md` + `python/blastit/fold.py` to one leaf's `paths`. Either
way, say which leaf owns `fold.py`: 4.3's description or 4.5's paths, not both.

### B2 · BLOCKER — the erasure guard is three mechanisms, two scan sets and two test names, and the design's version cannot satisfy the epic's own acceptance

The design declares `ADR-0012` co-authoritative ("The one source of truth for the implementation is
this document plus `ADR-0012`"), and the two disagree about the one mechanism that keeps the erasure
permanent:

| Source | Mechanism | Scanned set | Test identity |
|---|---|---|---|
| `ADR-0012` dec. 14 | "`scripts/check_retired_format.py` (gate `make check`, with a negative self-test)" | `crates/`, `python/`, `scripts/`, `e2e/`, `Makefile`, `docs/`, `README.md`, `ARCHITECTURE.md` | the script |
| design §15, §15.2, R29, §20 | `python/tests/test_docs_assets.py::the_retired_format_is_gone`, run by `make docs-unit`, "**no** file and **no** Makefile target" | strict = `crates/`, `python/`, `scripts/`, `e2e/`, `.opencode/`, `Makefile`, `README.md`, `ARCHITECTURE.md`, `AGENTS.md`, every `docs/**`; exempt = "`docs/design\|decisions\|testing\|research\|knowledge/**`, **`plan/**`**, `CHANGELOG.md`" | `the_retired_format_is_gone` |
| `MB.21.5.2` brief + `MB.21` acceptance 4 | the same pytest | also "every live brief: `plan/nodes/**/NODE.md` whose `status` is not `done`/`superseded`"; MB.21 acceptance 4 requires the guard to fail on "a crafted page, **a live brief**, or a crate/script/fixture" | `test_the_real_tree_carries_no_retired_toml_manifest` |

The design's exemption of all of `plan/**` makes it impossible for the guard to fail on a live brief,
which is a named epic acceptance case. The design's test name is not the one the epic's acceptance
and the leaf brief pin. The ADR still names a script the design forbids creating.

**Failure mode:** an implementer following §15.2 writes a guard that passes locally but fails
`MB.21` acceptance 4; an implementer following the ADR creates a `scripts/` file and a Makefile
target that §21's 5.2 paths and the brief's "wrong answer" text reject; the acceptance command that
invokes the test by name runs nothing.

**Suggestion:** one paragraph, three fixes — (a) amend `ADR-0012` decision 14 to the pytest and the
strict/exempt split (assign `docs/decisions/**` to a leaf, or make the amendment part of 5.2), (b)
scan **live** briefs while exempting finished briefs and the dated record, which satisfies both the
design's rationale and MB.21 acceptance 4, (c) use
`test_the_real_tree_carries_no_retired_toml_manifest` in §15, §15.2, R29 and §20.

### B3 · BLOCKER — the leaf that implements §13.3 is still briefed to ship a different `fix.action` vocabulary, and §21 assigns no re-wording

Design §13.3 and R22 keep the accepted action set and re-spell only `to`:
`add_group` → `trait connection`, `add_route` → no `to`, `declare_ingress`/`fix_ingress` → "**no
`to`**", `add_query` → `view catalog postgres`. The page fixes a closed set: "`fix.action` is one of
`add_query`, `add_group`, `remove_query`, `add_route`, `remove_route`, `add_test`,
`declare_ingress`, `fix_ingress` and `observe`".

`MB.21.4.3`'s brief — the file the plan tells the implementer to read — still mandates:

> connection-setup SQL → **`add_trait`** … instead of `add_group`;
> `unrouted` → **`add_view`** …; a body seen on a route without a codec → **`add_body`** …
> `missing_grant` → `add_query`, `to` names the view and the store (`view "restaurants" → postgres
> "db"`)

§21's closing note re-words only `MB.21.4.1`'s title and `MB.21.3.2`'s oracle; nothing re-words 4.3.

**Failure mode:** the implementer follows the leaf, pins `add_trait`/`add_view`/`add_body` and the
quoted-arrow locator in `python/tests/critical/test_check_contract.py`, and the 0.2.0 `--json`
document no longer matches the page, `ADR-0004`, R22 or §13.3 — while §22 claims R22 was accepted.

**Suggestion:** add the 4.3 re-wording to §21's closing note (and to the epic owner's pre-dispatch
`plan.py edit` list) next to the 4.1/3.2 re-wordings, and say the action names are unchanged so a
critic can check it.

---

## MAJOR

### M1 · MAJOR — the erasure's owning leaf is `MB.21.4.5` in §21 but `MB.21.4.4` in §4.1 and every §15.2 row, and the deleted `from_toml_str` has call sites no leaf owns

§21: 4.4 does "every `e2e/` and `python/tests/` fixture → `Blastfile`; the Makefile's `docs-serve`
stage; **no** deletion work", and a new **`MB.21.4.5`** deletes `parse.rs`, the PyO3 surface, the
dependency and the mentions. But §4.1 still says

> `parse.rs` # **DELETED by MB.21.4.4** (no replacement, no compatibility)

and §15.2's "Removed by" column names `MB.21.4.4` for the second reader, the retired reader,
`Manifest::from_toml_str`, the PyO3 surface and the TOML dependency. §13.3 repeats the split error
for fold ("`MB.21.4.3` … retires its `--manifest` flag") while 4.5's paths own `fold.py`.

The concrete compile break: `Manifest::from_toml_str` is defined in `model.rs` (line 610), not
`parse.rs`, and is called from `crates/blastit-manifest/src/edit.rs` (364, 977, 1503, 1526), from
`crates/blastit-env/src/{lib,postgresql}.rs` doctests and `crates/blastit-env/src/env.rs:382`
(a unit test). 4.5's paths are `parse.rs`, `blastit-manifest/Cargo.toml`,
`blastit-py/src/{lib,audit}.rs`, `python/blastit/{fold.py,__init__.py}`, the workspace
`Cargo.toml`/`Cargo.lock` and `python/tests/test_fold.py` — neither `model.rs` nor `edit.rs` nor
`crates/blastit-env/**`.

**Failure mode:** 4.5 lands the deletion, `make check` fails in two crates, and the leaf that broke
them has no path to fix them; or 4.4 is dispatched with the §15.2 list and does the erasure the
design says it must not do.

**Suggestion:** point §4.1 and every §15.2 row at `MB.21.4.5`, and add
`blastit-manifest/src/model.rs`, `blastit-manifest/src/edit.rs` and `crates/blastit-env/**` (or a
named 4.5b) to the erasure leaf's paths with the `from_toml_str` call-site rewrite as explicit work.

### M2 · MAJOR — the `workers` refusal is still required to name `ADR-0012` in `ADR-0012` and §12.3 while §6.7's sentence and R13 forbid it

`ADR-0012` decision 7: "a parse refusal **whose sentence names the removal and this ADR**". §12.3:
"writing it is the refusal in §6.7, **whose sentence names the removal and `ADR-0012`**".

But §6.7's pinned row is "`workers` was removed in 0.2.0: the scheduler owns parallelism, so delete
the line and add nothing" — no ADR id — R13 says "(**no ADR number in the message**)",
§22's C-m5/TS-23 say "no ADR number in a message", and §18 builds the new variants from the tables.

**Failure mode:** the implementer ships R13 and fails the ADR, or ships the ADR and breaks the
no-ADR rule; the one user-facing string the owner settled is whichever the implementer guessed.

**Suggestion:** decide once and say so in one place: either amend `ADR-0012` decision 7 + §12.3 to
the §6.7 sentence (owner sign-off required, since the ADR is Accepted), or quote the ADR id in the
§6.7 row and scope C-m5 to every message except this one.

### M3 · MAJOR — §16.2's pinned fence counts still contradict the measured pages, §16.3 and §22

§16.2 says "measured once by the harness and then pinned — the design does not guess a page's
numbers", then pins:

```rust
("docs/blastfile.md",            Counts { documents: 1, statements: 0, illustrations: 0 }, "MB.21.5.1"),
("docs/reference/blastfile.md",  Counts { documents: 0, statements: 14, illustrations: 1 }, "MB.21.5.1"),
```

Measured with §16.1's own rules: `docs/blastfile.md` is **1/0/3** (three `console` fences, which
§16.3 itself calls "the guide's three `console` fences") and `docs/reference/blastfile.md` is
**0/14/3** (the EBNF fence at l.17, the JSON fence at l.442 and the error fence at l.585). §22's
C-M7 row gives a third answer: "16 statement lists and 3 illustrations".

**Failure mode:** `MB.21.3.6`'s acceptance is "R18 against the current pages"; the harness pins the
wrong numbers, so the docs gate is red on its first run, and the likeliest repair is to edit the
pages to match the table — the exact inversion the rule exists to prevent.

**Suggestion:** pin `1/0/3` and `0/14/3`, correct C-M7 to `14/3`, and either delete the "does not
guess" sentence or state that 3.6 measures before pinning (the `README.md` row, `1/0/0`, needs the
same treatment).

### M4 · MAJOR — §22's resolution log asserts fixes the body denies, so the acceptance record cannot be audited

Six rows still describe the pre-revision document:

| §22 row | §22 says | Current text |
|---|---|---|
| S19 | "`to: "view products body json"` (the codec is in the node); `raw_ingress` -> `view upload body raw unvalidated`" | §13.3: no codec in the locator, "the payload therefore never carries `unvalidated`" |
| C-M4 | "`to` only on grant actions and `raw_ingress`, **with a codec in the ingress node** (S19)" | same §13.3 text; no codec |
| TS-14 | "R19's tier is unit" | R19's tier cell: "unit + docs" |
| TS-16 | "R23 is the crate's `[lints]` table + `clippy -D warnings`" | R23: workspace `[lints]` is **not** changed; module-level `#![deny]` |
| B1 | "`MB.21.3.1` opens with model parity and its `paths` are widened to `parse.rs`, `edit.rs`, …" | §21: model parity is new **`MB.21.3.0`**; 3.1 is the lexer |
| M2 | "leaf `MB.21.3.1`" for `check_uids`/`UidLayout` | §21 puts `uid.rs` in `MB.21.3.0` |

**Failure mode:** a reviewer checking "was S19 resolved?" finds a row mandating the opposite of
§13.3, and a cheap implementer can pick either. §22 is the artefact the acceptance decision cites.

**Suggestion:** rewrite the six rows to the current sections and leaf ids, and mark S19/C-M4
"superseded by round 2 (`S6'`)". A resolution table that is not true is worse than no table.

### M5 · MAJOR — two design rules still justify themselves with page sentences that do not exist

§12.2: "a **boot warning** … because **the reference says 'the `entrypoint` alone serves nothing'** —
a warning, not an error". The reference contains no such sentence; it says "`entrypoint` declares the
one program behind every view" and "Every view in the file reaches it, so a PHP app is at least one
catch-all view".

§7.5: "**the reference's own text tells a reader to rename the input's variable or the app**". Neither
page contains the word "rename" (searched); the reference says "Every input variable keeps its name
and gets the local value".

**Failure mode:** the `EntrypointNotReached` warning and the `output` drop rest on contract text a
reader cannot find, and the page's "a PHP app is at least one catch-all view" is quietly contradicted
without a recorded divergence.

**Suggestion:** quote the real sentences, state the php-no-view case as a recorded divergence
(warning vs the page's "at least one catch-all view") in §12.2/`ADR-0012`, and point `output`'s drop
at "Every input variable keeps its name".

### M6 · MAJOR — R10 says `..` and empty segments "stay refused exactly as today", while §9.2/§17.6 add a pre-routing `400` for them

R10: "… dotfiles, `..`, NUL, empty segments, absolute/backslash paths and symlinks **stay refused
exactly as today**."

§9.2: "this design **adds `..` and an empty segment (`//`)** to that refusal, before any mount or
view is consulted". §17.6: "`..`/`//` refused before routing | hardening (S12) | **today only the
static path refuses them**". §17.3's guarantee is "no view matches a path that contains a traversal
segment".

**Failure mode:** the R10 test writer asserts today's behaviour (a view may match
`/admin/../public`), while the security guarantee and the implementation say `400`. The requirement
and the design disagree on the one differential S12 exists to close.

**Suggestion:** move `..` and empty segments out of R10's "exactly as today" clause into their own
requirement ("the scheduler refuses `..` and `//` with a `400` before any mount or view") with the
`400`-vs-`404` differential test.

### M7 · MAJOR — §15.1's empty-`BLASTIT_MANIFEST` "exit 1 at every entry point" collides with `fmt`'s 0/1/2 contract

§15.1: "`BLASTIT_MANIFEST` set and empty | `BLASTIT_MANIFEST must not be empty…` (**exit 1**; `2` for
`check`)", and "A missing file produces one sentence, **at every entry point**". §14.4/R19 fix
`fmt`'s codes as `0` (clean/applied), `1` (`--check`, one or more not canonical), `2`
(parse/read/write/size) — plain `fmt` has no `1` at all, and §14.4 says `fmt` "uses the **same**
discovery as every other command … so `BLASTIT_MANIFEST` works".

**Failure mode:** a CI job with an accidentally empty `BLASTIT_MANIFEST` makes `blastit fmt --check`
exit `1`, which the table documents as "not canonical"; a human edits layout instead of the
environment, and a script branching on `1` mis-reports. Plain `fmt` emits a code its table omits.

**Suggestion:** scope §15.1's exit codes to boot/`dev`/`check` and give `fmt` a config-error code of
`2` for both forms (consistent with "cannot read"), or add the config error to §14.4/R19
explicitly.

### M8 · MAJOR — §13.1 keeps `LiteralQuery`, so the accepted page teaches a `query` form the boot refuses, and no leaf fixes the sentence

Reference, Grants:

> An exact `query` is **trusted as written** and runs without further checking.
> A statement that inlines a literal cannot be learned; bind the value (`$?`). **An exact `query`
> you write yourself may hold one — read it before pasting.**

§13.1: "validation refuses a redacted `…` (`RedactedQueryPlaceholder`) and an inlined literal
(`LiteralQuery`) **exactly as today**". `validate.rs::check_queries` refuses any normalised query
with a quoted literal outside `KNOWN_STATIC_SETUP_SQL`. §21's 5.1 named page fixes are only "the
`--learn`/`body` contradiction and the `$IDENT`-echo row".

**Failure mode:** a developer follows the page, writes
`query 'SELECT count(*) FROM products WHERE price > 10'`, and the boot refuses it with
`LiteralQuery` — a grant form the reference says is allowed. The page is the DX contract; the design
keeps the validator.

**Suggestion:** delete the "may hold one" and "trusted as written" sentences from the reference and
add that page edit to `MB.21.5.1`'s named fixes (with the fence example that validates), or weaken
`LiteralQuery` and say so in §13.1 — but do not leave the two silent.

### M9 · MAJOR — §9.4's `url_path = prefix + "/" + relative` doubles the slash for the root mount, so the page's own `static "/" { immutable "/assets/*" }` never matches

§9.4: "the resolver must therefore record `url_path` as **`prefix + "/" + relative`**, not the
stripped remainder, or `immutable "/assets/*"` under `static "/assets"` would never match." §9.1
requires a prefix not to end in `/` "unless it is `"/"`". For the root mount, a request for
`/assets/app.js` therefore records `"//assets/app.js"`. `ImmutableGlob::matches` is `/`-anchored and
matches the path **whole** (`static_asset.rs:200`, page: "`/`-anchored globs matched whole against
the request path of the file it answers"), so it does not match.

The reference's own static example is `static "/" { root "public"; index "index.html"; immutable
"/assets/*", "/fonts/*"; fallback "index.html" }`, and §16.2's shape canary expects prefix `"/"`
with `immutable ["/assets/*"]`.

**Failure mode:** every root-mounted site silently loses the year-long immutable header; the canary
passes because it checks the lowered policy, not the composed `url_path`.

**Suggestion:** define `url_path` as "the request path, or the mount prefix re-prepended without a
doubled slash" (today's `format!("/{rel}")` already does this for the root mount), and add the
`static "/"` + `immutable "/assets/*"` case to §20 R10 and the scheduler tests.

### M10 · MAJOR — `ARCHITECTURE.md` §2.4 still documents the order §9.2 replaces, and no leaf is told to rewrite it

`ARCHITECTURE.md` §2.4 ("The static responder") is the live technical spec for this decision:

```text
One total order decides, first match wins: the health path; a route with no
policy is untouched; a malformed path (`400`) or a dotfile at any depth (`404`,
except `.well-known` as the first segment); an exact regular file; a directory's
`index`; `spa = true` and `root/index`; the manifest's `app` when it declares
one; otherwise `404`. A refusal is terminal — never the app, never the fallback
```

After this design there is no `spa`, mounts precede views, the fallback is a step after the view
table, and `405` exists. §9's code-site list names scheduler files but not `ARCHITECTURE.md`; §21's
`MB.21.5.1` work is "rewrite site + `README.md` + `ARCHITECTURE.md` (**§5.1/§6** and every mention)"
— §2.4 is neither.

**Failure mode:** after 0.2.0 the architecture document teaches the old precedence (`spa`, `index`,
`app`, no fallback, no `405`) beside a scheduler that does something else; a reader debugging a
`405` or a fallback is sent to the wrong model.

**Suggestion:** add "rewrite `ARCHITECTURE.md` §2.4 to §9.2, drop `spa = true`" to `MB.21.5.1`'s
named work (and to §9's code site), and add a row to §22 so the change is auditable.

### M11 · MAJOR — the accepted page still says the `--json` keys are re-spelled at the cutover; R22/§13.3 say they are not, and no leaf fixes the sentence

Reference: "The keys stay `route` for a view and `proxy` for a store **until the cutover re-spells
them**." R22: "`check`'s finding kinds, keys (`route`, `proxy`) and exit codes are **unchanged**;
only `fix.to` values are re-spelled". §13.3 and `ADR-0012` decision 5 repeat it. The cutover *is*
0.2.0.

**Failure mode:** an agent reading the page expects `view`/`store` keys after 0.2.0 and branches on
the wrong field, while the shipped document keeps `route`/`proxy`. The page is the only place a
reader is told the key set will change.

**Suggestion:** assign the one-sentence page edit to `MB.21.5.1` and say so in §13.3 (or change R22
and the ADR and bump `schema_version`), and quote the sentence in the resolution row so it is
checkable.

### M12 · MAJOR — the boot-warning texts the design keeps "unchanged" still spell retired vocabulary

§11.1: "`ManifestWarning::UnattachedGroup` (**boot warning, unchanged text**)". §13.2 lists
`UngrantedProxy`, `UnrestrictedRoute`, `RawIngress` and `EgressRoute` as emitted "unchanged". Their
current user-facing strings (`error.rs:542–640`) are:

- `group {} is attached to no route and no global groups: it grants nothing` — a Blastfile has no
  "global groups" (always empty, §7.1) and calls the thing a `view`;
- `proxy {} has no grant in any route or group: it is unusable` — the developer wrote `postgres`
  and `trait`;
- `route {}: body = "raw" passes the request body through unvalidated` — the file says `body raw`;
- `route {} may run any statement on proxy {} (unrestricted = true)` and
  `route {}: egress = true opens any outbound address and port` — `=` is not Blastfile syntax.

§10.2 already establishes the principle ("Only the nouns change: the old table path →
`view "<name>"`, the old boolean key → `body raw unvalidated`") but applies it only to
`raw_ingress_refusals`.

**Failure mode:** a developer reads `body = "raw"` or `unrestricted = true` in a boot warning, edits
the file to match, and gets `` `=` is not Blastfile syntax ``; "no route and no global groups" names
two things the file cannot contain.

**Suggestion:** give the four warnings Blastfile nouns in §13.2 (view/trait/store, `body raw`,
`unrestricted`, `egress any`) and drop "unchanged text" from §11.1; the warning codes and sites can
stay identical, so no test contract moves.

### M13 · MAJOR — the `Method` predicate has two definitions, and the `get` refusal §6.3 quotes is absent from §6.7/§18's closed variant list

§4.2 and §6.3: "a word whose **first byte** is `A`–`Z` is a `Method`". §6.1's production: "a `Word`
whose **every byte** is an upper-case letter, digit or tchar … v1's `valid_method` accepted the same
set". `valid_method` (`validate.rs:729`) requires every byte to be a non-lowercase `tchar`, so §6.1
is right and §4.2/§6.3 are a different predicate. Under "first byte", `Get "/x"` is a `Method` token
and lowers to a `Route.methods` entry no HTTP request can ever send.

Second, §6.3 promises the refusal "`` `get` is not a method: methods are upper-case, `GET` ``". That
row is **not** in §6.7, and §18 states "the new variants are **exactly** the rows of §5.3, §6.7,
§8.3, §11.3, §12 and §9.1" — so the implementer has no variant or message for a sentence the design
quotes, and the parser's own rule would emit the generic "`get` is not a directive here" instead.

**Failure mode:** the implementer writes the first-byte predicate and accepts `Get` (a dead route
that `check` then reports unexercised) or writes the every-byte predicate and has no refusal row for
`Gett`; §18's closed list cannot be satisfied either way.

**Suggestion:** make "every byte is upper-case `tchar`" the one predicate (as `valid_method` does),
delete the first-byte phrasing in §4.2/§6.3, and add the `get`/`Get` refusal to §6.7 (or drop the
quoted message and let the unknown-directive row apply).

---

## MINOR (no effect on the verdict)

- **m1 — broken cross-references.** §2 Non-Goals cites "§16.7" for the unknown-keyword refusal (the
  table is §6.7); §16.1 cites "§19.7" for the fence alternative (§19.7 is `workers`, §19.8 is the
  fence); §1's CI row cites "§14, §15, §17" for findings/exit codes (§17 is the security analysis;
  the codes are §14.4/§15.1).
- **m2 — `spa` survives publicly.** §9.3 says "the `spa` naming disappears from the model's public
  docs", but `AnswerKind::SpaFallback` is a public variant (`static_asset.rs:126`) and §9.2 renames
  only `SpaLost`.
- **m3 — `ADR-0012`'s summary counts and scopes differ from the design.** Decision 5 "gains exactly
  four things" vs §4.3's table (`name`, `mounts`, `StaticPolicy::fallback`, `php`, `sandbox`,
  `equivalent`, builders); Consequences "four independent caps" vs §4.5's six; decision 9 "at parse
  time" vs §8.4's `lower`; decision 12 "`docs/**`" vs §16.1's `PAGES` scope.
- **m4 — §14.2's empty-block parenthesis is false for a header view.** "(a trait with no grant is
  legal … a view block and a store block are refused by §6.7, so **only a trait** reaches this
  rule)" — §6.7 refuses only the no-endpoint form, and the `--learn` writer emits
  `view x GET "/x" { }` (§14.5), which must parse for P4/R25 to hold.
- **m5 — placeholder spelling.** §4.5 uses `<path>:<l>:<c>:`; §18 and R2 use
  `<path>:<line>:<col>:`.
- **m6 — `VISION.md`'s "Every route runs in its own sandbox"** vs §10.3 (one sandbox per view,
  several Rust `Route`s). The product invariant holds at view granularity, but §21's 5.1 only
  removes VISION's mention; consider re-wording the sentence to "view".

---

## Rejected with owner decision

No finding here asks for a compatibility path for the retired TOML format — no detector, second
reader, `--format` flag, converter, migration sentence or `toml`/`toml_edit` dependency. B1 asks to
keep `fold`'s `--manifest PATH`, which names **a conforming Blastfile path** and is pinned by
`ADR-0002` for an offline tool; that is a CLI/ADR consistency defect, not a migration path, and is
recorded here as rejected-by-owner if the owner prefers discovery for fold.

---

## Verdict

Three BLOCKERs (fold's required `--manifest` is retired against `ADR-0002` and the CLI reference;
the erasure guard has three mechanisms, two scan sets and two names, and the design's version cannot
pass `MB.21` acceptance 4; the leaf implementing §13.3 is briefed to ship a different `fix.action`
contract). Thirteen MAJORs (the erasure's owning leaf and the unowned `from_toml_str` callers; the
`workers` ADR sentence; the pinned fence counts; six stale §22 rows; two fabricated page quotations;
R10's "exactly as today"; the `fmt` config-error exit; `LiteralQuery` vs the page; the root-mount
`url_path`; `ARCHITECTURE.md` §2.4; the `--json` keys sentence; retired vocabulary in boot warnings;
the `Method` predicate and its missing refusal). Six MINORs. The capability map itself is complete
and the language adds no syntax the pages do not show; the defects above are contract drift between
the design, the accepted pages, the ADR and the leaf briefs, and several are one-sentence fixes.

VERDICT: FAIL
