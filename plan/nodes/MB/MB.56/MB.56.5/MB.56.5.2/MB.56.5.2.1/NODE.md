---
id: MB.56.5.2.1
title: "Diary-comment strip: all crates except crates/blastit"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["crates/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T23:06"
updated: "2026-11-20T23:12"
---

# Goal
Finish the diary-comment strip for every crate **except** `crates/blastit`, which the next slice owns.

1. Remove plan-node citations and historical essays from `crates/{blastit-scheduler,blastit-view*,blastit-frame,blastit-sys,blastit-py-shim,blastit-py-sapi,blastit-php-sapi,blastit-sqlpolicy,blastit-manifest,blastit-lsp,blastit-dbproxy,blastit-env,blastit-events,blastit-ingress,blastit-http}/src/**/*.rs`. `crates/blastit-scheduler` and `crates/blastit-view-php` are half done — finish them (`src/lib.rs`, `src/error.rs`, `scheduler/admission.rs`, plus an artifact sweep for punctuation left behind by a removal).
2. Keep concise 1–3 line Rustdoc on public items, every `// SAFETY:` on an `unsafe` block, and any note a reader needs to avoid a real trap; compress history to present-tense fact, never delete knowledge.
3. Fix the stale references in scope: `crates/blastit-view-php/src/pinning.rs` (`scripts/check-no-test-seam.sh` → `scripts/build-standalone.sh`), `crates/blastit-scheduler/src/scheduler/views.rs` (points at a nonexistent `crates/blastit-view/src/php.rs`), and any `critical_<name>.rs` / `check_wheel_*` / `PY_SAPI_PROBE` reference — name the real owner (file + suite).

# Acceptance
- `make fast-check` green, zero warnings; `rg 'MB\.[0-9]+\.[0-9]+' crates/{blastit-scheduler,blastit-view*,blastit-frame,blastit-sys,blastit-py-shim,blastit-py-sapi,blastit-php-sapi,blastit-sqlpolicy,blastit-manifest,blastit-lsp,blastit-dbproxy,blastit-env,blastit-events,blastit-ingress,blastit-http}/src` returns nothing.
