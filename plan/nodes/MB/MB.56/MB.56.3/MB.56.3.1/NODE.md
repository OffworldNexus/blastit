---
id: MB.56.3.1
title: "Create e2e_cli.rs (§3 CLI & safe handoff) and delete absorbed suites"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths: ["crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T18:45"
updated: "2026-11-20T18:45"
---

# Goal
Create `crates/blastit/tests/e2e_cli.rs` (`TESTING_PLAN.md` §3 "Installation & Project Workflow" — CLI surface & safe handoff) and delete the suites it absorbs.

1. Fold into `e2e_cli.rs`, driving the real binary: `--version`, `help`/`-h`, hidden internal `--stage`/`--role` flags absent from `--help`, unknown `--preset` naming the valid presets and exiting `2`, `NO_COLOR`/`TERM=dumb` per stream.
2. Safe local CLI handoff: `dev`, `check`, `fmt`, `lsp` delegating to `.venv/bin/blastit`, `vendor/bin/blastit`, `node_modules/.bin/blastit`; skipping an unsafe ambient `$VIRTUAL_ENV`; refusing an unsafe `BLASTIT_BIN` or a wrong-uid binary; never handing off when the image is production `blastit`.
3. Absorb and delete: `critical_handoff.rs`, `critical_config_and_presets.rs` (the preset/flags half), `dx_capture.rs`/`dx_invariance.rs`/`dx-mask*`/`dx-parity*` where they assert CLI parity, and the CLI-facing parts of `smoke.rs`.
4. Keep every assertion **exact** (verbatim sentences, exit codes, stdout-vs-stderr), reuse `tests/common/`, and keep the file ≲ 1,400 LOC.
5. `crates/blastit/tests/critical_lsp.rs` is deleted separately (`MB.56.2.5` already ported its §7 legs) — do not touch it here.

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_cli` green; every absorbed file deleted.
