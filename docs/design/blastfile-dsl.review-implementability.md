# Implementability review — The Blastfile Language (acceptance round, fresh context)

Reviewed: `docs/design/blastfile-dsl.md` (all 22 sections), `docs/decisions/ADR-0012-blastfile-dsl.md`,
the accepted pages `docs/blastfile.md` / `docs/reference/blastfile.md`, and every brief under
`plan/nodes/MB/MB.21/**` (MB.21, 21.3.1–3.6, 21.4.1–4.4, 21.5.1–5.4). Each path, symbol, test name,
count and leaf id the design asserts was checked against the worktree. The plan tree is treated as
ground truth for leaf ids, `paths` and acceptance text.

Lens: can a cheap implementer build each leaf in ≤ 600 lines, from this document, without guessing?
Findings are limited to the four classes that block that: a rule that rejects a documented example, a
leaf that cannot compile or be built in budget, an ambiguous seam with no correct guess, and a cited
site that does not exist.

**Dependency / licence check passes.** No crate is added (§2), so no new SPDX surface; `regex`,
`serde`, `thiserror` are already workspace members. The only dependency removal (`toml`) is a single
serialized leaf. Nothing in the design asks for a compatibility path for the retired TOML format; see
"Rejected under the owner decision" at the end.

Summary: **3 BLOCKER · 9 MAJOR**.

## BLOCKER

### B1. `MB.21.3.0` cannot compile with its declared paths, and the 3.0a/3.0b split cannot either

§21 assigns model parity to one leaf:

- "**Model parity, alone.** `Manifest::name/mounts/php/equivalent`, `StaticMount`,
  `StaticPolicy::fallback`, `Route.sandbox`, **remove `Route.static_policy`/`php`**" (§21,
  `MB.21.3.0`).
- Acceptance: "the workspace compiles with `-D warnings`" (same row).

Measured in the worktree, these files call something the leaf removes or changes, and **none** is in
the leaf's `paths` (`model/routing/uid/validate/error/parse/edit.rs`, nine manifest test files, four
scheduler sources, three scheduler suites, `crates/blastit/src/{config,stage0,check,audit_v1}.rs`):

| File | What breaks | Count |
|---|---|---|
| `crates/blastit-manifest/tests/critical_static_manifest.rs` | `StaticPolicy { …, spa: true }` at :142, :154; `route.static_policy()` at :97, :509, :571 | 9 |
| `crates/blastit-manifest/tests/critical_php_manifest.rs` | `route.php()` at :58, :89, :198, :237, :247; `route.static_policy()` at :190 | 9 |
| `crates/blastit-manifest/tests/docs_fences.rs` | `StaticPolicy { …, spa: true }` at :140–143 | 5 |
| `crates/blastit/src/stage1.rs` | `route.static_policy().cloned()` and `route.php().is_some()` at :324–346 | 6 |
| `crates/blastit/src/php_view.rs` | `route.php()` at :296 | 1 |
| `crates/blastit/src/dev_learn.rs` | `app.static_policy()` / `app.php()` at :1043–1047 | 2 |

The row also misquotes one of its own paths: it lists
`crates/blastit-scheduler/tests/{critical_rewrite,critical_static_manifest,critical_static_entry}.rs`,
but `critical_static_manifest.rs` lives in `crates/blastit-manifest/tests/` (4.2's row has the right
crate), so the real suite is left unowned while a nonexistent path is claimed.

The split the row offers does not repair this: "split into `3.0a` (`model/routing/uid/validate`) and
`3.0b` (the scheduler and `blastit` call sites)". `3.0a` removes `spa` and the accessors, so
`critical_static_manifest.rs`, `critical_php_manifest.rs`, `docs_fences.rs` and `stage1.rs` fail to
build *at 3.0a*, which does not own them. Either leaf is red, and each later leaf assumes a green
tree (MB.21.3's acceptance 2: "no leaf needed a later leaf to be green").

Fix: make 3.0a **additive only** (add `name`, `mounts`, `fallback`, `sandbox`, `equivalent`, keep
`spa`/`static_policy`/`php`), then make 3.0b the removal and give it every consumer: the three
manifest suites, `docs_fences.rs` (or defer `StaticPolicy`'s shape change to 3.6, which owns that
file), `stage1.rs`, `php_view.rs`, `dev_learn.rs`,
`crates/blastit/tests/critical_learn_executor_blocks.rs`. State the two path sets in §21 and correct
the scheduler-test row.

### B2. `MB.21.3.2` cannot be built in its own ≤ 600-line / ≤ 8-file budget

§19.1: "Hand-written is **~1,100 lines across ten functions**". §21 gives that parser, **and**
lowering (§7–§13), **and** nine test files **and** the fixture corpus to one leaf:

> `MB.21.3.2` | LL(1) parser **and** lowering to the existing `Manifest` (§6, §7, §10–§13) … **added:**
> `tests/blastfile_{traits,grants,body,entrypoint}.rs`,
> `tests/critical_blastfile_{parity,traits,body,entrypoint,security}.rs`, `tests/fixtures/**`

The leaf's own acceptance caps it at "Files: `src/blastfile/{parser,lower}.rs` … and the one new
protected suite (**≤ 8 files, ≤ 600 changed lines**)". The design assigns ≥ 12 test files plus 15
fixtures (§20: "one file per §7.2 shape") on top of ~1,100 lines of parser + lowering. The arithmetic
cannot close, and this is not a preference: the leaf's acceptance is a literal count.

Round 3's finding I-B1 named exactly this ("3.2 is over the 600-line budget") but §22 resolves it by
splitting **3.0** and **4.4**; 3.2 is left whole. §21's preamble still says "**Two** leaves need a
split".

Fix: split 3.2 into 3.2a (parser + CST accessors + `blastfile_parse.rs`,
`critical_blastfile_parser.rs`) and 3.2b (lower + fixtures + `blastfile_lower.rs`,
`critical_blastfile_parity.rs`, the traits/grants/body/entrypoint suites), give each an explicit path
list, and update 3.3–3.6's `depends_on`. Or raise the leaf budget in the brief explicitly — but then
say so, because the current acceptance contradicts it.

### B3. §16.1's statement-list rule rejects three fences on the accepted reference page, and the pinned counts are wrong

§16.1 classifies a fence as a statement list when its body starts with a keyword, and checks it with
`Manifest::from_blastfile_str`. §4.4 defines that entry point as "**parse, lower**; no validate", and
§7.3's lowering performs cross-statement checks:

- step 2: "A second store named `db` is `DuplicateProxyName`";
- step 3: "An undeclared store inside a trait is `GrantOnUnknownProxy` with the trait's location".

`docs/reference/blastfile.md` then contains three keyword-opening fences that lowering must reject:

| Fence | Why it fails `from_blastfile_str` |
|---|---|
| `:113–125` (Stores) | `postgres $DATABASE_URL` and `postgres { host … }` are two unnamed stores → both lower to `db` → `DuplicateProxyName` |
| `:289–299` (connection setup) | `trait connection { postgres { query … } }` with no store declared → `GrantOnUnknownProxy` |
| `:325–332` (trait session) | same |

So `MB.21.3.6`'s acceptance 3 ("with today's pages untouched it is green") and `MB.21.5.2`'s fence
gate cannot be green, and the accepted page may not be edited to suit the harness (§16.1 itself says
the pages are the contract). The intent — "a fragment still proves the syntax it shows" — is a
*parse*-only check; the chosen entry point does parse + lower.

`MB.21.3.6`'s brief also does not know about this class at all: it requires a ```` ```toml ```` fence
to keep passing and recognises ```` ```blastfile ````, while §16.1 forbids both.

Two independent errors make 3.6 red on the first run, which confirms the rule was not measured:

- §16.2 pins `docs/reference/blastfile.md` at `{ documents: 0, statements: 14, illustrations: 1 }`.
  Measured: 14 statements and **3** illustrations (the EBNF at :17, the JSON at :442, the error line
  at :585).
- §16.2 pins `docs/blastfile.md` at `{ documents: 1, statements: 0, illustrations: 0 }`. Measured:
  1 document + **3** console fences (:37, :59, :87), and §16.3 itself says "the guide's three
  `console` fences".

Fix: check fragments with a parse-only entry point (expose `blastfile::parse`, or a
`Manifest::check_blastfile_syntax`), and either drop the illustrative counts or pin the measured
values; add the "zero `=`/zero retired spelling" rule to 3.6 only after the pages are migrated (5.1).

## MAJOR

### M1. The P1/P3/P4 oracles call `pub(crate)` items from integration-test crates

§4.4 declares `pub(crate) fn parse/print_lossless/lower`, and §4.1 says "integration tests
(`tests/blastfile_*.rs`) use only the public entry points". But §14.3's P1/P3 and §14.5's P4 are
expressed as `print_lossless(&parse(text)?, text)`, `lower(&parse(&fmt(text)?)?, …)` and
`verify(&lower(&parse(&edited)?, &edited)?, …)`, and §20/§21 place them in
`crates/blastit-manifest/tests/{blastfile_fmt,critical_blastfile_print}.rs` — integration crates that
cannot see `pub(crate)` items. `verify` is also never given a signature (today's `edit.rs::verify`
takes three arguments).

Failure mode: the protected printer suite does not compile, or the implementer silently widens the
public API (contradicting §4.4's table) to make its own acceptance pass.

Fix: state the test surface explicitly — either `#[doc(hidden)] pub fn parse/print_lossless/lower` in
`blastfile/mod.rs`, or move P1–P4's corpus assertions into `#[cfg(test)] mod tests` inside
`syntax.rs`/`print.rs`/`lower.rs` (which 3.4/3.2 own) and keep only `fmt`-level assertions in
`tests/`.

### M2. Test artefacts §20 names have no owning leaf or no such file

- `blastfile_parse.rs` and `blastfile_lower.rs` are the unit oracles for R4, R8, R11, R20 and R21
  (§6, §7, §20) but appear in **no** leaf's `paths`; 3.2's addition list is
  `blastfile_{traits,grants,body,entrypoint}.rs` only.
- §15/§20/§21 name `crates/blastit/tests/blastfile_discovery.rs`, while the real `MB.21.4.1` `paths`
  contain `crates/blastit/tests/critical_blastfile_discovery.rs`. Both are "added" in the same row, so
  the protected suite the brief actually owns is never mentioned.
- §21 4.1 lists `critical_config_and_presets.rs` as "**added**" although it is in the brief's `paths`
  already and exists today.
- §20 says "`MB.21.5.2` re-runs the same test over the migrated corpus and asserts **≥ 20 files**", but
  5.2's paths are `python/tests/runnable_docs/**`, `python/tests/test_docs_assets.py` and
  `docs_fences.rs`; the corpus test lives in
  `crates/blastit-manifest/tests/critical_blastfile_print.rs`, which 5.2 may not touch.

Failure mode: T-B1's class recurs — a cheap implementer creates a second, near-duplicate discovery
suite, or the ≥ 20 floor is never raised (the vacuity the floor exists to prevent).

Fix: add both unit files to 3.2's additions, replace the discovery name with the brief's, drop the
false "added", and give the floor to the leaf that owns `critical_blastfile_print.rs` (5.1 for the
corpus, or 4.4).

### M3. Two cited code sites do not exist

- §14 and §15 name `crates/blastit/src/fmt.rs` ("the command"). No such file exists, and 3.4's
  `paths` are `cli.rs`/`main.rs`/two test files, so the leaf may not create it.
- §21 3.0 names `crates/blastit-scheduler/tests/critical_static_manifest.rs`; the real file is in
  `crates/blastit-manifest/tests/`.

Failure mode: the implementer must guess whether the `fmt` arm lives in `cli.rs` (allowed) or a new
module (lane violation and a module-registration edit nobody owns), and one of 3.0's "update this
suite" instructions points at nothing.

Fix: either add `crates/blastit/src/fmt.rs` to 3.4's paths, or write "`cli.rs` (the `fmt` arm)" in
§14/§15; correct the scheduler-test path.

### M4. The `MB.21.4.4` → `4.4`+`4.5` split is not propagated to the briefs or to §4.1/§15.2

§21 says "`MB.21.4.4` keeps the *fixture translation* and hands the *erasure sweep* to a new
`MB.21.4.5`" and marks 4.4 "**no** deletion work". But:

- §4.1 still says `parse.rs # DELETED by MB.21.4.4`; §15.2's removal columns say `MB.21.4.4` for the
  reader, the PyO3 surface and the dependency.
- Real `MB.21.4.4`'s goal still says "**Delete the v1 path last, in this leaf**", its acceptance 2
  requires `grep toml::` to print nothing and `cargo tree | grep -ci toml` to print `0`, and
  `MB.21.4`'s acceptance 4 requires "`Manifest::from_toml_str` no longer exists". A 4.4 that obeys
  §21 fails its own acceptance.
- `MB.21.5.1` has `depends_on: [MB.21.4.4]`; if the reader survives until 4.5, 5.1 runs before the
  tree is clean.
- §21's closing note enumerates only two re-words (4.1's title, 3.2's oracle). 4.4's title, goal and
  acceptance, `MB.21.4`'s children table and acceptance 4, and 5.1's `depends_on` all need edits that
  the design does not record.

Failure mode: the cheapest implementer of 4.4 deletes the reader to satisfy its own brief (which
§21 says is 4.5's work) and collides with 4.5's paths, or follows §21 and fails 4.4's acceptance.

Fix: add the five plan edits to §21's closing note verbatim (`plan.py edit` targets: 4.4 title/goal/
acceptance, MB.21.4 children + acceptance 4, 5.1 `depends_on`, and §4.1/§15.2's "Removed by" cells).

### M5. `fold.py` is assigned to two different leaves, and neither assignment's paths allow it

§13.3: "`MB.21.4.3` (a) adds one PyO3 function … (b) **re-points `fold.py` at it and retires its
`--manifest` flag** in favour of discovery, and (c) keeps its statement policy unchanged". §21's 4.3
row adds `crates/blastit-py/src/lib.rs` and `python/tests/test_check.py` — **not**
`python/blastit/fold.py` and **not** `python/tests/test_fold.py`. §21's 4.5 row claims
`python/blastit/{fold.py,__init__.py}` and `python/tests/test_fold.py` for itself.

The real `MB.21.4.3` brief's goal includes "`blastit-manifest fold` … write and read `Blastfile`", and
its `paths` (check/dev_learn/audit_v1, blastfile/**, critical_check_engine,
critical_learn_executor_blocks, audit_group_hint, `python/tests/critical/test_check_contract.py`)
contain no Python fold file either.

Failure mode: a leaf whose brief says it re-points `fold` cannot edit `fold.py`; the Python fold
writer keeps the retired reader longer than the design says, and `python/tests/test_fold.py` (which
encodes the fold contract) is rewritten by the leaf that deletes the module.

Fix: give `fold.py` + `test_fold.py` to 4.3 (with the paths added), and leave 4.5 the deletion of the
`tomllib` path only; or move the whole fold re-point to 4.5 and remove it from §13.3 and 4.3's goal.

### M6. `MB.21.5.2`'s strict fence flip and guard test name contradict §16.1, §15.2 and ADR-0012

The design's end state: pages open documents with ```` ```text title="Blastfile" ````, "a docs test
asserts **no** page in `PAGES` opens ```` ```blastfile ````", and 5.2's strict flip "removes the
`blastfile` alias" (§16.1; ADR-0012 decision 12 refuses the `blastfile` opening for lack of a Pygments
lexer).

Real `MB.21.5.2`: "makes any complete-manifest fence whose opening is **not the `blastfile` one** a
failure naming the page and the expected opening" (goal and acceptance 3) — the exact inverse. There
is no way to satisfy both documents; the design records no re-word for 3.6 or 5.2 (only 4.1 and 3.2).

The guard has the same problem in two places: §15.2/R29 and 5.2 put it in
`python/tests/test_docs_assets.py` (correctly, "no new file and no new Makefile target"), while
ADR-0012 decision 14 — which the design calls "the one source of truth … plus `ADR-0012`" — names
`scripts/check_retired_format.py` "(gate `make check`)", a script 5.2's brief explicitly forbids. And
the guard's name differs: design `the_retired_format_is_gone` vs `MB.21.5.2` acceptance 4 and
`MB.21` acceptance 4, `test_the_real_tree_carries_no_retired_toml_manifest`.

Failure mode: whichever spelling the implementer picks, one acceptance fails, and the guard's home is
decided by which document is read first.

Fix: state the opener once and re-word 3.6/5.2 in §21's closing note (the design's `text
title="Blastfile"` is the one the pages already ship); correct ADR-0012 decision 14 to the pytest
home; adopt the brief's test name or re-word both epic and leaf acceptance.

### M7. `check`'s action vocabulary still differs between §13.3 and `MB.21.4.3`

§13.3 and R22 fix the finding vocabulary: `fix.action` values stay `add_query`, `add_group`,
`remove_query`, `declare_ingress`/`fix_ingress`, and "**only `fix.to` values are re-spelled**". The
reference page agrees (`docs/reference/blastfile.md:429–437`: `add_group` for connection setup).

Real `MB.21.4.3` says the opposite: "connection-setup SQL → **`add_trait`** … rather than
`add_group`"; "`unrouted` → **`add_view`**"; "a body … → **`add_body`**", and its acceptance 3 asserts
the exact strings. Its `to` example is `view "restaurants" → postgres "db"`, not §13.3's
`view catalog postgres`.

Failure mode: the protected `python/tests/critical/test_check_contract.py` and the page pin one
vocabulary; the leaf's acceptance pins another. An implementer of 4.3 renames the actions, and R22's
own `audit_v1.rs` unit assertions (§20) fail.

Fix: pick one vocabulary and write it in §13.3, R22, the page and 4.3's brief; if the brief's names
win, say that the action **names** change (not only `to`), and add the re-word to §21's note.

### M8. §9.4's `url_path` formula is undefined for `static "/"` and produces an empty segment

§9.4: "record `url_path` as **`prefix + "/" + relative`**", with `relative` never defined (§9.2 only
says "strip = prefix removed from the decoded path").

- For `static "/"`, `relative` is `assets/app.js` (prefix `/` stripped) → `//assets/app.js`.
- For `static "/assets"`, `relative` is `/app.js` (only the prefix removed) → `/assets//app.js`.

Both results contain the empty segment §9.2 now refuses, and no `*`-glob matches a path with `//` in
it, so `immutable` never fires. This is not hypothetical: the accepted
`docs/frameworks/static-and-spa.md` is `static "/"` + `immutable = ["/assets/*"]`, and §16.2's canary
pins exactly that shape ("prefix `"/"`, … `immutable ["/assets/*"]`").

Failure mode: the most common mount silently loses its year-long cache header, and the canary that is
supposed to catch the drift asserts the broken model.

Fix: define the concatenation once, e.g.
`url_path = if prefix == "/" { relative } else { format!("{prefix}{relative}") }` with `relative`
explicitly the decoded path with the prefix removed (leading `/` retained), and add both mounts to
§9.4's assertions.

### M9. §6.1's `patterns?` accepts a methods-only header that no rule lowers or refuses

§6.1 (authoritative): `view_header → methods patterns? | patterns`. So `view p GET { GET "/x" }`
parses: header methods, no header pattern, block of endpoints. §6.3 describes the lookahead as
"single signature with methods, then patterns, then `{`", which is stricter than the grammar; §6.7
refuses `view products GET` (no pattern, no block) and `view p GET "/p" { POST "/x" }` (header pattern
plus endpoint), but says nothing about header methods plus endpoints.

Failure mode: `lower` must invent whether header methods apply to every endpoint (merge), are
overridden by each endpoint, or are an error; `fmt` must decide whether to preserve the header. Two
implementers produce two models, and R8's parity table has no row for the shape.

Fix: make it `view_header → methods patterns` and add the §6.7 row (`` `view "p"` lists methods but
no pattern; write the path it answers ``), or define header-methods-apply-to-all-endpoints in §10.3
and add it to §6.7's exclusions.

## Checked and clean (no finding)

- §4.4's two parse entry points, `read`, `fmt`, `equivalent`, `apply_to` and `write_atomic` are
  consistent with today's `edit.rs`/`dev_learn.rs`/`check.rs` call shapes; `ManifestEdits` is already
  `serde`-derived, so `blastfile_apply_edits(source, edits_json)` needs no new Python export
  (`fold.py` imports `blastit._blastit.*` directly).
- §9.1's prefix/root/index/fallback shape rules, §9.6's `static_roots`/`check_static_roots` pair and
  §10.2's single `raw_ingress_refusals` sentence are buildable as written; both functions exist.
- §15.2's strict/exempt sets match `MB.21.5.2`'s scan list (`README.md`, `ARCHITECTURE.md`,
  `docs/VISION.md`, `crates/**`, `python/**`, `scripts/**`, `e2e/**`, `Makefile`, live briefs) apart
  from the guard-home conflict in M6.
- §16.3's inventory edit is correctly owned: `python/tests/runnable_docs/inventory.toml:310` carries
  the retired page's `[[page]]`, and both language pages are already registered (:68, :313).
- §19.4/§15.2's erasure matches the owner decision; no detector, converter, `--format` flag or
  migration sentence is specified anywhere in the document.

## Rejected under the owner decision (not a finding)

**A compatibility path for the retired TOML format** — detector, second reader, `--format` flag,
converter, migration sentence, key map, or a "coming from" page: **rejected, out of scope by owner
decision** (§15.2, §19.4, ADR-0012 decision 14). Nothing in this review asks for one. The only
remaining briefs that imply a second reader are `MB.21.3.6`'s "a ```` ```toml ```` fence must still
pass" and `MB.21.5.2`'s inverted strict flip; both are plan-text re-words (M6 and B3), not a
compatibility surface.

VERDICT: FAIL
