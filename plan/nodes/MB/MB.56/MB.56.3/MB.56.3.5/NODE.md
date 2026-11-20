---
id: MB.56.3.5
title: "Finish consolidation: four suites + minimal common/, <=5,500 LOC, fix dead test legs"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.3.4]
paths: ["crates/blastit/tests/**", "e2e/**", "docs/design/**", Makefile, ".github/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T18:45"
updated: "2026-11-20T18:50"
---

# Goal
Finish the consolidation: `crates/blastit/tests/` holds **exactly four** E2E suites plus a minimal shared `common/` helper — total **≤ 5,500 LOC**, no leftovers.

1. Delete every file not absorbed by the four suites (`smoke.rs` residue, `dx-*` fixtures, `critical_lsp.rs`, `common_harness_parity.rs`-style meta-tests, any orphan fixture) once its coverage lives in a suite.
2. Reduce `tests/common/` to what the four suites actually use (target ≤ 500 LOC): keep the real `pg_upstream` wire stub and the process helpers, delete per-suite duplicates; `MB.40.3.3`'s rule (one copy of `pg_upstream`, shared with `blastit-dbproxy`) must still hold.
3. Prove the §7 suites still own their surfaces (`make zed-check` green) and that `Makefile`/`.github` legs naming deleted `--test` targets are fixed (`rs-test-php`, `rs-no-default`, `rs-no-python`, `rs-test-critical` globs).
4. Report the final per-file LOC table in `RESULT.md`.

# Acceptance
- `make fast-check` green, zero warnings; `ls crates/blastit/tests` = four `e2e_*.rs` + `common/`; total ≤ 5,500 LOC.
