---
lens: testability
target: docs/design/view-crates-architecture.md
adr: docs/decisions/ADR-0016-view-crates-over-wire.md
round: 7 (convergence sign-off)
verdict: PASS
blockers: 0
majors: 0
minors: 9
---

# Testability review — `view-crates-architecture` (round 7, convergence sign-off)

- **Lens:** `testability` — every requirement maps to a fast unprivileged test, a protected
  critical test, and (where the claim is security/CVE) an exploit-reproduction test; every
  oracle can fail, and none can pass vacuously.
- **Document:** `docs/design/view-crates-architecture.md` (591 + 460 lines) and
  `docs/decisions/ADR-0016-view-crates-over-wire.md` (140 lines).
- **Tree:** `.worktrees/release-0.2.0` at 2026-11-17. Every number below is re-measured, not
  copied from the document.
- **Verdict:** **0 BLOCKER, 0 MAJOR, 8 MINOR** → gate passes (skill `design-docs` §3.3).

This is the last gate. The five convergence items (T1–T5) were checked first, then §4.5's
counts, then the whole R1..R18 oracle table for oracles that cannot fail or cannot pass.

## 1. Convergence items — all five verified

| Item | Claim | Evidence in the tree | Result |
|---|---|---|---|
| **T1** | One census, **four** names: `blastit-view-python/src/zygote.rs`, `blastit-view-php/src/spawn.rs`, `blastit-view-php/src/probe.rs`, and `blastit-view-php/src/pinning.rs`'s `#[cfg(test)]` call through `ChildPlan::for_test(io)`; `stage1::prepare_child` out of scope; §9's Round-6 row points at it | §4.4 D1 (409–416), §7 R4 (769–774), §9 Round 6 (1030). The three production sites exist today as three separate hand-rolled sequences (`zygote.rs:174-179`, `php_view.rs:713-720`, `php_view.rs:1056-1061`), and init's role children are separate (`stage1.rs:1080`, called at `:1037`/`:1064`). The `Shared`/`Off` census stays "three production sites, nothing in any view crate" because `for_test` lives in `blastit-sys`, outside the grep's scope | ✅ consistent (one spelling nit, T-2) |
| **T2** | CLI total is a **floor**; `critical_dev_reload` is **14 cases / 1 619 lines** today | §4.5 totals (668–672). Measured: `wc -l` = 1 619, `grep -cE '^\s*#\[test\]'` = 14 | ✅ totals paragraph correct (the row above it is stale, T-3) |
| **T3** | R17 uses `'Cargo.toml' '*/Cargo.toml'` | §7 R17 (787). Empirical: `git ls-files -- 'Cargo.toml' '*/Cargo.toml'` returns 17 files — root, all 14 crates, `e2e/php-sapi/seccomp-exec`, `editors/zed`. Git's pathspec `*` crosses `/`, so the nested manifests are covered | ✅ |
| **T4** | §4.5 names `critical-skip-check` as the third guard, with why `--list` alone cannot replace it | §4.5 (639–641). `Makefile:314` defines it, `Makefile:329` makes `test-critical` depend on it, and `CRITICAL_SKIP_PATTERN` (`Makefile:111`) covers `#[ignore]`, `needs_root(`, `pytest.mark.skip`, `importorskip(`, `unittest.skip`. No protected suite carries `#[ignore]` today (`php_native.rs:40` is non-protected) | ✅ |
| **T5** | R16 asserts the leaf's `Cargo.lock` ownership via `git show --stat` | §7 R16 (786) | ✅ present (one leaf cannot satisfy it literally, T-5) |

## 2. §4.5 counts — every per-row number matches the tree

Re-measured with `wc -l` and `grep -cE '^\s*#\[(tokio::)?test\]'`:

| Row | Doc | Tree |
|---|---|---|
| `critical_static_assets` | 14 / 1 152 | 14 / 1 152 ✅ |
| `critical_static_resolve` | 18 / 1 145 | 18 / 1 145 ✅ |
| `critical_static_entry` | 6 / 376 | 6 / 376 ✅ |
| `static_fixture` | — / 259 | 0 / 259 ✅ |
| `critical_loaded_files` | 2 / 443 | 2 / 443 ✅ |
| `serve` / `body` / `concurrency` / `environ` / `flow` / `gil` / `support` | 30/10/4/12/7/1 / 772/291/202/388/302/111/578 | identical ✅ |
| `php_native` | 1 (`#[ignore]`) / 247 | 1 / 247 ✅ |
| `critical_static_cli` / `critical_php_backend` / `critical_php_embed` / `critical_parallel_views` | 12/6/24→25/4, 777/1 101/4 052/1 245 | 12/6/24/4, identical lines ✅ |
| `common/libphp_probes` | 3 | 3 ✅ |
| `critical_dev_reload` row | 12 → ≥12, 1 407 | **14**, **1 619** (T-3) |
| `php_boot_gate` / siblings | 27 / 4+4 | 27 / 4+4 ✅ (modules at `stage0.rs:4371`/`:6082`/`:6224`) |

The 27/9/7-vs-24/6/4 explanation is real: `#[path = "common/libphp_probes.rs"]` appears in
`critical_php_embed.rs:140`, `critical_php_backend.rs:51`, `critical_parallel_views.rs:69`,
and that helper holds 3 cases. The four `include_str!("…/src/stage0.rs")` sites are exactly
the four the doc names (`libphp_probes.rs:133`, `critical_php_{embed,backend}.rs:869/226`,
`critical_parallel_views.rs:319`), and the two relative depths in §4.2.3 (`../../../` from
`tests/common/`, `../../` from `tests/`) resolve correctly to
`crates/blastit-view-php/src/discovery.rs`.

§4.2's in-file unit-case column is also exact (wire 6, flow 8, loaded 22, runtime 5, config 4,
lib 6, environ 11, input 11, request 4, response 5, php 22, path 5, immortal 17, zygote 0) —
which matters because R15's "no protected case lost" is only half the oracle; this column is
the other half.

## 3. Findings

### T-1 (MINOR) — case 5 asserts a uid the drop sentence does not carry

> "(5) `DropTo(uid_base + 4242)` exits non-zero with the **drop step's sentence** on stderr
> (the uid it tried to become), not a `prepare`/`setpgid`/filter sentence" (§5)

The sentence is `` #[error("dropping privileges failed at `{stage}`: {source}")] ``
(`crates/blastit-sys/src/error.rs:22`), built by `priv_drop(stage)` (`privs.rs:133`) from a
stage name and `errno` only; `prepare_child` prefixes the **role** name, not the uid
(`stage1.rs:1093`). So a literal `assert!(stderr.contains(&uid.to_string()))` fails on a
correct implementation, and the only way to make it pass is a new user-visible string — which
R14 ("zero behaviour change") forbids.

**Fix:** assert the literal prefix `` dropping privileges failed at `setresuid` `` (or
`setgroups`/`clearcaps`, the first step that fails unprivileged) and drop the parenthetical; or
say "the drop step's sentence, which names the failing call and not the uid".

### T-2 (MINOR) — the fourth census call is spelled in a form the R4 grep cannot see

> D1: "The **fourth** name is `blastit-view-php/src/pinning.rs`'s `#[cfg(test)]` module, which
> calls `enter(ChildPlan::for_test(io))`" (§4.4)
> R4: "`grep -rln 'child::enter(' …` names **exactly the four files D1 lists**" (§7)

D1 pins the fully qualified spelling only for the three production sites ("the fully qualified
form is what the census greps"). A `pinning.rs` test written as `use blastit_sys::child::enter;`
+ `enter(ChildPlan::for_test(io))` matches nothing, so the gate returns three files and goes red
on a correct implementation. R4's "exactly the four files" is the self-correcting half, but the
implementer sees the contradiction only after the red.

**Fix:** spell the fourth call `blastit_sys::child::enter(ChildPlan::for_test(io))` in D1, or add
"all four are written fully qualified" to the census sentence.

### T-3 (MINOR) — §4.5's CLI total is not reproducible from its own rows

> "**58** protected CLI cases stay in the protected tier; the total is a **floor**
> (`critical_dev_reload` is 14 cases / 1 619 lines today …)" (668–672)
> row: "`critical_dev_reload` | 12 → ≥12 | 1 407 (measured today …)" (663)

Measured: 14 / 1 619. `58 = 12+6+24+4+12` (pre-move); with today's numbers and the added pin leg
it is `61 = 12+6+25+4+14`. The floor wording saves the oracle, but the row and the paragraph
give two different "today" values three lines apart, and a reader cannot reproduce 58.

**Fix:** make the row "14 → ≥14 | 1 619 (today)" and the total "58 → 61 (a floor)".

### T-4 (MINOR) — the `--all-features`/`--list` rationale miscounts both clauses

> "`--all-features` because four staying suites are feature-gated and otherwise list **0
> tests**, and the same command because four protected suites use `#[tokio::test]`" (§4.5)

Measured: whole-file `#![cfg(feature = "python")]` exists in exactly two suites —
`critical_dev_reload.rs:47` (stays) and `critical_loaded_files.rs:30` (moves).
`critical_parallel_views` and `critical_static_cli` gate individual items and still list their
tests. Six protected suites use `#[tokio::test]` (`blastit-dbproxy` 9, `blastit-http` 5,
`blastit-scheduler` 10/7/15/17), and **none** of them is in the counted set; the only tokio
suite in §4.5's neighbourhood is `critical_rewrite` (17), which stays and is not counted by
`--list` here.

The command is right; the reasons are wrong, and a reviewer who checks them loses trust in the
rest of the section.

**Fix:** "`--all-features` because two suites are whole-file feature-gated (one moves, one
stays)"; delete the tokio clause or replace it with "`--list`, not `grep '#[test]'`, because
`#[tokio::test]` is a different token".

### T-5 (MINOR) — R16's `Cargo.lock` clause cannot hold for `MB.36.7`

> "`git show --stat <leaf commit>` names `Cargo.lock` for that leaf and no other leaf's commit
> in its range does" (§7 R16)

`MB.36.7` adds `crates/blastit/tests/dx_invariance.rs`, `dx_capture.rs` and
`docs/testing/AUDIT-MB.36.md` (§8.6) and edits no manifest, so its `--stat` cannot name
`Cargo.lock`. Read literally, the oracle fails on the epic's last leaf.

**Fix:** "a leaf that edits a manifest names `Cargo.lock`, and no two leaves in the epic both
do" (or scope the clause to `MB.36.2`–`.6`).

### T-6 (MINOR) — case 1 asserts a step the design documents as best-effort

> "(1) `dumpable()` is `false` after `enter`" (§5)

`enter` discards `set_dumpable`'s error on purpose, and §5 says `privs.rs` records that gVisor
ignores it. On such a host the protected case fails on a correct implementation, for a reason
the document itself states. The order is right (`set_dumpable` after the drop, so `setresuid`
cannot reset it), so this is only about the platform.

**Fix:** state the Linux precondition in the case's doc comment, or have `enter` expose the
step's outcome to the test (`#[doc(hidden)]`) and assert `false` only when it reported success.

### T-7 (MINOR) — cases 3 and 5 invert under uid 0

> "the drop proven by a `DropTo(other uid)` that must fail before `READY` (no root runner)"
> (§9 Round 4)

The CI `smoke (root)` job builds only `-p blastit` (`ci.yml:202`), so this new `blastit-sys`
suite is *not* run as root there and the claim holds for CI. But `drop_privileges(0)` refuses
by design (`privs.rs:55`) and `DropTo(uid+4242)` succeeds as root, so a developer running
`cargo test -p blastit-sys` in a root container gets a red on correct code — the exact failure
mode `ci.yml:184-196` documents for two other suites.

**Fix:** one line in the suite: assert `effective_uid() != 0` with a message naming the
unprivileged contract (the `/proc/self/status` helper already exists at
`critical_php_embed.rs:1604`).

### T-8 (MINOR) — two citation/budget nits

- `dumpable` is `privs.rs:248`, not `:247` (§5, case 1). The `seccomp.rs:125/661/1597`
  citations are correct.
- "≤ 2 × the suite's `blastit_scheduler::`-prefixed **import lines** (4/1/3 today)": the 4 in
  `critical_static_assets.rs` includes `:89`'s function-body type, not an import, so the budget
  is 9 rather than 8. Harmless (it is a budget), but the word "import" is wrong.

### T-9 (MINOR) — P5(e) should also add the missing mutation package

§8.6 names `make mutants-rust PKG=blastit-view-python`, but `MB.36.7`'s acceptance
(`plan/nodes/MB/MB.36/MB.36.7/NODE.md:55-58`) lists only `blastit-view`, `blastit-view-static`
and `blastit-view-php`. The crate holding the epic's only `unsafe` (`immortal.rs`) would be the
one crate without a mutation audit, and P5's list of node corrections does not mention it.

**Fix:** add `PKG=blastit-view-python` to P5(e)'s list of `MB.36.7` acceptance edits.

## 4. What is genuinely strong (so it is not re-litigated)

- **R14 is falsifiable in both directions.** The mask is enumerated; the normaliser has two
  self-tests (a masked `ts` vs a kept `status`; an unparseable line is a failure, never
  dropped); the `ETag` is compared unmasked by the hex shape that `static_response.rs:203`
  really formats (`{:x}-{:x}`, verified); and rows 1/3 have an in-process twin. This is the
  strongest DX-invariance oracle in the repository.
- **R4's five cases are unprivileged and ordered.** Case 3 is non-vacuous because
  `EmbeddedView` refuses `setpgid` (`seccomp.rs:661`, test at `:1597`), so "the child prints
  only if `setpgid` ran before the filter" cannot pass by accident; case 5 is the denied
  direction the `testing` skill demands.
- **The anti-gutting stack is three-deep and each layer has a distinct job:** `--list` counts
  (case loss), `critical_guard.py` (authorisation to touch), `critical-skip-check` (`--list`
  blindness to `#[ignore]`). The changed-line budget on the committed range closes the
  "rewrite an assertion in place" hole.
- **Every R1..R18 row names a command or a grep**, and the ones I could execute (T3's pathspec,
  the per-row counts, the `blastit_scheduler::` counts, the `#[path]`/`include_str!` depths,
  the `unsafe`/hardened-call censuses — both currently return 0 in the view crates) hold.
- **`critical_check_schema` exists** (`crates/blastit-py/tests/critical_check_schema.rs`), so
  row 5's schema reference is live, not a forward reference.

## 5. Verdict

No `BLOCKER` and no `MAJOR`: every requirement has a runnable, unprivileged oracle; the five
convergence items are present and mutually consistent; §4.5's per-row counts match the tree
exactly. The nine MINORs are wording and arithmetic fixes that do not change a single command
or test, and none of them can make a correct implementation pass vacuously.

VERDICT: PASS
