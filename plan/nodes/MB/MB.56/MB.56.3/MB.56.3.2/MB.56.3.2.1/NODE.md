---
id: MB.56.3.2.1
title: "e2e_dev.rs part 1: Python WSGI multi-view, host rules, venv activation"
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths: ["crates/blastit/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T18:54"
updated: "2026-11-20T18:54"
---

# Goal
Create `crates/blastit/tests/e2e_dev.rs` covering the **Python WSGI** half of `TESTING_PLAN.md` §2 & §4 and delete the suites it absorbs.

1. Python WSGI multi-view app (CPython 3.13 **and** 3.14): `trait` grants shared across views, parameterized `$1` table/column read/write grants, ungranted SQL and ungranted column rejected at runtime, zero database credentials in the worker environment.
2. Host-interpreter rules, verbatim: `blastit dev: CPython 3.12 at <path> is not supported for WSGI views (CPython 3.13 and 3.14 …)`, the free-threaded refusal, the missing-`libpython` sentence naming the minor and every remedy, an unsupported/relative `BLASTIT_PYTHON`.
3. The venv/`sys.path` activation rule (`embed_venv_sys_path` behaviour): a project venv's site-packages is what the view imports from, an activated ambient venv is skipped.
4. Absorb and delete: `critical_python_shim.rs`, `python_activation_gate.rs`, `embed_venv_sys_path.rs`, and the WSGI half of `critical_parallel_views.rs`.
5. Start the file with the shared harness top (spawn `CARGO_BIN_EXE_blastit`, temp project root, free port from `28080+`, deadline-based waits) reused by `MB.56.3.2.2`/`.3`; target ≲ 800 LOC at the end of this slice.

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_dev` green; every absorbed file deleted.
