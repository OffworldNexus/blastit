---
id: MB.56.1.2
title: "Inline blastit check grant-diff into crates/blastit/src/check/, delete blastit-audit crate, drop duplicate blastit-dbproxy bin + blastit_build backend"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.1.1]
paths:
  - Cargo.toml
  - Cargo.lock
  - pyproject.toml
  - blastit_build.py
  - "crates/blastit-audit/**"
  - "crates/blastit-dbproxy/**"
  - crates/blastit/src/lib.rs
  - crates/blastit/src/check.rs
  - "crates/blastit/src/check/**"
  - crates/blastit/src/visibility.rs
  - crates/blastit/src/host.rs
  - crates/blastit/src/roles.rs
  - "crates/blastit/src/stage0/**"
  - crates/blastit/tests/critical_artifacts.rs
  - "scripts/build-wheel.sh"
  - "scripts/build-standalone.sh"
  - scripts/repack_universal_wheel.py
  - ".github/actions/build-wheel/action.yml"
  - ".github/workflows/publish-pypi.yml"
  - e2e/rootless/run.py
  - "e2e/venv-smoke/run.sh"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T16:04"
updated: "2026-11-20T16:04"
---

# Goal
Inline the live `blastit check` grant-diff/report modules into `crates/blastit/src/check/`, delete the `blastit-audit` crate and the duplicate `blastit-dbproxy` `[[bin]]` + custom build backend, and repoint every `_bin/blastit-dbproxy` consumer at `_bin/blastit --role=dbproxy`.

# Context
- `crates/blastit-audit/` is 6814 LOC but `crates/blastit/src/check.rs` (2248) only needs the grant-diff + human/`--json` report path; it reaches the crate through `crates/blastit/src/lib.rs` (`pub use blastit_audit as audit_v1; pub use blastit_audit::infer_routes;`). The retired `0.1.0` `audit()` / `covering()` / `blastfile_model()` shims are dead.
- `crates/blastit-dbproxy/src/main.rs` is a duplicate `[[bin]] name = "blastit-dbproxy"`; `blastit` already ships the same role as `blastit --role=dbproxy`, so the wheel ships the proxy twice. `blastit_build.py` exists only to stage that extra bin.
- Consumers to repoint: `crates/blastit/src/{check.rs,visibility.rs,host.rs,roles.rs,stage0/fork_exec.rs}`, `crates/blastit/tests/critical_artifacts.rs` (assertions only; the file is deleted in `.3`), `e2e/rootless/run.py`, `e2e/venv-smoke/run.sh`, `scripts/build-wheel.sh`, `scripts/build-standalone.sh`, `scripts/repack_universal_wheel.py`, `.github/actions/build-wheel/action.yml`, `.github/workflows/publish-pypi.yml`.
- Single Cargo writer: this slice owns `Cargo.toml`/`Cargo.lock`; nothing else may build concurrently.

# Work
1. Create `crates/blastit/src/check/` and move exactly the modules `check` uses (dead-grant diff, coverage/`Coverage`, human + `--json` report rendering, route inference), deleting the unused `audit()`/`covering()`/`blastfile_model()` shims; replace `pub use blastit_audit as audit_v1` and the `infer_routes` re-export in `crates/blastit/src/lib.rs`.
2. Delete `crates/blastit-audit/` and its workspace member; resolve `Cargo.lock`.
3. Delete the `blastit-dbproxy` `[[bin]]` source under `crates/blastit-dbproxy/` and `blastit_build.py`; set `pyproject.toml` to `build-backend = "maturin"` and drop the `[tool.maturin] include` of `python/blastit/_bin/blastit-dbproxy`.
4. Repoint every `_bin/blastit-dbproxy` path to `_bin/blastit --role=dbproxy` in the list above (stage 0 `fork_exec.rs` argv, `check.rs`/`visibility.rs`/`host.rs`/`roles.rs` resolution, both build scripts, repack script, GHA action + publish workflow, `e2e/rootless/run.py`, `e2e/venv-smoke/run.sh`); keep `crates/blastit/tests/critical_artifacts.rs` assertions compiling in place.
5. Verify no `#[cfg(test)]` mock servers were introduced: proxy/daemon paths keep real `UnixStream::pair()` / `UnixListener` coverage.

# Acceptance
- `make fast-check` green
- `rg -n "blastit-audit|blastit_audit|blastit_build|_bin/blastit-dbproxy" crates/ scripts/ .github/ e2e/ pyproject.toml Cargo.toml` returns nothing
- `cargo test -p blastit --test critical_artifacts` green (assertions retargeted)
- `ls crates/blastit-dbproxy crates/blastit-audit` absent; `target/` unchanged elsewhere

# Notes
- Wheel size halves; `scripts/build-wheel.sh` remains the only wheel builder, now driving plain `maturin build`.
