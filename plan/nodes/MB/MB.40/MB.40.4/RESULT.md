# Gate: MB.40.4 — God-file splits & serde wire structs

## Evidence

| Check | Command | Result |
|---|---|---|
| Rust + Python leaf gate | `make fast-check` | exit 0 (all workspace targets, ruff clean, `plan OK`) |
| Feature matrix | `make rs-no-python rs-no-default` | exit 0 (0 pyo3 deps; `static_feature_off` 2 passed) |
| Protected critical tests | `critical_guard.py --base <slice base> --node <slice>` | OK per slice, no protected test changed by any slice |
| Invariants | `guard.py` / `plan.py validate` | OK / `plan OK` |

Full `make check` (wheel, standalone ELF matrix, runnable docs, Zensical) runs **once** at the `MB.40` gate.

## Result

| File | Before | After |
|---|---|---|
| `crates/blastit-scheduler/src/scheduler.rs` | 5,650 lines (~3,433 prod) | 9 modules, largest 678 prod (`response.rs`) |
| `crates/blastit/src/config.rs` | 4,463 (~2,081 prod) | 13 modules, largest 421 prod (`env.rs`); 18 hand-rolled `json_*` accessors replaced by derived wire structs |
| `crates/blastit-sqlpolicy/src/analysis.rs` | 2,749 (~1,899 prod) | 10 modules (incl. `resolve/`), largest 491 prod (`walk.rs`); `OnceLock<Mutex<Lru>>` replaced by an injected `AnalysisCache` |
| `crates/blastit/src/stage0.rs` + `stage1.rs` | 5,043 + 3,003 lines | `stage0/{mod,gates,dev,fork_exec}` + `stage1/{boot,serve,adopt,spawn,python_env}`, largest 557 prod (`boot.rs`); `HttpFork`/`SchedulerFork`/`DevExec` parameter structs |

Every split was reviewed for **behaviour preservation**, not just compilation:

- scheduler: 160/160 function bodies byte-identical, 183 string literals identical, zero new panic paths.
- config: the `--config-fd` document contract is pinned by `the_document_key_set_is_the_stage_0_contract` (key set, D6 nesting, absent-tolerant members, byte-identical `to_json` round-trip); every `ConfigError` sentence unchanged.
- sqlpolicy: every analysed body token-identical; `the_cache_stays_bounded` passes unmodified; fail-closed on analysis error/cache miss unchanged.
- stage0/1: fd/`clear_cloexec`/keep-list/`drop`/`setresuid`/seccomp order preserved on every path including the failed-`fork` return and each `exec` divergence; all 477 operator-facing literals identical.

## Review outcomes

Genuine defects found and fixed (all with a new pinning test):

- `[MAJOR]` config: an absent `privilege` key (a pre-D6 document) became `missing field` — a config the base booted would have been refused. `#[serde(default)]` restored, with a test removing all three D6 members.
- `[MAJOR]` stage0/1: the protected one-file source scan in `critical_loaded_files.rs` silently narrowed to the root shim once boot code spanned six files. Not editable, so `crates/blastit/tests/stage1_install_sites.rs` restores the crate-wide guarantee, including a completeness check that the scanned set equals the directory's real `.rs` set.
- `[MINOR]` fixed: serde type names (`expected struct PhpWire`) leaking into config errors; `proxy_max_connections`/`privilege` absent-case sentences; 18 `pub(crate)` items narrowed to `pub(super)`; a misplaced static-slot predicate; a pin for the deliberate fail-closed `capabilities` type check.

Filtered as non-actionable: pre-existing `fork_dbproxy_dev` 7-arg shape, `load` staying in the root shim (required by a protected `include_str!`), file-naming preferences, RESULT.md paperwork.

## Accepted deviations

- `crates/blastit/src/stage1.rs` survives as a 278-prod-LOC root shim because protected `critical_loaded_files.rs` does `include_str!` on that exact path; the boot body lives in `stage1/boot.rs`.
- `static SHARED: OnceLock<AnalysisCache>` remains in `sqlpolicy/analysis/cache.rs` as the node's sanctioned, explicitly-sized delegate behind the manifest-only `analyze`/`cached_analyses` pair; `Allowlist` uses the same instance.
