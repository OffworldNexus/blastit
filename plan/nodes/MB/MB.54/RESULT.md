# MB.54 Result — `TESTING_PLAN.md`

## Summary
- Ran a full read-only LOC inventory across `crates/` (20 crates), `python/`, `e2e/`, `scripts/`, and `tools/`, measuring `109,181` prod/build LOC (`95,362` Rust/C incl. `29,762` comment lines, `6,339` Python, `7,480` build scripts) against `174,265` test/meta-test LOC (`51,460` `src/` inline/module tests, `81,622` `crates/*/tests/` across 127 files, `18,563` `python/tests/`, `11,163` `e2e/`, `11,457` `scripts/` meta-linters).
- Authored `TESTING_PLAN.md` (`163` lines, scannable, zero internal node jargon) with:
  1. **Measured Baseline & Per-Crate X-Ray** showing all 20 Rust crates, `python/`, `e2e/`, and `scripts/`, plus why the bloat happened (`critical_*.rs` proliferation, 3x–4x vertical test duplication, YAML/prose meta-linters, `29.8k` LOC of Rust essay comments, and `4,525` LOC of legacy `0.1.0` `pytest_plugin.py`/`fold.py`/`logs.py` inside `python/blastit/`).
  2. **Strict Testing Rules:** 100% user-story E2E traceability; unit tests banned outside 4 narrow pure-parser/wire boundaries capped at `<= 4,500` LOC total (`sqlpolicy` `<=1.5k`, `manifest` `<=1.5k`, `http` `<=800`, `sys`+`frame` `<=700`); zero YAML/script/prose meta-tests.
  3. **Complete User Stories (`US-1`..`US-5`)** mapped 1-to-1 to **8 consolidated E2E suites** (`e2e_cli.rs`, `e2e_dev.rs`, `e2e_check_fmt.rs`, `e2e_lsp.rs`, `e2e_prod_refusals.rs`, `us1_packaging.py`, `us2_runtimes.py`, `us5_prod_sandbox.py`).
  4. **Concrete Keep / Merge / Delete Maps** cutting tests/meta-tests by **~89%** (`174.3k` → `~19.5k` LOC) and production/script code by **~46%** (`109.2k` → `~58.5k` LOC, including retiring `blastit-audit`, `pytest_plugin.py`, `fold.py`, `logs.py`, and 35 `scripts/` files).
  5. **5 Crisp Yes/No Owner Decisions (`D1`–`D5`)** + 4-slice execution sequence once approved.

## Verification Evidence
- `wc -l TESTING_PLAN.md` → `163` lines (`<350` budget).
- `git status --short` → only `TESTING_PLAN.md` and `plan/nodes/MB/MB.54/RESULT.md` created; zero Cargo builds run and zero existing code or tests modified.

## Follow-ups
- Await owner sign-off on `D1`–`D5` in `TESTING_PLAN.md` before planning/dispatching pruning slices.
