---
id: MA.1
title: Rewrite lean Local/CI/Deploy user documentation (DX spec) first
kind: task
status: active
owner: "doc-writer"
priority: 1
depends_on: []
paths: [README.md, "docs/**", zensical.toml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T02:05"
---

# Goal

**Docs Before Design, Design Before Code.** Rewrite `README.md` and `docs/**` (and simplify the `zensical.toml` nav where it helps) into the leanest, most foolproof developer experience for Blastit `0.1.1`, covering all three environments:

- **(a) Local machine:** `blastit dev` on a plain Linux laptop, **zero `sudo`**, Getting Started in **≤ 3 steps** to a working hello world, with `--learn` capturing routes and SQL grants in one pass.
- **(b) CI/CD:** one unprivileged command that runs the app's own test suite through Blastit's brokers/validators and proves 4-way completeness (every observed request routed, every outgoing operation granted, every declared route/grant exercised, every body-accepting route declares ingress validation), with human output plus `--json` for AI agents.
- **(c) Deployments:** copy-pasteable `digitalocean-paas` and `compose-traefik` presets, plus a generic 12-factor container / systemd recipe, where production `blastit` **fails closed** without real OS isolation.

This task delivers **user-facing documentation only** — no internal design, no ADRs, no code. The technical design that makes these pages executable is `MA.5`; the implementation is `MA.2`. The documented UX is the source of truth the design must follow, not the reverse.

# Context

- Load skills `dx-first`, `design-docs`, `security-invariants`.
- Read `docs/VISION.md`, `README.md`, `zensical.toml`, `docs/index.md`, `docs/getting-started.md`, `docs/frameworks/*`, `docs/infra/*`, `docs/reference/*`, `docs/how-it-works.md`, `docs/security.md`, and `docs/knowledge/LESSONS.md`.
- Key UX pillars to nail with minimum steps and zero bloat:
  1. **Local developer machine:** zero `sudo`, zero `chmod o+rx ~`, zero env-var soup. One command starts `blastit dev` (unprivileged single-UID simulation with the real process topology, real stage-0 `execve` secret swap, per-route sockets in a runtime dir), plus optional `--learn` to record routes + normalized SQL / minimal grants in one pass. Optional real-capability local testing gets one short recipe, not a lecture.
  2. **CI/CD:** one unprivileged command working identically for Django, Flask, SQLAlchemy or any WSGI app; fails on unrouted requests, missing SQL grants, dead grants or unexercised routes; offers both a human summary and `--json` output an AI agent can apply in one edit.
  3. **Deployments:** lean one-screen recipes for DigitalOcean App Platform (`--preset digitalocean-paas`), Docker Compose + Traefik (`--preset compose-traefik`), and generic Linux / Kubernetes / systemd, where production `blastit` strictly requires real UIDs/capabilities + seccomp and fails closed when they are missing.
  4. **Lean prose:** cut word count hard; keep kernel/gVisor/OS mechanics in `how-it-works.md` and `security.md`, never in Getting Started or framework guides.
- Ownership split (do not cross it): `MA.1` owns `README.md`, the public `docs/**` pages and `zensical.toml`. `MA.5` owns `docs/design/ma-lean-dx.md` + `docs/decisions/ADR-*.md` (technical design + ADRs) with the `implementability`/`testability`/`consistency` lenses. `MA.2`/`MA.3` own code and runnable-docs/visual/e2e verification.

# Acceptance

- [ ] `README.md` and `docs/**` present the complete, ultra-lean 0.1.1 developer experience for **Local machine**, **CI/CD** and **Deployments**. Getting Started requires **≤ 3 steps** and **zero `sudo`**, and no kernel/gVisor/`chmod o+rx`/sticky-bit/`sys.path` trivia appears in Getting Started or any framework guide. Verify: `grep -RIn -iE "gvisor|chmod o\+rx|sticky|setcap|sys\.path" README.md docs/getting-started.md docs/frameworks/` prints nothing.
- [ ] `make docs-check` (`uvx --offline zensical build --strict`) passes with zero warnings, broken links or broken anchors, and the `zensical.toml` nav matches the shipped page set (no orphan pages, no nav entries pointing at excluded pages).
- [ ] Fresh-context goldfish `critic` reviews with lenses `dx`, `ai-ergonomics` and `security` are written to `docs/design/ma-lean-dx.review-dx.md`, `.review-ai-ergonomics.md` and `.review-security.md` (review artefacts only), and every `BLOCKER`/`MAJOR` finding is resolved — in the docs, or recorded with a `## Review Resolution` section in `plan/nodes/MA/MA.1/RESULT.md` naming each finding and its fix.
- [ ] No design document and no ADR is authored by this task: `docs/design/ma-lean-dx.md` and every file under `docs/decisions/**` are `MA.5`'s, and the only files this task may create under `docs/design/` are the three `ma-lean-dx.review-*.md` critic artefacts above. Verify: this task's `RESULT.md` lists no `docs/design/ma-lean-dx.md`, no other `docs/design/*.md` content file and no `docs/decisions/ADR-*.md` as created or modified.

# Notes

- **Docs before design:** if a documented step cannot be implemented, do not silently weaken the docs — record the conflict and escalate to `strategist`/`architect` (`skill design-docs`).
- `MA.5` consumes these accepted pages as the source of truth for the technical design and ADRs; keep documented flags, `blastit.toml` keys and exit-code promises explicit and unambiguous so the design invents nothing.
- `docs/design/**`, `docs/decisions/**`, `docs/research/**` and `docs/knowledge/**` are excluded from the public Zensical build; the critic review files live under `docs/design/` for exactly that reason.
- Automated runnable-snippet verification of every documented command belongs to `MA.3`; this task only guarantees the prose, nav and structure are lean and buildable.
- Extend `paths` only if a required page or nav change falls outside `README.md`, `docs/**`, `zensical.toml`.
