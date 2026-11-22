---
id: MB.66
title: Shim packages recommend (not depend on) the distro language runtime
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - "scripts/rpm/**"
  - "scripts/check-bundle-layout.sh"
  - scripts/test_install.sh
  - e2e/run_packaging.py
  - docs/install.md
  - docs/install.sh
  - "packaging/package-set.sh"
  - scripts/platform_matrix.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:23"
updated: "2026-11-23T00:39"
---

# Goal

Owner decision: the per-interpreter shim packages must **not hard-depend** on the distribution's language package. A `Depends: python3.13` / `Depends: libphp8.4-embed` makes `blastit-python-3.13` uninstallable for anyone whose interpreter does not come from the distro (pyenv, a source build, Sury/Remi, Homebrew-style layouts), and uninstallable on a distro that simply lacks that minor. Move the runtime requirement to **`Recommends:`** on **both** formats, so `apt`/`dnf` still pull the distro runtime by default but the package installs when it is absent or provided by other means.

## Exact dependency shape after this change

- `blastit` (core): `Depends: libc6 (>= <measured floor>), libgcc-s1` — unchanged, **no shim, no language**.
- every shim package (`blastit-python-3.13`, `blastit-python-3.14`, `blastit-php-8.3`…`8.6`):
  - `Depends: blastit (= <version>)` (+ the measured `libc6` floor it needs);
  - `Recommends: <runtime>` — `python3.13`/`python3.14`, `libphp8.3-embed`…`libphp8.6-embed`.
- RPM: `Requires: blastit = <version>`; `Recommends: <runtime>` for **both** Python and PHP (today PHP is `Suggests:` — promote it so the two formats agree).

Keep the dependency direction (`shim → blastit`) untouched: core must never require a shim.

## Where

- `scripts/build-deb.sh` (`Depends:`/`Recommends:` in the control template) and the RPM spec/build (`scripts/build-rpm.sh` or the `.spec` it renders).
- Any assertion that reads those fields: `e2e/run_packaging.py`, `scripts/check-bundle-layout.sh`, `scripts/test_install.sh`, and the `--dry-run`/set checks — a test that asserts `Depends:` must assert `Recommends:` now.
- `docs/install.md`: one short sentence — the shim packages *recommend* the matching distribution runtime, so a `pyenv`/source-built PHP or Python works; `apt-get install blastit-php-8.4` still installs Debian's `libphp8.4-embed` by default. No apology, no platform matrix essay.

## Acceptance

- `dpkg-deb --field <deb> Depends` shows only `blastit (= 0.2.1)`, `libc6 (>= …)` (+ `libgcc-s1` if it is genuinely linked); `Recommends` names the runtime. Same statement for `rpm -qpR` / `rpm -qp --recommends`.
- Install proof for the non-distro case: in `debian:12` (which has **no** `python3.13`), `apt-get install ./blastit-python-3.13_*.deb` **succeeds** without pulling a Python runtime, and `blastit --version` still works; the shim is simply inert until an interpreter is available. Repeat with `--no-install-recommends`.
- Negative/positive pair for the distro case: in `debian:trixie`, `apt-get install blastit-python-3.13` pulls `python3.13` through `Recommends`.
- The six-distribution install matrix from `MB.65` still passes.
- `make docs-check`, `guard.py`, `plan.py validate` clean.
