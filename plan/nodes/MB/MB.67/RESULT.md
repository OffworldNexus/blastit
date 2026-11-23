# RESULT — MB.67 gate: canonical supported-language table

**Full `make check` on the gate tree: EXIT 0** (`/tmp/mb67/make-check.log`, 68 green test binaries):
`guard`, `rs-fmt`, `rs-lint` (clippy `-D warnings`, build scripts included), `rs-doc`, `rs-test`,
`test-critical` (all `crates/*/tests/critical_*.rs`, zero skips), `py-lint`, `docs-unit`,
`docs-check` (strict; `gen_language_blocks.py --check` + zensical), `rs-no-python`, `rs-no-default`,
**`e2e-universal` (6 passed, 0 skipped — one artifact served every leg, PHP 8.4 on this host)**,
`zed-check` (11), `plan-validate` (`plan OK`).

## What now derives from `packaging/languages.tsv` (the only version data)

| Consumer | Commit | Derivation |
|---|---|---|
| `packaging/package-set.sh`, `packaging/languages.sh`, `scripts/languages.py` | `152c1f4` | rows/names/shims/orphans, byte-identical to the old 7-row table; one bash + one Python parser |
| `scripts/platform_matrix.py`, `scripts/rpm/blastit.spec` (+ `gen_spec.py`) | `2731596` | five matrices + the whole spec generated; `package-set.sh` stops asserting a spec copy; stale spec fails `--check` with a diff; unmeasured declared minors get convention legs with `suites: false` |
| `build-deb/rpm --print-set`, `check-bundle-layout.sh`, `build-package-bundle.sh`, `build-standalone*.sh`, `py_sapi_leg.sh`, `build-musl-wheel.sh`, `Makefile`, CI steps | `6de380e` | counts and CPython minors from the loaders; an undeclared minor is refused by name |
| `view-php`, `php-sapi`, `py-shim`, `py-sapi`, stage-0 sentence | `375a89e` | one Rust reader (`packaging/languages.rs.inc`, include-only, no dep) → four `build.rs` tables + freshness tests |
| `docs/install.md`, `docs/install.sh`, e2e drivers, prose pages | `815e5d8` | generated blocks + `--print-legs`; every published minor list replaced by a pointer to the install table |
| workspace version fixtures/examples | `81783a0` | `scripts/version.sh` from `Cargo.toml`; installer stamp, doc sample, fake packages derive it |

Two support fixes were needed on the way: an epic plan/bookkeeping commit `b8d14be` (new slices,
version rule) and `bc5d013` (test fixtures honour `TMPDIR`; a 109-byte socket path under
`/tmp/opencode` reddened every slice's `fast-check` on this host).

## Propagation proof (epic acceptance #2/#3)

Adding `php 8.7` to the TSV and touching nothing else: `package-set.sh --rows` gains
`blastit-php-8.7`/`libblastit_php87-*-gnu.so`; `gen_spec.py` gains the `%package`, `Recommends:` and
`%files` stanzas; `--matrix packages`/`--matrix shims` gain the leg (`--count shims` 4→5); all three
e2e `--print-legs` gain the package/shim; `cargo test -p blastit-view-php -p blastit-php-sapi`
passes and `target/debug/build/*/out/languages.rs` carries `Php87`, `php87` and `libphp8.7.so`;
`gen_language_blocks.py --check` fails showing exactly the new docs row, `--write` makes that the
only diff. Revert is byte-identical (`cmp`).

Removing `php 8.5`: names lose `blastit-php-8.5`, the package matrix loses the leg, `gen_spec.py
--check` exits 1 until regenerated, the docs table shrinks 7→6 rows (page 129 lines). Reverted.

The workspace version bumped to `9.9.9` in `Cargo.toml` alone moved `scripts/version.sh`,
`package-set.sh --version`, `docs/install.sh`'s generated `BLASTIT_DEFAULT_VERSION`, the doc sample
filenames and `test_install.sh`'s fake packages; reverted with no drift.

## Deliberate decisions

- `--matrix install` does not change with a version: it declares distributions × architectures, not
  versions. The installed package set is proved through `--leg install`/`--print-legs`.
- `make e2e` (Docker §2 apps) was not run: the only `crates/blastit` edit wires a generated refusal
  sentence (no logic); `e2e-universal` served every leg with the real binary, PHP included.
- Tool pins stay literal: `PYTHON_VERSION`/`python-version` (PYO3 build interpreter) and
  `wheels.yml`'s `PY_VER=3.13` (wheel-build interpreter) are floors, not the supported-shim list.
- History is untouched: `0.2.0`/`v0.2.0` past-event comments, `workers … removed in 0.2.0`,
  `CHANGELOG`, ADRs, `plan/`, `docs/testing/*`.
- Follow-up (recorded, not done): `scripts/platform_matrix.py` is now ~1900 LOC; `PHP_LEG_DATA` +
  `_php_leg` and `check()` are a natural module split, deliberately not churned inside this epic.

## Post-gate CI fix (PR #66 shim lint)

PR #66 reddened `.github/workflows/py-sapi.yml`'s `lint: the shim crates` job on:
`cargo clippy -p blastit-py-shim --features extension-module --all-targets --locked -- -D warnings`
→ `error: constant BUILT_PY_MINOR is never used` (the `surface` module that reads it is compiled
out under `all(test, feature = "extension-module")`, but `build.rs` emitted the const
unconditionally).

- `crates/blastit-py-shim/build.rs` now emits **both** `const BUILT_PY_MINOR` and the
  `const _: () = assert!(supports_py_minor(BUILT_PY_MINOR));` block under
  `#[cfg(not(all(test, feature = "extension-module")))]` — the exact shape that compiles the
  reader — keeping `if artifact` on the assert. No `#[allow(dead_code)]`; the assertion stays.
- `make rs-lint` (used by both `fast-check` and `check`) now also runs the exact CI invocation,
  commented as mirroring `py-sapi.yml`'s lint job, so the feature shape cannot pass the epic gate
  while failing CI.
- The gate gap: `make check` EXIT 0 did not cover the shim's `extension-module` + `test` shape;
  the workspace clippy (default features) and the artifact build never compile it.

Verified: exact CI command EXIT 0; `cargo test -p blastit-py-shim` 2 passed; no-feature clippy
EXIT 0; artifact build (`--features extension-module --release`) EXIT 0. Final `make check` below.

