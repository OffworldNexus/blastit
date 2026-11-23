---
id: MB.75.3
title: "MB.75.3 Docs/installer/e2e: stable 3.13-3.15 blocks, nightly 3.16 prose, derived counts"
kind: task
status: ready
owner: writer
priority: 3
depends_on: [MB.75.1, MB.75.2]
paths:
  - scripts/gen_language_blocks.py
  - docs/install.md
  - docs/install.sh
  - docs/index.md
  - RELEASING.md
  - e2e/run_packaging.py
  - scripts/test_install.sh
  - e2e/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T11:41"
updated: "2026-11-23T11:41"
---

# Goal

The generated user documentation, the installer mapping and the packaging e2e counts are derived per channel: stable docs show 3.13/3.14/3.15 and never 3.16; nightly prose names 3.16 and how to install it (`--nightly`). No literal count or version list in the e2e harness.

# Scope

- `scripts/gen_language_blocks.py`: channel-aware generation for the Markdown blocks (`apt`, `dnf`, `table`, `sentence`) and the `docs/install.sh` mapping. Stable blocks filter to stable rows only; add/keep one nightly block or prose path that names 3.16 as nightly-only. `--check` fails on a stale committed block.
- Regenerate `docs/install.md` (stable apt/dnf case lists, sentence, package table: 3.13/3.14/3.15 + PHP 8.3–8.6, no 3.16), plus the nightly paragraph explaining `Components: nightly` / nightly RPM base URL and `apt-get install blastit-python-3.16`.
- `docs/install.sh`: the generated `detect_shims` mapping lists stable CPython minors only (3.13/3.14/3.15; PHP unchanged); `--nightly` still selects the nightly repository, and the "no shim for this interpreter" note points at the nightly docs for 3.16.
- `docs/index.md` and `RELEASING.md`: update any version sentence to the stable set and document that 3.16 is nightly-only (built from `cpython-3.16.0a2`).
- `e2e/run_packaging.py`: every expected shim/package name and probe list comes from `languages`/`package-set.sh` with the channel; no literal `3.13`/`3.14` pair, no literal count. Stable derivation is 8 names / 32 artifacts; the `--print-legs` output reflects the new rows.
- `scripts/test_install.sh` / `e2e/README.md` prose where the stable minor list or count appears.

# Acceptance

- `uv run --no-project scripts/gen_language_blocks.py --check`
- `make docs-check`
- `sh -n docs/install.sh` (and `dash -n` when available); `sh scripts/test_install.sh`
- `grep -n 3.15 docs/install.md` shows it in the stable blocks; the generated stable blocks and the installer mapping contain no `3.16`; `docs/install.md` nightly prose names `blastit-python-3.16` and `--nightly`.
- `uv run --no-project e2e/run_packaging.py --print-legs` derives 3.15 and 3.16 probes and 8 stable package rows, with no literal count in the source.

# Notes

Slice 3 of 4; depends on MB.75.1 (channel API) and MB.75.2 (matrix/channel plumbing). Stable docs never advertise a nightly-only package. No commit.
