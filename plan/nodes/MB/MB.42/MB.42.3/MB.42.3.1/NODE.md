---
id: MB.42.3.1
title: "Native packages: build-deb.sh + build-rpm.sh + BLASTIT_DEB/BLASTIT_RPM container smoke"
kind: task
status: review
owner: implementer
priority: 1
depends_on: []
paths: [Makefile, "scripts/**", "e2e/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T01:33"
updated: "2026-11-20T01:49"
---

# Goal
`scripts/build-deb.sh` + `scripts/build-rpm.sh`: package the universal bundle into `blastit_<ver>_{amd64,arm64}.deb` and `blastit-<ver>-<rel>.{x86_64,aarch64}.rpm`, and prove them by installing inside throwaway containers from a **locally built** file.

Authoritative detail: `plan/nodes/MB/MB.42/MB.42.3/NODE.md` §1 (builders) and §6 (containers). Read it.

# Exact Deliverables
1. `scripts/build-deb.sh --version --release --arch --bundle <dir>`: stage `bin/{blastit,blastit-lsp}` -> `/usr/share/blastit/lib/…`, `share/blastit/{LICENSE,lib,python/blastit/*.py}`, then `dpkg-deb --root-owner-group --build`. Control fields per §6.1: `Depends: libc6 (>= <measured>), libgcc-s1` — the floor **measured** from the shipped ELFs (`scripts/check-portable-elf.sh --max-glibc`), never hard-coded, and **no** hard php/python dependency.
2. `scripts/build-rpm.sh` with a checked-in `.spec`: `rpmbuild -bb` on the host, or inside a `rockylinux:9` builder container when `rpmbuild` is absent. Same layout, `Requires: glibc >= <measured>, libgcc`, `AutoReqProv: no`.
3. Both are idempotent, take an input bundle dir (so `make install PREFIX=<dir>` output can be packaged directly), and write **only** under `/tmp/opencode/…` or a caller-supplied output dir.
4. `scripts/test_packages.sh` (or an `e2e/packages.py`) honouring **`BLASTIT_DEB=<path>`** and **`BLASTIT_RPM=<path>`**: install the local file in a clean `debian:trixie-slim` / `rockylinux:9` container, then run `blastit --version`, a real `blastit dev` on a fixture, and `blastit check`. No network publish.
5. Wire a Make target (e.g. `packages` / `packages-test`) with a `##` help line, and add it to `make check` **only** if it is fast enough; otherwise state which gate runs it.

# Notes
- **Never** write `/home/remy/.local` or `/usr/bin/blastit`; every package install goes into a container.
- Never run two Cargo/Docker builds concurrently with another node; you are the only writer.
- `<500` prod LOC per file; zero new `scripts/check_*.py` source-grep guards (the ELF assertion belongs in `check-portable-elf.sh`, which already owns it).
- Never touch `docs/**`.

# Acceptance
- `.deb` built, installed in `debian:trixie-slim`, `--version` + `dev` + `check` green
- `.rpm` built, installed in `rockylinux:9`, same three proofs
- the `Depends`/`Requires` floor equals the measured max, quoted in `RESULT.md`
- `make fast-check`
