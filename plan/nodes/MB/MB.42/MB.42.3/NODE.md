---
id: MB.42.3
title: "Build & container-verify signed APT/RPM repository publishing (Cloudsmith + Forgejo, main/nightly), docs/install.sh, universal py3-none wheels, Composer wrapper & CI/release workflows"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.2, MB.42.5]
paths:
  - Makefile
  - pyproject.toml
  - "scripts/**"
  - "packages/**"
  - docs/install.sh
  - ".github/workflows/**"
  - "python/**"
  - "crates/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T14:30"
updated: "2026-11-19T15:48"
---

# Goal
Build and container-verify the **signed APT & RPM repository** distribution of Blastit (Cloudsmith primary + Forgejo mirror, `main`/`nightly` channels), `docs/install.sh` (signed-repo install with `~/.local` fallback), universal `py3-none-*` PyPI wheels, the `packages/composer-blastit/` Composer wrapper, and the updated CI / Release / Nightly workflows.

# Owner clarification (authoritative, 2026-11-19)
Blastit ships through **signed package repositories**, NOT as a bare `.deb`/`.rpm` download link. `install.sh` sets up the repository and lets `apt`/`dnf` install and upgrade `blastit`. Direct archive downloads are the *exception* paths only.

## A. Repository publishing (Cloudsmith primary)
- `CLOUDSMITH_OWNER=offworld-nexus`; authenticated with `secrets.CLOUDSMITH_API_KEY`.
- `.deb` -> `cloudsmith push deb offworld-nexus/${CLOUDSMITH_APT_REPO:-apt}/any-distro/any-version <blastit_<ver>_<arch>.deb> --component ${COMPONENT} --republish`
  (`COMPONENT` = `main` for a tagged release, `nightly` for a nightly build).
- `.rpm` -> `cloudsmith push rpm offworld-nexus/${CLOUDSMITH_RPM_REPO:-rpm}/any-distro/any-version <blastit-<ver>-<rel>.<arch>.rpm> --republish`
  (RPM repos carry a single component: the repo itself is chosen, no `--component`).
- Signing/keys are Cloudsmith-side (Entitlements/repo GPG), so `install.sh` only needs the generated `bash.deb.sh` / `bash.rpm.sh` setup scripts.

## B. Forgejo mirror (best-effort, never fatal)
- `FORGEJO_URL=${vars.FORGEJO_URL || 'https://forge.offworld.nexus'}`, `FORGEJO_OWNER=OffworldNexus`, `FORGEJO_USER=Xowap`, `secrets.FORGEJO_PACKAGES_TOKEN`.
- `.deb` -> `POST ${FORGEJO_URL}/api/packages/${FORGEJO_OWNER}/debian/pool/any-distro/${COMPONENT}/upload`
- `.rpm` -> `POST ${FORGEJO_URL}/api/packages/${FORGEJO_OWNER}/rpm/${COMPONENT}/upload`
- HTTP basic auth `${FORGEJO_USER}:${FORGEJO_PACKAGES_TOKEN}`, multipart `package=@<artifact>`. **Skip cleanly** (exit 0 with a `::notice::`) when the token is unset, so forks and `pull_request` runs stay green.

## C. Channels, versions & retention
- **Tagged `v*` release:** publish version `0.2.0` to the `main` component of both apt and rpm repos, and attach `.deb` + `.rpm` + `.tar.gz` + `SHA256SUMS` to the GitHub Release (archives stay as release assets even though installs go through the repos).
- **Nightlies (`schedule:` + `workflow_dispatch`, on `develop`):** stamp git-derived versions
  - `.deb`: `0.2.0~nightly<YYYYMMDD>.r<run>+g<sha>` (Debian-native `~` sorts *below* the release, so a nightly never shadows `0.2.0`),
  - `.rpm`: `0.2.0-0.nightly.<YYYYMMDD>.r<run>.g<sha>` (`-0.` release prefix, `+` is illegal-but-legal-escaped in RPM so use `.`),
  into the `nightly` component; verify clean container installs across the supported Debian/Ubuntu **and** Fedora/Rocky releases; then **prune nightlies keeping the newest 7** (Cloudsmith API list + delete, newest-first by version string).

# Exact Deliverables
1. **`.deb` / `.rpm` Builders (`scripts/build-deb.sh`, `scripts/build-rpm.sh`):**
   - Package the universal release bundle (`blastit`, `blastit-lsp`, `libblastit_php*.so`, `libblastit_py*.so`, `python/blastit/*.py`, `LICENSE`) into:
     * `blastit_<version>_amd64.deb` / `blastit_<version>_arm64.deb`: `/usr/bin/{blastit,blastit-lsp}` implemented as symlinks (or tiny `exec` shims) into `/usr/share/blastit/lib/...`; `dpkg-deb --root-owner-group --build`.
     * `blastit-<version>-<release>.x86_64.rpm` / `...aarch64.rpm`: `rpmbuild -bb` with a checked-in `.spec`, or run the build inside a `rockylinux:9` / `fedora` builder container when `rpmbuild` is absent on the host.
   - Both take `--version`, `--release`, `--arch` and an input bundle dir, and are idempotent (clean staging under `/tmp/opencode/...`; **never** write to `/home/remy/.local` or `/usr/bin/blastit` on the dev host).
2. **`docs/install.sh` (served at `https://offworldnexus.github.io/blastit/install.sh`; `scripts/install.sh` points at the same file):**
   - POSIX `sh`, `set -eu`, no bashisms, honours `NO_COLOR`/`TERM=dumb`.
   - Options: `--version <ver>`, `--nightly`, `--mirror cloudsmith|forgejo`, `--user`, `--prefix <dir>`, `--develop [ref]`, `-y/--yes`, `-h/--help`. Env: `BLASTIT_RELEASE_BASE_URL`, `BLASTIT_DEB`, `BLASTIT_RPM`.
   - **Default = signed repository install:**
     * Debian/Ubuntu (`command -v apt-get`): fetch the Cloudsmith apt setup script (`https://dl.cloudsmith.io/public/offworld-nexus/${CLOUDSMITH_APT_REPO:-apt}/bash.deb.sh`), run it, then `apt-get update && apt-get install -y blastit` (via sudo when `uid != 0`). `--nightly` selects the `nightly` component in the setup script.
     * Fedora/RHEL/Rocky/Alma (`command -v dnf`): fetch `https://dl.cloudsmith.io/public/offworld-nexus/${CLOUDSMITH_RPM_REPO:-rpm}/bash.rpm.sh`, run it, then `dnf install -y blastit`.
     * `--mirror forgejo` swaps the base to `${FORGEJO_URL}/api/packages/OffworldNexus/{debian|rpm}` and writes the Forgejo repo source/`.repo` file instead.
   - **Direct-archive paths only for:** `--develop` (unreleased branch build), `--user` or an unprivileged non-sudo install (`~/.local/bin/blastit`, 1-line note explaining the fallback), and Alpine/musl or any distro with neither apt nor rpm (`/usr/local` as root, else `~/.local`).
   - Every downloaded artifact is verified against `SHA256SUMS` **before** it is installed or executed. Repository installs rely on apt/dnf GPG signature verification; never `--allow-unauthenticated` / `--nogpgcheck`.
   - Never prompts when stdin is not a TTY; prints the exact next command on refusal; exit `2` on a usage error.
   - **Serving:** `.github/workflows/docs.yml` must publish the exact same bytes at `https://offworldnexus.github.io/blastit/install.sh` (copy `docs/install.sh` to the built site root after `make docs-check`; a Zensical build does not copy non-`.md` files by itself). Prove it: `curl -fsSL <local site>/install.sh | sh -s -- --help` in CI, and a documented `curl | sh` smoke that only exercises `--help` (never a real install) so the pipe stays POSIX-`sh` clean.
3. **Universal `py3-none-*` PyPI wheels (`scripts/build-wheel.sh`, `pyproject.toml`):**
   - `py3-none-manylinux_2_28_{x86_64,aarch64}.whl` and `py3-none-musllinux_1_2_{x86_64,aarch64}.whl` carrying the universal binary + PHP/Python shims + the pure-Python `blastit` package, valid for CPython 3.12, 3.13 and 3.14+.
   - `scripts/check-portable-elf.sh --allow-origin-rpath` must still pass on every embedded artifact; zero `libpython` `DT_NEEDED`.
4. **Composer wrapper (`packages/composer-blastit/`):**
   - `composer.json` for `offworldnexus/blastit` (`"bin": ["bin/blastit"]`), `bin/blastit` resolves the pinned version's `blastit-<arch>-<libc>.tar.gz`, verifies an embedded SHA-256 table (or `SHA256SUMS` when `BLASTIT_RELEASE_BASE_URL` is set), caches under `vendor/offworldnexus/blastit/.runtime/`, then `exec`s with `BLASTIT_HANDOFF_DONE=1`.
   - Honours `BLASTIT_RELEASE_BASE_URL` for offline/local CI, and `BLASTIT_COMPOSER_CACHE` for a pre-seeded cache.
5. **Workflows & harness updates:**
   - New `.github/workflows/packages.yml`: build `.deb`/`.rpm` on tag `v*` -> `main`; container-install verification on Debian 12/13 (`debian:trixie-slim`), Ubuntu 22.04/24.04, Rocky 9, Fedora latest; publish to Cloudsmith (+ Forgejo mirror) only after every install leg is green; attach release assets.
   - **New `.github/workflows/py-sapi.yml`** (mirroring `php-sapi.yml`): builds the CPython shims with `make py-sapi PY_SAPI_REQUIRE_ALL=1` for every `py_shims` leg, and gates on the artifact-level proof (`nm -D --defined-only` carries `blastit_py_abi_version`/`blastit_py_minor`/`blastit_py_abi_v1`; `DT_SONAME` matches; `UND Py_InitializeEx`; zero `NEEDED libpython`). Without this the shipped shims are an unproven promise — it must land in **this release**, not later.
   - **`scripts/platform_matrix.py` gains `py_shims` legs** (`{3.13,3.14} x {x86_64,aarch64} x {gnu,musl}` = `8`) with a `PROVEN` row per leg, and `--matrix py-shims`/`--count py-shims` wired the same way as `shims`.
   - New `.github/workflows/nightly.yml`: `schedule:` + `workflow_dispatch` on `develop`; stamps the nightly versions, publishes to the `nightly` component, runs the same container install legs, prunes to the newest 7.
   - Update `.github/workflows/{ci,publish-pypi,wheels}.yml` for the universal `py3-none` wheels (drop the per-CPython-minor matrix), the Composer wrapper smoke, and the `py-sapi` shims.
   - Update `scripts/platform_matrix.py` (`--matrix packages`, `--matrix wheels`, `PROVEN`), `scripts/check_workflows.py`, and `scripts/test_packaging_gates.py` so the new workflows/legs are declared once and checked.
6. **Local container smoke harness (`scripts/test_packages.sh` / `e2e/`):**
   - Must accept `BLASTIT_DEB=<path>` and `BLASTIT_RPM=<path>` to install a **locally built** package inside a clean `debian:trixie-slim` / `rockylinux:9` container (`dpkg -i` / `rpm -Uvh`), run `blastit --version`, `blastit dev` on a fixture and `blastit check`, proving the PR-built packages before anything is published.
   - Also prove `install.sh` default (repo), `install.sh --user`, and `install.sh --develop` against a local `BLASTIT_RELEASE_BASE_URL` fixture server.

# Acceptance
- Local container smoke: `.deb` in `debian:trixie-slim` + `.rpm` in `rockylinux:9` via `BLASTIT_DEB`/`BLASTIT_RPM` (with `--version`/`dev`/`check`), `install.sh` repo + `--user` + `--develop`, universal wheel on CPython 3.13 **and** 3.14, and the Composer wrapper in `composer:2`.
- Cloudsmith/Forgejo publish steps validated offline (dry-run mode / token-unset skip) — no real publish in a leaf slice.
- `make fast-check && make workflow-check packaging-gates-test platform-matrix-check`
