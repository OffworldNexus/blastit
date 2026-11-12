---
id: MA.1
title: "Write lean Local/CI/Deploy documentation & 0.1.1 DX design spec first"
kind: task
status: ready
owner: architect
priority: 1
depends_on: []
paths: [README.md, "docs/**", zensical.toml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

**Docs Before Code:** Imagine the sharpest, simplest, most foolproof developer experience for Blastit's current feature scope (Python WSGI + PostgreSQL), rewrite the user-facing documentation (`README.md` and `docs/**`) to reflect that lean UX across **Local machine**, **CI/CD**, and **Deployments**, critique it through fresh-context `dx`, `ai-ergonomics`, and `security` lenses, and produce the companion technical design `docs/design/ma-lean-dx.md` (+ ADRs) specifying the exact CLI/manifest/runtime changes `MA.2` will implement.

# Context

- Load skills `dx-first`, `design-docs`, `security-invariants`, `rust-standards`, `python-standards`.
- Read `docs/VISION.md`, `README.md`, `zensical.toml`, `docs/index.md`, `docs/getting-started.md`, `docs/frameworks/*`, `docs/infra/*`, `docs/reference/*`, and `ARCHITECTURE.md` §§2, 4, 5, 6, 8.
- Key UX pillars to nail with minimum steps and zero bloat:
  1. **Local developer machine:** Zero `sudo`, zero `chmod o+rx ~`, zero env-var soup. E.g. `blastit dev` (unprivileged single-UID simulation with real processes, real stage-0 `execve` secret swap, per-route sockets in a temp/runtime dir, pretty logs on TTY, and optional `--learn` to record routes + normalized SQL or minimal table grants in one pass), plus a clean 1-command path when testing real Linux capabilities locally.
  2. **CI/CD:** One unprivileged command (working identically for Django, Flask, SQLAlchemy, or any WSGI app!) that runs the test suite against Blastit's wire brokers/routing and fails on unrouted requests, missing SQL grants, dead grants, or unexercised routes — with both human summary and `--json` output tailored for AI agents.
  3. **Deployments:** Lean, copy-pasteable 1-screen recipes for DigitalOcean App Platform (`--preset digitalocean-paas`), Docker Compose + Traefik (`--preset compose-traefik`), and generic Linux / Kubernetes / systemd — where production `blastit` strictly requires real UIDs/capabilities + seccomp (failing closed if missing).
  4. **Lean prose:** Cut documentation word count dramatically; move kernel/gVisor deep dives out of Getting Started and framework guides.

# Acceptance

- [ ] `README.md` and `docs/**` (+ `zensical.toml` nav if simplified) present the complete, ultra-lean 0.1.1 developer experience for Local, CI/CD, and Deployments, with Getting Started requiring ≤ 3 steps and zero `sudo`.
- [ ] `make docs-check` (`uvx --offline zensical build --strict`) passes with zero warnings or broken links/anchors.
- [ ] Technical design doc `docs/design/ma-lean-dx.md` and proposed ADRs (`docs/decisions/ADR-0001-*.md`...) specify exact CLI subcommands/flags, `blastit.toml` keys, wire-level `--learn` & `test`/`check` mechanics, unprivileged `dev` vs fail-closed production invariants, and an ordered leaf task breakdown for `MA.2`.
- [ ] Parallel goldfish `critic` reviews (`docs/design/ma-lean-dx.review-dx.md`, `.review-ai-ergonomics.md`, `.review-security.md`, `.review-implementability.md`) have been run and all `BLOCKER` and `MAJOR` findings are resolved in `## Review Resolution`.

# Notes

- Escalate to `strategist` for core DX/design acceptance once critic findings are resolved (`skill design-docs`).
