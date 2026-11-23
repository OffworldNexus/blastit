---
id: MB.75
title: "Add a channel dimension: Python 3.15 stable, 3.16 nightly-only"
kind: epic
status: ready
priority: 1
depends_on: []
paths:
  - "packaging/**"
  - scripts/languages.py
  - scripts/platform_matrix.py
  - "scripts/rpm/**"
  - "scripts/build-*.sh"
  - scripts/gen_language_blocks.py
  - docs/install.sh
  - docs/install.md
  - docs/index.md
  - RELEASING.md
  - e2e/run_packaging.py
  - .github/workflows/packages.yml
  - .github/workflows/nightly.yml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T11:40"
updated: "2026-11-23T11:42"
---

# Goal

Owner: Python support must include **3.13, 3.14 and 3.15**, with **3.16 additionally in the nightly channel**. Verified available to `uv` on this host: `cpython-3.15.2` (stable) and `cpython-3.16.0a2` (alpha, nightly).

That means `packaging/languages.tsv` needs a **channel dimension**, not just two more rows: a version can ship in `stable`, in `nightly`, or in both.

# Schema change (extend the existing file; do not add a second file)

Add a sixth column, `channels`, to `packaging/languages.tsv`:

```
language<TAB>version<TAB>slug<TAB>deb-runtime<TAB>rpm-runtime<TAB>channels
```

- `channels` is a space-separated subset of `stable nightly`, in that order; a row with neither is invalid; a duplicate is invalid.
- Existing rows become `stable nightly`.
- New rows: `python 3.15 py315 python3.15 python3.15 stable nightly` and `python 3.16 py316 python3.16 python3.16 nightly`.

Channel semantics — get this distinction right, it is the whole point:

- **Runtime acceptance (Rust)**: the shipped binary must be able to select *any* declared minor, including a nightly-only one, because the same `blastit` binary serves nightly installs. So the generated Rust tables (`SUPPORTED_*`, `NEWEST_*`) cover **all** rows, and `NEWEST_PY_MINOR` becomes 3.16 — state that explicitly and add a test that a nightly-only row is accepted by the runtime while remaining absent from stable packaging.
- **Packaging (`stable`)**: only rows whose `channels` contain `stable` — core + `blastit-python-3.13/3.14/3.15` + the four PHP shims = **8 names**, so **32 artifacts** (8 × amd64/arm64 × deb/rpm), and `blastit-python-3.16` must not exist.
- **Packaging (`nightly`)**: every row containing `nightly` — 3.16 included.
- **Docs / install.sh**: the stable install table and the installer's generated mapping list stable rows only; the nightly documentation says 3.16 is nightly-only and how to install it (`--nightly`).

# Where

- `packaging/languages.tsv`, `packaging/languages.sh`, `scripts/languages.py`, `packaging/languages.rs.inc` (all three loaders extend; still one parser per ecosystem).
- `scripts/platform_matrix.py`: every matrix gains a channel selector, defaulting to `stable` for tag/stable paths and `nightly` for `nightly.yml`. No matrix row may hardcode a version.
- `scripts/rpm/gen_spec.py` (spec rows for the stable set), the builders, `packaging/package-set.sh` (name set per channel), `docs/install.md` block generator, `docs/install.sh` mapping generator, `docs/install.md`/`docs/index.md` prose where a version list appears.
- `.github/workflows/packages.yml` (stable) and `nightly.yml` (nightly) must simply pass the channel; any literal version list there is a bug.

# Acceptance

- `packaging/package-set.sh --check`, `scripts/languages.py --check`, `gen_spec.py --check`, `platform_matrix.py --check`, `gen_language_blocks.py --check` all green, and the committed artifacts regenerated.
- **Stable**: `--channel stable` yields 8 package names and excludes 3.16 everywhere (package set, spec, matrices, docs table, installer mapping). **Nightly**: includes `blastit-python-3.16`.
- **Real shim builds**, not just tables: `make py-sapi` for 3.15 and for 3.16 (the latter against 3.16.0a2) produces loadable shims that pass the ELF contract, and one boot-level probe runs with the 3.15 shim.
- **Runtime acceptance test**: a 3.16 shim is selectable by the binary (nightly), while a stable artifact set contains no 3.16 package.
- The existing install-coverage legs (`MB.73`) and the `MB.71` arch selection still pass with the new counts (they are TSV-derived, so this is the test that the derivation actually holds).
- `make fast-check`, `make docs-check`, `guard.py`, `plan.py validate` green.
