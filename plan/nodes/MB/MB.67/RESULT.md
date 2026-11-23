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

## Post-gate CI fix (PR #66: probe comparison + sdist packaging)

The remaining 34 red jobs had two causes, both introduced by `MB.67.4`:

### 1. `e2e/php_sapi_probe.py` compared the wrong representation (the oracle was right)

`verify()` asked `str(report["php_minor"]) in SUPPORTED_DIGITS`, where
`SUPPORTED_DIGITS` was the TSV's digits (`{"83","84","85","86"}`). The shim reports
`Minor::code()` — `major * 100 + minor`, i.e. **803**
(`crates/blastit-php-sapi/src/probe/main.rs:136,676`) — so all three egress cases
failed *after* every request had passed, with:

```
php_minor 803 is not one of ['83', '84', '85', '86'] (packaging/languages.tsv's php rows)
```

`must_fail`, `inet_socket_errno`, `classes` and the classification were never
wrong; only the comparison was. The expectation is now derived from the same rows
in the code's own representation:

```python
SUPPORTED_MINORS = {row.version for row in languages.load() if row.language == "php"}
EXPECTED_CODES = {int(v.split(".")[0]) * 100 + int(v.split(".")[1]) for v in SUPPORTED_MINORS}
want(report.get("php_minor") in EXPECTED_CODES, ...)
```

Before / after on the real value the probe prints:

```
SUPPORTED_MINORS: ['8.3', '8.4', '8.5', '8.6']
EXPECTED_CODES  : [803, 804, 805, 806]
before: str(803) in {83,84,85,86} -> False     <- the red jobs
after : 803 in EXPECTED_CODES     -> True
```

Proof with the real image (`ubuntu:24.04` + `php8.3-dev` + `libphp8.3-embed`,
Docker 29.10.0): `make php-sapi-e2e PHP_MINOR=8.3 PHP_SAPI_LIBC=gnu` — all three
egress cases `ok` (INI-only, filter + INI, filter with the INI control removed),
after `readelf -d` proved the shim links no `libphp`.

### 2. The `sdist` carried no `packaging/`, so a source build could not compile

`[tool.maturin] include` listed only `crates/**/*`, while
`crates/blastit-php-sapi/build.rs:25` (and the view-php/py-sapi/py-shim build
scripts) `include!("../../packaging/languages.rs.inc")` at compile time. Installing
from the sdist died with "couldn't find file `packaging/languages.rs.inc`". The
sdist now carries exactly the two files a source build reads — not `scripts/` and
not the rest of `packaging/` (that is release tooling):

```toml
{ path = "packaging/languages.tsv", format = "sdist" },
{ path = "packaging/languages.rs.inc", format = "sdist" },
```

Proof, mirroring `publish-pypi.yml`'s three steps:

```
$ uvx maturin sdist --out /tmp/mb67/sdist2
📦 Including files matching "packaging/languages.tsv"
📦 Including files matching "packaging/languages.rs.inc"
📦 Built source distribution to /tmp/mb67/sdist2/blastit-0.2.1.tar.gz
$ tar tzf /tmp/mb67/sdist2/blastit-0.2.1.tar.gz | grep packaging/
blastit-0.2.1/packaging/languages.rs.inc
blastit-0.2.1/packaging/languages.tsv

$ uv venv --python 3.13 /tmp/mb67/from-sdist2
$ VIRTUAL_ENV=/tmp/mb67/from-sdist2 uv pip install --no-cache /tmp/mb67/sdist2/blastit-0.2.1.tar.gz
   Building blastit @ file:///tmp/mb67/sdist2/blastit-0.2.1.tar.gz
   Built blastit @ file:///tmp/mb67/sdist2/blastit-0.2.1.tar.gz
Prepared 1 package in 1m 33s
 + blastit==0.2.1 (from file:///tmp/mb67/sdist2/blastit-0.2.1.tar.gz)

$ /tmp/mb67/from-sdist2/bin/blastit --help | grep -q "stage 0"        -> found
$ find /tmp/mb67/from-sdist2 -name 'blastit-dbproxy' -print -quit     -> (empty)
-rwxr-xr-x … /tmp/mb67/from-sdist2/bin/blastit                        (one program, 16.9 MB)
```

Also green: `cargo test -p blastit-php-sapi` (5 + 1 passed),
`scripts/platform_matrix.py --check`, `make fast-check`.

