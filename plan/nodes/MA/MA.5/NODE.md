---
id: MA.5
title: "Write 0.1.1 technical design doc & ADRs from the accepted DX docs"
kind: task
status: done
owner: architect
priority: 1
depends_on: [MA.1]
paths: ["docs/design/**", "docs/decisions/**"]
attempts: 1
max_attempts: 3
escalation: strategist
created: "2026-11-12T01:41"
updated: "2026-11-12T13:10"
---

# Goal

Translate the **accepted** lean DX documentation from `MA.1` into a precise technical design doc plus ADRs, so `MA.2` can implement every documented command, flag, manifest key and security invariant without guessing.

# Context

- Depends on `MA.1` (accepted `README.md` / `docs/**` + resolved `docs/design/ma-lean-dx.review-*.md` findings). The DX docs are the source of truth: the design must not invent new UX, only specify how the documented UX is built.
- Load skills `design-docs`, `dx-first`, `security-invariants`, `rust-standards`, `python-standards`.
- Read `docs/VISION.md`, `ARCHITECTURE.md` (§§2, 4, 5, 6, 8), `plan/nodes/MA/MA.2/NODE.md`, and the accepted DX pages.
- The design must specify, concretely:
  1. **Exact CLI surface**: subcommands and flags for `blastit dev`, `blastit check`, `blastit test`, `--learn`, `--json`, and the `--preset digitalocean-paas` / `--preset compose-traefik` deployment presets — including exit codes and stdout/stderr contracts.
  2. **`blastit.toml` keys**: auto-discovery of `./blastit.toml` when `BLASTIT_MANIFEST` is unset, non-secret `app` / `preload` entries, and the full `BLASTIT_*` env-override precedence order.
  3. **Wire-level `--learn` mechanics** at `scheduler` (HTTP ingress/routing) and `dbproxy` (SQL): log-and-allow recording of routes, normalized statements and minimal grants in one pass, plus the record format written back to `blastit.toml`.
  4. **4-way completeness verification**: (a) every observed request matched an intended route, (b) every observed outgoing operation is granted, (c) every declared route/grant was exercised, (d) every body-accepting route declares ingress validation (or explicit loud opt-out) — with human and `--json` diagnostics.
  5. **Security invariants**: unprivileged `blastit dev` single-UID simulation (real process topology, stage-0 `execve` secret stripping, per-route private socket dirs) versus production `blastit` failing closed without real UIDs/capabilities + seccomp, and which crate enforces which check.
  6. **Ordered leaf task breakdown for `MA.2`**: every leaf named with its paths, acceptance command and test tier, respecting a dedicated `Cargo.toml`/`Cargo.lock` dependency task so parallel Rust leaves never collide.

# Acceptance

- [ ] `docs/design/ma-lean-dx.md` exists and covers all six specification areas above with concrete Rust traits/crate boundaries and named requirement ids (`R1..Rn`).
- [ ] ADRs exist under `docs/decisions/ADR-000N-*.md` for each irreversible decision (unprivileged `dev` mode, `--learn` log-and-allow, manifest auto-discovery/precedence, preset model).
- [ ] `make docs-check` (`uvx --offline zensical build --strict`) passes with zero warnings.
- [ ] Fresh-context goldfish `critic` reviews written to `docs/design/ma-lean-dx.review-implementability.md`, `.review-testability.md`, `.review-consistency.md`, and every `BLOCKER`/`MAJOR` finding is resolved in a `## Review Resolution` section.
- [ ] Every `MA.2` leaf task is named in the design (id-free names are fine) with paths, acceptance command and critical-test location.
- [ ] No production code or user docs are modified by this task (design + ADRs only).

# Notes

- Do not re-open DX decisions already accepted in `MA.1`; if the design cannot satisfy the docs, log the conflict and escalate to `architect` via `plan.py escalate` rather than silently changing the docs.
- `docs/design/**` and `docs/decisions/**` are excluded from the public Zensical build.
