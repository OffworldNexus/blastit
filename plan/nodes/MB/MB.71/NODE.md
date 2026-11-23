---
id: MB.71
title: "RPM install legs must not see the other architecture's packages"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [e2e/run_packaging.py, .github/workflows/packages.yml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T10:34"
updated: "2026-11-23T10:40"
---

# Goal

The `v0.2.1` `Packages` run now **builds** all RPMs (28 jobs green) but fails the four RPM **install** legs:

```
`dnf install -y --setopt=install_weak_deps=False /pkg/blastit-*.rpm` exits 1:
Problem 1: conflicting requests
 - package blastit-0.2.1-1.aarch64 from @commandline does not have a compatible architecture
```

Two independent over-broad selections put both architectures in one container:

1. `.github/workflows/packages.yml`'s `install` job downloads `pattern: package-*` with `merge-multiple: true` (≈line 278-282), so an `x86_64` leg also receives `package-aarch64-deb` and `package-aarch64-rpm`.
2. `e2e/run_packaging.py:1392` installs a **generic** `/pkg/blastit-*.rpm` (the deb leg already names `_<package_arch>.deb`), so even a correctly-downloaded directory would be globbed across arches.

# Change

- `packages.yml` install job: download `pattern: package-${{ matrix.arch }}-*` (the matrix's `arch` is `x86_64`/`aarch64`, and artifacts are named `package-<arch>-<format>`), keeping `merge-multiple: true`. Leave the `stage` job's `package-*` download alone — it must see all 28.
- `e2e/run_packaging.py` rpm leg: install the arch-scoped glob (`/pkg/blastit-*.<rpm_arch>.rpm`, where the arch comes from the leg/matrix, not a literal), matching what the deb leg already does. Assert the leg saw exactly the seven packages for its architecture, and fail with the count if not (a leg that installs six of seven must not pass).
- Do not weaken the install assertions: same phases (`--version`, `fmt`, `check`, `blastit-lsp --version`, a served page).

# Acceptance

- A local reproduction of the selection logic: in a temp dir holding **both** arches' RPMs (real artifacts from the last run, or `rpmbuild`-produced dummies), show the rpm leg's command now names only `<arch>` files, and that the old generic glob is what included the foreign arch.
- State explicitly that `install_matrix.sh:112` was already arch-scoped and was not the cause, so the fix targets the two places that were not.
- `make fast-check` green (this touches Python + a workflow), plus the workflow-syntax gate.
