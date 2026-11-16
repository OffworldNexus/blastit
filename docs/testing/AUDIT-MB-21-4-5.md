# AUDIT MB.21.4.5 — erasing the retired manifest format from Rust

- **Date:** 2026-11-16
- **Scope:** `crates/blastit-manifest` (`src/validate.rs`, `src/model.rs`, `src/routing.rs`,
  `src/error.rs`, `src/blastfile/**`) and `crates/blastit-sqlpolicy/src/{learn,tables}.rs`.
- **Base:** `2ef786ce381dce05208d8d4d09a50f8eb4d7589b` (MB.21.4.9); tip audited `f390bd2`.
- **Requirements:** design §15.2 (R27–R29), §21's `MB.21.4.5` row, §4.1 (`parse.rs` deleted with no
  replacement), §18 (the error contract), `ADR-0012` §12.2/§12.3; skills `testing`,
  `security-invariants`.

## Summary

**Verdict: PASS WITH GAPS.** 465 in-scope cases were read (337 integration, 128 unit tests; 150 of
the integration cases are protected). Every refusal that the retired reader carried still has an
assertion, and each one I sampled asserts both the allowed and the denied side. No tautology, no
skipped test, nothing misplaced. Twelve `ManifestError` variants and 1277 lines of `parse.rs` left
with their subjects, and the `Location`/`Display` contracts they fed are pinned by
`tests/mutation_gaps.rs` and `critical_blastfile_errors.rs`.

Two classes of gap remain, both proven by mutation rather than by reading:

1. **Dead validation code that no input can reach** — the per-route `check_static`/`check_php`
   walks and four variants (`StaticDeadDeclaration`, `PhpDeadDeclaration`, `PhpWorkersInvalid`,
   `PhpEntryInvalid`). They are the ~25 mutants below that *no file can kill*. `MB.21.4.5.2`'s
   follow-up asked `MB.21.4.5.6` to delete them; `5.6` deferred them to the `§4.3` pair removal
   (blocked on `MB.21.5.1`), so `validate.rs` keeps code and error variants no document reaches.
2. **Refusals pinned only at the union of two implementations** — the `root`/`index`/dotfile rules
   are implemented twice (`blastfile/lower.rs` and `validate.rs`) and the protected static suite
   asserts whichever stage refuses, on purpose (`critical_static_manifest.rs:77`, `refusals()`).
   Neither copy is pinned: a regression in one is invisible while the other still refuses.

## Method

- Inventoried all 28 test binaries and 11 unit-test modules; classified each case
  `critical`/`useful`/`fluff`/`misplaced`/`broken-oracle` against §4 of the `testing` skill.
- Mapped each of the 39 deleted `parse.rs` rows to its recorded successor (`MB.21.4.5.1`'s table),
  re-checked the 12 rows marked *no successor* against the grammar, and re-read every protected
  suite the epic rewrote.
- `cargo-mutants` is installed (`~/.cargo/bin/cargo-mutants`). The full package is **1395 mutants**
  (measured ~16 s/mutant at `-j3`, ≈6 h), so the campaign was scoped to the policy/validation
  modules and run through the same wrapper and caps `make mutants-rust` uses:
  `TMPDIR=$(make -s print-mutants-tmp) MUTANTS_JOBS=3 MUTANTS_TIMEOUT=90 MUTANTS_MEMORY_MAX=12G
  scripts/mutants-run.sh -p blastit-manifest -f <file>` (cgroup v2 scope active). No source or test
  file was edited; `git status --porcelain crates/blastit-manifest` stayed empty throughout.

| Module | Mutants | Killed | Viable | Kill rate | Survivors |
|---|---|---|---|---|---|
| `src/validate.rs` + `src/error.rs` | 173 | 125 | 162 | **77.2 %** | 36 (+1 timeout) |
| `src/routing.rs` | 29 | 17 | 22 | **77.3 %** | 5 |
| `src/blastfile/lower.rs` | 132 | 106 | 114 | **93.0 %** | 8 |
| **Total run** | **334** | **248** | **298** | **83.2 %** | **49** |

Not run (recorded, not guessed): `model.rs` 218, `parser.rs` 221, `write.rs` 226, `syntax.rs` 110,
`lexer.rs` 99, `print.rs` 93, `pattern.rs` 41, `blastfile/mod.rs` 18, `uid.rs` 15, `edit.rs` 15,
`tokens.rs` 5 — 1061 mutants, ≈4.5 h at the measured rate.

## Meaningful survivors

| # | Mutant | Why it survived | Class |
|---|---|---|---|
| 1 | `validate.rs:537` `check_static_declarations`→`()` | only reachable through `Route::with_static`; a `Blastfile` never sets it | dead |
| 2 | `validate.rs:710` `check_php`→`()` | `lower` sets `manifest.php()`, never `Route::php()` | dead |
| 3 | `validate.rs:741,742,750,753` `check_php_entry` | the rule is enforced by `lower.rs:362`; this copy is unreachable | dead |
| 4 | `validate.rs:773,775` `entry_is_under_root` (6), `780,782` `path_segments` (5) | only caller is the dead `check_php_entry`; §12.2 dropped the rule | dead |
| 5 | `validate.rs:792,795,797` `check_php_workers` (5) | `workers` has no spelling: `WorkersRemoved` at parse (`ADR-0012` §12.3) | dead |
| 6 | `validate.rs:815,829,832` `check_php_declarations` (3) | same walk; `PhpDeadDeclaration` is unconstructible from a file | dead |
| 7 | `validate.rs:590` `bridged` `==`→`!=` | no route carries a `StaticPolicy`, so the skip never fires | dead |
| 8 | `validate.rs:419,420,450` + `lower.rs:912,930,935,937` (7) | each copy is masked by the other; the suite asserts the union only | **meaningful** |
| 9 | `error.rs:1403,1404` `ManifestError::position`→NONE / arm deleted | no test calls `.position()`; its only caller is `cli.rs:557` | **meaningful** |
| 10 | `error.rs:82` `Position::col`→0 / →1 | the accessor has **no caller at all** (`Display` reads the field) | fluff API |
| 11 | `lower.rs:369` `byte < 0x20`→`<=` | no accepted php entry has a byte at 0x20, so a space would be wrongly refused | **meaningful** |
| 12 | `lower.rs:574` `"query"` arm deleted | unreachable: a `query` directive is a CST *node*, not a `Word`; `Clause::Query` is never set (proven — `row_query_is_stored_normalised` passes) | equivalent |
| 13 | `lower.rs:492` `store.is_none()`→`true` | no fixture writes two store words before the block | manual |
| 14 | `validate.rs:229` `||`→`&&` | equivalent: the mutated pair (`starts_with("information_schema.") && table == "pg_catalog"`) is unsatisfiable | equivalent |
| 15 | `error.rs:94` `Display for Position`→`Ok(())` | **timeout** at 90 s, not a demonstrated survivor | inconclusive |

## Protected (`critical`) tests: what holds

- **The security pairs hold.** `critical_blastfile_security.rs` asserts a `$IDENT` is a name and
  never a value (with `HOME` set, both the URL slot and a block slot) **and** that a refusal yields
  no manifest; `critical_blastfile_errors.rs` asserts escaping, the 120-char cut, one line per
  problem, and that no refusal quotes the PHP entry it refused.
- **The translated refusals hold, both sides.** `a_system_catalogue_table_grant_is_refused` covers
  six refused names, a trait's grant, the model-only qualified name, *and* the accepted
  `audit.pg_class`; `critical_manifest_011` keeps all 17 cases with 62→70 assertion lines;
  `critical_static_manifest` pins both ends of `byte < 0x20` at 0x00 and 0x1f for root, index,
  immutable, fallback and php entry.
- **The `for_learn` invariants are protected**, but not in this crate: slot 0 never matches and
  declared routes sit at `1..=N` in `routing.rs`'s doctest and `stage1::tests` (compiled only with
  `--features python`, which `make rs-test` passes), and `methods_for`'s 405 union only in
  `crates/blastit-scheduler/tests/critical_blastfile_parity.rs:755`.

## Broken oracles (none proven; two weak oracles)

1. `critical_static_manifest.rs:77 refusals()` — stage-agnostic by design; it cannot tell the
   lowering copy from the validation copy, which is exactly what survivor 8 shows.
2. `docs_fences.rs` — counts the eight retired fences instead of executing them
   (`THE_RETIRED_OPENING_IS_GONE = false`). Deliberate and recorded by `MB.21.4.5.3`; `MB.21.5.2`
   flips it strict.

## Gaps (proposals, id-free)

1. Add a `blastit-manifest` test pinning `ManifestError::position()` for a positioned and a
   position-less variant, plus the `blastit fmt` spelling `./Blastfile:2:10: …` (`cli.rs:557`).
2. Add an accepted php-entry twin containing a space (`entrypoint php "pub lic/index.php"`), so
   `check_php_entry`'s `byte < 0x20` is pinned at 0x20.
3. Pin the entry points separately for the static key rules: assert `from_blastfile_str` (lowering
   only) refuses a bad `root`/`index`, so neither copy of the duplicated rule is masked.
4. Add `RoutingTable::methods_for` unit tests (union, declaration order, dedup, `None`): 4 of the 5
   `routing.rs` survivors, today covered only from the scheduler crate.
5. Delete the dead validate arms and their four variants (`check_static_declarations`, `check_php`,
   `check_php_entry`, `entry_is_under_root`, `path_segments`, `check_php_workers`,
   `check_php_declarations`, `check_mounts`'s `bridged`) or test them through the model.
6. Delete the unreachable `"query"` arm in `lower.rs::pg_grant`.
7. Add a two-word grant header case (`postgres db extra { … }`) so the `store.is_none()` guard is
   pinned, or refuse it explicitly.
8. Replace `TableGrants::to_toml` with a `Blastfile` grant-block renderer, drop the `"toml"` key
   from `blastit check --json`, and re-point `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:740,761`,
   `python/tests/test_fold.py`, `python/tests/test_pytest_plugin.py:1115` and `audit.rs:110`.
9. Assert one end-to-end `--learn` attribution (a request to declared view *N* is attributed to
   view *N*'s name), so `for_learn`'s shift and `stage1`'s `RouteInfo` prepend cannot drift apart.

## Carried forward (MB.21.5)

1. `docs_fences::THE_RETIRED_OPENING_IS_GONE` is still `false`; the eight retired fences are counted,
   not executed, until `MB.21.5.2` (pages: `MB.21.5.1`).
2. `Route::static_policy`/`Route::php`/`with_static`/`with_php` (§4.3) — removal blocked on
   `MB.21.5.1`; it is what keeps the dead validate walks compiling.
3. `Proxy::output` and `blastit-env`'s output-map rendering (`default_outputs`, the rename map) —
   no reader can produce the field after this epic.
4. `TableGrants::to_toml` and the `"toml"` key of `check --json` — the retired `[route.db.tables]`
   block is still rendered to users, with "paste this in" advice the parser now refuses.
5. Doc comments still spelling `[route.ingress]`/`[route.static]` in
   `crates/blastit-scheduler/src/{scheduler,rewrite,static_asset}.rs` (MB.21.4.5.5 follow-up).

## Notes

- The epic's acceptance greps cannot see `to_toml`, `TableGrants`' renderer or the `"toml"` JSON key:
  none spells `toml::` or `blastit.toml`. The format is gone from the *reader*; one *writer* remains.
- `blastit-manifest` and `blastit-sqlpolicy` were clean before and after every run; one sibling
  process was building `crates/blastit` concurrently and no out-of-scope failure occurred.
- `model.rs`, `parser.rs` and the formatter (`print.rs`/`write.rs`) were not mutated for time; the
  survivors above are all in modules that were.
