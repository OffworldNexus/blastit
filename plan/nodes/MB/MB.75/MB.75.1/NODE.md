---
id: MB.75.1
title: "MB.75.1 Schema: channels column in all three loaders + package-set channel + all-row Rust tables"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - packaging/languages.tsv
  - packaging/languages.sh
  - packaging/languages.rs.inc
  - packaging/README.md
  - scripts/languages.py
  - "packaging/package-set.sh"
  - "crates/blastit-py-shim/build.rs"
  - "crates/blastit-py-shim/tests/languages.rs"
  - "crates/blastit-py-sapi/build.rs"
  - "crates/blastit-py-sapi/tests/languages.rs"
  - "crates/blastit-php-sapi/build.rs"
  - "crates/blastit-php-sapi/tests/languages.rs"
  - "crates/blastit-view-php/build.rs"
  - "crates/blastit-view-php/tests/languages.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T11:41"
updated: "2026-11-23T11:41"
---

# Goal

`packaging/languages.tsv` grows its sixth `channels` column, and the one bash / Python / Rust loader each parse and validate it. `packaging/package-set.sh` gains a channel selector, and the generated Rust tables cover **every** row (3.16 included) while stable packaging names only stable rows. Prove the epic's key split: a nightly-only row is accepted by the runtime and absent from the stable package set.

# Scope

- `packaging/languages.tsv`: header gains `channels`; existing rows become `stable nightly`; add `python 3.15 py315 python3.15 python3.15 stable nightly` and `python 3.16 py316 python3.16 python3.16 nightly`. `channels` is a space-separated subset of `stable nightly` in that order; empty or duplicate is invalid.
- `packaging/languages.sh`: parse six fields, validate channels, and let `blastit_language_rows`, the CLI (`--names`, `--rows`, `--shims`, `--py-minors`, `--php-minors`) take a channel. Default `stable`; `--channel nightly` (or `BLASTIT_LANGUAGES_CHANNEL`) selects nightly rows.
- `scripts/languages.py`: `Language.channels`, validation, `names(rows, channel=...)`, and `--channel` on every listing command. Default stable.
- `packaging/languages.rs.inc`: `Row.channels` + the same channel validation. **No filtering**: `rows_of`/`versions` stay all-row, so build scripts keep covering 3.16 and `NEWEST_PY_MINOR` is 3.16.
- `packaging/package-set.sh`: `--channel stable|nightly` (default stable), passed through to the loader; `--check` validates both channels, stable is exactly 8 names (core + py 3.13/3.14/3.15 + four PHP shims).
- `packaging/README.md`: the schema text is six fields.
- Update the `include!` consumers only where the six-field parse or the tests require it: `crates/blastit-py-shim/{build.rs,tests/languages.rs}`, `crates/blastit-py-sapi/{build.rs,tests/languages.rs}`, `crates/blastit-php-sapi/{build.rs,tests/languages.rs}`, `crates/blastit-view-php/{build.rs,tests/languages.rs}`. Add the runtime acceptance test here: `SUPPORTED_PY_MINORS`/`SUPPORTED_MINORS` contain `316` and `315` (newest first), the 3.16 row's channels are `nightly`, yet the stable package set contains no 3.16. Rust tables stay all-row by construction.

# Acceptance

- `uv run --no-project scripts/languages.py --check`
- `bash packaging/languages.sh --check` and `bash packaging/package-set.sh --check`
- `bash packaging/package-set.sh --channel stable --names` prints 8 names, contains `blastit-python-3.15`, excludes `blastit-python-3.16`; the list minus 3.15 is byte-identical to HEAD's `--names` output.
- `bash packaging/package-set.sh --channel nightly --names` contains `blastit-python-3.16`.
- `bash packaging/languages.sh --channel nightly --py-minors | head -1` is `3.16`; an unknown channel exits 2 with a usage line.
- `cargo test -p blastit-py-shim -p blastit-py-sapi -p blastit-php-sapi -p blastit-view-php --tests` (the runtime-acceptance test above is red if 3.16 is filtered out).

# Notes

Slice 1 of 4, first in strict order; the later slices consume the `--channel` API. One sequential lead, no fan-out: this worktree is shared. Stay inside `paths`; record discoveries under `## Follow-ups` in `RESULT.md`. Do not commit.
