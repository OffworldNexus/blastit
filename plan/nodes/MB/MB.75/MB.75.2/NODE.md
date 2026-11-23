---
id: MB.75.2
title: "MB.75.2 Channel-aware matrices, RPM spec, builders, bundle layout and workflows"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.75.1]
paths:
  - scripts/platform_matrix.py
  - scripts/rpm/gen_spec.py
  - scripts/rpm/blastit.spec
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - "scripts/build-package-bundle.sh"
  - "scripts/check-bundle-layout.sh"
  - .github/workflows/packages.yml
  - .github/workflows/nightly.yml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T11:41"
updated: "2026-11-23T11:41"
---

# Goal

Every matrix leg, RPM spec stanza, package builder and workflow is channel-aware, with `stable` the committed/default shape and `nightly` deriving 3.16. No consumer restates a version list; each one calls `languages.py` / `package-set.sh` with a channel.

# Scope

- `scripts/platform_matrix.py`: add `--channel stable|nightly` to every matrix/leg/count that names a language shim or package (default `stable`). `py-shims --channel stable` is 3.13/3.14/3.15; `--channel nightly` adds 3.16. `--check` validates both channels, asserts stable has no 3.16 and nightly has it, and still runs `gen_spec.py --check` and the package-set/shim diffs per channel.
- `scripts/rpm/gen_spec.py`: `--channel` (default stable) on `--stdout`/`--write`/`--check`. The committed `scripts/rpm/blastit.spec` is the **stable** spec (core + py 3.13/3.14/3.15 + PHP 8.3–8.6). The nightly workflow asks for the nightly rendering; do not commit a second spec.
- `scripts/build-deb.sh`, `scripts/build-rpm.sh`: accept `--channel` (default stable) and pass it to `package-set.sh` for names, globs and runtimes; `--print-set` honours it. Stable is 8 names / 32 artifacts; nightly adds `blastit-python-3.16`.
- `scripts/build-package-bundle.sh`, `scripts/check-bundle-layout.sh`: channel-aware (or an explicit all-rows mode) so a dev/e2e bundle may carry the 3.16 shim, while a stable release bundle's required set is the stable names only; the layout check must not call a nightly shim "unowned".
- `.github/workflows/packages.yml`: every matrix/spec/build invocation passes `--channel stable` explicitly. `.github/workflows/nightly.yml`: passes `--channel nightly`. Remove any literal shim/package version list; a remaining literal is only a toolchain pin (e.g. the CLI `PYTHON_VERSION`), never a package row.
- Keep `make fast-check` green (it runs `workflow-syntax`, so the new shell words must parse).

# Acceptance

- `uv run --no-project scripts/platform_matrix.py --check`
- `uv run --no-project scripts/platform_matrix.py --matrix py-shims --channel stable` lists 3 minors, no 3.16; `--channel nightly` lists 4 and includes 3.16.
- `uv run --no-project scripts/rpm/gen_spec.py --channel stable --check` is green and the committed spec contains no `blastit-python-3.16`; `gen_spec.py --channel nightly --stdout | grep -q blastit-python-3.16`.
- `bash scripts/build-deb.sh --print-set --channel stable | wc -l` is 8; `--channel nightly` is 9. Same for `build-rpm.sh`.
- `grep -rn '3\.1[3-6]' .github/workflows/` shows no package/shim version list (toolchain pins only), and both workflows pass a channel.
- `make fast-check`.

# Notes

Slice 2 of 4; depends on the channel API from MB.75.1. One sequential lead. Regenerate committed artifacts with the generator, never hand-edit them. No commit.
