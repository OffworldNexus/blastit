---
id: MB.45
title: "Stage the four PHP SAPI shims into the universal wheels (publish-pypi.yml)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - ".github/workflows/publish-pypi.yml"
  - ".github/actions/build-wheel/action.yml"
  - scripts/check_workflows.py
  - scripts/test_packaging_gates.py
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T06:26"
updated: "2026-11-20T06:26"
---

# Goal
`publish-pypi.yml` must stage the four PHP SAPI shims into each universal wheel, so a
`uv add blastit` install can serve PHP. Today it cannot: `wheels_gnu` and `wheels_musl`
call `./.github/actions/build-wheel` with only `manylinux`/`platform`/`artifact`, the
action defaults `require-shims: "py"` and `shims-from: ""`, and neither job has a
`download-artifact` of `shim-*` (unlike `binaries_gnu`/`binaries_musl`). So the shipped
wheel holds `blastit`, `blastit-lsp` and the two CPython shims, **zero** PHP shims —
while `ADR-0017` Decision 4 says a wheel is "repacked with `blastit`, `blastit-lsp`,
`4` PHP shims (`83`–`86`), and `2` Python shims (`313`–`314`)".

# Context
- `scripts/build-wheel.sh --require-shims all` already fails closed on a short set
  (`error: --require-shims all needs 2 CPython + 4 PHP shims, … holds 2 + 0`), so the
  gate exists and only the workflow wiring is missing.
- `binaries_gnu` (`publish-pypi.yml:251`) and `binaries_musl` (`:339`) already download
  `shim-*` and assert the archive set; the wheel jobs are the two that were missed.
- `scripts/check_workflows.py` rule 9 requires every `download-artifact` producer job to
  be a transitive `needs:` entry, so `wheels_*` must add `shims` to `needs:`.
- `MB.44`'s `CHANGELOG.md`/`RELEASING.md` wording was reviewed against this gap; the
  release notes deliberately do not enumerate `blastit/_bin/` contents.

# Acceptance
- `wheels_gnu` and `wheels_musl` download the `shim-*` artifacts for their own
  `(arch, libc)` and pass `require-shims: all` + `shims-from:` to the action.
- `make workflow-check` is green (rule 9 satisfied via `needs: [guard, matrix, shims]`).
- `make packaging-gates-test` gains/holds a case proving a wheel job without
  `shims-from` is caught, or the existing wheel-floor cases cover it.
- Built wheel's `zipinfo` lists `blastit/_bin/libblastit_php{83,84,85,86}-<arch>-<libc>.so`
  beside the two `libblastit_py3{13,14}` shims.

# Notes
Found while fact-checking `MB.44` (`review-ux`, base `d671081`). Out of `MB.44`'s paths
(`CHANGELOG.md`, `RELEASING.md`). `MB.42.13` recorded the local `--require-shims all`
failure as a worktree limitation (no `target/php-sapi`), not as a workflow gap.
