---
id: MB.67
title: "Canonical supported-language table: one file, everything derives"
kind: epic
status: ready
priority: 1
depends_on: []
paths:
  - "packaging/**"
  - scripts/languages.py
  - scripts/version.sh
  - scripts/platform_matrix.py
  - "scripts/build-deb.sh"
  - "scripts/build-rpm.sh"
  - "scripts/rpm/**"
  - "scripts/check-bundle-layout.sh"
  - "scripts/build-package-bundle.sh"
  - "scripts/build-standalone.sh"
  - "scripts/build-standalone-in-image.sh"
  - scripts/py_sapi_leg.sh
  - "scripts/build-musl-wheel.sh"
  - scripts/test_install.sh
  - "scripts/offworld**"
  - "crates/blastit-view-php/**"
  - "crates/blastit-php-sapi/**"
  - "crates/blastit-py-sapi/**"
  - "crates/blastit-py-shim/**"
  - "crates/blastit-languages/**"
  - crates/blastit/src/stage0/project_python.rs
  - "crates/blastit/tests/**"
  - docs/install.md
  - docs/install.sh
  - README.md
  - RELEASING.md
  - "e2e/**"
  - Makefile
  - ".github/workflows/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-23T00:43"
updated: "2026-11-23T00:57"
---

# Goal

One canonical file declares **which languages Blastit supports and at which versions**; every other place — packaging table, RPM spec, CI matrices, Rust shim discovery, installers, docs — **derives from it**. Today the same fact is spelled in ~20 files and six of them are "kept in sync" by a `--check` diff (`packaging/package-set.sh` vs `scripts/rpm/blastit.spec` vs `scripts/platform_matrix.py`). Detecting drift is not the same as designing it out.

## The canonical file (create it)

`packaging/languages.tsv` — tab-separated, `#` comments, one header line, one line per supported language version:

```
language	version	slug	deb-runtime	rpm-runtime
python	3.13	py313	python3.13	python3.13
python	3.14	py314	python3.14	python3.14
php	8.3	php83	libphp8.3-embed	php-embedded
php	8.4	php84	libphp8.4-embed	php-embedded
php	8.5	php85	libphp8.5-embed	php-embedded
php	8.6	php86	libphp8.6-embed	php-embedded
```

Derivation rules (implement them once, in the one loader, and use them everywhere):

- package name = `blastit-<language>-<version>`
- shim file = `libblastit_<slug>-<arch>-<libc>.so`
- the core package `blastit` is not a language row; it is the row-less base (`(none)` in the existing `package-set.sh` vocabulary)
- `deb-runtime` / `rpm-runtime` are **`Recommends:`** values (never `Depends:`), per `MB.66`
- ordering is the file's order; a version is a string, never a float (8.10 must sort after 8.9 where versions are compared)

Format constraints: readable by **bash** (`while IFS=$'\t' read`), **Python** (stdlib, no deps) and **Rust** (a `build.rs` may `include_str!`/read it at compile time). No TOML/JSON: bash would need a parser.

## Consumers that must stop spelling versions (convert to derivation)

Inventory measured on this tree (line counts of version-bearing lines):

| File | Today | After |
|---|---|---|
| `packaging/package-set.sh` | hardcoded 7-row table | derives rows (core + one per line) from the TSV |
| `scripts/platform_matrix.py` (103) | own PHP/Python lists + shim names + runtimes | builds `shims`/`packages`/`wheels`/`install` legs from the TSV |
| `scripts/rpm/blastit.spec` (51) | copies the table, checked by `--check` | `%package`/`%files`/`Recommends` stanzas **generated** from the TSV by a script, with a drift test on the committed output |
| `scripts/build-deb.sh`, `scripts/build-rpm.sh` | package list per build | read `package-set.sh` (which now derives) |
| `crates/blastit-view-php/src/discovery.rs` (79) | hardcoded PHP minors + libphp names | `build.rs`-generated table from the TSV |
| `crates/blastit-php-sapi/src/toolchain.rs` (30) + `build.rs` | hardcoded minors | generated from the TSV |
| the CPython shim list (`blastit-py`/view-python side) | hardcoded 3.13/3.14 | generated from the TSV |
| `docs/install.md` (12) | hand-written table | generated block, committed, with a drift test |
| `docs/install.sh` | package-name mapping | generated mapping block, committed, with a drift test |
| `e2e/run_packaging.py`, `e2e/php_sapi_probe.py`, `e2e/universal_binary.py`, `e2e/run_prod.py` | expectations | read the loader at test time |
| `packaging/README.md`, `RELEASING.md` | prose lists | point at the file, no list |

## Rules

- Exactly **one** loader per language for the derivation (`scripts/languages.py` for Python, a shell function in `package-set.sh` or a small `packaging/languages.sh` for bash, a `build.rs` helper for Rust). No fourth copy of the parsing.
- For files that cannot run a generator at their use site (the RPM spec, the docs table, `docs/install.sh`'s mapping), the generator writes them and a **drift test fails** when the committed output is stale — that is a generator + freshness assertion, **not** a source-grep guard script.
- No new regex/`check_*.py` source scanners.
- `packaging/stage-dist.py` stays byte-identical to the skill copy — do not regenerate or touch it.
- Rust: the generated code must be `cargo fmt`-clean, and a test asserts the generator output matches the committed/generated value. Prefer `build.rs` reading the TSV over a committed copy.
- Keep `make fast-check`, `make docs-check`, the six-distribution install matrix and `packaging/package-set.sh --check` green.

## Acceptance

1. `packaging/languages.tsv` exists and is the only place a language version appears as data (`grep -rn "8\.3" --include=*.rs --include=*.py --include=*.sh --include=*.yml --include=*.spec` shows only the TSV, tests, generated files and prose that names a single example).
2. **Propagation proof**: add `php 8.7` to the TSV (with a plausible runtime), then without touching any other file show that `packaging/package-set.sh --rows`, the RPM spec regeneration, `scripts/platform_matrix.py --matrix packages|shims|install`, the Rust generated table and `docs/install.md` all pick it up; then revert the row and show the tree is clean again. Paste the before/after output.
3. Removing a version likewise shrinks every consumer (show one).
4. `make fast-check`, `make docs-check`, `guard.py`, `plan.py validate`, the install matrix and the packaging e2e stay green.

## Extension: the workspace version (`MB.67.5`)

The same disease, one fact over: the **workspace version** is typed by hand in executable
places. `scripts/test_install.sh` fakes `blastit-0.2.0-1.<arch>.rpm` on a 0.2.1 tree;
`docs/install.md`'s "Without `curl | sh`" filenames and `e2e/README.md`'s examples are typed.
The one source is `Cargo.toml`'s `[workspace.package] version`, which `scripts/dev-version.sh`
already reads.

Rules for `MB.67.5`:

- No script, test, fixture or generated doc example spells `0.x.y` literally; derive it from a
  single loader (`scripts/version.sh`, sourceable, one `sed` line over `Cargo.toml`, also
  exposed through `packaging/package-set.sh` like the language loader).
- Doc examples are either generated (user-facing `docs/install.md`, with the same
  generator + freshness assertion the language blocks get) or version-agnostic
  (`e2e/README.md`: `<version>`, defined as the `Cargo.toml` value).
- **History is exempt and must stay.** A comment explaining a past event — "why v0.2.0's
  packages were never built" in `publish-pypi.yml`, a `"workers was removed in 0.2.0"` error
  string, `CHANGELOG.md`, ADRs, `plan/`, `docs/testing/*` — is a record, not configuration.
  Never blank those to satisfy a grep.

Acceptance (folded into the epic's provenance evidence):

- `grep -rnE "0\.[0-9]+\.[0-9]+-" scripts/ e2e/ docs/install.md` returns only history/comments
  or nothing.
- Bump `Cargo.toml`'s workspace version to a scratch value, show `scripts/test_install.sh` and
  the generated doc examples follow it with no other edit, then revert; tree clean.
- `make fast-check`, `make docs-check` and `scripts/test_install.sh` stay green.
