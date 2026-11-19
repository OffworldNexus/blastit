---
id: MB.42.2.2
title: "blastit-audit extraction + blastit --internal-json + pure-Python _blastit.py bridge; retire blastit-py/pyo3"
kind: epic
status: ready
priority: 1
depends_on: [MB.42.2.1]
paths: [Cargo.toml, Cargo.lock, "crates/**", "python/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T17:18"
updated: "2026-11-19T20:07"
---

# Goal
Retire the per-CPython extension: **one** grant-diff/route-trace owner (`crates/blastit-audit`), a `blastit --internal-json` NDJSON dispatcher, and a 100%-stdlib `python/blastit/_blastit.py` fork-safe client, so Python `>=3.12` runs `blastit check`/`fold` with no `.so` in `.venv` — `docs/design/universal-distribution.md` §3, `ADR-0017` Decision 4.

# Exact Deliverables
1. **`crates/blastit-audit`** (zero `pyo3`): fold **both** `crates/blastit-py/src/audit.rs` (3.6k lines) and `crates/blastit/src/audit_v1.rs` (2.1k lines) into `<500`-LOC modules `normalize`, `analyze`, `covering`, `model`, `obs`, `evaluate`, `learn`, `report`; one public API consumed by `blastit`, `blastit-lsp` and `--internal-json`; `crates/blastit/src/audit_v1.rs` becomes a thin re-export (or is deleted with every caller moved).
2. **`crates/blastit/src/cli/internal_json.rs` (<200 LOC)**: NDJSON request/response loop (`{"op":…,"args":{…}}` -> `{"ok":true,"result":…}` / `{"ok":false,"error":…}`) implementing **exactly** the 12 rows of design §3 (`normalize`, `has_quoted_literal`, `redact_literals`, `audit`, `analyze`, `covering`, `classify`, `check_audit`, `plan_learn`, `probe_ingress`, `blastfile_apply_edits`, `blastfile_model`) with the listed Python signatures, arg keys and result types; hidden from `--help`; never hands off.
3. **`python/blastit/_blastit.py`** (100% stdlib, `>=3.12`): lazy `blastit --internal-json` pipe child guarded by `(owner_pid == os.getpid(), threading.Lock)` + `os.register_at_fork(after_in_child=…)` (re-lock + close inherited fds without flush) + `atexit` cleanup; every `{"ok":false}` raises `ValueError(msg)`; `bin_path()` resolution `<pkg>/_bin/blastit` -> `<pkg>/../../lib/blastit` -> `BLASTIT_BIN` -> `PATH`; `main(argv, executable, dbproxy="") -> int` sets `BLASTIT_WHEEL_PYTHON` via `setdefault` + `BLASTIT_HANDOFF_DONE="1"` then `os.execvpe(bin_path(), [bin_path(), *argv], env)`.
4. **`python/blastit/__main__.py`** -> `_blastit.main(sys.argv[1:], sys.executable, "")`; `main_lsp()` `execvpe`s the sibling `blastit-lsp`; `python/blastit/_check_pytest.py` `_probe` calls `_probe_stdlib` directly, and `check.rs::bundled_python` resolves `<exe_dir>/../python` or `<exe_dir>/../../..`.
5. **Deletions**: `crates/blastit-py/` (workspace member, `pyo3`, `extension-module`), `Host::wheel`/`Install::Wheel`, and `crates/blastit-dbproxy/src/main.rs` + its `[[bin]]` stanza (`blastit-dbproxy` stays a library crate; `Makefile install:` and `blastit_build.py` drop `-p blastit-dbproxy`; `Host::binary()` serves `--role=dbproxy`).

# Acceptance
- `make fast-check`
- `cargo test -p blastit-audit -p blastit --test critical_internal_json` (new suite: all 12 ops, a denied/adversarial frame, and an `{"ok":false}` round trip with zero secret text in the error).
- `make test-critical` — protected `python/tests/critical/test_lsp_stdio.py` + `test_check_contract.py` pass **unedited**, and `python/tests/test_edits_binding.py` passes.
- Real 3.12 proof with no extension: `uv venv --python 3.12 /tmp/opencode/mb422-check && …/bin/python -m blastit check --json` plus `…/bin/python -m blastit fold --check` on a fixture project.
- `uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.2` -> clean.

# Notes
- **1 Cargo writer**; `Cargo.toml`/`Cargo.lock` edits belong to this node alone while it runs.
- Never write `/home/remy/.local`, `/usr/bin/blastit`, or the host's PHP/Python packages; package work happens in a container.
- Do not edit or weaken existing `crates/*/tests/critical_*.rs` or `python/tests/critical/` — they are the proof that the bridge is contract-compatible.
- Fork-safety is the point: no fd or lock may leak into a `fork`ed child; a hung/absent child must raise, never hang.
- Keep files `<500` prod LOC, zero `unwrap()`/`expect()` on untrusted JSON, no `scripts/check_*.py` guards; out-of-scope finds go to `## Follow-ups`.
