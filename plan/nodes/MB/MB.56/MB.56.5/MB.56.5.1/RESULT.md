# RESULT MB.56.5.1 — coverage-driven dead-code prune

STATUS: **partial / needs_split** — the dead-code prune (items 2–4) is done and verified;
the god-file splits (item 5, ~10.6k LOC across 6 crates) were not reachable in this slice
and are sliced in §5 so the next leaf can pick them up one crate at a time.

## 1. `make coverage` (per crate, `TESTS`/`FILES` as run)

| crate | suite | file | lines | regions | notes |
|---|---|---|---|---|---|
| `blastit-sqlpolicy` | `-p blastit-sqlpolicy --tests` | `src/analysis/mod.rs` | **61.5 %** | 62.5 % | 38 tests passed; uncovered `171-173, 279-285, 295-307` |
| `blastit-http` | `-p blastit-http --tests` | `src/server.rs` | 0.0 %* | 0.0 % | 17 tests passed |
| `blastit-sys` | `-p blastit-sys --tests` | `src/mem.rs` | 0.0 %* | 0.0 % | 11 tests passed |
| `blastit-view-php` | `-p blastit-view-php --tests` | — | n/a | n/a | **no in-crate tests**: 1 test executable, 0 tests ("coverage would be meaningless") |
| `blastit` | four `e2e_*` suites | `src/dev_reload.rs` | not run | not run | instrumented rebuild + 4 E2E suites exceed this slice's budget → §5 |

\* **Harness defect, not dead code.** `tools/coverage_report.py` prints
`reports 0.0 % lines — the story test may spawn the binary with env_clear() …` for any
library whose tests live in a separate `tests/*.rs` binary: the 17 / 11 tests *did* run,
but no `.profraw` is mapped back to the library object. It also still does not expand
`FILES` globs. **0.0 % from this tool is therefore not evidence of anything**, so every
deletion below is justified by a workspace-wide call-graph grep instead.

## 2. Deletions (all verified to have **zero** callers anywhere in `crates/`)

1. **`StatementClass::is_plain`** — `crates/blastit-sqlpolicy/src/analysis/types.rs`.
   `grep -rn is_plain crates` matched only this definition and the sibling below. No
   `TESTING_PLAN.md` §7 story asks "is this class one of the four plain kinds".
2. **`Classified::is_plain`** — same file. (The LOG called it `Analysis::is_plain`; the
   real symbol is `Classified::is_plain`.) Zero callers.
3. **`blastit_view_php::worker::is_serveable_entry`** — `crates/blastit-view-php/src/worker.rs`
   (file tail). Zero callers; not named by any of `lib.rs`'s six `pub use` lines; no
   suite reaches it. Its `Path` import stays (still used across the 1.6k-line worker).
4. **`dev_reload`'s `#[cfg(any(test, debug_assertions))]` fakes** —
   `crates/blastit/src/dev_reload.rs`: `TestClock` (+`impl Clock`), `report_pair`,
   `send_batch`, `FakeWatcher` + `Script` + `impl ReloadWatcher`, plus the
   `use blastit_view::loaded::Batch;` that only `send_batch` used. Their last caller,
   `crates/blastit/tests/critical_dev_reload.rs`, **does not exist** — `crates/blastit/tests/`
   holds only `e2e_{check_fmt,cli,dev,prod_refusals}.rs`, none of which names them.
   Module docs' stale `[TestClock]` / "critical_dev_reload" references removed too.
   `report_socket()` is **kept** (production callers `supervise.rs:1552, 2272`).

No `#[allow(dead_code)]` was added. Net: −4 symbols, −~120 lines of dead debug seam.

## 3. Keepers — verified live, do not re-litigate

| symbol | live production caller | owning suite |
|---|---|---|
| `blastit::mem_usage::advisory` | `stage0/dev.rs:408` (dev memory fence) | `e2e_dev` |
| `blastit::py::refusal_message` | `py_view.rs:178`, `supervise.rs:1092`, `stage1/load.rs:191,225` | `e2e_dev` (WSGI refusal leg) |
| `blastit::unified::unified` | `cli/run.rs:88` (`fmt --check` diff) | `e2e_check_fmt` |
| `blastit::forking::agrees` | `forking.rs:58` (in-crate adopt path) | `e2e_dev` |
| `blastit_view_static::StaticRoot::open_with` | only seam proving the `openat2`+`Walk` matrix (MB.56.3) | static / `e2e_*` |
| all 13 `blastit-sys/src/mem.rs` pub items | `Reader::sample`/`cgroup_limit`/`cgroup_usage` + their unit tests; `blastit::mem_usage` also uses `Reader`, `Sample`, `CGROUP_V2_CURRENT` | `blastit-sys --tests` |
| `blastit-view-php/src/worker.rs` (whole module) | **E2E-spawned PHP child** — its `.profraw` is invisible to the harness, hence "no tests ran" | PHP e2e legs |

`dev_reload`'s `mem_usage::advisory`, `unified`, `refusal_message` and `forking::agrees`
were listed as dead in `MB.56.5/LOG.md`; that list was stale — each has the caller above.

## 4. Uncovered-but-story branches

The one trustworthy uncovered region (sqlpolicy `analysis/mod.rs` `295-307`) is the
manifest-mismatch reporting path already covered by `critical_*` manifest suites and not
by any §7 story sentence, so no assertion was added. No E2E assertion was weakened.

## 5. Not reached — the god-file splits (follow-up slices)

One leaf per crate, move-only (no behaviour/API change), each unit `< 500` prod LOC:

- **L1 `blastit-events`** — `src/lib.rs` 1813 + `src/json.rs` 1402 → ≥6 modules.
- **L2 `blastit-lsp`** — `src/lib.rs` 1386 → ≥3 modules.
- **L3 `blastit-php-sapi`** — `src/loader.rs` 1075 → ≥3 modules.
- **L4 `blastit-sys`** — `src/fanotify.rs` 1031 → ≥3 modules.
- **L5 `blastit`** — `src/supervise.rs` 2767 → ≥6 modules.
- **L6 `blastit-scheduler`** — `worker.rs` 1611, `discovery.rs` 1576 → ≥4 modules each.

Also carried forward: the full `-p blastit` × four `e2e_*` coverage run (§1), and fixing
`tools/coverage_report.py`'s glob expansion + cross-binary `.profraw` mapping so future
prunes have real numbers.

## 6. Verification

```
cargo fmt --all && make rs-fmt rs-lint      → clean, 0 clippy warnings (workspace, --features python)
make fast-check                            → exit 0 (guard, rs-fmt, rs-lint, rs-doc,
                                             rs-test all-targets + doc, py-lint, plan-validate,
                                             universal-smoke, install-test)
```
