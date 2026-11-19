---
id: MB.42.4
title: "Rewrite hero & docs around zero-decision install.sh (signed APT/RPM repos) + bare blastit CLI + lockfile wrappers, and pass full make check + make e2e"
kind: task
status: ready
owner: writer
priority: 1
depends_on: [MB.42.3]
paths: [README.md, "docs/**", zensical.toml, "crates/blastit-manifest/tests/**", "python/tests/**", "e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T14:30"
updated: "2026-11-19T14:30"
---

# Goal
Rewrite the hero snippet and install/usage commands across `README.md` and `docs/**` to take full advantage of the universal `blastit` binary, the signed-repository `install.sh`, the apt/rpm packages, and local CLI handoff — eliminating the decision-dense `# Python; PHP and static use the archive` / `uv run blastit` clutter.

# Owner clarification (authoritative, 2026-11-19)
`install.sh` installs from **signed APT/RPM repositories** (Cloudsmith primary, Forgejo mirror), not a bare package download. Docs must describe that accurately, give the mirror escape hatch, and attribute Cloudsmith OSS hosting.

# Exact UX Requirements
1. **Zero-Decision Hero on `docs/index.md` and `README.md`:**
   - First snippet is **3 universal lines** — identical for Python, PHP and Static, zero `# Python vs archive` comments, zero `uv run` prefix:
     ```console
     $ curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh
     $ blastit dev
     $ blastit check
     ```
   - One short sentence under it: the installer adds the signed Blastit APT (Debian/Ubuntu) or RPM (Fedora/RHEL/Rocky/Alma) repository and installs `blastit`; it falls back to `~/.local/bin` when it cannot use a system package manager.
2. **Bare `blastit ...` everywhere** in `docs/getting-started.md`, `docs/blastfile.md`, `docs/local.md`, `docs/ci.md`, `docs/deploy/*`, `docs/frameworks/**`, `docs/reference/*` — no `uv run` prefixes on `blastit dev` / `blastit check` / `blastit fmt`.
3. **`docs/getting-started.md` Step 1 (`Install`):**
   - Default path = 1-line `install.sh` (states it configures the signed apt/rpm repo, or `~/.local/bin` fallback).
   - Compact admonition/tab for the non-default paths, each one line, no decision paralysis on the main path:
     * `--nightly` (nightly channel), `--mirror forgejo` (`https://forge.offworld.nexus`), `--user` (unprivileged `~/.local`), `--develop` (branch build), Alpine/musl (`.tar.gz`).
   - Second compact block: **"Or pin it in your project lockfile"** — `uv add --dev blastit` (Python) · `composer require --dev offworldnexus/blastit` (PHP), noting the global `blastit` automatically hands off to `.venv/bin/blastit` / `vendor/bin/blastit`.
4. **New/updated install reference page (`docs/install.md` or the Getting Started install section):**
   - "Managed by your package manager" section: `apt-get install blastit`, `dnf install blastit`, `apt-get upgrade blastit`; how to add the repo manually on a machine without `curl | sh`.
   - "Repositories" section naming the Cloudsmith repos (`offworld-nexus/apt`, `offworld-nexus/rpm`, components `main` / `nightly`) and the Forgejo mirror `https://forge.offworld.nexus`.
   - **Cloudsmith OSS attribution**: the hosted-by badge/link plus one factual sentence (e.g. *"Package repositories are hosted by [Cloudsmith](https://cloudsmith.io) as part of their free open-source plan."*). No marketing prose.
5. **PHP 8.3, 8.4, 8.5 and 8.6 stated consistently** (`MB.42.5` owns the code; you own the prose):
   - Every page that names supported PHP minors says `PHP 8.3, 8.4, 8.5 and 8.6` (or `8.3–8.6`) — `docs/frameworks/php.md`, `docs/getting-started.md`, `docs/deploy/*`, `docs/reference/*`, `README.md`.
   - The NTS note stays factual: `8.3, 8.4, 8.5, 8.6` embedded (`libphpX.Y-embed` / `phpXY-embed`), never `php-fpm`.
   - `uv run --no-project scripts/platform_matrix.py --check` must stay green (it refuses a page claiming more shims than the 16-leg matrix declares, or more than are proven).
6. **`docs/deploy/*` simplification:** Dockerfiles become one universal install (`curl -fsSL .../install.sh | sh` as root, apt/dnf path) or `--user`; a single runtime image now serves Python, PHP and Static — delete the "Python image vs PHP image vs archive" branching.
7. **Sync harnesses & visual QA:**
   - Update `crates/blastit-manifest/tests/docs_fences.rs` (fence classification: `curl ... install.sh | sh` is `mode=static`/non-runnable; `blastit dev` / `blastit check` fences must be runnable against the built binary), `python/tests/runnable_docs/` (`inventory.toml`, `harness.py`), and `e2e/` where install paths changed.
   - Mermaid `flowchart`/`sequenceDiagram` on every changed guide page; pages stay `<130` lines; `make docs-check` strict clean.
   - Run `visual-checker` on the new hero (`docs/index.md`) and `docs/getting-started.md` in light + dark + mobile.

# Acceptance
- `make check`
- `make e2e`
- `visual-checker` PASS on the new hero and Getting Started pages
