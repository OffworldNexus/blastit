---
id: MB.44
title: "Sync CHANGELOG.md (0.2.0) and RELEASING.md for universal binary, PHP 8.6, Cloudsmith/Forgejo repos, py3-none wheels & Composer wrapper"
kind: task
status: done
owner: writer
priority: 1
depends_on: []
paths: [CHANGELOG.md, RELEASING.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T06:23"
updated: "2026-11-20T06:27"
---

# Goal
Update `CHANGELOG.md` (`## 0.2.0 (2026-11-20)`) and `RELEASING.md` so the release notes and maintainer release checklist accurately reflect everything shipped in `0.2.0` (`MB.40`–`MB.42`):
1. **`CHANGELOG.md` (`## 0.2.0 (2026-11-20)`):**
   - Update PHP support everywhere in the `0.2.0` entry from `8.3, 8.4 and 8.5` to `8.3, 8.4, 8.5 and 8.6` (16 per-minor C SAPI shims across `x86_64`/`aarch64` × `gnu`/`musl`).
   - Replace the outdated pre-`ADR-0017` "Cargo-feature-gated view crates" bullet with the **Universal `blastit` binary & distribution (`ADR-0017`)** bullet:
     * One universal `blastit` binary (`libc`/`libm`/`libgcc_s` only — zero `libpython`, zero `libphp`, zero `RUNPATH`) serving Python WSGI (CPython 3.13 & 3.14 via `(st_dev, st_ino)`-pinned `dlopen` shims), PHP 8.3–8.6, static sites/SPAs, `dbproxy`, and `lsp`.
     * Signed **Cloudsmith APT & RPM repositories** (`main` & `nightly`, mirrored to Forgejo) + **`install.sh`** (`https://offworldnexus.github.io/blastit/install.sh`), **4 universal `py3-none-{manylinux_2_28,musllinux_1_2}` PyPI wheels** (running on Python 3.12+ via the pure-stdlib `--internal-json` bridge), **Composer binary wrapper** (`offworldnexus/blastit`), and **zero-TOCTOU local CLI handoff** (`.venv/bin/blastit`, `vendor/bin/blastit`, `node_modules/.bin/blastit`).
2. **`RELEASING.md`:**
   - Update wheel count/tags (`4` `py3-none-{manylinux_2_28,musllinux_1_2}_{x86_64,aarch64}` wheels), PHP SAPI shims (`16` across PHP `8.3`–`8.6`), Python SAPI shims (`8` across CPython `3.13` & `3.14`, `.github/workflows/py-sapi.yml`), and `.deb`/`.rpm` Cloudsmith + Forgejo publishing (`.github/workflows/packages.yml` on `v*` tag push and `.github/workflows/nightly.yml` for nightlies).

# Acceptance
- `scripts/release-notes.sh --require-dated "v0.2.0" > /dev/null`
- `uv run --no-project scripts/platform_matrix.py --check`
- `make docs-check && make docs-unit && make workflow-check`
