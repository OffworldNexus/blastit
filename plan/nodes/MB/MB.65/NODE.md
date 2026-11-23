---
id: MB.65
title: Offworld Nexus packaging pipeline + one Debian package per interpreter version
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "packaging/**"
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - "scripts/build-package-bundle.sh"
  - "scripts/check-bundle-layout.sh"
  - scripts/publish_packages.py
  - scripts/test_install.sh
  - scripts/platform_matrix.py
  - scripts/install_matrix.sh
  - Makefile
  - .github/workflows/packages.yml
  - .github/workflows/nightly.yml
  - ".github/workflows/publish-pypi.yml"
  - .github/workflows/docs.yml
  - docs/install.md
  - docs/install.sh
  - docs/index.md
  - "docs/getting-started.md"
  - README.md
  - RELEASING.md
  - CHANGELOG.md
  - Cargo.toml
  - Cargo.lock
  - scripts/rpm/blastit.spec
  - e2e/run_packaging.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-22T23:00"
updated: "2026-11-22T23:52"
---

# Goal

Migrate Blastit's native packaging to the organization's **zero-secret, event-driven Offworld Nexus pipeline**, and split the Debian packages **one per interpreter version, per Debian conventions**, instead of one `.deb` bundling every shim.

Read the skill at `/home/remy/offworld-nexus/.opencode/skills/offworld-packaging/SKILL.md` and use its `scripts/stage_dist.py` **verbatim** (do not re-implement it). Note: the skill's referenced `../tpmd/.github/workflows/packaging-deb.yml` is **not yet migrated** (it still uploads to Cloudsmith) — the skill text is authoritative, tpmd will follow us.

## A. Package split (Debian conventions; mirror sensibly for RPM)

Every shim already lands in `/usr/share/blastit/lib/`, so this needs **no Rust/Python code change** — only the builders.

Per architecture (`amd64`/`arm64` for deb, `x86_64`/`aarch64` for rpm), build **7 binary packages** instead of one:

| Package | Payload | Depends (deb) |
|---|---|---|
| `blastit` | `blastit`, `blastit-lsp`, `LICENSE`, the 7 pure-Python modules, `/usr/bin` symlinks | measured `libc6 (>= X)` (as today); **never** a shim |
| `blastit-python-3.13` | `libblastit_py313-<arch>-gnu.so` | `blastit (= <version>)`, `python3.13` |
| `blastit-python-3.14` | `libblastit_py314-<arch>-gnu.so` | `blastit (= <version>)`, `python3.14` |
| `blastit-php-8.3` | `libblastit_php83-<arch>-gnu.so` | `blastit (= <version>)`, `libphp8.3-embed` |
| `blastit-php-8.4` | `libblastit_php84-<arch>-gnu.so` | `blastit (= <version>)`, `libphp8.4-embed` |
| `blastit-php-8.5` | `libblastit_php85-<arch>-gnu.so` | `blastit (= <version>)`, `libphp8.5-embed` |
| `blastit-php-8.6` | `libblastit_php86-<arch>-gnu.so` | `blastit (= <version>)`, `libphp8.6-embed` (Ondřej Surý's repo; say so in docs) |

Owner's deliberate naming: `blastit-<lang>-<version>` (not `<interpreter>-blastit`), so
`apt-get install blastit-php-8.4` reads as "Blastit's PHP 8.4 shim"; every shim pins the
exact core build it was compiled against, and the core never depends on a shim.

- RPM mirrors the same split; use the distro's real embed package name for `Requires:` where `scripts/platform_matrix.py` already knows it, and `Recommends:`/`Suggests:` only where no stable name exists (document the choice; do not write an unsatisfiable hard `Requires:`).
- Keep the measured glibc floor logic (`scripts/check-portable-elf.sh`) applied to the ELF in each package, and keep a single source of truth for names/paths.
- Adapt `scripts/check-bundle-layout.sh` (or its replacement) so "the full set installs the same tree" is still enforced — a split package set that loses a shim must still fail.
- `scripts/build-deb.sh` / `scripts/build-rpm.sh` gain the split; keep their CLIs honest and documented in `--help`.

## B. Pipeline (skill rules, non-negotiable)

1. Copy the skill's `scripts/stage_dist.py` to `packaging/stage-dist.py` **verbatim**; `chmod +x`. Never hand-roll `manifest.json`.
2. `.github/workflows/packages.yml`: keep the build jobs and the existing install-verify job(s) (`verify`), **delete** the Cloudsmith/Forgejo `publish` job and any retention/pruning, and add a `stage` job (`needs: [..., verify]`) that runs `packaging/stage-dist.py` with one `--file` per artifact family (`deb:amd64:dist/*_amd64.deb`, `deb:arm64:dist/*_arm64.deb`, `rpm:x86_64:dist/*.x86_64.rpm`, `rpm:aarch64:dist/*.aarch64.rpm`) and uploads exactly one artifact named `offworld-dist` (`actions/upload-artifact@v4`, `if-no-files-found: error`).
3. Triggers: `push: tags: v*` (annotated) and `workflow_dispatch` with a **`rehearsal` boolean** that runs the full build+verify matrix and `stage-dist.py --dry-run` while skipping the `offworld-dist` upload. `permissions: contents: read` only. No `pull_request` runs.
4. `.github/workflows/nightly.yml`: same graph, staggered cron minute in the `03:xx UTC` band (e.g. `41 3 * * *`), nightly stamp grammar exactly per the skill (`0.2.0~nightly<YYYYMMDD>.r<run>+g<sha7>` for deb, `0.<date>...` release stamp for rpm), no local retention (server-side keeps the newest 7).
5. Delete `scripts/publish_packages.py` and every `CLOUDSMITH_*`/`FORGEJO_*` reference in workflows, docs, README and tests — grep to prove zero remain (except this node's own history).

## C. Docs (DX first — write the docs before the builders)

- `docs/install.md` + `docs/index.md` hero: the canonical `apt.offworld-nexus.app` / `rpm.offworld-nexus.app` snippets from the skill (§5), adapted to the split (`sudo apt-get install blastit php8.4-blastit` etc.), with a one-line-per-package table. **Only `*.offworld-nexus.app`** — never any internal Chulak hostname.
- `docs/install.sh`: rewrite so its default path configures the canonical signed repos and installs `blastit` plus the shim packages the host actually needs (detect `php`/`python3` versions; `--user`/Alpine fall back to archives as today). Remove the Cloudsmith/Forgejo setup functions.
- `RELEASING.md`: describe the real flow (tag → packaging workflow → `offworld-dist` → the App signs and publishes, posting an `offworld-pkg / publish` check), and drop the stale `Live (DigitalOcean)` claims.

## D. Local proof (all in `RESULT.md`)

- Build every package for **amd64** and run `packaging/stage-dist.py --dry-run --source blastit --channel stable --version 0.2.0 --ref refs/tags/v0.2.0` against the real artifacts — it must accept the whole set (this proves names, arch tags, `Depends`, and the manifest).
- Install the packages in their containers (`debian:12`, `debian:13`, `ubuntu:22.04`, `ubuntu:24.04`, `rockylinux:9`, `fedora:latest`) and smoke-test `blastit --version` + a served view, exactly as `e2e/run_packaging.py --leg install` does.
- A negative proof: a package set missing one shim package must fail the layout check (not pass silently).
- `make fast-check` (or the honest subset: ruff on changed Python, `scripts/test_install.sh`, `make docs-check`), `guard.py`, `plan.py validate`.

## E. Version, and out of scope

Owner decision: this release is **0.2.1**, not a re-point of the published `v0.2.0`. The bump (`Cargo.toml`, the intra-workspace requirement lines, `Cargo.lock`, a dated `## 0.2.1` `CHANGELOG.md` entry) is part of this node, and so is the `publish-pypi.yml` dispatch that makes `Packages` actually run on the new tag. Still out of scope: creating, moving or deleting tags (conductor).

# Acceptance

- `scripts/dev-version.sh --check` and `scripts/release-notes.sh --require-dated v0.2.1` pass.
- Seven `.deb` and seven `.rpm` packages per architecture, with the names in §A and the dependency direction the owner fixed (core never depends on a shim).
- `packaging/stage-dist.py --dry-run --source blastit --channel stable --version 0.2.1 --ref refs/tags/v0.2.1` accepts the real amd64 set; `scripts/check-bundle-layout.sh --deb-set`/`--rpm-set` pass, and a set missing one shim package fails by name.
- The set installs and smokes in `debian:12`, `debian:13`, `ubuntu:22.04`, `ubuntu:24.04`, `rockylinux:9` and `fedora:latest` (`scripts/install_matrix.sh --dir dist`).
- Zero `CLOUDSMITH_`/`FORGEJO_`/`cloudsmith.io`/`forge.offworld.nexus`/`chulak` reference outside `plan/` history, `docs/design/` and `docs/decisions/`.
- `make docs-check`, `make guard`, `uv run --no-project tools/plan.py validate` pass.
