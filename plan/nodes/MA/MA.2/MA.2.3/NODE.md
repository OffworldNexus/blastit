---
id: MA.2.3
title: "CLI, unprivileged dev runtime and framework-agnostic check"
kind: epic
status: ready
priority: 2
depends_on: [MA.2.1, MA.2.2]
paths: [Makefile, "crates/blastit/**", "crates/blastit-events/**", "crates/blastit-py/**", "python/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Container for the three developer-facing commands: Python hygiene and `Makefile` gates, `blastit`/`blastit dev`/`blastit check` argument parsing with CWD manifest discovery and presets, the unprivileged `dev` supervisor with `--learn`, and the framework-agnostic `check` audit engine plus its pytest orchestrator.

# Context

- Technical design: `docs/design/ma-lean-dx.md` §1, §4.2 C/E, §7.1, §8 leaves 8–12; `ADR-0001`, `ADR-0002`, `ADR-0004`, `ADR-0005`; requirements `R1..R6`, `R9..R10`, `R17..R23`, `R27..R42`.
- Children are mostly sequential: `MA.2.3.1` (Python/`Makefile` hygiene) can start as soon as sqlpolicy lands; `MA.2.3.2` needs manifest + http/scheduler + dbproxy; `MA.2.3.3` needs the CLI; `MA.2.3.4` needs the supervisor; `MA.2.3.5` closes over `MA.2.3.1` + `MA.2.3.4`.
- DX status: `README.md`, `docs/local.md`, `docs/ci.md`, `docs/deploy/**`, `docs/reference/**` are accepted; this epic makes those commands run verbatim and must not edit them. Truthfulness fixes are `MA.3`.

# Acceptance

- [ ] `blastit dev` boots unprivileged on this host with the exact 3-line banner and `"privilege": "unprivileged-dev"`; production `blastit` still exits `1` without privileges (protected test).
- [ ] `blastit check` on a raw-WSGI + Flask-like mini-runner exits `0`, prints exactly one JSON document under `--json`, and maps pytest `1`/`5` to exit `3` with empty stdout.
- [ ] `--learn` writes only inferred routes and exact `queries`, atomically, and refuses literal-bearing SQL.
- [ ] `uvx --offline ruff check python tools` and `uvx --offline ruff format --check python tools` pass and are wired into `make py-lint`.
- [ ] `make check` passes, including `crates/blastit/tests/critical_{config_and_presets,dev_and_fail_closed,check_engine}.rs`, `crates/blastit-py/tests/critical_check_schema.rs`, and `python/tests/critical/`.

# Notes

- No leaf runs `make e2e` or builds a wheel: Docker/rootless e2e belongs to `MA.3`. Up to three children may run in parallel only where their `paths` are disjoint.
