---
id: MB.42
title: "Universal libpython-free blastit binary, PHP 8.3-8.6, signed Cloudsmith/Forgejo APT+RPM repos & install.sh, py3-none wheels, Composer wrapper, local CLI handoff & zero-decision hero DX"
kind: epic
status: active
priority: 1
depends_on: []
paths: ["**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T14:30"
updated: "2026-11-20T06:01"
---

# Goal
Eliminate Blastit's split-brain distribution so **one universal `blastit` binary** (`libc`-only, loading Python and PHP via per-minor `dlopen` shims) runs every runtime, delivered via:
1. **Signed APT and RPM repositories** — Cloudsmith (`offworld-nexus/apt`, `offworld-nexus/rpm`) as the primary, Forgejo (`https://forge.offworld.nexus`) as a mirror, with `main` (tagged `v*` releases) and `nightly` (scheduled `develop` builds, newest 7 retained) channels; `.deb`/`.rpm`/`.tar.gz`/`SHA256SUMS` also attach to the GitHub Release,
2. **`docs/install.sh`** (`https://offworldnexus.github.io/blastit/install.sh`) which sets up the **signed APT repository** on Debian/Ubuntu (`apt-get install -y blastit`) or the **signed RPM repository** on Fedora/RHEL/Rocky/Alma (`dnf install -y blastit`), with `--nightly` / `--mirror forgejo`, and which falls back to a verified `.tar.gz` install in `~/.local` only for `--user`, `--develop`, unprivileged non-sudo, or Alpine/other Linux,
3. **Thin ecosystem lockfile wrappers:** Python-version-agnostic `py3-none-*` PyPI wheels (`uv add --dev blastit` / `uv tool install blastit`) and the `packages/composer-blastit` Composer wrapper (`composer require --dev offworldnexus/blastit`),
4. **Safe local CLI handoff:** global `blastit` (`dev`/`check`/`test`/`fmt`/`lsp`) delegates seamlessly to a project-local `.venv/bin/blastit`, `vendor/bin/blastit`, or `node_modules/.bin/blastit` when present,
5. **Zero-decision hero documentation:** 3-line hero snippet (`curl ... install.sh | sh` -> `blastit dev` -> `blastit check`) with zero `# Python vs archive` branching.
6. **PHP 8.6 support end-to-end**, so `0.2.0` states `PHP 8.3, 8.4, 8.5 and 8.6` everywhere (`MB.42.5`): the `libblastit_php86-<arch>-<libc>.so` shim, `Minor::Php86` in `SUPPORTED_MINORS`, 8.6-first discovery probes, 16 PHP shim legs (4 minors x 4 targets), and `libblastit_php86` inside every `.deb`/`.rpm`/`.tar.gz`/`py3-none-*` wheel.

# Acceptance
- `make check`
- Container smoke proofs for the locally built `.deb` (clean Debian/Ubuntu), `.rpm` (clean Fedora/Rocky) via `BLASTIT_DEB` / `BLASTIT_RPM`, `install.sh` (repo + `--user` + `--develop`), universal `py3-none` PyPI wheel (CPython 3.13 & 3.14), and Composer wrapper
- Cloudsmith/Forgejo publish steps validated in dry-run only at leaf level; the real publish is the `v*` tag workflow
