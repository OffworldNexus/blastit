# RESULT — MB.67.1 canonical `languages.tsv` + bash/Python loaders + `package-set` derives

STATUS: blocked — every MB.67.1 acceptance item passes; `make fast-check` is red only on one pre-existing,
out-of-scope host-specific test (`e2e_check_fmt::learn_writes_one_grant_without_widening_a_view`, a 109-byte
fixture socket path under `/tmp/opencode` vs `SUN_LEN` 108), which no file in this node's `paths` can affect.

## Files changed

- `packaging/languages.tsv` (new) — the one declaration: `language<TAB>version<TAB>slug<TAB>deb-runtime<TAB>rpm-runtime`,
  `#` comments, one header, the six rows in the epic's order (`python 3.13/3.14`, `php 8.3/8.4/8.5/8.6`).
- `packaging/languages.sh` (new, 0755) — the one bash loader (`while IFS=$'\t' read`, no python):
  `blastit_language_rows` (core first), `blastit_language_field NAME INDEX`,
  `blastit_language_php_minors`, `blastit_language_py_minors`, `blastit_language_check`; resolves the TSV
  relative to its own dir (`BLASTIT_LANGUAGES_TSV` overrides it for tests).
- `scripts/languages.py` (new) — the one Python loader (stdlib, importable, no side effects):
  `--rows --names --shims --php-minors --py-minors --json --check [--tsv PATH]`.
- `packaging/package-set.sh` — hardcoded `BLASTIT_PACKAGE_ROWS` deleted; rows come from `languages.sh`.
  Every public reader, `validate_table`, `check_spec` and the spec assertion kept. `validate_table` now
  requires exactly one core, `rows == 1 + TSV language rows`, and fails on zero TSV rows. Doc comment and
  usage updated (versions derive from the TSV; the spec copy stays asserted until `MB.67.2`).

## Proof

### Baseline captured before editing, and empty after diffs

```
$ for m in rows names shims; do packaging/package-set.sh --$m > /tmp/mb67/before-$m; done   # before
$ wc -l /tmp/mb67/before-*
  7 /tmp/mb67/before-names
  7 /tmp/mb67/before-rows
  6 /tmp/mb67/before-shims

$ for m in rows names shims; do packaging/package-set.sh --$m > /tmp/mb67/after-$m; done    # after
$ for m in rows names shims; do diff -u /tmp/mb67/before-$m /tmp/mb67/after-$m; done
  (no output: all three diffs empty)
```

### `packaging/package-set.sh --check` (spec copy still asserted)

```
$ packaging/package-set.sh --check
package-set: ok (7 packages; scripts/rpm/blastit.spec's %package stanzas and %files globs match)
```

### bash loader vs Python loader agree

```
$ for m in rows names shims; do diff <(packaging/package-set.sh --$m) <(uv run --no-project scripts/languages.py --$m); done
bash==python rows
bash==python names
bash==python shims
```

### `scripts/languages.py --check` green

```
$ uv run --no-project scripts/languages.py --check
languages: ok (6 language rows: 4 php, 2 python)
```

### Negative checks on a temporary TSV copy (then reverted)

```
$ uv run --no-project scripts/languages.py --tsv /tmp/mb67/dup.tsv --check          # duplicate name row
/tmp/mb67/dup.tsv:24: package blastit-python-3.13 is declared twice (first on line 18)
/tmp/mb67/dup.tsv:24: slug py313 is declared twice (first on line 18)
exit=1

$ uv run --no-project scripts/languages.py --tsv /tmp/mb67/noruntime.tsv --check     # python 3.9 py39 (none) (none)
/tmp/mb67/noruntime.tsv:24: shim blastit-python-3.9 needs a deb-runtime and rpm-runtime Recommends value ((none) is only for the core package)
exit=1

$ BLASTIT_LANGUAGES_TSV=/tmp/mb67/noruntime.tsv bash -c 'source packaging/languages.sh; blastit_language_check'
languages: /tmp/mb67/noruntime.tsv:24: shim blastit-python-3.9 needs a deb-runtime and rpm-runtime Recommends value ((none) is only for the core package)
exit=1

$ uv run --no-project scripts/languages.py --check                                   # original restored
languages: ok (6 language rows: 4 php, 2 python)
exit=0
```

Version ordering is component-wise, never float: a temp TSV with `php 8.9` and `php 8.10` yields
`--php-minors` = `8.10 8.9 8.6 8.5 8.4 8.3`.

### Syntax

```
$ bash -n packaging/languages.sh packaging/package-set.sh
bash -n ok
```

### `make fast-check` — **red on one unrelated, pre-existing host-specific test**

```
$ make fast-check
...
failures:

---- learn_writes_one_grant_without_widening_a_view stdout ----
thread 'learn_writes_one_grant_without_widening_a_view' (3152440) panicked at crates/blastit/tests/e2e_check_fmt.rs:642:5:
assertion `left == right` failed: blastit check: /tmp/opencode/blastit-e2e-dev-3152280-learn-write/tmp/blastit-check-1000-3152454-740016900-0/pg/.s.PGSQL.5432: path must be shorter than SUN_LEN

  left: Some(2)
 right: Some(0)

test result: FAILED. 23 passed; 1 failed; ... test result: FAILED ...
make: *** [Makefile:153: rs-test] Error 101
exit=2
```

Not caused by this slice (nothing under `packaging/` or `scripts/languages.py` is read by `blastit check`):
`crates/blastit/tests/common/project.rs::scratch_root()` hardcodes `/tmp/opencode` whenever it is
creatable, and the fixture path `…/blastit-e2e-dev-<pid>-learn-write/tmp/blastit-check-…/pg/.s.PGSQL.5432`
is **109 bytes** — one over `SUN_LEN` (108) — so the fake server's `bind` is refused. Deterministic on this
host (three reruns identical; PID is 7 digits), and the harness only falls back to `std::env::temp_dir()`
when `/tmp/opencode` cannot be created. No file in this node's `paths` can change it.

The rest of `rs-test` is green with only that one test skipped, and the doc tests pass:

```
$ cargo test --workspace --all-targets --locked --features python -- --skip learn_writes_one_grant_without_widening_a_view
... test result: ok. ...
exit=0
$ cargo test --workspace --doc --locked --features python
... test result: ok. ...
exit=0
```

Running the gate with `/tmp/opencode` hidden (a mount namespace) is not a valid substitute: as
namespace-root the uid-dependent `e2e_cli::a_binary_owned_by_another_uid_is_refused` fails on `chown`.

## Follow-ups

- **Gate blocker (out of scope):** `crates/blastit/tests/common/project.rs::scratch_root()` needs a short
  scratch root (honour `TMPDIR`, or drop the long `learn-write`/`blastit-check-…` segments) so the
  fixture socket path stays under `SUN_LEN`; `e2e_check_fmt::learn_writes_one_grant_without_widening_a_view`
  is then green on hosts where `/tmp/opencode` exists.
- `MB.67.2` should generate `scripts/rpm/blastit.spec` stanzas from `packaging/languages.tsv` and replace
  the copy assertion in `package-set.sh --check` with a drift test of the generated output.
- `scripts/build-deb.sh`, `scripts/build-rpm.sh`, `scripts/check-bundle-layout.sh` and
  `scripts/platform_matrix.py` still hardcode their `7`/version lists; later slices derive them from the
  loaders (no out-of-scope edit here).

## Follow-up fix
Added `#!/usr/bin/env bash` as line 1 of `packaging/languages.sh`, so `./packaging/languages.sh --rows` runs from a non-shell exec.
Verified without cargo/make: `diff <(./packaging/languages.sh --rows) <(bash packaging/package-set.sh --rows)` empty, `bash -n packaging/languages.sh` clean, `packaging/package-set.sh --check` → `ok (7 packages; … spec … match)`.
