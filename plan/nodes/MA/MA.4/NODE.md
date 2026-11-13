---
id: MA.4
title: "Fix make check on this host: PyO3 test binaries need PYTHONHOME"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [Makefile, docs/knowledge/LESSONS.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T01:41"
updated: "2026-11-12T02:05"
---

# Goal

Make `make check` green on this host again by teaching the Makefile's Rust test environment about the uv-managed CPython prefix, so PyO3 test binaries can locate the 3.14 stdlib they embed.

# Context

- `make rs-test` currently aborts the entire `blastit` lib test binary at `immortal::tests::a_skipped_pass_walks_nothing` with:
  - `Could not find platform independent libraries <prefix>`
  - `ModuleNotFoundError: No module named 'encodings'`
- Root cause: the Rust test binary embeds CPython (PyO3) and cannot locate the uv-managed 3.14 stdlib. `PYO3_PYTHON` alone is not enough for the embedded interpreter at runtime; `PYTHONHOME` must point at the **prefix** of `$(PYO3_BIN)` (i.e. the parent of `bin/`), which is the same directory whose `../lib` the Makefile already puts on `LD_LIBRARY_PATH` via `PYO3_LIBDIR`.
- Conductor verified the fix locally: with `PYTHONHOME=$prefix` all 20 immortal tests pass, 1 ignored.
- Relevant Makefile lines: `PYO3_BIN` (line ~16), `PYO3_LIBDIR` (~17), `RUST_ENV` (~18), `rs-test` (~54). `RUST_ENV` is shared by `rs-lint`, `rs-doc`, `rs-test`, `rs-test-critical` and `mutants`, so fixing it once fixes every Rust target.
- Load skill `rust-standards` (build/tooling section) and read `docs/knowledge/LESSONS.md`.

# Acceptance

- [ ] `make check` exits 0 from a clean shell on this host (no manually exported `PYTHONHOME`/`PYO3_PYTHON`/`LD_LIBRARY_PATH`).
- [ ] `make rs-test` runs the **full** workspace suite: no test binary aborts on interpreter startup, and the previously failing `blastit::immortal` tests execute (20 pass, 1 ignored).
- [ ] The Makefile `RUST_ENV` derives the prefix from `$(PYO3_BIN)` and exports `PYTHONHOME` there, without hardcoding any host-specific path.
- [ ] `docs/knowledge/LESSONS.md` PyO3 bullet documents the `PYTHONHOME` requirement (embedded interpreter needs the uv-managed stdlib prefix, not just `PYO3_PYTHON` + `LD_LIBRARY_PATH`).
- [ ] No test is weakened, skipped or newly `#[ignore]`d, and no file under `crates/*/tests/critical_*.rs` or `python/tests/critical/` is modified.
- [ ] `RESULT.md` pastes command + output tails for `make rs-test` and `make check`.

# Notes

- Scope is exactly `Makefile` + `docs/knowledge/LESSONS.md`; do not touch crate sources.
- If a `PYTHONHOME` export leaks into `rs-doc`/`clippy` and causes noise, keep it in `RUST_ENV` anyway (it is harmless for non-embedding targets) rather than duplicating env blocks.
- Run `uv run --no-project tools/plan.py --agent implementer log MA.4 "..."` with the failing/passing output tails.
