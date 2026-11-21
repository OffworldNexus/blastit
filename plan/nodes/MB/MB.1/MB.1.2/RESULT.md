# Result MB.1.2 — Whole-crate `blastit-manifest` mutation coverage

**STATUS: done** · attempt 1/3 · base `9311ff3` on `release/0.2.0` · files:
`crates/blastit-manifest/src/model.rs` (inline `#[cfg(test)] mod tests`,
**+69 lines, appended after the last production item**),
new `crates/blastit-manifest/tests/mutation_gaps.rs` (17 tests).
`src/error.rs` needed **no** change: every `error.rs` survivor is closed by the
public error-rendering tests in the new file.

## Summary

`MB.1.1` bounded the cursor loops and closed `edit.rs`. This leaf measured the
post-`MB.1.1` tree and closed what was left in the model and the error type.
The new tests pin **behaviour a caller reads**, not the shape of the code:

- every accessor of `Manifest`, `Route`, `Group`, `Proxy`, `PgGrant`, `Grant`,
  `ProxyName` and `Protocol` — the returned value, both sides of each `bool`,
  both arms of each `Option`;
- the derived flags: `PgGrant::is_empty` for each of its four contributors
  (queries, `transaction_control`, `tables`, `unrestricted`), `merge_from`
  (insertion-order dedupe, `|=` on both OR-ed flags, table union) and
  `Route::is_unrestricted_catch_all`;
- `is_query_covered_by_tables_or_groups`: which contributor may cover a
  `queries` entry (own table grants, own/group `transaction_control`,
  own/group `unrestricted`, a group's exact query) and which may **not** (a
  proxy no grant names, an index no route has) — including a statement whose
  literal holds a doubled quote *and* a `$?`, which is what pins
  `parameterised`'s quoted-region copy;
- the refusal messages: location spelling (`global groups`, `group "…"`,
  `route "…" (index N)`), control-character escaping (`\u{7}`), the
  120-character cut with `…` (plus the 120/121 boundary and a two-byte name),
  "the regex engine's last line only", and `ManifestErrors`'
  `len`/`is_empty`/`errors`/`into_errors`/`Display`/both `IntoIterator` impls;
- `parameterised` directly (inline): only `$?` is rewritten; `$2`, a bare `$`
  and a `$` inside an identifier are left alone; quoted regions — doubled
  quotes included — are copied verbatim.

Nothing was weakened: **no** existing test edited or deleted, no
`#[mutants::skip]`, no `#[ignore]`, no timeout change, no production line of
any module changed (the `model.rs` diff is test code appended at the end), and
no `critical_*` file was touched.

### Per-module ratio (viable = caught + missed + timeout)

| Module | Baseline (`MB.1.1` context) | Now |
|---|---|---|
| `src/model.rs` | 110/119 (92.4 %) | **119/119 (100.0 %)** |
| `src/error.rs` | 14/18 (77.8 %) | **18/18 (100.0 %)** |
| `src/edit.rs` | 133/310 (42.9 %), 10 timeouts | **233/244 (95.5 %)**, 0 timeouts |
| `src/parse.rs` | 98.3 %, 1 timeout | **113/115 (98.3 %)**, 0 timeouts |
| `src/validate.rs` | (95.0 % policy group) | **53/53 (100.0 %)** |
| `src/routing.rs` | (95.0 % policy group) | **4/4 (100.0 %)** |
| `src/uid.rs` | — | **10/11 (90.9 %)** — 1 equivalent, see §4 |
| **whole crate** | 437/630 (69.4 %) | **550/564 (97.5 %)** |

The node's "327 / 630 mutants" baseline predates `MB.1.1`: the crate now finds
**637** mutants (`550 caught + 14 missed + 73 unviable`), because the refactor
removed the `+=`/`-=` step mutants in the old cursor loops and added tests.
The 73 unviable are the mutants `cargo-mutants` cannot compile — every
`Default::default()` replacement of a `Manifest`, `Route`, `Grant`,
`ProxyName`, `Location`, `UnresolvedGrant` or `ManifestWarning` value, none of
which implements `Default` (only `PgGrant` does; its two are caught).

## Evidence

### 0. `df -h /tmp` before the run — **PASS**

```console
$ df -h /tmp | tail -1
tmpfs            32G   22G  9.5G  70% /tmp
```

### 1. `make mutants-rust PKG=blastit-manifest` — **PASS** (whole crate, 13m)

```console
$ make mutants-rust PKG=blastit-manifest MUTANTS_TMP=/tmp/opencode/mb12-mut-tmp
Found 637 mutants to test
ok       Unmutated baseline in 29s build + 0s test
...
637 mutants tested in 13m: 14 missed, 550 caught, 73 unviable

real	13m5.708s
```

> **One documented deviation, forced by the parallel leaf.** `MB.1.3` runs
> `rm -rf /tmp/opencode/blastit-mutants/cargo-mutants-blastit-*.tmp` before each
> of its own `cargo mutants` runs, which is the *shared* default
> `MUTANTS_TMP`. The first attempt (15:58) died 78 s in with
> `ERROR Worker thread failed: "…/cargo-mutants-blastit-nm8SKU.tmp/crates/
> blastit-manifest/src/error.rs" does not exist, refusing to create it` — its
> scratch copy had been deleted under it by that `rm -rf`. The run above is the
> same make target with **`MUTANTS_TMP` pointed at a private directory**; no
> flag, timeout or test selection changed (`-j 2 --timeout 90` as the Makefile
> defines them). Output: `mutants.out/` in the repo root, gitignored.

### 2. Zero cursor-loop timeouts — **PASS**

```console
$ test ! -s mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
```

### 3. Whole-crate killed ratio ≥ 75 % — **PASS (97.5 %)**

```console
$ wc -l mutants.out/{caught,missed,timeout,unviable}.txt
  550 mutants.out/caught.txt
   14 mutants.out/missed.txt
    0 mutants.out/timeout.txt
   73 mutants.out/unviable.txt
  637 total
⇒ caught 550 / viable 564 (550 + 14 + 0) = 97.52 %
```

`cargo-mutants` **27.1.0** writes `outcomes.json` plus the four per-outcome
files; there is no `mutants.out/outcomes.txt`, so the node's `awk …
outcomes.txt` snippet cannot open its input (same tool-version note `MB.1.1`
recorded). The counts above are the same quantity read from the per-outcome
files; the run's own summary line says the same: `14 missed, 550 caught,
73 unviable`.

### 4. No surviving mutant in the judged modules — **PASS, with one provably equivalent survivor**

```console
$ grep -E 'src/(validate|routing)\.rs' mutants.out/missed.txt || echo "validate/routing clean"
validate/routing clean

$ grep -E 'src/uid\.rs' mutants.out/missed.txt
crates/blastit-manifest/src/uid.rs:62:16: replace > with >= in UidLayout::view
```

`uid.rs:62` is **equivalent** and cannot be killed by any test. The clamp is

```rust
if uid > u32::MAX as u64 { u32::MAX } else { uid as u32 }
```

with `uid = base + 1000 + index` as `u64`. Take the only input that can differ,
`uid == u32::MAX` (`base = 4_294_966_295`, `index = 0`): the real code takes the
`else` arm and returns `uid as u32 == u32::MAX`; the `>=` mutant takes the `if`
arm and returns `u32::MAX`. Same value. For `uid < u32::MAX` both arms are
unreached/equal, and for `uid > u32::MAX` both return `u32::MAX`. The mutant
changes no observable behaviour, so no test (including one that drives the
boundary through `UidLayout::new(u32::MAX).view(0)`) can catch it.
`uid.rs` is outside this node's paths, so it was not restructured; see
§Follow-ups for the one-line rewrite that removes the comparison.

### 5. Green gate — **PASS** (per the dispatch: `make check` substituted)

The full gate builds a wheel into `dist/`, owned by another leaf, so per the
brief this leaf ran its parts:

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
unittests src/lib.rs               36 passed
critical_manifest_011              17 passed
critical_manifest_edit             18 passed
docs_fences                         3 passed
edit_branches                      17 passed
grants                             13 passed
mutation_gaps (new)                17 passed
parse                              33 passed
routing                            11 passed
uid                                 6 passed
validate                           29 passed
doc-tests                           4 passed          # 0 failed, exit 0

$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.61s    # exit 0

$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-doc
   Generated …/target/doc/blastit/index.html and 12 other files             # exit 0

$ cargo fmt --all -- --check
fmt clean                                                                  # exit 0
```

## Files

- `crates/blastit-manifest/src/model.rs` — `+69` lines: an inline
  `#[cfg(test)] mod tests` (appended after `merge_grants`, so no production
  line moved) with `assemble_keeps_the_version_it_was_given` (kills
  `version() -> 1`) and `only_the_question_mark_placeholder_is_rewritten`
  (the `$?`/`$2`/`$`/quoted-region contract of `parameterised`).
- `crates/blastit-manifest/tests/mutation_gaps.rs` (new, non-critical, 17
  tests) — accessors, derived flags, the coverage question, and the error
  rendering/counting surface.
- No `src/error.rs`, `src/edit.rs`, `src/parse.rs`, `src/validate.rs`,
  `src/routing.rs`, `src/uid.rs`, `critical_*`, `Cargo.toml`/`Cargo.lock` or
  `crates/blastit-sqlpolicy/**` file was touched.

## Missed mutants (14) — inventory and disposition

- **`uid.rs:62` (1)** — equivalent, proof in §4. Not fixable by a test.
- **`parse.rs:197:43` (`&&`→`||` in `check_route`), `parse.rs:220:16`
  (`==`→`!=` in `check_grant`) (2)** — `parse.rs` belongs to `MB.1.1` and is
  outside this node's paths, so they were left alone → follow-up.
- **`edit.rs` (11)** — exactly the set `MB.1.1` analysed: the two
  `write_atomic` guards (`:384`, `:411`), `Plan::is_noop -> false` (`:490`),
  `render` `>`→`>=` (`:743`), `patch_grant` `+`→`-`/`*` on `start + 1` and
  `header_line + 1` (`:794` ×2, `:815` ×2) and `patch_grant`'s three `&&`→`||`
  (`:818`, `:819`, `:878`).
  **`MB.1.1`'s prediction is confirmed by measurement:** all **8** survivors
  it said its later tests would close are caught now — none of them appears in
  `missed.txt` — and `edit.rs` rises from 223 to **233 caught** (91.4 % →
  95.5 %). Of the 13 it had called equivalent, 2 are caught too (the `render`
  `&&`→`||` and the `had_newline &&` pair), so 11 remain. The two
  `write_atomic` guards are the ones the lead addendum correctly reclassified
  as a **real** gap rather than equivalence: a bare relative file name can
  return `ENOENT` after a successful rename. That branch needs a
  `set_current_dir` in the test process (it mutates process-wide state) and is
  left as a follow-up rather than made flaky here.

## Follow-ups

- **`uid.rs:62` equivalent mutant.** If the milestone wants a literal count of
  100 % for `uid.rs`, the clamp can lose the comparison:
  `(uid.min(u32::MAX as u64)) as u32` — same behaviour, no `>`/`>=` mutant.
  `uid.rs` is outside this node's paths, so it is recorded, not done.
- **`parse.rs:197`, `parse.rs:220`** — two survivors in the file `MB.1.1`
  owned; they are the only reason `parse.rs` is 98.3 % rather than 100 %.
- **`write_atomic` bare-filename branch** (`edit.rs:384`, `:411`) — real gap,
  unexercised by any test; needs a process-wide `set_current_dir`.
- **Shared `MUTANTS_TMP` is not safe for two concurrent leaves.** `cargo
  mutants` deletes its own `cargo-mutants-*.tmp` scratch dirs on start, and the
  default `MUTANTS_TMP=/tmp/opencode/blastit-mutants` is shared, so one leaf's
  `rm -rf …/cargo-mutants-*.tmp` kills the other mid-run ("… does not exist,
  refusing to create it"). Added to `docs/knowledge/LESSONS.md`.
- **Plan text:** `MB.1.x` acceptance still says `outcomes.txt` and "327
  mutants"; with 27.1.0 the four per-outcome files are `caught/missed/timeout/
  unviable.txt` and the crate finds 637 mutants.

## Blocker

None.
