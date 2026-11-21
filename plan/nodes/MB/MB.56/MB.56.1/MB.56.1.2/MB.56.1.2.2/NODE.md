---
id: MB.56.1.2.2
title: "Drop the duplicate blastit-dbproxy binary: one ELF, _bin/blastit --role=dbproxy, maturin backend"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.1.2.1]
paths:
  - pyproject.toml
  - Cargo.toml
  - Cargo.lock
  - blastit_build.py
  - "crates/blastit-dbproxy/**"
  - "crates/blastit/src/check/**"
  - crates/blastit/src/host.rs
  - crates/blastit/src/roles.rs
  - crates/blastit/src/visibility.rs
  - "crates/blastit/src/stage0/**"
  - crates/blastit/tests/critical_artifacts.rs
  - "scripts/**"
  - ".github/**"
  - "e2e/**"
  - Makefile
  - "python/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T16:25"
updated: "2026-11-20T16:54"
---

# Goal
Drop the duplicate `blastit-dbproxy` binary: the universal `blastit` already runs `--role=dbproxy`, so every bundle ships **one** ELF instead of two and `pyproject.toml` uses maturin's own backend.

1. **Delete the second binary:** `crates/blastit-dbproxy/src/main.rs` and its `[[bin]] name = "blastit-dbproxy"` block (the crate stays a library), and `blastit_build.py`. `pyproject.toml` becomes `build-backend = "maturin"` (`backend-path` dropped), its `[tool.maturin] include` loses the `_bin/blastit-dbproxy` and `blastit_build.py` entries.
2. **Repoint stage 0 and the bundle walk to `_bin/blastit --role=dbproxy`:** `crates/blastit/src/{check.rs,host.rs,roles.rs,visibility.rs,stage0/fork_exec.rs}` — the credentials-holding role is forked/`execve`d as the *same* image with `--role=dbproxy`, with the same empty environment and descriptor hand-off it already uses.
3. **Bundle layout & packaging:** `scripts/build-wheel.sh`, `scripts/repack_universal_wheel.py`, `scripts/collect-shim.sh` (if it names dbproxy), `scripts/check-bundle-layout.sh`, `scripts/build-standalone.sh`, `scripts/build-package-bundle.sh`, `scripts/build-{deb,rpm}.sh` — one ELF in `_bin/`/`lib/`, no `blastit-dbproxy`.
4. **GHA + e2e:** `.github/actions/build-wheel/action.yml` (the two `grep -qa`/`zipinfo` assertions), `.github/workflows/publish-pypi.yml:646`, `e2e/rootless/run.py:85-100,599-603`, `e2e/venv-smoke/run.sh:34,61,244,283`, and `crates/blastit/tests/critical_artifacts.rs` (its member table) must assert the single-binary layout. `.github/workflows/ci.yml`'s dbproxy test job keeps working against the library crate.
5. **Prove the role still refuses to run interpreter-linked:** `blastit --role=dbproxy` must stay a stage-0-only path (`make rs-no-python` stays green) and hold no credentials in any child's environment.

# Acceptance
- `make fast-check` green (incl. `universal-smoke`), zero warnings; `rg '_bin/blastit-dbproxy|blastit_build|blastit-dbproxy\b' --glob '!target' --glob '!plan'` returns nothing outside `crates/blastit-dbproxy/` itself (library name only).
- `cargo test -p blastit --test critical_artifacts` green; `bash -n` clean on every touched script.
