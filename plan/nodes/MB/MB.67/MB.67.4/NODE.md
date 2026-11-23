---
id: MB.67.4
title: "Generated docs/installer blocks, e2e + php 8.7 propagation proof"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.67.3]
paths:
  - docs/install.md
  - docs/install.sh
  - scripts/gen_language_blocks.py
  - scripts/test_install.sh
  - e2e/run_packaging.py
  - e2e/php_sapi_probe.py
  - e2e/universal_binary.py
  - e2e/run_prod.py
  - e2e/README.md
  - packaging/README.md
  - RELEASING.md
  - README.md
  - "docs/getting-started.md"
  - docs/index.md
  - docs/security.md
  - docs/troubleshooting.md
  - docs/deploy/index.md
  - docs/frameworks/php/index.md
  - docs/frameworks/python/index.md
  - docs/reference/settings.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T00:44"
updated: "2026-11-23T01:23"
---

# Goal

Finish the derivation on the user-facing edges — docs table, installer mapping, e2e expectations — and discharge `MB.67`'s acceptance #2/#3: prove that adding `php 8.7` (then removing `php 8.5`) changes every consumer with **no other file touched**.

# Scope

- `scripts/gen_language_blocks.py` — the one generator for files that cannot run code at use site: rewrites the marked block in `docs/install.md` (the `blastit-<language>-<version>` table: package, shim, deb runtime, rpm runtime) and the marked `case`/mapping block in `docs/install.sh` from `packaging/languages.tsv`. `--check` exits non-zero with a unified diff when a committed block is stale. Delimited by `<!-- BEGIN/END generated:languages -->` (md) and `# BEGIN/END generated:languages` (sh) — regenerate, never hand-edit the block.
- `docs/install.sh` — the `8.3|8.4|8.5|8.6` case arms and the "8.3–8.6 only" refusal come from the generated block; wording says "the versions Blastit ships" rather than a range when the range is not derivable.
- `scripts/test_install.sh` — cases (h)/(i) read the generated mapping rather than literals, and a new case runs `gen_language_blocks.py --check`, so a stale doc block fails `install-test` (already in `make fast-check`). Keep no-`sudo`/no-network guarantees.
- `e2e/run_packaging.py`, `e2e/php_sapi_probe.py`, `e2e/universal_binary.py`, `e2e/run_prod.py` — expectations (minors, shim names, package names) come from `scripts/languages.py` at test time; give each a `--print-legs`/dry mode so acceptance can prove the derivation without Docker.
- `packaging/README.md`, `RELEASING.md`, `README.md` — delete the prose version lists; point at `packaging/languages.tsv` as the one place a version is added (keep one worked example naming a single minor).

# Out of scope

Earlier slices' loaders/matrices/Rust tables, `packaging/stage-dist.py`. Do not alter `docs/install.md`'s three-step zero-`sudo` lead or its Mermaid diagram.

# Acceptance

1. `uv run --no-project scripts/gen_language_blocks.py --check` green; `make docs-check`; `sh scripts/test_install.sh`.
2. `uv run --no-project e2e/run_packaging.py --print-legs`, `... php_sapi_probe.py --print-legs`, `... universal_binary.py --print-legs` each equal the loader's declared minors/names.
3. **Propagation proof (acceptance #2).** With the tree clean, append `php\t8.7\tphp87\tlibphp8.7-embed\tphp-embedded` to `packaging/languages.tsv` and touch nothing else, then show: `packaging/package-set.sh --rows | grep php-8.7`; `uv run --no-project scripts/rpm/gen_spec.py | grep -A2 'blastit-php-8.7'`; `--matrix packages`, `--matrix shims`, `--matrix install` each gain the 8.7 leg; the Rust freshness tests pass (build.rs re-ran) and a `grep 8.7` in the generated table/`cargo test` output; `gen_language_blocks.py --check` now FAILS showing the 8.7 docs diff, then passes after regeneration with `git diff docs/install.md` showing exactly the new row. Revert the TSV and re-run the generators; `git status --porcelain` shows only the intended committed changes.
4. **Removal proof (acceptance #3).** Delete the `php 8.5` row, show `package-set.sh --names` loses `blastit-php-8.5`, `--matrix packages` loses the leg, the spec check fails until regenerated, and the docs block shrinks; revert.
5. `make fast-check`; `uv run --no-project tools/plan.py validate`.

# Proof (slice-specific)

Paste both transcripts (add 8.7 → revert, remove 8.5 → revert) with before/after command output, plus `make docs-check` and `install-test` tails. This slice carries the epic's provenance evidence.
