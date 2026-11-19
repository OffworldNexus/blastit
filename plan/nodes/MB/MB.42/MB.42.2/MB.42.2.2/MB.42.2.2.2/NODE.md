---
id: MB.42.2.2.2
title: "blastit --internal-json (12-row contract) + pure-stdlib _blastit.py bridge; retire crates/blastit-py"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.2.2.1]
paths: [Cargo.toml, Cargo.lock, "crates/**", "python/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T20:07"
updated: "2026-11-19T20:07"
---

# Goal
Replace the per-CPython-minor `blastit._blastit` extension with a **pure-stdlib Python bridge** so one `py3-none-*` wheel works on every CPython `>=3.12`: a hidden `blastit --internal-json` NDJSON dispatcher (design §3's 12-row contract) plus a fork-safe `python/blastit/_blastit.py` client. Then retire `crates/blastit-py`.

# Exact Deliverables (design §3)
1. **`blastit --internal-json`** (hidden from `--help`, refused together with `--stage=1`/`--role=*`): line-delimited JSON `{"op":"…","args":{…}}` -> `{"ok":true,"result":…}` / `{"ok":false,"error":"…"}`. Implement **exactly** design §3's 12-row table — same `op` names, same `args` keys, same result types (`audit`/`analyze`/`covering`/`classify`/`check_audit`/`plan_learn`/`probe_ingress`/`blastfile_model` return **JSON text**; `normalize`/`redact_literals`/`blastfile_apply_edits` return `str`; `has_quoted_literal` returns `bool`). Errors raise `ValueError` on the Python side, never a `PyResult`. `probe_ingress` takes `prefix_b64`. Backed by `crates/blastit-audit` (`MB.42.2.1`) + `crate::audit_v1`'s successors; the dispatcher stays `<200` prod LOC.
2. **`python/blastit/_blastit.py`** — 100% stdlib, Python `>=3.12`, no compiled extension:
   - lazy NDJSON pipe child, guarded by `(owner_pid == os.getpid(), threading.Lock)`, with `os.register_at_fork(after_in_child=…)` resetting the lock and closing inherited fds **without flushing**, plus `atexit` cleanup;
   - `bin_path()` resolution `<pkg>/_bin/blastit` -> `<pkg>/../../lib/blastit` -> `BLASTIT_BIN` -> `PATH`;
   - the 12 functions with the §3 signatures; a `{"ok":false}` response raises `ValueError(msg)`;
   - `main(argv, executable, dbproxy="") -> int` kept as a thin `execvpe(bin_path(), …)` wrapper **so protected `python/tests/critical/test_lsp_stdio.py` needs zero edits**.
3. **`python/blastit/__main__.py`:** `main()` sets `BLASTIT_WHEEL_PYTHON` (`setdefault`) + `BLASTIT_HANDOFF_DONE=1`, then `_blastit.main(...)`; `main_lsp()` `execvpe`s the sibling `blastit-lsp`.
4. **`python/blastit/_check_pytest.py`:** `_probe` calls its own `_probe_stdlib` unconditionally (no `from blastit import _blastit` for the ingress probe).
5. **`Makefile` `test-critical`:** stage `target/debug/blastit` at `$(CRITICAL_STAGE)/blastit/_bin/blastit` **and** symlink `_bin/blastit-dbproxy -> blastit`, so `test_lsp_stdio.py` and `test_check_contract.py` pass with zero edits.
6. **Retire `crates/blastit-py`** only after 1–5 are green: delete the crate, `Host::wheel`, `Install::Wheel` (keep `Host::binary()` handling `--role=dbproxy` everywhere), and drop it from the workspace. **This is the step that finally closes the `LESSONS.md` #42 feature-unification edge** — prove it with `cargo build --workspace --locked` + `scripts/check-portable-elf.sh`.
7. **Retire the compatibility re-exports** left by `MB.42.2.2.1`: `crates/blastit/src/lib.rs:55` (`pub use blastit_audit as audit_v1;`) and `:67` (`infer_routes`). They were a deliberate shim that kept the three protected audit suites byte-identical during the move; a crate named `audit_v1` re-exported as `blastit`'s public audit surface will outlive its meaning. Rename the call sites (`check.rs`, `dev_learn.rs`, `cli/**`) to `blastit_audit` directly and delete both re-exports — the protected tests name `crate::audit_v1::…`, so if that forces an edit to one of them, **stop and report** rather than editing it.

# Hard constraints
- **`_blastit.py` must not import a compiled module and must not need a `.so`**: it is what makes `py3-none-*` possible. Prove it by running `blastit check` and `fold` from a **CPython 3.12** venv with no per-minor artifact.
- Fork safety: a bridge call must never be made after a `fork` from the child of a view (document the rule and assert the `owner_pid` guard).
- Never leak a secret into `error` text; SQL/manifest text may appear, credentials may not.
- `<500` prod LOC per Rust file, `<200` for `internal_json.rs`, `<=6` args; zero globals/`Mock*Server`/`#[cfg(test)]` env seams.
- Do not modify `crates/*/tests/critical_*.rs` or `python/tests/critical/` — if one must change, stop and report.
- Never touch `docs/**`.
- One Cargo writer; never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `cargo test -p blastit --locked` + the new `critical_internal_json.rs` (all 12 rows: a valid frame, an adversarial/oversized frame refused, `{"ok":false}` raising `ValueError`, no secret in any error)
- `python/tests/runnable_docs/`-independent check: from a **CPython 3.12** venv, `blastit check` and `python -m blastit fold` work with no per-minor `.so` (`uv venv --python 3.12 /tmp/opencode/py312 && uv pip install --no-build-isolation -e python/` or the built wheel)
- protected `python/tests/critical/test_check_contract.py` and `test_lsp_stdio.py` pass **unedited**
- `python/tests/test_edits_binding.py` passes
- after the `blastit-py` deletion: `cargo build --workspace --locked` + `scripts/check-portable-elf.sh target/debug/blastit` -> zero `libpython`/`libphp`/`RUNPATH`; `cargo tree -p blastit --locked -e normal | grep -c pyo3` -> `0`
- `make fast-check`
