---
id: MA.2
title: "Implement 0.1.1 DX upgrades (blastit dev, auto-manifest, --learn, check)"
kind: epic
status: done
priority: 2
depends_on: [MA.5]
paths: ["crates/**", "python/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T22:58"
---

# Goal

Implement every CLI, manifest, runtime (`blastit dev` unprivileged simulation), wire-learning (`--learn`), and framework-agnostic CI verification (`blastit test` / `blastit check` + `--json`) feature specified in `docs/design/ma-lean-dx.md`, with protected critical tests proving both DX behaviour and fail-closed production security invariants.

# Context

- Depends on accepted `MA.1` (`docs/design/ma-lean-dx.md` and updated `docs/**`).
- Load skills `dx-first`, `security-invariants`, `rust-standards`, `python-standards`, `testing`.
- Also clean up existing ruff lint issues in `python/` and add `python/` to `make py-lint` so the entire Python tree stays lint-gated alongside `tools/`.

# Acceptance

- [ ] Every requirement `R1..Rn` in `docs/design/ma-lean-dx.md` is implemented across `crates/*` and `python/blastit/*`.
- [ ] Production `blastit` refuses to run without root or required capabilities (critical test verifying non-zero exit and zero silent fallback to single-UID mode), while `blastit dev` runs completely unprivileged with real stage-0 `execve` secret stripping, per-route sockets, and policy enforcement.
- [ ] Framework-agnostic `--learn` and CI completeness check work end-to-end on both a Django app and a non-Django WSGI app (e.g. Flask / SQLAlchemy / raw WSGI) without `sudo`, producing both human and `--json` reports.
- [ ] `python/` passes `uvx --offline ruff check python tools` and `Makefile` gates `python` in `py-lint`.
- [ ] `make check` passes completely (including protected critical tests in `crates/*/tests/critical_*.rs` and `python/tests/critical/`).

# Notes

- `planner` will decompose `MA.2` into 4–7 small, parallelizable tasks once `MA.1` is accepted.
