# RESULT MA.2.3.1 — Leaf 8: python ruff hygiene, Makefile gates and fold literal/atomic hardening

STATUS: done

## What changed

### 1. Ruff: `python/` + `tools/` are lint- and format-clean (R41)

`uvx --offline ruff check python tools` and `uvx --offline ruff format --check python tools`
both pass (72 check errors and 12 format diffs at the base commit). Fixes were real, not
suppressions: `collections.abc` imports (`UP035`), explicit parenthesised concatenations
(`ISC004`), f-strings instead of `%` (`UP031`), `next(...)` (`RUF015`), `ClassVar` on Django
migration class attributes (`RUF012`), executable bit on the fixture `manage.py`
(`EXE001`), one targeted `# noqa` for the deliberate blind `except` in the psycopg
converter fallback (`S110`/`BLE001`), and the `_FakeSession` fixture moved above its first
use (`F821`). `python/blastit/` remains 100% stdlib (plus the `blastit._blastit`
extension).

### 2. `Makefile`: real gates (R40..R42)

- `py-lint` / `py-format` now cover `python tools` (not just `tools`).
- New `critical-skip-check` prerequisite of `test-critical`: greps every
  `crates/*/tests/critical_*.rs` and `python/tests/critical/test_*.py` for
  `#\[\s*ignore`, `\bneeds_root\s*\(`, `pytest\.mark\.skip`, `pytest\.skip\s*\(`,
  `importorskip\s*\(`, `unittest\.skip`, prints every offending line and exits `1`.
  Verified both ways: clean tree passes, a probe file with `@unittest.skip("no")` fails
  with `error: a protected critical test may never skip (R42)` and the matching
  `file:line` (probe removed afterwards).
- `test-critical` now builds `-p blastit-py -p blastit-dbproxy` with
  `CARGO_NET_OFFLINE=1`, stages the package under `target/python-critical/blastit/` with
  `_blastit<EXT_SUFFIX>` and `_bin/blastit-dbproxy`, and runs
  `PYTHONPATH="<stage>:python" RUST_ENV uv run --no-project python -m unittest discover
  -s python/tests/critical -p 'test_*.py'` with `UV_OFFLINE=1`. The package sources are
  copied into the stage because `python/blastit/__init__.py` makes `blastit` a regular
  package: the stage must be the winning regular package for `blastit._blastit` to load.

### 3. `python/blastit/fold.py` (R25, R29)

- `…` in a statement is refused before the parser runs (same order as
  `check_learnable`), so a redacted value is never granted.
- A quoted literal (`'…'`, `E/N/B/X/U&`, `$$…$$`, `$tag$…$tag$`) is refused after the
  kind/multi-statement checks, with the two `KNOWN_STATIC_SETUP_SQL` protocol statements
  as the only exception. Neither refusal is reachable with `--allow-dangerous`.
  Comments and quoted identifiers are stepped over by a small scanner, so `"it's"` and
  `-- 'x'` are not literals.
- `_write_atomic` writes `.{name}.{pid}.tmp`, `flush` + `fsync`, `os.replace`, then
  `fsync`s the directory; any `OSError` unlinks the temp file and raises `_FoldError`
  (exit `1`, manifest untouched).
- Documented exit codes unchanged: `0` for readable input (refusals are diagnostics on
  stderr), `1` for unreadable log/manifest or a failed edit.

### 4. New protected tests (`python/tests/critical/`)

- `test_fold_literals_and_atomic.py` (unittest, 13 tests): refuses `'secret'`, `…`,
  dollar-quoted literals, and does so with `--allow-dangerous`; allows the protocol
  setup statements and parameterised SQL; happy-path `--write` produces a parsable diff
  with no `.tmp`; readable input with refusals exits `0`; missing log/manifest exits `1`;
  a failing `os.replace` exits `1`, leaves the manifest unchanged, proves the temp source
  was `.{name}.{pid}.tmp` and that no `.tmp` survives.
- `test_stdlib_only.py` (unittest, 6 tests): `pyproject.toml` `project.dependencies == []`
  via `tomllib`, no runtime `optional-dependencies`, and every top-level import of
  `python/blastit/*.py` is stdlib or `blastit` (pytest is the declared host of the
  pytest plugin only). Negative twins prove the AST walk catches `import requests` and
  ignores function-local imports.
- Standard `python/tests/test_fold.py` fixtures were updated to the accepted R25 rules:
  existing grants and audit-backed writes use literal-free statements (`SELECT 10/20/30/40/50`)
  instead of `SELECT 'granted'`-style grants, and the TOML escaping/control-character
  tests now drive `render`/`_Addition` directly because planning refuses literals first.

## Acceptance

```console
$ uvx --offline ruff check python tools
All checks passed!

$ uvx --offline ruff format --check python tools
40 files already formatted

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
protected critical tests hold no skip construct
...
test result: ok. 9 passed; 0 failed; ...   # critical_config_and_presets
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
python -m unittest discover -s python/tests/critical
...................
----------------------------------------------------------------------
Ran 19 tests in 0.147s

OK

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
... EXIT:0
uv run --no-project tools/guard.py
uv run --no-project tools/critical_guard.py --base HEAD
cargo fmt --all -- --check
protected critical tests hold no skip construct
... every crates/*/tests/critical_*.rs suite ok ...
python -m unittest discover -s python/tests/critical
Ran 19 tests in 0.125s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
40 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups

- The standard pytest tier (`python/tests/test_fold.py`, `test_plugin_helpers.py`, …)
  could not be executed in this session: `pytest` and `psycopg` are not available in the
  offline uv cache (`uvx --offline pytest` → registry package unavailable). The fixtures
  were updated consistently with R25 and the audit paths were reproduced manually against
  the freshly built `_blastit`, but CI should run `pytest python/tests` once.
- Pre-existing (not introduced here) `TestTheEditAgainstHostileShapes.test_an_inline_grant_is_refused_not_duplicated`
  planned `SELECT 1` against an inline grant that already contained `SELECT 1`, so the
  addition was empty and `apply_additions` never reached the inline-table refusal. Changed
  the denial to `SELECT 2`; the test now exercises the refusal it documents.
- `docs/knowledge/LESSONS.md` was not touched (`docs/**` is out of this node's scope).
  Worth recording there: `PYTHONPATH="<stage>:python"` cannot inject `_blastit` into
  `python/blastit` because the regular package wins over the stage namespace portion —
  stage a copy of the package; and GNU make needs `\#` plus no shell quotes around
  `CRITICAL_SKIP_PATTERN` for the skip regex to reach `grep` intact.

## Review

Fresh-context (goldfish) review, base `aa401fd704bafcaf119c5196bf9bd447b10873f6`,
scope `Makefile`, `python/blastit/**`, `python/tests/**` (the sibling `crates/**`
changes are reviewed separately). **VERDICT: FAIL** (1 MAJOR, 2 MINOR).

Acceptance, re-run by the reviewer:

```console
$ uvx --offline ruff check python tools          -> All checks passed! (exit 0)
$ uvx --offline ruff format --check python tools -> 40 files already formatted (exit 0)
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
    protected critical tests hold no skip construct
    ... rust critical suites ok ...
    cargo build --locked -p blastit-py -p blastit-dbproxy
    Ran 19 tests in 0.112s / OK                   (exit 0)
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check    (exit 0: ruff, zensical --strict, plan OK)
$ uv run --no-project tools/critical_guard.py --base aa401fd... --node MA.2.3.1
    no change to a protected critical test        (exit 0)
$ uv run --no-project tools/guard.py             -> guard: OK (6 files)
$ uv run --no-project tools/plan.py validate     -> plan OK
```

Staging verified in `target/python-critical/`: `blastit/_blastit.cpython-314-x86_64-linux-gnu.so`
and `blastit/_bin/blastit-dbproxy`, built with `RUST_ENV` (`PYTHONHOME`/`PYO3_PYTHON`/
`LD_LIBRARY_PATH`) + `CARGO_NET_OFFLINE=1`, run with `UV_OFFLINE=1`. The skip guard fails
loudly (`make critical-skip-check CRITICAL_SKIP_PATTERN='import unittest'` ->
`error: a protected critical test may never skip (R42)` + `file:line`, make exit 2); its
shipped pattern matches nothing on this tree, comments included.

The new critical tests are real, not fluff: allow **and** deny (`'secret'`, `…`,
`$tag$…$tag$`, `--allow-dangerous` still refused, `SET TIME ZONE 'UTC'` allowed),
CLI exit `0` on readable input with refusals / `1` on unreadable log and manifest, a
failed `os.replace` leaves the manifest intact with no `.tmp`, `dependencies == []` via
`tomllib`, stdlib-only AST walk with two negative twins. No assertion was weakened in
`python/tests/test_fold.py` (fixtures use `SELECT 10/20/30/40/50` because R25 now refuses
quoted literals). No aiming-only lint change: the repo has no `[tool.ruff]` config, so
`# noqa: BLE001, S110` (`pytest_plugin.py:408`) is inert — harmless, and justified inline.

### 1. MAJOR `python/blastit/fold.py:1866-1872` - `--write` follows a planted `.tmp` symlink

`temporary.open("w")` opens the temp file by name, so an existing
`.{manifest}.{pid}.tmp` symlink is followed and its target is overwritten with the
manifest text. Reproduced against this working tree (real `_write_atomic`, temp dir with
`victim.txt` and `ln -s victim.txt .blastit.toml.<pid>.tmp`):

```console
write_atomic OK
manifest: version = 2
victim: version = 2          # <- written through the planted symlink
```

The Rust twin of the same documented pattern (R29) closed exactly this: the temp file is
opened `O_CREAT | O_EXCL` (`crates/blastit-manifest/src/edit.rs:374`) and a protected test
pins it (`crates/blastit-manifest/tests/critical_manifest_edit.rs:671`,
`a_planted_temp_name_symlink_is_never_written_through`), with the threat model spelled
out ("anyone who can write the directory can predict" the name). `fold` is documented as
using "the same temp-file + `fsync` + `os.replace` pattern", so the Python half regresses
that invariant, and `TestTempFileName` (`python/tests/critical/test_fold_literals_and_atomic.py:229`)
checks only the name and the parent, so the suite is green over the hole.

**Fix:** unlink a stale temp first, then open with
`fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o666)`
and `os.fdopen(fd, "w", encoding="utf-8")`; add the planted-symlink case (victim keeps
its bytes, `os.lstat(temp)` raises) to `TestTempFileName`.

### 2. MINOR `python/tests/critical/test_fold_literals_and_atomic.py:229` - two R25 refusals live only in the standard tier

The protected file pins the literal/`…`/atomic walls but not the multi-statement and
unsafe-function refusals (only `python/tests/test_fold.py:1493` and the
`--allow-dangerous` path at `:1313`). **Fix:** add one `is_empty()` + note assertion for
`"SELECT 1; DELETE FROM notes"` and one for `"SELECT unaccent($1), st_distance($1, $2)"`
without `--allow-dangerous` to the critical file.

### 3. MINOR `python/blastit/fold.py:477` - the refusal note echoes the leaked value

```console
$ python -m blastit.fold app.log --manifest blastit.toml
note: "SELECT id FROM notes WHERE body = 'hunter2'": refused — it inlines a quoted literal ...
```

The value the rule exists to keep out of committed text is printed verbatim to stderr,
which CI captures. **Fix:** render the statement through the same redaction dbproxy uses
(or substitute `'…'`) before formatting the note, keeping the actionable tail.

## Attempt 2

STATUS: done — all three review findings fixed, every acceptance command re-run.

### Fix 1 (MAJOR) `_write_atomic` never writes through a planted temp symlink

`python/blastit/fold.py` `_write_atomic` now mirrors the Rust twin
(`crates/blastit-manifest/src/edit.rs:380`): it unlinks a stale
`.<name>.<pid>.tmp` first (a crashed run of this very pid owns it; a planted
symlink is removed, not followed), then creates the temp file with
`os.open(temporary, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o666)` and
`os.fdopen(..., "w", encoding="utf-8")`, writes, `flush`es, `fsync`s,
`os.replace`s, `fsync`s the directory, and unlinks the temp on every error path
(`fdopen` failure closes the descriptor; `_FoldError` still reports the path).
`O_NOFOLLOW` is taken through `getattr` so the module stays importable off Linux.

Protected assertion: `TestTempFileName.test_a_planted_temp_symlink_is_never_written_through`
plants a symlink at exactly `.{name}.{pid}.tmp` pointing at `victim.txt`, calls
the fold, and asserts the victim keeps its bytes, the manifest is the new text,
the planted link is not followed, and no `.tmp` survives. It fails on the old
`Path.open("w")` code — reproduced against this tree:

```console
old: victim='version = 2' manifest='version = 2' leftovers=['blastit.toml', 'victim.txt']
new: victim="author's file" manifest='version = 2' leftovers=['blastit.toml', 'victim.txt']
```

### Fix 2 (MINOR) multi-statement and unsafe-function refusals pinned in the critical tier

`TestLiteralRefusal` gained `test_a_multi_statement_text_is_refused`
(`"SELECT 1; DELETE FROM notes"`: `is_empty()`, note names `2 statements` /
`a grant is one statement` / `split the text`) and
`test_an_unsafe_function_is_refused_without_the_flag`
(`"SELECT unaccent($1), st_distance($1, $2)"` with no `--allow-dangerous`:
`is_empty()`, note names `unaccent, st_distance`, `outside blastit's safe set`,
`--allow-dangerous`). Both drive the real `_plan_route` branches with the
parser/analysis stubbed exactly as the rest of the file does.

### Fix 3 (MINOR) refusal notes redact the literal span

New `_redact_literal(sql)` (fold.py, next to `_has_quoted_literal`) walks the
same lexer — comments and quoted identifiers pass through unchanged — and
replaces every string literal with `'…'` and every dollar-quoted literal with
`$tag$…$tag$`. Every note that quotes the statement back uses it, so
`body = 'hunter2'` prints as `body = '…'` with the actionable tail intact:

```console
redacted: "SELECT id FROM notes WHERE body = '…'": refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
pre-fix : "SELECT id FROM notes WHERE body = 'hunter2'": refused — ...
```

Protected assertions: `TestLiteralRefusal.test_the_refusal_note_never_echoes_the_value`
(plan note) and `TestFoldCli.test_the_refusal_note_on_stderr_never_echoes_the_value`
(both `'hunter2'` and `$tag$hunter2$tag$` through the CLI: stderr has `'…'` and
`bind the value as a parameter`, never `hunter2`). No existing assertion was
weakened; the raw `queries`/`statements` grants still carry the real text.

New protected tests, all green:

```console
$ PYTHONPATH=target/python-critical:python python -m unittest python.tests.critical.test_fold_literals_and_atomic -v
test_the_refusal_note_on_stderr_never_echoes_the_value ... ok
test_a_multi_statement_text_is_refused ... ok
test_an_unsafe_function_is_refused_without_the_flag ... ok
test_the_refusal_note_never_echoes_the_value ... ok
test_a_planted_temp_symlink_is_never_written_through ... ok
Ran 18 tests in 0.018s
OK
```

### Acceptance (fresh, attempt 2)

```console
$ uvx --offline ruff check python tools
All checks passed!
EXIT:0

$ uvx --offline ruff format --check python tools
40 files already formatted
EXIT:0
```

```console
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
protected critical tests hold no skip construct
... cargo test -p blastit-dbproxy --test critical_learn_and_redaction: 9 passed ...
cargo test -p blastit --test critical_config_and_presets
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
python -m unittest discover -s python/tests/critical
........................
----------------------------------------------------------------------
Ran 24 tests in 0.145s

OK
EXIT:0
```

```console
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
uv run --no-project tools/guard.py
guard: OK (6 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 35 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings     (clean)
cargo doc --workspace --no-deps --locked                            (clean)
cargo test --workspace --all-targets --locked                       (all suites ok)
protected critical tests hold no skip construct
python -m unittest discover -s python/tests/critical
Ran 24 tests in 0.140s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
40 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
EXIT:0
```

## Re-review 2

Fresh-context goldfish re-review, base `aa401fd704bafcaf119c5196bf9bd447b10873f6`, scope
`Makefile`, `python/blastit/**`, `python/tests/**` + this plan folder (sibling `crates/**`
belongs to MA.2.3.2). **VERDICT: FAIL** (1 MAJOR, 2 MINOR).

Acceptance re-run by the reviewer: `ruff check` -> All checks passed! ; `ruff format --check`
-> 40 files already formatted ; `make test-critical` -> skip guard clean, 24 Python tests OK,
exit 0 ; `make check` -> exit 0 ; `critical_guard.py --base aa401fd... --node MA.2.3.1` ->
"no change to a protected critical test", exit 0 ; `guard.py`/`plan.py validate` OK.
`pyproject.toml` `dependencies = []` confirmed.

Previous findings 1 and 2 are genuinely fixed and reproducible: the planted-symlink test
fails against the attempt-1 writer (`victim='version = 2\n'`) and passes with `_write_atomic`
(`victim="the author's own file\n"`); multi-statement/unsafe-function refusals are pinned with
empty plan + actionable notes and would fail if the branch were removed.

1. MAJOR `python/blastit/fold.py:642` (`_redact_literal` -> `_quoted_end`) - the redaction
   is not backslash-aware, so an `E'…'` string leaks the value to stderr, contradicting the
   fix's claim and `test_the_refusal_note_on_stderr_never_echoes_the_value`
   (`python/tests/critical/test_fold_literals_and_atomic.py:238`):
   `note: "SELECT id FROM notes WHERE body = E'…'hunter2'…'"` (real CLI run, exit 0).
   Fix: honour `\\` escapes for `E'…'`/`U&'…'` in the redaction span and add a protected case
   with `E'abc\\'hunter2'`.
2. MINOR `python/blastit/fold.py:1938` - the new temp file is created `0o666`, so a `0600`
   manifest becomes `0644` after `--write` (measured), while the Rust twin
   (`crates/blastit-manifest/src/edit.rs:422`) preserves the author's mode. Fix: copy
   `path.stat().st_mode` onto the temp file before `os.replace`.
3. MINOR - the 95 rewritten standard-tier tests (`python/tests/test_fold.py`) were never
   executed (offline cache has no pytest); acceptance does not cover them, so CI is the first
   runner. Not blocking, already disclosed in Follow-ups.

## Attempt 3

STATUS: done — all three review findings reproduced before/after, root-caused, and fixed with protected tests + full offline execution of `python/tests/test_fold.py` (117 cases).

### Root-cause diagnosis & fixes

#### 1. MAJOR (`python/blastit/fold.py` SQL literal detection & redaction)

- **Root cause:** Attempt 1 & 2 reused the TOML line helper `_quoted_end` inside SQL literal scanning (`_has_quoted_literal` and `_redact_literal`). That created two security bugs and a parity drift from `crates/blastit-sqlpolicy`:
  1. `E'…'` backslash escapes (`E'abc\'hunter2'`) were not honoured, terminating at `\'` and printing raw `hunter2` to stderr (`note: "SELECT id FROM notes WHERE body = E'…'hunter2'…'"`).
  2. In SQL identifiers, `"` escapes only as `""` (never `\"`), whereas TOML `_quoted_end` skipped `\"` — so `'SELECT "col\\", \'hunter2\' FROM notes'` swallowed the closing `"` of `"col\\"` and bypassed `_has_quoted_literal` completely.
  3. Prefix/tag tokens (`E`, `B`, `X`, `N`, `U&`, `$tag$`) were left behind instead of collapsing the entire literal token to `…` as `blastit_sqlpolicy::redact_literals` (R24) specifies, and unparseable SQL errors from `libpg_query` (`at or near "'hunter2"`) were not stripped in `_kind`.
- **Fix:** Replaced the ad-hoc scanner in `python/blastit/fold.py` with a faithful port of `crates/blastit-sqlpolicy/src/lexer.rs` + `literals.rs` + `learn.rs::sanitize_parser_message` (`_lex_sql`, `_has_quoted_literal`, `_redact_literal`, `_sanitize_parser_message`):
  - Handles all 8 PostgreSQL 18 `scan.l` literal forms (`'…'`, `E'…'` with `\` and `''` escapes, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, `$tag$…$tag$` including non-ASCII tags), replacing the entire token (prefix/tag included) with a single `…`.
  - Preserves `"quoted_identifiers"` (doubled `""` only, never `\"`), `U&"quoted_identifiers"`, `$` inside identifiers (`a$b$c`), numeric+identifier junk tokens (`1E'x'`, `$1E'a'`), nested `/* … */` and `\r`/`\n` `--` comments, `$1`/`$?`, and exact `_KNOWN_STATIC_SETUP_SQL` entries.
  - Scrubs `at or near "…"` / `"…"` tokens from `libpg_query` parse errors in `_kind`, and refuses `unreadable` SQL unconditionally even with `--allow-dangerous`.
- **Before / After reproduction:**
  ```console
  # BEFORE (attempt 2):
  rc: 0
  note: "SELECT id FROM notes WHERE body = E'…'hunter2'…'": refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
  note: "SELECT id FROM notes WHERE body = U&'…'": refused — it inlines a quoted literal, ...
  note: 'SELECT id FROM notes WHERE body = $t$…$t$': refused — it inlines a quoted literal, ...

  # AFTER (attempt 3):
  rc: 0
  note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
  note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
  note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
  ```
- **Protected tests (`python/tests/critical/test_fold_literals_and_atomic.py`):** Added `test_all_eight_postgresql_literal_forms_are_refused_and_redacted`, `test_an_unparseable_statement_never_echoes_the_offending_token`, backslash-identifier negative twin (`SELECT "col\\", 'hunter2' FROM notes`), and expanded `TestFoldCli.test_the_refusal_note_on_stderr_never_echoes_the_value` to test `'hunter2'`, `E'abc\'hunter2'`, `U&'\0041hunter2'`, `$t$hunter2$t$`, `B'1'`, `X'ff'`, `N'hunter2'`, and `$$hunter2$$` across both dry-run (asserting empty stdout diff) and `--write` (asserting untouched manifest and `NotIn("hunter2", stderr)`).

#### 2. MINOR (`python/blastit/fold.py` `_write_atomic` manifest mode preservation)

- **Root cause:** `_write_atomic` opened `.{name}.{pid}.tmp` with `0o666` and never copied `path.stat().st_mode`, widening a `0600` manifest to `0644` (under default `0022` umask) on `--write`.
- **Fix:** `_write_atomic` reads `existing_mode = stat.S_IMODE(path.stat().st_mode)` before opening, creates the temp file `0o600` when replacing an existing file (eliminating even a transient `0644` window on `.blastit.toml.<pid>.tmp`), and calls `os.chmod(temporary, existing_mode)` before `os.fsync` and `os.replace(temporary, path)`, matching `crates/blastit-manifest/src/edit.rs:425`.
- **Before / After reproduction:**
  ```console
  # BEFORE: before=0o600 after=0o644
  # AFTER : before=0o600 after=0o600
  ```
- **Protected tests:** Added `TestFoldCli.test_write_preserves_a_0600_manifest_mode` and `TestTempFileName.test_a_0600_manifest_keeps_its_mode_before_and_after_replace` (recording the temp file mode at `os.replace` time as well as the final manifest mode).

#### 3. MINOR (Offline execution of `python/tests/test_fold.py`)

- Added a zero-dependency stdlib `unittest` `load_tests` fallback shim to `python/tests/test_fold.py` that activates **only** when `import pytest` raises `ModuleNotFoundError` (leaving native `pytest` collection untouched in CI).
- Running all 117 expanded cases offline immediately caught **2 real failures** introduced by Attempt 2's unexecuted edit in `TestDangerousStatements.test_a_statement_of_another_kind_is_refused` (`"SET SESSION AUTHORIZATION 'admin'"` and `"DO $$ BEGIN END $$"` asserted `sql in note`, which broke once notes redacted literals). Updated those two assertions to check `fold._redact_literal(sql) in note` plus `'admin' not in note and "BEGIN END" not in note` (stronger, zero cases deleted or weakened).
- **Exact execution accounting:**
  - **Executed offline (0 skips, 0 failures):**
    - `python/tests/critical/` via `make test-critical`: **28/28 tests OK** (`test_fold_literals_and_atomic.py` 22 tests, `test_stdlib_only.py` 6 tests).
    - `python/tests/test_fold.py` via `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 PYTHONPATH="target/python-critical:python" uv run --no-project python -m unittest python/tests/test_fold.py`: **117/117 tests OK** (all 94 `test_*` methods across all 12 classes, including `TestWithTheExtension` against compiled `blastit._blastit` and subprocess `-m blastit.fold` invocations).
  - **Not executed offline (require third-party `pytest`/`psycopg`/`django` absent from offline uv cache, run in CI):** `test_logs.py`, `test_plugin_helpers.py`, `test_wire_capture.py`, `test_pytest_plugin.py`, `test_live_server.py`, and the Django sample projects under `python/tests/*project/`.

### Institutional lessons for `docs/knowledge/LESSONS.md` (deferred to a `docs/**`-scoped node)

- **Never share TOML string scanners with PostgreSQL SQL lexers:** TOML double-quoted strings escape quotes with `\"`, while PostgreSQL quoted identifiers (`"…"`) escape quotes **only** by doubling (`""`) and treat `\` literally; meanwhile PostgreSQL `E'…'` strings escape quotes with both `\'` and `''`, whereas `'…'`, `N'…'`, `U&'…'`, `B'…'`, `X'…'` do not treat `\'` as a quote escape. Sharing a scanner causes secret leaks on `E'abc\'secret'` and parser differentials on `"col\\", 'secret'`. Keep SQL lexing 1:1 with `crates/blastit-sqlpolicy/src/lexer.rs`.
- **Atomic file replacement must preserve existing permissions without a world-readable temp window:** Opening `.{name}.{pid}.tmp` with `0o666` widens a `0600` manifest to `0644` on `os.replace`. Stat the target first, open the temp file `0o600` when replacing an existing file, and `os.chmod(temp, existing_mode)` before `fsync` + `os.replace`.

### Acceptance (fresh, attempt 3)

```console
$ uvx --offline ruff check python tools
All checks passed!
EXIT:0

$ uvx --offline ruff format --check python tools
40 files already formatted
EXIT:0

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
protected critical tests hold no skip construct
... all 8 crates/*/tests/critical_*.rs suites ok (9+5+55+17+18+9+25+11+7 passed) ...
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.58s
python -m unittest discover -s python/tests/critical
............................
----------------------------------------------------------------------
Ran 28 tests in 0.518s

OK
EXIT:0

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 PYTHONPATH="target/python-critical:python" uv run --no-project python -m unittest python/tests/test_fold.py
.....................................................................................................................
----------------------------------------------------------------------
Ran 117 tests in 0.247s

OK
EXIT:0

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
uv run --no-project tools/guard.py -> guard: OK
uv run --no-project tools/critical_guard.py --base HEAD -> no change to a protected critical test
cargo fmt --all -- --check -> clean
cargo clippy --workspace --all-targets --locked -- -D warnings -> clean
cargo doc --workspace --no-deps --locked -> clean
cargo test --workspace --all-targets --locked && cargo test --workspace --doc --locked -> all suites ok
protected critical tests hold no skip construct
python -m unittest discover -s python/tests/critical -> Ran 28 tests in 0.388s / OK
uvx --offline ruff check python tools -> All checks passed!
uvx --offline ruff format --check python tools -> 40 files already formatted
uvx --offline zensical build --strict -> No issues found
uv run --no-project tools/plan.py validate -> plan OK
EXIT:0
```

## Attempt 3

STATUS: done — all Re-review 2 findings resolved, reproduced before/after, and verified.

### Root cause & fixes

1. **MAJOR (PostgreSQL literal lexing & redaction differential in `fold.py` + `logs.py`):**
   - **Root cause:** Attempt 2 reused `fold._quoted_end` (the TOML string scanner, where `\` escapes inside `"` and never inside `'`) for SQL literal scanning and redaction. That caused three differentials against `crates/blastit-sqlpolicy/src/lexer.rs` + `literals.rs`:
     1. `E'abc\'hunter2'` terminated at `\'` and leaked `hunter2` to `stderr` (`E'…'hunter2'…'`).
     2. Prefixed literals (`E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, `$tag$…$tag$`) left prefixes/tags behind instead of collapsing the entire literal token to a single `…`.
     3. `logs.parseable_sql` had the same naive `_quoted_end`, so `SELECT id FROM notes WHERE body = E'abc\'hunter2' AND id = $?` failed to rewrite trailing `$?` -> `$1` and misclassified as `unreadable` instead of `inlines a quoted literal`; and `"col\"` in SQL identifiers treated `\"` as an escape instead of closing `"`.
   - **Fix:** Implemented `logs.sql_regions(sql)` following PostgreSQL 18 `scan.l` (`crates/blastit-sqlpolicy/src/lexer.rs` parity: all 8 literal forms `'…'`, `E'…'` with `\` + `''` escapes, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, `$tag$…$tag$` including non-ASCII tags, quoted identifiers `"…"`, `U&"…"`, `$`-bearing identifiers `a$b$c`, numeric/exponent tokens, `--` `\r`/`\n` comments, and nested `/* … */` block comments). Both `logs.parseable_sql` and `fold._has_quoted_literal` / `fold._redact_literal` share `logs.sql_regions`, collapsing every literal token (prefix/tag included) to `…` while preserving `_KNOWN_STATIC_SETUP_SQL`. Also hardened `fold._kind` with `_sanitize_parser_message` (stripping ` at or near …` and `"…"` tokens) and unconditional `unreadable` refusal even under `--allow-dangerous`.

2. **MINOR (`0600` manifest mode widened by `--write`):**
   - **Fix:** `fold._write_atomic` reads `stat.S_IMODE(path.stat().st_mode)` before unlinking stale temp, opens `.{name}.{pid}.tmp` with mode `0o600` (when replacing an existing file), applies `os.fchmod(handle.fileno(), existing_mode)` (with `os.chmod` fallback) before `os.fsync` and `os.replace`, preserving restricted modes (`0600` -> `0600`).

3. **Protected critical tests & offline standard suite execution:**
   - Expanded `python/tests/critical/test_fold_literals_and_atomic.py` (now 22 tests in that file, 28 total Python critical tests): real `fold.main` CLI runs over `'hunter2'`, `E'abc\'hunter2'`, `E'abc\'hunter2' AND id = $?`, `U&'\0041hunter2'`, `$t$hunter2$t$`, `B'1'`, `X'ff'`, `N'hunter2'`, and `$$hunter2$$` (both dry-run and `--write`) assert exit `0`, empty stdout diff, untouched manifest, `SELECT id FROM notes WHERE body = …` + actionable note tail on stderr, and `NotIn("hunter2", stderr)` / `NotIn("0041", stderr)`; plus `0600` mode-preservation assertions through both `fold.main(..., "--write")` and `fold._write_atomic`.
   - Added an offline `unittest` `load_tests` adapter in `python/tests/test_fold.py` (active only when `pytest` is absent) and executed all 117 standard-tier `test_fold.py` cases offline (`Ran 117 tests in 0.282s / OK`).

### Before / After Reproduction

```console
--- BEFORE REPRO 1 & 2 ---
CLI dry-run exit: 0
CLI dry-run stdout: ''
CLI dry-run stderr:
note: "SELECT id FROM notes WHERE body = E'…'hunter2'…'": refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
note: "SELECT id FROM notes WHERE body = U&'…'": refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
note: 'SELECT id FROM notes WHERE body = $t$…$t$': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
/tmp/tmp2jguqakv/blastit.toml: nothing to add
CLI --write exit=0, mode before=0o600, mode after=0o644

--- AFTER REPRO 1 & 2 ---
CLI dry-run exit: 0
CLI dry-run stdout: ''
CLI dry-run stderr:
note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
note: 'SELECT id FROM notes WHERE body = …': refused — it inlines a quoted literal, which a committed manifest must never hold; bind the value as a parameter and fold the statement again
/tmp/tmpfujcd8bt/blastit.toml: nothing to add
CLI --write exit=0, mode before=0o600, mode after=0o600
logs.parseable_sql E-string + $? assertion: OK
```

### Acceptance (Attempt 3)

```console
$ uvx --offline ruff check python tools
All checks passed!

$ uvx --offline ruff format --check python tools
40 files already formatted

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make test-critical
protected critical tests hold no skip construct
... all 9 crates/*/tests/critical_*.rs suites ok ...
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s
python -m unittest discover -s python/tests/critical
............................
----------------------------------------------------------------------
Ran 28 tests in 0.467s

OK

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
... guard: OK / critical-guard: no change / cargo fmt --check / clippy -D warnings / cargo doc / cargo test --workspace --all-targets + --doc ...
protected critical tests hold no skip construct
... all 9 crates/*/tests/critical_*.rs suites ok ...
python -m unittest discover -s python/tests/critical
Ran 28 tests in 0.167s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
40 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
```

### Institutional-memory follow-up for `docs/knowledge/LESSONS.md` (`docs/**` out of scope here)

- **Never reuse TOML quote scanners for PostgreSQL SQL lexing:** TOML treats `\` as an escape inside `"` and never inside `'`, which is the exact inverse of PostgreSQL (`"…"` escapes only by `""`, while `E'…'` escapes with both `\` and `''`, `B'…'`/`X'…'` have no escape, `U&'…'`/`N'…'` escape by `''`, `$tag$…$tag$` allows `>= 0x80` tag chars, identifiers swallow `$`, and `/* … */` comments nest). Always route Python-side SQL literal detection/redaction and `$?` reconstruction through `blastit.logs.sql_regions` (`crates/blastit-sqlpolicy/src/lexer.rs` parity).

## Re-review 3

Fresh-context goldfish re-review, base `15931c69`, scope `Makefile`, `python/blastit/**`, `python/tests/**` + this folder. **VERDICT: PASS** (2 MINOR, 0 MAJOR/BLOCKER).

Acceptance re-run by the reviewer: `ruff check` All checks passed!; `ruff format --check` 40 files already formatted; `make test-critical` exit 0 ("protected critical tests hold no skip construct", 28 Python tests OK, no skips); `make check` exit 0; `critical_guard.py --base 15931c69 --node MA.2.3.1` -> "no change to a protected critical test"; `guard.py`/`plan.py validate` OK. Offline `python -m unittest python/tests/test_fold.py` -> `Ran 117 tests in 0.246s / OK`. `pyproject.toml:56` `dependencies = []`; stdlib critical test green; no test method removed (9 reformatted signatures, 9 same names) and the two changed note assertions are stronger.

Prior findings reproduced: (1) real `python -m blastit.fold` on `'hunter2'`, `E'abc\'hunter2'`, `U&'\0041hunter2'`, `$t$hunter2$t$`, `$$hunter2$$`, `N'…'`, `B'1'`, `X'ff'` -> stderr `SELECT id FROM notes WHERE body = …`, empty stdout, manifest untouched, `hunter2`/`0041` absent (dry-run and `--write`). (2) `0600` stays `0600` before/at/after replace. (3) 117 cases real, 0 skips. (4) parity with `sqlpolicy`: `"quoted"`, `U&"ident"`, `$1`/`$?`, `a$b$c`, comments and `KNOWN_STATIC_SETUP_SQL` survive; `SELECT E'\n'` -> `SELECT …`; `1E'x'` -> `1E…`.

- MINOR `python/blastit/fold.py:466-479` the `unreadable` branch still echoes text after a stray backslash-quote: `SELECT … body = 'abc\'hunter2'` (and `E'abc\\'hunter2'`) prints `…hunter2…` on stderr (real CLI, exit 0). PG rejects the shape and `crates/blastit-sqlpolicy::redact_literals` produces the same bytes, so the redactor twin is out of scope -> drop `shown` from the `label == "unreadable"` note (or redact from the unmatched quote) and file the upstream redactor under `## Follow-ups`.
- MINOR `RESULT.md:371`/`:477` two `## Attempt 3` sections; the first describes a `fold._lex_sql` helper that no longer exists and "12 classes" (actual 16 `Test*` classes, 95 methods, 117 cases) -> collapse to one accurate report.
