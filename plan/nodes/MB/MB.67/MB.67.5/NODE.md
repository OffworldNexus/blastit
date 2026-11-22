---
id: MB.67.5
title: "Workspace version has one source (Cargo.toml), every fixture and doc example derives"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.67.4]
paths:
  - scripts/version.sh
  - scripts/test_install.sh
  - docs/install.md
  - docs/install.sh
  - scripts/gen_language_blocks.py
  - "packaging/package-set.sh"
  - packaging/README.md
  - e2e/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T00:47"
updated: "2026-11-23T00:47"
---

# Goal

The workspace version has exactly one source — `Cargo.toml`'s `[workspace.package] version`,
the line `scripts/dev-version.sh` already reads — and every script, test, fixture and
generated doc example derives it. Today executable places still type it:
`scripts/test_install.sh` fakes `blastit-0.2.0-1.<arch>.rpm` on a 0.2.1 tree, `docs/install.md`'s
"Without `curl | sh`" filenames are typed, and `e2e/README.md` and `packaging/README.md` show
`0.2.0` examples.

# Scope

- `scripts/version.sh` (new) — the one loader: sourceable (`blastit_workspace_version`) and
  runnable (`scripts/version.sh` prints it). It reads the `[workspace.package] version` with the
  same single `sed` line `scripts/dev-version.sh` uses; no second parser anywhere. `packaging/package-set.sh`
  re-exposes it (`blastit_package_version` or a direct source) the way it re-exports the language
  rows.
- `scripts/test_install.sh` — case (i)'s fake `.rpm` names come from the loader; nothing else changes.
- `docs/install.md` — the "Without `curl | sh`" console block becomes a generated block
  (`<!-- BEGIN/END generated:version -->`) written by the existing `scripts/gen_language_blocks.py`
  (one generator for install blocks), with `--check` covering it.
- `e2e/README.md`, `packaging/README.md` — version-agnostic examples (`<version>`, with the one
  sentence that it is the `Cargo.toml` value); no freshness machinery for a developer README.

# History is exempt — do not "fix" it

A comment or string that explains a past event is a record, not configuration. Leave these
exactly as they are: `"why v0.2.0's packages were never built"` in `publish-pypi.yml`, the
`"workers was removed in 0.2.0"` refusal strings, `CHANGELOG.md`, ADRs, `plan/`, `docs/testing/*`,
and `scripts/dev-version.sh`/`check-release-tag.sh` prose that quotes `0.1.1-dev42` as an
explanation. A grep that wants them gone is wrong; only executable/configuration spellings move.

# Out of scope

`Cargo.toml` (source of truth, not edited by this slice), the language table and its slices.

# Acceptance

1. `grep -rnE "0\.[0-9]+\.[0-9]+-" scripts/ e2e/ docs/install.md packaging/README.md` returns
   only the exempt history quoted above (list it) or nothing.
2. Bump `[workspace.package] version` in `Cargo.toml` to a scratch value (e.g. `9.9.9`), then
   `scripts/version.sh`, `scripts/gen_language_blocks.py --check` (fails until regenerated),
   regenerate, and `sh scripts/test_install.sh` all follow it with no other edit; show the fake
   rpm name and the generated doc lines; revert and `git status --porcelain` is clean.
3. `scripts/version.sh` output equals `Cargo.toml`'s line and equals what
   `packaging/package-set.sh` exposes.
4. `make fast-check`, `make docs-check`, `sh scripts/test_install.sh` green.

# Proof

`RESULT.md`: the grep before/after, the scratch-bump transcript (loader, stale `--check`, fresh
`--check`, test_install passing, revert), and the three green command tails.
