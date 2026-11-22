---
id: MB.67.1
title: "Canonical languages.tsv + bash/Python loaders + package-set derives"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [packaging/languages.tsv, packaging/languages.sh, "packaging/package-set.sh", scripts/languages.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T00:44"
updated: "2026-11-23T00:57"
---

# Goal

Create `packaging/languages.tsv` as the single declaration of supported language versions, and make the two runtime loaders (bash + Python stdlib) plus `packaging/package-set.sh` derive from it, with **byte-identical output** to today's hardcoded table.

# Scope

- `packaging/languages.tsv` — exact format from `MB.67`: `language\tversion\tslug\tdeb-runtime\trpm-runtime`, `#` comments, one header, rows in the epic's order (`python 3.13/3.14`, `php 8.3/8.4/8.5/8.6`). Versions are strings; `8.10` must sort after `8.9` wherever order is compared (file order is the order; never parse a version as a float).
- `scripts/languages.py` — the one Python loader (stdlib only, `uv run --no-project`). Derivation lives here: package `blastit-<language>-<version>`, shim `libblastit_<slug>-<arch>-<libc>.so`, core `blastit` is the row-less base spelled `(none)`, runtime columns are `Recommends:` values. Subcommands the later slices will call: `--rows` (the five-field package-set form, core first), `--names`, `--shims`, `--php-minors`, `--py-minors`, `--json`, and `--check` (duplicate names/slugs, bad field count, `core` vs `(none)`, runtime on core, float-looking version).
- `packaging/languages.sh` — the same loader for bash, sourced by `package-set.sh` and by the builders: `blastit_language_rows`, `blastit_language_field`, `blastit_language_php_minors`, `blastit_language_py_minors`. `while IFS=$'\t' read` only; no `python`.
- `packaging/package-set.sh` — replace the hardcoded `BLASTIT_PACKAGE_ROWS` array with rows built from `languages.sh`; keep every public reader (`--rows`, `--names`, `--shims`, `--owners`, `--deb-runtime`, `--rpm-weak`, `--kind`), `validate_table`, `--check`, and the existing doc comment intact (rewrite the "two files keep a copy" paragraph: the spec copy is still asserted here until `MB.67.2` generates it).

# Out of scope

`packaging/stage-dist.py` (byte-identical to the skill copy — do not read, regenerate or format it). `scripts/rpm/blastit.spec`, `scripts/platform_matrix.py`, Rust, docs and e2e are later slices.

# Acceptance

1. Baseline captured **before** editing: `for m in rows names shims; do packaging/package-set.sh --$m > /tmp/MB67-before-$m; done`; after the change each `diff /tmp/MB67-before-$m <(packaging/package-set.sh --$m)` is empty (proves the derivation reproduces the 7-row table exactly).
2. `packaging/package-set.sh --check` green (spec copy still matches).
3. Both loaders agree: for `rows names shims`, `diff <(packaging/package-set.sh --$m) <(uv run --no-project scripts/languages.py --$m)` is empty.
4. `uv run --no-project scripts/languages.py --check` green; and it fails on a temporary TSV with a duplicate name and on `python\t3.9\tpy39\t(none)\t(none)` (runtime required for shims), then passes after revert.
5. `bash -n packaging/languages.sh packaging/package-set.sh`; `make fast-check`.

# Proof (slice-specific)

Paste in `RESULT.md`: the empty before/after diffs, `--check` output, the python-vs-bash equality diff, and the two negative `--check` runs.
