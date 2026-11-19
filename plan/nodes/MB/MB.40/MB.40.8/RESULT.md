# MB.40.8 — `make install` bundle + embedded `PYTHONHOME` + `.venv` discovery

## Delivered

1. **`crates/blastit/build.rs`** — inside `CARGO_FEATURE_PYTHON` only, a
   `PYO3_PYTHON` that resolves to `<prefix>/bin/...` with `libpython<X.Y>.so`
   under `<prefix>/lib` or `lib64` now emits `BLASTIT_PYTHON_HOME=<prefix>` and
   `BLASTIT_PYTHON_MM=<X.Y>` (`libpython3.14.so` → `3.14`). A build without the
   feature emits neither, so `make rs-no-python` carries no build-host path.
2. **`crates/blastit/src/stage0/project_python.rs`** (new) +
   `stage0/{mod,dev,fork_exec,gates}.rs`:
   - stage 1's `execve` environment gains `PYTHONHOME=<BLASTIT_PYTHON_HOME>`
     for `Install::Binary` when the recorded prefix is a directory and the
     operator set none (`stage1_env`, `python_home`);
   - a standalone `entrypoint python`/`BLASTIT_APP` boot discovers
     `$VIRTUAL_ENV` (with `pyvenv.cfg`) else `<manifest_dir>/.venv`, reads
     `lib|lib64/python<X.Y>/site-packages`, matches it against
     `BLASTIT_PYTHON_MM`, prepends the bytes path to `config.sys_path`, and
     refuses a venv built for another minor in one line naming both versions and
     both fixes. `emit_runtime` names the discovered venv in the
     `INFO runtime python` line (`python 3.14  environment …/.venv`).
3. **`crates/blastit/src/check.rs`** — `python_program` now resolves
   `BLASTIT_PYTHON` → wheel → `$VIRTUAL_ENV` → `<manifest_dir>/.venv` → `PATH` →
   `<BLASTIT_PYTHON_HOME>/bin/python3` (only when `PATH` is set and names no
   `python3`, so `critical_static_cli`'s "no `PATH` → no interpreter" contract is
   kept); `run_pytest` prepends
   `<exe_dir>/../python` to `PYTHONPATH` when the staged bundle holds
   `blastit/_check_pytest.py` (existing `PYTHONPATH` entries preserved).
   `python/blastit/_check_pytest.py` answers `_probe_stdlib` (strict JSON/form,
   duplicate keys, `NaN`, lone surrogates, bad `%XX`, NUL, truncation) when
   `blastit._blastit` raises `ImportError`; `blastit/__init__.py` still imports
   no extension eagerly.
4. **`Makefile`** — `PREFIX ?= $(HOME)/.local` keeps `~/.local` the default and
   tests run with `PREFIX=/tmp/opencode/test-install`; `install` builds
   `--release --locked` (`blastit --features python,php,static`, `blastit-lsp`,
   `blastit-py`, `blastit-dbproxy`) + `php-sapi`, stages
   `$(PREFIX)/share/blastit/{lib,python}`, symlinks
   `$(PREFIX)/bin/{blastit,blastit-lsp}`, verifies `--version`; `uninstall`
   removes both symlinks and `share/blastit`. Both are in `.PHONY` and `make
   help`.

## Consolidated fix pass (review-arch / review-ux)

1. **`stage1.rs` (`Embed`):** `SITE_PACKAGES` now runs first and
   `restore_sys_path(py, &config.sys_path)` runs for **both** branches, so the
   project `.venv`'s `site-packages` lands in front of the base interpreter's
   purelib. Proven twice: `crates/blastit/tests/embed_venv_sys_path.rs` (new
   integration leg: `venv_only_mod.py` exists only under
   `.venv/lib/python<MM>/site-packages`, never in the cwd, and `blastit dev`
   answers `200`) and the installed-bundle smoke app, whose `app.py` imports
   `venv_only_mod`.
2. **`_check_pytest._probe_stdlib`:** `MAX_JSON_DEPTH = 64` (container depth
   counted by a non-recursive scanner, so 65 and 2000 are `False`, never
   `RecursionError`), a blanket `except Exception` guard so untrusted bytes can
   never raise, ASCII-only essence lowercasing (U+212A stays) and a 64-**byte**
   UTF-8-boundary cap (not 64 chars). `test_check_pytest_gate.py` gained
   `SharedCorpusTest`: documented goldens always, plus byte-for-byte parity with
   `_blastit.probe_ingress` over JSON, depth 64/65/2000, duplicate keys, `NaN`,
   u64 overflow, `APPLICATION/JSON\u212a`, an 80-byte non-ASCII MIME and form
   bodies — run both with the staged extension and without it.
3. **Security & determinism:** `project_venv(config, host, allow_env_var)` reads
   `$VIRTUAL_ENV` only for `dev`/`check` (`virtual_env` is a pure decision); a
   production boot accepts only `<manifest_dir>/.venv`, which stage 1's
   `check_view_paths` authorises. `Venv::release` reads `version_info`/`version`
   from `pyvenv.cfg` so the `runtime` line can say `3.14.10`, not just `3.14`.
   `build.rs` asks `PYO3_PYTHON -c 'sys.version_info'` for
   `BLASTIT_PYTHON_MM` (the `libpython*.so` filename is only the
   cross-compiling fallback), so `read_dir` order cannot record the wrong ABI.
   `bundled_python` requires both staged markers (`_check_pytest.py` **and**
   `_bin/blastit-dbproxy`), and `run_pytest` takes a `PytestSpawn<'a>` instead
   of eight positional arguments (the clippy allow is gone).
4. **Refusal & Makefile UX:** the mismatch refusal is stage 0's multi-line
   advice naming `uv venv --clear --python <MM>` (an existing `.venv` makes a
   plain `uv venv` fail) and `uv add blastit && uv run blastit dev`. `Makefile`
   refuses an empty `PREFIX` at parse time, prints capabilities, the installed
   `libblastit_php*.so` names, the `--version` and a `PATH` hint, and
   `uninstall` only removes `$(INSTALL_BINDIR)/{blastit,blastit-lsp}` when
   `readlink -f` says they are this bundle's symlinks, reporting "nothing
   installed" otherwise.

## Evidence

`make install PREFIX=/tmp/opencode/test-install` (tail):

```
php-sapi: 1 shim(s) for x86_64-gnu in …/target/php-sapi; skipped: 8.5 8.3
install: /tmp/opencode/test-install/bin/blastit -> …/share/blastit/lib/blastit
install: /tmp/opencode/test-install/bin/blastit-lsp -> …/share/blastit/lib/blastit-lsp
install: capabilities: python, php, static
install: php sapi shims: libblastit_php84-x86_64-gnu.so
install: python gate under /tmp/opencode/test-install/share/blastit/python
install: blastit 0.2.0
install: add it to PATH:  export PATH="/tmp/opencode/test-install/bin:$PATH"
```

`env -i PATH=/usr/bin:/bin` smoke test on the installed bundle. The Python app's
`app.py` now does `from venv_only_mod import VALUE`, and `venv_only_mod.py` was
written **only** to `.venv/lib/python3.14/site-packages` (never the cwd):

```
== dev: static ==          HTTP/1.1 200 OK / runtime static
== dev: python .venv ==    HTTP/1.1 200 OK
INFO  runtime  python 3.14  environment /tmp/opencode/smoke/python-app/.venv
== check: python .venv without blastit ==
1 passed in 0.00s
blastit check: 1 request, 0 statements, 1/1 view, clean
```

The new integration leg proves the same hop in-repo (module only in the venv,
`dev` answers 200):

```
cargo test -p blastit --features python --test embed_venv_sys_path
INFO  runtime  python 3.14.0  environment /tmp/blastit-embed-venv-…/.venv
INFO  request  GET /hello/ → hello  200
test a_module_that_exists_only_in_the_project_venv_is_served ... ok
```

Mismatched-venv leg (`.venv` created with CPython 3.13, bundle embeds 3.14):

```
blastit dev: the virtualenv …/python-app-313/.venv was built for CPython 3.13,
but this blastit embeds CPython 3.14
  rebuild it:  uv venv --clear --python 3.14
  or run from the project venv:  uv add blastit && uv run blastit dev
dev exit: 1
blastit check: 1 request, 0 statements, 1/1 view, clean   # stdlib probe fallback
```

`make uninstall PREFIX=/tmp/opencode/test-install` (and its two negatives: a
foreign `bin/blastit` is left in place with "nothing installed", and
`make PREFIX= uninstall` is a parse-time `PREFIX must not be empty` error):

```
uninstall: removed the bundle under /tmp/opencode/test-install
uninstall verified: bin/blastit, bin/blastit-lsp and share/blastit are gone
uninstall: leaving /tmp/opencode/test-uninstall-neg/bin/blastit: it is not a
symlink into …/share/blastit/lib
uninstall: nothing installed under /tmp/opencode/test-uninstall-neg
Makefile:38: *** PREFIX must not be empty …  Stop.
```

Rust unit tests: `cargo test -p blastit --features python --lib` → `305 passed`.
Python gate tests, **with the staged extension** (`PYTHONPATH` pointing at
`python/` + `_blastit.cpython-314-….so`):
`python -m unittest python.tests.test_check_pytest_gate` → `Ran 13 tests … OK`
(`SharedCorpusTest` compares `_probe_stdlib` byte-for-byte with
`_blastit.probe_ingress`), and `Ran 13 … OK` without the extension too.

## Follow-ups

- `blastit check` still resolves `pytest` through the first interpreter it
  finds; a project whose activated venv lacks pytest falls back to a later
  candidate only when the earlier one has no `python3` at all. A future slice
  could skip a candidate whose `import pytest` exits `1` and continue down the
  list (the preflight runs once, so this is a small, local change).
