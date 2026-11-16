---
lens: testability
document: docs/design/blastfile-dsl.md
round: 4 (final acceptance)
reviewer: critic (fresh context)
ground_truth: .worktrees/release-0.2.0
---

# Testability review — the Blastfile DSL (final acceptance round)

Read fresh: the design, `ADR-0012`, the accepted pages (`docs/blastfile.md`,
`docs/reference/blastfile.md`), `docs/VISION.md`, and the real tree — the
`plan/nodes/MB/MB.21/**/NODE.md` leaves with their actual `paths`, the real
`crates/*/tests/`, `python/tests/test_docs_assets.py`,
`python/tests/runnable_docs/inventory.toml`, `crates/blastit-manifest/tests/docs_fences.rs`,
the `Makefile` targets (`docs-unit`, `docs-test`, `rs-test`, `test-critical`,
`check`), and `tools/critical_guard.py`. The four `## 22 Review Resolution`
tables are treated as claims, not evidence; several entries (T-B1, T-B2, C-M7)
are contradicted by the tree.

Lens question: does every `R1..R29` have a named, runnable, unprivileged oracle;
does every named test/gate exist under a leaf that may actually write it; and is
every invariant written as an assertion that can fail? The answer is still no.

---

## BLOCKER

### B1. The erasure gate (R27–R29) still cannot pass: strict-set files and the harness itself are in no leaf's `paths`

R27 says the retired name appears nowhere in the *"strict set: `crates/`,
`python/`, `scripts/`, `e2e/`, `.opencode/`, `Makefile`, `README.md`,
`ARCHITECTURE.md`, `AGENTS.md` and **every** `docs/**` page."* §15.2 repeats it
and R29 makes `python/tests/test_docs_assets.py::the_retired_format_is_gone`
the proof. The leaf split in §21 does not cover the strict set:

- `MB.21.4.4` is narrowed to *"`e2e/**`, `python/tests/**`, `Makefile`"* and
  *"**no** deletion work"*; the new `MB.21.4.5` owns *"`crates/blastit-manifest/src/parse.rs`
  (deleted), `crates/blastit-manifest/Cargo.toml`, `crates/blastit-py/src/{lib,audit}.rs`,
  `python/blastit/{fold.py,__init__.py}`, `Cargo.toml`/`Cargo.lock`,
  `python/tests/test_fold.py`"*.
- The tree still carries the name in files outside both lists:
  `crates/blastit-env/tests/environment.rs:469`
  (`("BLASTIT_MANIFEST", "/app/blastit.toml")`) and
  `crates/blastit-sqlpolicy/src/learn.rs:4` (`saw into` `blastit.toml`).
  The real `MB.21.4.4` brief owns `crates/**` and names both files explicitly
  (lines 48–50); the design's split drops that ownership, so `MB.21.4.5` (as
  specified) may not edit them and 5.2's guard goes red.
- `crates/blastit-manifest/tests/critical_php_manifest.rs` is in the strict set
  (it still writes v1 bodies through `Manifest::from_toml_str`) and §12 says it
  is *"rewritten"*, but no leaf lists it: 3.0's path list omits it, 3.2's
  additions omit it, 4.2's `crates/blastit/tests/**` cannot reach the manifest
  crate, and 4.5's list omits it. The workspace cannot compile at 4.5 while it
  calls the deleted reader.
- `crates/blastit-manifest/tests/parse.rs` (the v1 reader's own suite) calls
  `Manifest::from_toml_str` and must be deleted with the reader; it is in 3.0's
  paths (but 3.0 must not delete the reader) and in no later leaf's paths.
- `docs_fences.rs` must assert §16.2's rule that no retired-format (`toml`) fence exists on any page in
  `PAGES`. It is a `crates/` file, so the harness has to contain the retired
  opening to detect it. The real `MB.21.4.4` brief already prescribes the fix
  (*"build its opening constant from two fragments … so the tree never carries
  the contiguous name"*); the design never states it, so a 3.6/5.2 implementer
  following §16.2 writes the literal and trips R27 in `crates/`.

**Failure mode.** At `MB.21.5.2` the guard fails on `blastit-env`/`sqlpolicy`
(and/or `docs_fences.rs`) with no leaf allowed to fix it; at `MB.21.4.5`
`cargo test --workspace` fails to compile `tests/parse.rs` and
`tests/critical_php_manifest.rs`.

**Suggestion.** Give 4.5 the real plan's `crates/**` + `scripts/**` (or list
every remaining file explicitly: the two hits above, `tests/parse.rs`,
`tests/critical_php_manifest.rs`, and the harness's construction rule), and say
in §16.2 that the retired fence opening is composed, never written contiguously.

### B2. The guard the plan names is not the guard the design names, and its scope contradicts the epic

The epic's acceptance is explicit:

> *"`test_the_real_tree_carries_no_retired_toml_manifest` in
> `python/tests/test_docs_assets.py` (run by `docs-unit`, and so by
> `make check`) is green, and it fails on a crafted page, **a live brief**, or a
> crate/script/fixture that names the retired file."*

The design names a different function (`the_retired_format_is_gone`, R29/§15.2),
exempts **all** `plan/**` (including live `status: ready` briefs), and omits the
epic's other records (`docs/changelog.md`, `docs/archive/**`) from the exempt
set while making *every* `docs/**` page strict. §21's closing note assigns
plan edits for 4.1's title and 3.2's oracle but not for this name/scope delta.

**Failure mode.** `make check` is green while the epic's own acceptance grep
(`plan/nodes --include=NODE.md`, filtering only `done`/`superseded`) still lists
the live `MB.21` brief, which contains the name throughout; and a 0.2.0
changelog sentence ("Blastfile replaces the TOML manifest") turns the guard red
with no exempt path.

**Suggestion.** Use the plan's function name (or add the plan edit to §21),
scope it like the existing PHP precedent (`live_briefs(root)`, only
`done`/`superseded` exempt), and add `docs/changelog.md`/`docs/archive/**` to
the record set. If all of `plan/**` must stay exempt, record that as an owner
decision and change the epic's acceptance, as was done for 4.1/3.2.

### B3. The pinned fence counts are wrong for pages that already exist, so R18 is red at `MB.21.3.6`

§16.2 says *"counts **measured once by the harness and then pinned** — the
design does not guess a page's numbers"*, then pins:

```rust
("docs/blastfile.md",            Counts { documents: 1, statements: 0, illustrations: 0 }, "MB.21.5.1"),
("docs/reference/blastfile.md",  Counts { documents: 0, statements: 14, illustrations: 1 }, "MB.21.5.1"),
```

Against §16.1's rule (*illustration = "any other fence, whatever its language
(`text`, `console`, `json`, `rust`)"*):

- `docs/blastfile.md` has one document fence **and three `console` fences**
  (§16.3 itself calls them *"the guide's three `console` fences"* and keeps
  their inventory rows). Illustrations = 3, not 0.
- `docs/reference/blastfile.md` has 16 `text` + 1 `json` fences; 14 are
  keyword-led statement lists and 3 are illustrations (2 `text` + the `json`).
  The pin says 1 illustration, and C-M7's resolution claims *"16 statement
  lists and 3 illustrations"* — three numbers for one page.
- `README.md` is pinned at 1/0/0 while it carries five fences today (one
  document, four others) and will keep console fences after 5.1.

Both pages predate 5.1 and 5.1 only fixes sentences, so these are today's counts.

**Failure mode.** `MB.21.3.6`'s acceptance is *"R18 against the current pages"*;
`docs_fences` runs inside `rs-test` (`cargo test --workspace --all-targets`), so
the wrong constants turn `make check` red for every leaf after 3.6, and a cheap
implementer may "fix" the page to match a guessed number.

**Suggestion.** Delete the sample numbers, require 3.6 to measure the current
pages and pin them, and add the assertion that would have caught this:
`documents + statements + illustrations == every fence on the page`.

### B4. `tests/blastfile_parse.rs` and `tests/blastfile_lower.rs` are named as the unit oracles of R4/R8/R20/R21 but belong to no leaf

§5: *"Proved by: `tests/blastfile_parse.rs` (unit, one test per production plus
one per §6.7 row)"*. §7: *"Proved by: `tests/blastfile_lower.rs` (unit, one test
per row)"*. §20 names them for R4, R8, R11 (`blastfile_lower.rs::body_table`),
R20/R24 (`names_are_never_resolved`) and R21
(`blastfile_parse.rs::no_partial_manifest`). `MB.21.3.2` is the only leaf that
can create them, and its additions are:

> *"`tests/blastfile_{traits,grants,body,entrypoint}.rs`, `tests/critical_blastfile_{parity,traits,body,entrypoint,security}.rs`, `tests/fixtures/**`"*

Neither `blastfile_parse.rs` nor `blastfile_lower.rs` is there, and the real
`MB.21.3.2` paths hold only `blastfile/**`, `lib.rs`, `model.rs` and
`critical_blastfile_parser.rs`.

**Failure mode.** The leaf's own acceptance (*"R4, R8, R11–R14, R20, R21"*) is
unreachable without writing outside `paths`; the lowering map (the core of the
design) has no unit oracle a leaf may create.

**Suggestion.** Add both files to 3.2's additions, or state that these are
`#[cfg(test)] mod tests` inside `parser.rs`/`lower.rs` and rename §20's cells
accordingly.

---

## MAJOR

### M1. The load-bearing round-trip invariants still cannot compile where §20 puts them

§4.4 declares `parse`, `lower` and `print_lossless` **`pub(crate)`**; §4.1 says
*"integration tests (`tests/blastfile_*.rs`) use only the public entry points"*.
Yet §20 names integration files as the unit oracles: R5
`blastfile_fmt.rs::p1_lossless`, R7 `blastfile_fmt.rs::p3_meaning_preserved`,
R8 `blastfile_lower.rs`. Those files cannot call `print_lossless`/`lower`.
Two spellings are also still stale:

- R5: `print_lossless(cst(text)) == text` — `cst` is defined nowhere.
- R7 and §20 R7: `lower(fmt(t)).equivalent(&lower(t))` — one argument, and
  `fmt` returns `Result`; §14.3's P3 already uses the two-argument form.

P1 in particular has no public equivalent, so it must be a module test inside
`syntax.rs`; P3 can use the public `Manifest::from_blastfile_str`.

**Suggestion.** Put P1 in `syntax.rs`'s `mod tests`, write P3/R7 against
`Manifest::from_blastfile_str`, and normalise R5/R7/§20 to the §4.4 signatures.

### M2. The ≥20 corpus floor is assigned to `MB.21.5.2`, whose `paths` cannot reach the test

§20: *"`MB.21.3.4` asserts P1–P3 over the fixtures 3.2 creates, with the exact
count those fixtures have; `MB.21.5.2` re-runs the same test over the migrated
corpus and asserts **≥ 20 files**"*. The test is
`crates/blastit-manifest/tests/critical_blastfile_print.rs` (3.4, protected).
5.2's paths are `python/tests/runnable_docs/**`,
`python/tests/test_docs_assets.py` and `docs_fences.rs`.

**Failure mode.** The floor is never raised; the vacuity guard (round-2 M10)
silently disappears, and the exact-count assertion then fails when 4.4 migrates
the e2e corpus without the test being allowed to update.

**Suggestion.** Add `critical_blastfile_print.rs` to 5.2's paths (and run
`critical_guard.py`), or move the floor into `docs_fences.rs`, which 5.2 owns.

### M3. Two gates declared to live in `make check` have no leaf that can add the Makefile line

§3 defines `gate` as *"a lint/script in `make check`"*.

- R24: tier `unit + gate`, oracle `scripts/check_manifest_purity.py`. 3.1
  creates the script but its paths contain no `Makefile`; `make check` has no
  line invoking it (`guard` runs `tools/guard.py`, `py-lint` runs ruff).
- R6: *"`fmt --check` in `make check`"*; 3.4 says *"the repository-wide check
  arrives with 4.5"*, but 4.5's paths have no `Makefile`, and 4.4 (which owns
  the Makefile) is scoped to migration and says *"no deletion work"*.

**Failure mode.** Both gates exist as files/sentences and never run in CI; the
purity invariant and repository-wide canonicality have no failing oracle.

**Suggestion.** Give 3.1 a Makefile edit for the purity target and 4.5 (or 4.4)
the `fmt --check` target, and name the target in each row's acceptance.

### M4. R18's "a fence cannot appear unlisted" is not what the oracle asserts

R18: *"docs_fences counts three fence classes per page … and asserts parity, so
a fence cannot appear unlisted."* The §16.2 oracle is
`count(page) == PAGES[page]` for **listed** pages, plus `TOTAL` = the sum over
`PAGES`. A new page under `docs/` with a complete fence is never looked at, so
the stated property cannot fail.

**Suggestion.** Assert `set(pages with fences) == set(PAGES)`, or derive the
page set from `public_pages()`/`docs/**/*.md` and fail on a page carrying a
fence but no row.

### M5. R13 contradicts §12.3, `ADR-0012` and the plan on the `workers` sentence

R13: *"its refusal names the removal (**no ADR number in the message**)"* and
C-m5: *"no ADR number in a message"*. §12.3: *"the refusal in §6.7, whose
sentence names the removal **and `ADR-0012`**"*; `ADR-0012` decision 7 and the
plan's settled unit 6 say the same. §6.7's actual row has no ADR id.

**Failure mode.** `critical_blastfile_entrypoint.rs` cannot be written
unambiguously: the exact string is either with or without `ADR-0012`, and two
requirements demand opposite answers.

**Suggestion.** Pick one string and fix R13, §6.7, §12.3 and `ADR-0012` together
(the plan and accepted ADR say "name the ADR", so adding it to §6.7 is the
smaller change; C-m5 then needs the stated exception).

### M6. `MB.21.3.0` names a file in the wrong crate, so its "workspace compiles" acceptance cannot pass

§21 3.0: *"`crates/blastit-scheduler/tests/{critical_rewrite,critical_static_manifest,critical_static_entry}.rs`"*.
`critical_static_manifest.rs` lives at
`crates/blastit-manifest/tests/critical_static_manifest.rs` (4.2 names the right
one). 3.0 removes `StaticPolicy::spa` and `Route::static_policy`, both used by
that suite (`spa: true`, `.static_policy()`).

**Failure mode.** 3.0's acceptance *"the workspace compiles with `-D warnings`"*
is unachievable within its paths; the protected suite is left for 4.2, which
depends on 3.4/3.5.

**Suggestion.** Move `critical_static_manifest.rs` into 3.0's manifest-tests
group and drop the scheduler-tests entry.

### M7. Two new boot behaviours have no requirement and no named test

§12.2/§13.2 introduce `ManifestWarning::EntrypointNotReached` (*"new"*), and §9.5
redefines `can_boot_without_app`/`is_all_static` so a static-only file *"must
boot"* with an empty route table. Neither appears in §20 or has an `R`; §20's
R10/R13 rows name routing and PHP-path tests only.

**Failure mode.** A regression that suppresses the warning, or a static-only
deploy that refuses to boot, has no failing test.

**Suggestion.** Add requirement rows and tests
(`critical_php_manifest.rs::php_entry_with_no_view_warns`,
`critical_static_manifest.rs::a_mounts_only_manifest_boots`); both files then
also gain an owner (B1/M6).

### M8. R23 still claims `todo!` is enforced; no lint covers it

R23 lists *"no `unsafe`, no `unwrap`/`expect`/`panic!`/`todo!`"* but the
enforcement is `#![forbid(unsafe_code)]` plus
`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]`.
`clippy::panic` does not catch `todo!`, and `clippy::todo` is absent.

**Suggestion.** Add `clippy::todo` (and `unimplemented`, if meant) to the deny
list, or delete `todo!` from R23's claim.

---

## Sound, and worth keeping as written

- R3a's mode-`000` over-cap fixture (*"`TooLarge`, **not** `PermissionDenied` —
  the observable 'the size was known before any read'"*) is a sharp,
  unprivileged oracle.
- R8's expected `Manifest` built in the test, with the dropped rows asserted
  **absent**, is the right shape and removes the retired-reader dependency.
- R22's positive `fix.to` assertions plus the absence checks (no `unvalidated`,
  no retired path prefix) is a genuine two-sided test.
- The two-set strict/exempt guard model and its negative self-test are the right
  pattern; B1/B2 are about the sets, names and scopes, not the pattern.

## Rejected by owner decision (recorded, not graded)

The compatibility path is out of scope. I did not raise, and would reject:
keeping a retired-format reader/detector for one release; a `--format` flag or
dual parse; a converter or migration paragraph for `fold.py`, CI or an existing
deployment; keeping `toml`/`toml_edit` "temporarily"; or making a missing/foreign
file print a friendlier hint. §15.2, §19.4 and `ADR-0012` decision 14 already
state the correct position. B1–B2 ask only that the *proof* of that erasure be
landable by the leaves that claim to own it.

VERDICT: FAIL
