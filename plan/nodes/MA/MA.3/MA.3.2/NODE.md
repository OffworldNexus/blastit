---
id: MA.3.2
title: "Reconcile docs, README and ARCHITECTURE drift with the shipped 0.1.1 CLI"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MA.3.1]
paths:
  - README.md
  - ARCHITECTURE.md
  - docs/index.md
  - "docs/getting-started.md"
  - docs/local.md
  - docs/ci.md
  - "docs/deploy/**"
  - "docs/frameworks/**"
  - "docs/reference/**"
  - "docs/how-it-works.md"
  - docs/security.md
  - docs/troubleshooting.md
  - docs/changelog.md
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-13T01:14"
---

# Goal

Reconcile every drift between the prose of `README.md`, `docs/` and `ARCHITECTURE.md` and the shipped `0.1.1` CLI, manifest, presets and exit codes, so docs and code agree 100% and the runnable-docs harness is green.

# Context

- Docs were written first (`MA.1`) and the code landed after (`MA.2`); `MA.3.1` now executes them and reports where they no longer match.
- Truth sources: `blastit --help`, `blastit dev --help`, `blastit check --help`, `blastit check --json`, `crates/blastit/src/cli.rs`, `crates/blastit/src/check.rs`, `crates/blastit-manifest/src/{model,parse,validate}.rs`, `docs/design/ma-lean-dx.md` §1.
- `ARCHITECTURE.md` must describe `ExecutionMode::Dev`, `--learn`, `blastit check`, and the 0.1.1 manifest fields (`app`, `preload`, `[route.ingress]`).
- Load skills `dx-first`, `design-docs`. Fix, don't expand: every page stays lean.

# Acceptance

```console
$ make docs-test                 # harness green, no finding left open
$ make docs-check                # zensical build --strict
$ grep -RIn '0\.1\.0' README.md docs/ | grep -v changelog
$ bash -c 'blastit --help; blastit check --help' >/tmp/help.txt && grep -c -- '--learn' /tmp/help.txt
```

- Every flag, exit code (`0/1/2/3`), finding name and `--json` key documented in `docs/reference/cli.md`, `docs/ci.md`, `docs/local.md` exists in the binary; paste the flag-by-flag check in `RESULT.md`.
- `ARCHITECTURE.md` section list shows `dev`, `--learn`, `check`, and the 0.1.1 manifest keys.
- `grep` for `blastit.toml` example drift: every documented example parses with `blastit check --dry-run` or the manifest test, and none names a reserved key.
- Any remaining `0.1.0` mention is release history only.

# Notes

- Docs prose only. Never touch `python/tests/runnable_docs/**` (harness inventory), `docs/testing/**`, `zensical.toml` or `docs/stylesheets/**`.
- If a documented behaviour is genuinely missing from the code, do not write around it: open a follow-up node and report it in `RESULT.md`.
