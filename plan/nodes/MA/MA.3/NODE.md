---
id: MA.3
title: "Verify runnable docs, visual QA, e2e & prepare 0.1.1 release"
kind: epic
status: done
priority: 3
depends_on: [MA.2]
paths:
  - README.md
  - CHANGELOG.md
  - ARCHITECTURE.md
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - zensical.toml
  - "docs/**"
  - "python/tests/**"
  - "e2e/**"
  - "crates/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-13T03:12"
---

# Goal

Prove that every single command, `blastit.toml` snippet, local workflow, CI workflow, and deployment recipe in the new `0.1.1` documentation works verbatim; verify visual quality of the rendered Zensical site via `visual-checker`; run Docker & rootless e2e suites; and prepare the `0.1.1` release (`Cargo.toml` + `CHANGELOG.md` + `ARCHITECTURE.md` sync).

# Context

- Depends on `MA.2`.
- Load skills `dx-first`, `testing`, `rust-standards`, `python-standards`.
- Reference `RELEASING.md` for version & changelog rules (`Cargo.toml` `[workspace.package] version = "0.1.1"` and `CHANGELOG.md` `## 0.1.1 (...)`).

# Acceptance

- [ ] Automated runnable-docs test suite executes the Getting Started, Local `blastit dev` / `--learn`, and CI check snippets from `README.md` and `docs/` against the built package and passes unprivileged.
- [ ] Any drift discovered between `docs/`, `ARCHITECTURE.md`, and the implemented CLI/manifest is reconciled so docs and code agree 100%.
- [ ] `visual-checker` inspects the built Zensical site (`site/`) across desktop light/dark, mobile (`390x844`), and Mermaid diagrams, returning `VERDICT: PASS`.
- [ ] `make check` and `make e2e` (Docker root + Docker rootless capabilities) pass green, and wheel metadata check (`scripts/check-long-description.py`) passes on built artifacts.
- [ ] `Cargo.toml` workspace version is bumped to `0.1.1` (with `Cargo.lock` updated cleanly) and `CHANGELOG.md` documents `0.1.1` crisply.

# Notes

- Never run `git tag` or `git push`; `strategist` will verify the `MA` milestone gate and post the `v0.1.1` tag/push instructions to `plan/HUMAN.md`.
