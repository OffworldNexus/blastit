---
id: MB.56.5.2.2
title: "Diary-comment strip: crates/blastit + stale refs + final gates"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.5.2.1]
paths: ["crates/**", "scripts/**", "e2e/**", docs/knowledge/LESSONS.md, ARCHITECTURE.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T23:06"
updated: "2026-11-20T23:17"
---

# Goal
Finish the strip for `crates/blastit` itself, fix the remaining stale references, and land the final gates.

1. Remove plan-node citations and historical essays from `crates/blastit/src/**/*.rs` (`stage0/*`, `stage1/*`, `supervise.rs`, `lib.rs`, `cli/*`, `host.rs`, `dev_reload.rs`, `visibility.rs`, `dev_learn.rs`, `py*.rs`, `child.rs`, `forking.rs`, `dbproxy_wiring.rs`, `coverage.rs`, `config/*`, `check/*`).
2. Stale references: `crates/blastit/src/stage0/gates.rs` (`scripts/check-no-test-seam.sh` → `scripts/build-standalone.sh`), `scripts/build-wheel.sh` (`_bin/blastit-dbproxy` → the single `_bin/blastit`), `e2e/apps/php/probes.md` (drop the `MB.*` prose, keep the table byte-identical for `php2shell.py --self-test`), and any `critical_<name>.rs`/`e2e/run.py`/`--internal-json` mention — name the real owner.
3. Write `RESULT.md`: per-crate before/after **prod and test LOC** table, the diary paragraph count removed, and every stale reference with its new owner.
4. Run the gates: `make fast-check`, `make docs-check`, `make zed-check`, plus `uv run --no-project e2e/run_apps.py` (local Docker, ports `28080+`, never stop `n2f-*`).

# Acceptance
- All gates green; `rg 'MB\.[0-9]+\.[0-9]+' crates/*/src` returns nothing.
