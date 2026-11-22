---
id: MB.67.2
title: "CI & packaging matrices + RPM spec generated from languages.tsv"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.67.1]
paths:
  - scripts/platform_matrix.py
  - scripts/rpm/gen_spec.py
  - scripts/rpm/blastit.spec
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - scripts/py_sapi_leg.sh
  - "scripts/build-musl-wheel.sh"
  - .github/workflows/packages.yml
  - .github/workflows/nightly.yml
  - ".github/workflows/php-sapi.yml"
  - ".github/workflows/py-sapi.yml"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T00:44"
updated: "2026-11-23T00:44"
---

# Goal

Make every CI/packaging matrix and the RPM spec **generated** from `packaging/languages.tsv`, so adding or removing a version row changes the released artifact set without editing a second file.

# Scope

- `scripts/platform_matrix.py` — delete the hardcoded PHP/Python version lists, shim names and runtime values; build `shims`, `py-shims`, `packages` and `install` legs from the loader (`scripts/languages.py`, imported as a module — one parser, no second copy). Keep `--check` as the packaging self-consistency gate, and extend it to (a) refuse a matrix leg naming a version absent from the TSV and (b) run `scripts/rpm/gen_spec.py --check` so the committed spec is proven fresh by the existing `packages.yml`/`nightly.yml` `--check` step.
- `scripts/rpm/gen_spec.py` + `scripts/rpm/blastit.spec` — the spec's `%package -n`, `%files` and `Recommends:` stanzas are written by the generator from the TSV (measured glibc/libgcc macro plumbing unchanged; `Recommends:`, never `Requires:`, for the interpreter). `--check` exits non-zero with a unified diff when the committed spec is stale.
- `scripts/build-deb.sh`, `scripts/build-rpm.sh` — take the package list, shim globs and runtimes from `package-set.sh` (already sourced; remove the per-build copies and version prose), and gain a `--print-set` dry mode that prints the derived rows without building.
- `scripts/py_sapi_leg.sh`, `scripts/build-musl-wheel.sh` — accept the CPython minors the TSV declares instead of `3.13|3.14` literals; reject a minor the TSV does not declare.
- `.github/workflows/{packages,nightly,php-sapi,py-sapi}.yml` — keep consuming `--matrix`/`--check`; remove any version list that is now a matrix output.

# Out of scope

Rust crates (`MB.67.3`), docs/installers/e2e (`MB.67.4`), `packaging/stage-dist.py`.

# Acceptance

1. Baseline captured **before** editing: `for m in shims py-shims packages install wheels; do uv run --no-project scripts/platform_matrix.py --matrix $m > /tmp/MB67-before-$m.json; done`; after, each `diff` is empty.
2. `uv run --no-project scripts/platform_matrix.py --check` green; `uv run --no-project scripts/rpm/gen_spec.py --check` green and a one-character TSV edit makes it fail with a diff (revert).
3. `bash scripts/build-deb.sh --print-set` and `bash scripts/build-rpm.sh --print-set` print the same seven rows as `packaging/package-set.sh --rows`; `bash -n` on all four shell scripts.
4. `uv run --no-project scripts/py_sapi_leg.sh --minor 3.15 …` (or the loader-level equivalent) is rejected with the declared minors in the message; `--minor 3.13` still works.
5. `make fast-check`.

# Proof (slice-specific)

`RESULT.md`: the five empty matrix diffs, generated-spec `git diff --stat` (zero after regeneration + clean `--check`), the stale-spec failure tail, the three `--print-set` outputs, and the rejected-minor message.
