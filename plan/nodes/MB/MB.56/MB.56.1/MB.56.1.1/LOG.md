# Log MB.56.1.1

## 2026-11-20T16:04 [planner]

created (task) Retire legacy Python modules + --internal-json RPC bridge; slim blastit wheel to a pure-stdlib launcher

## 2026-11-20T16:04 [planner]

edited fields

## 2026-11-20T16:04 [unknown]

Paths include pyproject.toml+Makefile; build-backend stays blastit_build until .2 to keep each slice fast-check green.

## 2026-11-20T16:05 [lead]

status ready -> active

## 2026-11-20T16:05 [lead]

attempt 1/3 started

## 2026-11-20T16:07 [lead]

base 51cc758369fcab406722062ce066f536cd2dbe57 (baseline make fast-check green)

## 2026-11-20T16:19 [lead]

REVIEW FIX: (1) __main__.py main_lsp exec-loop/fail-open: add EXEC_GUARD+resolve-before-env+SystemExit; (2) test_launcher.py add main_lsp empty-_bin oracle; (3) delete orphan crates/blastit/src/cli/internal_json/args.rs; (4) test_stdlib_only.py drop pytest_plugin carve-out; (5) main.rs:4 doc drift; (6) check-bundle-layout.sh+test_packages.sh 7->4 module lists; (7) wheel-smoke.sh drop [pytest11] and 3->2 console scripts; (8) wheel-drive.sh drop fold step; (9) e2e/universal_binary.py delete fold leg; (10) docs.yml/ci.yml/e2e/dx_replay.py retired runnable_docs refs. DEFERRED->MB.56.1.3: ARCHITECTURE.md fold/--internal-json drift, scripts/test_packaging_gates.py (file is deleted in .3; fast-check unaffected) + ARCHITECTURE.md

## 2026-11-20T16:19 [lead]

edited fields

## 2026-11-20T16:19 [lead]

edited fields

## 2026-11-20T16:24 [lead]

status active -> done
