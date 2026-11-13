---
id: MA
title: "Lean DX & Show-the-World Polish (0.1.1)"
kind: milestone
status: ready
priority: 1
depends_on: []
paths: []
attempts: 0
max_attempts: 3
escalation: strategist
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

Reimagine and ship **Blastit `0.1.1`** (`v0.1.0` was already released on 2026-10-27): turn today's high-friction CLI and verbose documentation into the **leanest, most foolproof developer experience** for Python WSGI + PostgreSQL — something crisp enough to show the world immediately.

Everything in this milestone starts **from the documentation inward**: first write and critique the lean user-facing documentation for **(1) the local machine** (zero `sudo`), **(2) CI/CD** (100% confidence zero SQL queries or routes were forgotten), and **(3) deployments** (presets & 12-factor containers), then implement the exact CLI/server/manifest ergonomics that make every documented command work verbatim.

# Context

- Vision & rules: `docs/VISION.md`, skill `dx-first`, skill `security-invariants`.
- Current baseline: `ARCHITECTURE.md`, `README.md`, `docs/**`, `zensical.toml`, `crates/*`, `python/blastit/*`.
- Pain points in `0.1.0` to eliminate:
  - Requiring `sudo env BLASTIT_APP=... BLASTIT_MANIFEST=$PWD/blastit.toml PORT=8080 .venv/bin/blastit` just to try Blastit on a laptop.
  - Defaulting `BLASTIT_MANIFEST` only to `/app/blastit.toml` instead of auto-discovering `./blastit.toml` and allowing non-secret app wiring (`app`, `preload`) inside `blastit.toml`.
  - Aborting on the first ungranted SQL query per request during development instead of offering a single-pass wire-level `--learn` mode at `dbproxy`/`scheduler`.
  - Restricting test-suite manifest verification to `pytest-django` only, leaving Flask, SQLAlchemy, and other WSGI frameworks without first-class CI completeness checking.
  - Verbose documentation pages filled with kernel trivia (`chmod o+rx` on `~/.local/share/uv`, sticky-bit `sys.path` lectures) right in Getting Started.

# Acceptance

- [ ] `docs/` and `README.md` are rewritten to be radically leaner, concrete, and structured around **Local machine** (zero `sudo`), **CI/CD**, and **Deployments**, with `make docs-check` (`zensical build --strict`) passing and `visual-checker` returning `PASS`.
- [ ] `blastit dev` runs unprivileged on a Linux laptop and in CI under the caller's own UID while preserving the real multi-process architecture (`http`, `scheduler`, `dbproxy`, `views`), stage-0 `execve` secret stripping, per-route private socket dirs, and full SQL/route policy checks — emitting an unmistakable dev-mode banner while production `blastit` still **fails closed** without root or capabilities.
- [ ] `./blastit.toml` is discovered automatically when `BLASTIT_MANIFEST` is unset, and non-secret app wiring (`app`, `preload`) can live cleanly in `blastit.toml` (with `BLASTIT_*` env vars still overriding).
- [ ] Both humans and AI coding agents can populate and verify `blastit.toml` in a single pass across **any** Python WSGI framework (Django, Flask, SQLAlchemy, raw psycopg) via wire-level `--learn` and strict CI verification (`blastit check` / `blastit test` with `--json` output).
- [ ] Every command and `blastit.toml` example in `README.md` and `docs/` is verified by automated runnable-docs tests.
- [ ] `make check` and `make e2e` pass green, `test-curator` milestone audit passes (with `cargo-mutants` on `blastit-manifest` / `blastit-sqlpolicy`), `Cargo.toml` version is `0.1.1`, `CHANGELOG.md` has the `0.1.1` entry, and a `HUMAN.md` sign-off item is opened for tagging `v0.1.1`.

# Notes

- Do **not** run `git push` or `git tag`; leave pushing/tagging for the owner via `HUMAN.md`.
- Keep `python/blastit/` 100% stdlib (`dependencies = []` in `pyproject.toml`).
