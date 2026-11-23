---
id: MB.67.7
title: "Packaging builders & shell helpers derive counts and minors from the loaders"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.67.2]
paths:
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - "scripts/check-bundle-layout.sh"
  - "scripts/build-package-bundle.sh"
  - "scripts/build-standalone.sh"
  - "scripts/build-standalone-in-image.sh"
  - scripts/py_sapi_leg.sh
  - "scripts/build-musl-wheel.sh"
  - Makefile
  - ".github/workflows/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:57"
updated: "2026-11-23T01:10"
---

# Goal

The packaging builders and the CI shell helpers stop counting or naming interpreter minors themselves:
every one reads `packaging/languages.tsv` through the existing loaders (`packaging/languages.sh`,
`scripts/languages.py`) — the same one parser per language (`MB.67`'s rule). Today
`build-deb.sh` hardcodes 7/6, `check-bundle-layout.sh` 7/2/4, `build-package-bundle.sh`
`require_py=2`/`require_php=4`, `build-standalone.sh` `--require-py-shims 2`,
`build-standalone-in-image.sh` `PY_MINORS="3.14 3.13"`, `py_sapi_leg.sh`/`build-musl-wheel.sh`
`3.13|3.14`, and the `Makefile`'s `PY_MINORS`/`PY_SAPI_PYTHON_<digits>`.

# Scope

- `scripts/build-deb.sh`, `scripts/build-rpm.sh` — counts derive from `blastit_package_names` /
  `blastit_package_shims` (never `-eq 7`/`-eq 6`); prose lists collapse to one example or a
  pointer at the TSV; both gain `--print-set`, which prints `packaging/package-set.sh --rows`
  and exits before touching a bundle. Measured floors and `--max-glibc` plumbing unchanged.
- `scripts/check-bundle-layout.sh` — the package count and `BUNDLE_REQUIRE_PY_SHIMS` /
  `BUNDLE_REQUIRE_PHP_SHIMS` set-mode values are the per-language shim counts from the loader;
  failure messages name the table instead of "2 and 4".
- `scripts/build-package-bundle.sh` — `require_py`/`require_php` from the loader; messages stop
  enumerating versions.
- `scripts/build-standalone.sh` — `--require-py-shims` default is the number of CPython rows;
  usage/doc comments point at the TSV (a single worked example is fine).
- `scripts/build-standalone-in-image.sh` — `PY_MINORS` and `PY_SAPI_PYTHON_<digits>` derive from
  `packaging/languages.sh --py-minors` inside the image (`/opt/python/cp<digits>-cp<digits>`);
  a missing interpreter still fails closed when `PY_SAPI_REQUIRE_ALL=1`.
- `scripts/py_sapi_leg.sh`, `scripts/build-musl-wheel.sh`, `Makefile` — accept/iterate the
  declared CPython minors; **reject** a minor the TSV does not declare, naming the declared
  list. `Makefile`: `PY_MINORS ?= $(shell …loader…)` and the per-minor interpreter variables
  keep their `PY_SAPI_PYTHON_<digits>` names (callers set them).
- `.github/workflows/*` — only if a workflow still spells a minor/count after `MB.67.2`; state
  which, or state that none does.

# Out of scope

`scripts/platform_matrix.py`, `scripts/rpm/**` (the generator/spec: `MB.67.2`), Rust (`MB.67.3`),
docs/installer/e2e (`MB.67.4`), `packaging/stage-dist.py`.

# Acceptance

1. `bash scripts/build-deb.sh --print-set` and `bash scripts/build-rpm.sh --print-set` print the
   same rows as `packaging/package-set.sh --rows`; `bash -n` clean on every touched script.
2. `scripts/py_sapi_leg.sh --minor 3.15 --arch x86_64 --libc gnu` is rejected with the declared
   list in the message; `--minor 3.13` passes its argument validation (no build needed).
3. `make -n py-sapi` / `make -p` shows `PY_MINORS` equal to `packaging/languages.sh --py-minors`.
4. `grep -nE "8\.[3-7]|3\.1[3-9]"` over the touched scripts shows only prose naming one example
   (or nothing).
5. `make fast-check`.

# Proof

`RESULT.md`: the two `--print-set` outputs beside `package-set.sh --rows`, the rejected 3.15
message, the `make -n py-sapi` line, the grep, and the `make fast-check` tail.
