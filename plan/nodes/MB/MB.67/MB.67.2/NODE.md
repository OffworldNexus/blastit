---
id: MB.67.2
title: "CI & packaging matrices + RPM spec generated from languages.tsv"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.67.1]
paths:
  - scripts/platform_matrix.py
  - "scripts/rpm/**"
  - scripts/languages.py
  - packaging/languages.sh
  - "packaging/package-set.sh"
  - .github/workflows/packages.yml
  - .github/workflows/nightly.yml
  - ".github/workflows/php-sapi.yml"
  - ".github/workflows/py-sapi.yml"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:44"
updated: "2026-11-23T01:10"
---

# Goal

Make every CI/packaging matrix and the RPM spec **generated** from `packaging/languages.tsv`, so adding or removing a version row changes the released artifact set without editing a second file.

# Scope

- `scripts/platform_matrix.py` — delete the hardcoded PHP/Python version lists, shim names and runtime values; build `shims`, `py-shims`, `packages` and `install` legs from the loader (`scripts/languages.py`, imported as a module — one parser, no second copy). Keep `--check` as the packaging self-consistency gate, and extend it to (a) refuse a matrix leg naming a version absent from the TSV and (b) run `scripts/rpm/gen_spec.py --check` so the committed spec is proven fresh by the existing `packages.yml`/`nightly.yml` `--check` step.
- `scripts/rpm/gen_spec.py` + `scripts/rpm/blastit.spec` — the spec's `%package -n`, `%files` and `Recommends:` stanzas are written by the generator from the TSV (measured glibc/libgcc macro plumbing unchanged; `Recommends:`, never `Requires:`, for the interpreter). `--check` exits non-zero with a unified diff when the committed spec is stale.
- `packaging/package-set.sh` — the spec is no longer a hand-kept copy, so drop `check_spec` and
  its `--check` assertion (the freshness gate is `scripts/rpm/gen_spec.py --check`, run by
  `platform_matrix.py --check`); `--check` validates the table only and the doc comment says so.
- `.github/workflows/{packages,nightly,php-sapi,py-sapi}.yml` — keep consuming `--matrix`/`--check`; remove any version list that is now a matrix output.

# Out of scope

The builders and shell helpers (`MB.67.7`: `build-deb.sh`, `build-rpm.sh`, `check-bundle-layout.sh`,
`build-package-bundle.sh`, `build-standalone*.sh`, `py_sapi_leg.sh`, `build-musl-wheel.sh`,
`Makefile`). Rust crates (`MB.67.3`), docs/installers/e2e (`MB.67.4`), `packaging/stage-dist.py`.

# Acceptance

1. Baseline captured **before** editing: `for m in shims py-shims packages install wheels; do uv run --no-project scripts/platform_matrix.py --matrix $m > /tmp/MB67-before-$m.json; done`; after, each `diff` is empty.
2. `uv run --no-project scripts/platform_matrix.py --check` green; `uv run --no-project scripts/rpm/gen_spec.py --check` green and a one-character TSV edit makes it fail with a diff (revert).
3. Adding a plausible `php 8.7` row makes `--matrix packages`, `--matrix shims` and
   `--matrix py-shims` (unchanged) emit the 8.7 leg and `gen_spec.py` emit its `%package`,
   `Recommends:` and `%files` stanzas with no other edit; revert.
4. `packaging/package-set.sh --check` passes and no longer mentions the spec.
5. `make fast-check`.

# Proof (slice-specific)

`RESULT.md`: the five empty matrix diffs, generated-spec `git diff --stat` (zero after regeneration + clean `--check`), the stale-spec failure tail, the `php 8.7` matrix/spec snippet, and the `package-set.sh --check` line.
