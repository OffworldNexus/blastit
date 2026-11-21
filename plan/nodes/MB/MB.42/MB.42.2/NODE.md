---
id: MB.42.2
title: "Implement universal libpython-free blastit binary (per-minor dlopen Python shims) & safe local CLI handoff"
kind: epic
status: done
priority: 1
depends_on: [MB.42.1]
paths: [Cargo.toml, Cargo.lock, Makefile, "crates/**", "python/**", "scripts/**", "e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T14:30"
updated: "2026-11-19T23:35"
---

# Goal
Implement the universal `libpython`-free `blastit` binary (loading Python via per-minor `libblastit_py3{13,14}-<arch>-<libc>.so` `dlopen` shims with `(st_dev, st_ino)` pinning, mirroring PHP) and safe local CLI handoff (`./.venv/bin/blastit`, `./vendor/bin/blastit`, `./node_modules/.bin/blastit`).

# Exact Deliverables (per `ADR-0017` & `docs/design/universal-distribution.md`)
1. **Per-Minor Python SAPI Shims & `dlopen` Loader:**
   - Extract/build `libblastit_py3{13,14}-<arch>-<libc>.so` (`cdylib` wrapping `blastit-view-python` with undefined dynamic `Py*` symbols, built via `make py-sapi` for every CPython 3.13/3.14 available via `uv python find`, with `DT_SONAME` matching `libblastit_py3XX-<arch>-<libc>.so`).
   - Remove `pyo3` from `crates/blastit`'s direct dependencies so `cargo build -p blastit` produces **one universal binary** with **zero `DT_NEEDED` on `libpython`** (`scripts/check-portable-elf.sh` passes on the full all-features `blastit` binary!) while serving Python WSGI (3.13 & 3.14), PHP (8.3, 8.4, 8.5), Static, and LSP!
   - Preserve CoW preload, `Py_Immortalize`, `fanotify` loaded-file tracking, `PyCode::compile` reload syntax check, `(st_dev, st_ino)` fd-pinning across stage 0 -> stage 1, and production `visibility`/ownership checks!
   - Also expose a clean CLI/JSON subcommand or pure-Python equivalent for any `fold.py` / `_check_pytest.py` helper that previously imported `blastit._blastit` so Python `3.12+` can run `blastit check` and `fold` without needing a per-CPython-minor `.so` inside `.venv`!
2. **Safe Local CLI Handoff (`crates/blastit/src/cli/`):**
   - On `dev`, `check`, `test`, `fmt`, `lsp` (NEVER production `blastit`, `--stage=1`, or `--role=dbproxy`), when `BLASTIT_NO_LOCAL_HANDOFF` and `BLASTIT_HANDOFF_DONE` are unset, check `$VIRTUAL_ENV/bin/blastit` / `./.venv/bin/blastit`, `./vendor/bin/blastit`, `./node_modules/.bin/blastit`; verify regular executable, different `(st_dev, st_ino)`, safe owner (`geteuid()` or `0`) and non-group/world-writable file & parent dir, set `BLASTIT_HANDOFF_DONE=1`, and `execv`!
   - Add unit + integration tests proving handoff works on valid `.venv`/`vendor`/`node_modules` binaries, prevents self-handoff loops, refuses `0777` / wrong-uid binaries, and NEVER runs on production `blastit`!
3. **Update `Makefile` (`py-sapi`, `install`, `fast-check`, `rs-no-python`):**
   - `make install` stages the universal `blastit`, `blastit-lsp`, `libblastit_php*.so`, `libblastit_py*.so`, and `python/blastit/*.py` under `$(PREFIX)/share/blastit/`.

# Acceptance
- `scripts/check-portable-elf.sh target/debug/blastit` (0 `libpython` in the full `blastit` binary!)
- `make fast-check && make rs-test-php`
- Test universal `blastit` binary serving Python 3.13 `.venv` AND Python 3.14 `.venv` AND PHP 8.4 AND Static, plus local CLI handoff tests!

## Slices
1. `MB.42.2.1` — Shared fd pinning + `blastit-py-sapi`/`blastit-py-shim` + Stage-0/1 `dlopen` + `make py-sapi` (one `libpython`-free binary).
2. `MB.42.2.2` — `blastit-audit` extraction + `blastit --internal-json` + pure-Python `_blastit.py`; retire `blastit-py`/`pyo3`.
3. `MB.42.2.3` — Safe local CLI handoff (`crates/blastit/src/cli/handoff.rs`), zero-TOCTOU.
4. `MB.42.2.4` — Integration glue: `make install`/`test-critical` staging + one-binary four-runtime E2E.
