# MB.40.4.2 — `analysis.rs` split + injected `AnalysisCache`

STATUS: done

`crates/blastit-sqlpolicy/src/analysis.rs` (2,749 lines / **1,899 prod LOC**, 113 fns)
is now `crates/blastit-sqlpolicy/src/analysis/` — ten production files, all well
under the 500–600 prod LOC bar — and the `static CACHE: OnceLock<Mutex<Lru<..>>>`
inside a free `fn cache()` is gone: the cache is an `AnalysisCache` handle that
`Allowlist` owns and `check` uses.

Pure refactor of API shape: no assertion, sentence, SQL fixture or public
re-export changed.

## Module layout

| File | Contents | Prod LOC |
|---|---|---|
| `analysis/mod.rs` | module docs (deny-on-unknown + cache + layout), `mod` wiring, re-exports, `is_system_table`, `table_key`, `merge_outputs`/`union_of`, `is_safe_cast_type`, `last_name` | 171 |
| `analysis/types.rs` | `StatementKind`, `StatementClass`, `Classified`, `Op`, `Access`/`Extent`, `Analysis`, `AnalysisError` | 213 |
| `analysis/scope.rs` | `Reads`, `Derived`, `Rel`, `Named`, `Frame`, `Scope` (the derived-relation model) | 201 |
| `analysis/classify.rs` | `SAFE_FUNCTIONS`, `is_safe_function`, `classify`, `classify_label`, `is_revoke`, `is_reset`, `statement_label`, `syntax` | 215 |
| `analysis/walk.rs` | `MAX_DEPTH`, `HANDLED`, `Resolver` + dispatch/coverage/structural walk, `fn resolve`, the JSON accessors (`unwrap`, `one_of`, `typed_body`, `node_field`, `node_list`, `text`, `flag`, `string_node`) | 354 |
| `analysis/resolve/mod.rs` | read side: `statement`, `with_clause`, `select`, `branch`, `collect_from`, `build_relation`, `join`, `range_var`, `alias_columns`, `locking`, `relation_reference` | 347 |
| `analysis/resolve/write.rs` | write side: `insert`, `on_conflict`, `update`, `delete`, `write_target`, `returning` | 154 |
| `analysis/resolve/expressions.rs` | `target`, `column_ref`, `unqualified`, `columns_of`, `column_of`, `star`, `func_call`, `type_cast` | 209 |
| `analysis/resolve/helpers.rs` | `star_target`, `output_name`, `res_target_name`, `insert_columns`, `target_table`, `alias_name`, `names_of`, `set_operation`, `is_star` | 78 |
| `analysis/cache.rs` | `CACHE_CAPACITY`, `AnalysisCache`, `analyze`, `cached_analyses` | 56 |
| `analysis/tests.rs` | **untouched** (`git diff` empty) | — |

The node anticipated five modules plus "a second resolve file". Two more were
needed to keep every file under the LOC bar: `scope.rs` (the ~500-line
`Reads`/`Derived`/`Named`/`Scope`/`Resolver` group would have pushed
`types.rs` over) and a third file inside `resolve/` (the `impl Resolver` block is
~1,070 raw lines). The `resolve/` directory is the "delegate to a second cohesive
file" the node sanctioned, split by job: read, write, expressions, accessors.

Measured with the acceptance snippet:

```
213 crates/blastit-sqlpolicy/src/analysis/types.rs
201 crates/blastit-sqlpolicy/src/analysis/scope.rs
215 crates/blastit-sqlpolicy/src/analysis/classify.rs
354 crates/blastit-sqlpolicy/src/analysis/walk.rs
347 crates/blastit-sqlpolicy/src/analysis/resolve/mod.rs
154 crates/blastit-sqlpolicy/src/analysis/resolve/write.rs
209 crates/blastit-sqlpolicy/src/analysis/resolve/expressions.rs
78 crates/blastit-sqlpolicy/src/analysis/resolve/helpers.rs
56 crates/blastit-sqlpolicy/src/analysis/cache.rs
171 crates/blastit-sqlpolicy/src/analysis/mod.rs
```
(pre-split file was 1,899 prod LOC: 1,899 ≈ 213+201+215+354+347+154+209+78+56+171 + per-file `use`/wiring overhead.)

## `AnalysisCache` — shape, owner, seams

```rust
#[derive(Clone, Debug)]
pub(crate) struct AnalysisCache { entries: Arc<Mutex<Lru<String, Cached>>> }
```

* **Owner:** `crate::Allowlist` (`crates/blastit-sqlpolicy/src/lib.rs:147`):
  `analysis: AnalysisCache`. `Allowlist::new`, `Allowlist::unrestricted` and the
  derived `Default` build it from `AnalysisCache::default()`, and
  `Allowlist::check` analyses against `self.analysis.analyze_cached(…)`
  (`lib.rs:274` was `analysis::analyze_cached(…)`). No cache is looked up behind
  the allowlist's back any more.
* **Behaviour:** `Default` hands over the per-process memo blastit already had
  (`AnalysisCache::shared`, one `OnceLock<AnalysisCache>` — the single, documented
  module-level item constraint 6 allows; a process memo is unavoidable for the
  free fns below), so `check` parses no more than before. A cache shared by
  several allowlists is one `Arc` clone.
* **Sanctioned escape hatch, sized explicitly:** `analyze` (5 statements) and
  `cached_analyses` (3 statements) are documented manifest-only delegates over
  `AnalysisCache::shared()`. They have no `Allowlist` to hang a cache on —
  `blastit-manifest`/the Python audit analyse standalone statements — and the
  frozen `the_cache_stays_bounded` pins that `analyze` populates what
  `cached_analyses` reports.
* `AnalysisCache::private()` is the isolated-memo constructor (`pub(crate)`, used
  by `shared()`); it is the seam for a future per-route cache. No unused builder
  was added — a `with_analysis_cache` with no caller would be dead code under
  `-D warnings` (follow-up below).

## Proof the frozen test and API did not move

* `git diff crates/blastit-sqlpolicy/src/analysis/tests.rs` → **empty** (no
  assertion, import or `mod` wiring line changed; `mod tests;` still resolves from
  `analysis/mod.rs`).
* `cargo test -p blastit-sqlpolicy --locked the_cache_stays_bounded` →
  `test analysis::tests::the_cache_stays_bounded ... ok`.
* `lib.rs` re-export block byte-identical (`pub use analysis::{Access, Analysis,
  AnalysisError, Classified, Op, SAFE_FUNCTIONS, StatementClass, StatementKind,
  analyze, cached_analyses, classify, is_safe_function, is_system_table}`); the
  crate-internal paths `crate::analysis::{Extent, one_of, syntax, unwrap}` that
  `tables.rs`/`learn.rs` use are re-exported with the same visibility.
* No `crates/blastit-sqlpolicy/tests/critical_*.rs` edit (all 88 critical tests
  pass unmodified).

## Evidence tails

`cargo clippy -p blastit-sqlpolicy --all-targets --locked -- -D warnings`

```
    Checking blastit-sqlpolicy v0.2.0 (...release-0.2.0)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.62s
```

`cargo test -p blastit-sqlpolicy --locked`

```
test result: ok. 258 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (lib, incl. analysis::tests)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (critical_learn_policy)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out     (critical_lexer_boundaries)
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (critical_literals)
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (critical_policy_surface)
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (critical_query_template)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (policy_module_edges)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (policy_module_tables)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (Doc-tests)
```

`cargo test -p blastit-sqlpolicy --locked the_cache_stays_bounded`

```
running 1 test
test analysis::tests::the_cache_stays_bounded ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 257 filtered out; finished in 0.14s
```

`grep -rn "OnceLock\|static mut\|lazy_static" crates/blastit-sqlpolicy/src/`

```
crates/blastit-sqlpolicy/src/analysis/cache.rs:23:use std::sync::{Arc, Mutex, OnceLock};
crates/blastit-sqlpolicy/src/analysis/cache.rs:80:        static SHARED: OnceLock<AnalysisCache> = OnceLock::new();
```

`grep -rn "\.unwrap()\|\.expect(" crates/blastit-sqlpolicy/src/analysis/{*.rs,resolve/*.rs}`
→ only the doctest's `.expect("a plain SELECT is analysed")` and `tests.rs`;
production paths use `unwrap_or_default`/`ok()`/`?` only.

`git status --porcelain` (task-owned paths)

```
 D crates/blastit-sqlpolicy/src/analysis.rs
 M crates/blastit-sqlpolicy/src/lib.rs
?? crates/blastit-sqlpolicy/src/analysis/cache.rs
?? crates/blastit-sqlpolicy/src/analysis/classify.rs
?? crates/blastit-sqlpolicy/src/analysis/mod.rs
?? crates/blastit-sqlpolicy/src/analysis/resolve/
?? crates/blastit-sqlpolicy/src/analysis/scope.rs
?? crates/blastit-sqlpolicy/src/analysis/types.rs
?? crates/blastit-sqlpolicy/src/analysis/walk.rs
```

(`plan/*` entries in the same status output are the orchestrator's, not this
slice's.)

`make fast-check` (final tail)

```
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups

* `Allowlist` has no public/crate builder for an isolated `AnalysisCache`
  (`AnalysisCache::private()` exists but no caller wants it yet). When a route or
  a test needs its own memo, add `with_analysis_cache` together with its first
  caller, so it does not land as dead code under `-D warnings`.
* `grep -n "analysis.rs" docs/ ARCHITECTURE.md` was not part of this slice; if any
  doc names the old single file, it is a docs-only follow-up.
