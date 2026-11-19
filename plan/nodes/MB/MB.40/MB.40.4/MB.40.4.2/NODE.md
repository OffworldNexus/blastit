---
id: MB.40.4.2
title: "Split blastit-sqlpolicy analysis.rs into cohesive modules + injected AnalysisCache"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.40.4.1]
paths:
  - "crates/blastit-sqlpolicy/src/analysis.rs"
  - "crates/blastit-sqlpolicy/src/analysis/**"
  - "crates/blastit-sqlpolicy/src/lib.rs"
  - "crates/blastit-sqlpolicy/src/lru.rs"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T03:13"
updated: "2026-11-19T03:13"
---

# Goal

Behaviour-preserving split of `crates/blastit-sqlpolicy/src/analysis.rs` (2,749 lines / ~1,899 prod LOC, 113 fns) into `crates/blastit-sqlpolicy/src/analysis/{mod.rs,types.rs,classify.rs,walk.rs,cache.rs}` (plus the extra resolve module noted below), and replacement of the `OnceLock<Mutex<Lru>>` global with a constructor-injected `AnalysisCache` threaded through `Allowlist`. Every resulting production file must stay under 500–600 prod (non-blank, non-comment) LOC.

This is a **pure refactor of API shape, not of semantics**: `blastit check`, SQL grant matching, and every existing assertion must be unchanged.

# Exact Targets (file:line pre-diagnosed)

Line numbers are from the pre-split `crates/blastit-sqlpolicy/src/analysis.rs` at `MB.40.4` closure. Do not re-audit.

**`cache.rs` (the injection point):**
- `CACHE_CAPACITY` `:114`, `pub fn analyze` `:696`, `analyze_cached` `:715`, `pub fn cached_analyses` `:734`, `fn cache()` `:743` holding `static CACHE: OnceLock<Mutex<Lru<String, Cached>>> = OnceLock::new();` at `:744-745`.
- New `pub struct AnalysisCache` wrapping `Mutex<Lru<String, Cached>>` with the same capacity and the same lookup/insert/len behaviour as `analyze_cached` + `cached_analyses` do today.

**`types.rs`:** `StatementKind` `:242`, `StatementClass` `:281`, `Classified` `:322`, `Op` `:435`, `Access` `:477`, `Analysis` `:600`, `AnalysisError` `:656`, `Reads` `:954`, `Derived` `:1027` (second impl at `:2332`), `Rel` `:1122`, `Named` `:1131`, `Frame` `:1193`, `Scope` `:1202`, `Resolver` `:1249`.

**`classify.rs`:** `is_safe_function` `:236`, `classify` `:367`, `classify_label` `:403`, `is_revoke` `:412`, `is_reset` `:421`, `statement_label` `:829`, `syntax` `:807`.

**`walk.rs`:** `fn resolve` `:749` and the `impl Resolver` block `:1263-2332` (~1,070 lines). **This alone exceeds the LOC bar**, so `walk.rs` must delegate to a second cohesive file (e.g. `walk/` directory or `resolve.rs`); the audit already flagged this — do not try to cram it into one file.

**`mod.rs`:** module docs, imports, re-exports, the remaining free helpers — `is_safe_cast_type` `:2378`, `is_system_table` `:2470`, `table_key` `:2483`, `merge_outputs` `:2498`, `union_of` `:2518`, `last_name` `:2536`.

# Hard constraints

1. **The cache test pins the semantics.** `crates/blastit-sqlpolicy/src/analysis/tests.rs:835 the_cache_stays_bounded` requires that `analyze(sql)` populates a cache which `cached_analyses()` reports and which never exceeds `CACHE_CAPACITY`. Therefore the `OnceLock` may only be removed by (a) giving `Allowlist`/`check` (`crates/blastit-sqlpolicy/src/lib.rs:274`) an injected `AnalysisCache` and (b) keeping the public `analyze`/`cached_analyses` pair behaviourally identical — a thin delegating implementation of those two free fns is allowed and is the sanctioned escape hatch, but it must be explicitly sized and documented as manifest-only convenience. A cache-only rewrite that drops them fails that test unmodified.
2. **Public API unchanged.** `crates/blastit-sqlpolicy/src/lib.rs` re-exports (`analyze`, `cached_analyses`, `Analysis`, `AnalysisError`, `Access`, `Op`, `Classified`, `StatementKind`, `classify`, `is_safe_function`, `is_system_table`, `CACHE_CAPACITY` visibility as today) must resolve with identical visibility. `src/analysis/tests.rs` already exists, so `mod tests;` must keep resolving from the new `mod.rs`.
3. **`tests.rs` assertions are frozen.** Only import paths / `mod` wiring lines may change if visibility moves; every `assert_*`, expected sentence and SQL fixture stays byte-identical.
4. **serde derive is already a dependency** (`crates/blastit-sqlpolicy/Cargo.toml:26`). No manifest change expected; `Cargo.toml`/`Cargo.lock` are in `paths` only if a bump proves unavoidable.
5. **No protected tests involved.** `crates/blastit-sqlpolicy/tests/critical_*.rs` need no edit and must not be modified.
6. **No new globals.** After this slice `grep -n "OnceLock\|static mut\|lazy_static" crates/blastit-sqlpolicy/src/` must show only the sanctioned module-level item (ideally none). No new traits, no `unwrap()`/`expect()` on parsed SQL.

# Acceptance

```
make fast-check
cargo test -p blastit-sqlpolicy --locked
cargo test -p blastit-sqlpolicy --locked the_cache_stays_bounded
cargo clippy -p blastit-sqlpolicy --all-targets --locked -- -D warnings
cargo fmt --all -- --check
for f in $(find crates/blastit-sqlpolicy/src/analysis -name '*.rs' -not -name 'tests.rs'); do
  printf '%s %s\n' "$(awk 'NF && $1 !~ /^\/\// ' "$f" | wc -l)" "$f"
done
```

Report tails in `RESULT.md`, plus `git status --porcelain` proving only `analysis.rs` → `analysis/` changed and `git diff crates/blastit-sqlpolicy/src/analysis/tests.rs` shows no assertion edits.
