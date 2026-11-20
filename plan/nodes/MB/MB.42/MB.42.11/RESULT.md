# MB.42.11 — `blastit check` from a package/bundle: the gate marker pair

## What changed

`crates/blastit/src/check.rs` only:

* `bundled_python()` now keeps a candidate root that carries **`_check_pytest.py`
  + `__main__.py`**, and no longer demands `_bin/blastit-dbproxy` — that path is
  the *retired* wheel layout `scripts/check-bundle-layout.sh` refuses and
  `make install` never stages, so every `.deb`, `.rpm`, `.tar.gz` and
  `make install` tree resolved no root, set no `PYTHONPATH`, and `pytest -p
  blastit._check_pytest` died with `No module named 'blastit'` (exit 3).
* The stale comment above it now names the real roots: a bundle's
  `<exe_dir>/../python` (`<prefix>/share/blastit/python`) and the staged
  `<exe_dir>/../../..`; it says why `_bin` is deliberately **not** a marker.
* `bundle_root(exe_dir)` is the one-line pure seam `bundled_python()` calls with
  `current_exe().parent()`, so a temp tree can be tested without a global or an
  env hook. No other restructuring; `check-bundle-layout.sh` is untouched and
  still refuses `_bin`.

New test `check::tests::an_installed_bundle_is_a_gate_root_and_a_cargo_tree_is_not`
(in `mod tests`): a `0700` `CheckRoot` holding a **bundle-shaped** prefix —
seven modules under `share/blastit/python/blastit/` (`__init__ __main__
_blastit _check_pytest fold logs pytest_plugin`), `share/blastit/lib/blastit`,
`bin/blastit -> ../share/blastit/lib/blastit`, **no `_bin`** — resolves
`share/blastit/python`. Denied twins: dropping `__main__.py` (then
`_check_pytest.py`) makes it `None`; a cargo tree
(`repo/target/debug/deps` beside `repo/python/blastit`) is `None`.

## Evidence

### The reproduction — package-shaped prefix, WSGI fixture, `env -i`, no `PYTHONPATH`

Bundle: `<prefix>/share/blastit/{lib/blastit, lib/libblastit_py313-*.so,
python/blastit/*.py}` (seven modules, no `_bin`), `bin/blastit ->
../share/blastit/lib/blastit`. Fixture: `app.py` (`application`), `Blastfile`
(`entrypoint python "app.py:application"`, `view home regex '.*' {}`), a
`tests/test_app.py` that drives `blastit._check_pytest.wrap`, and a `.venv` with
pytest 9.2.0.

```
$ [ ! -e PREFIX/share/blastit/python/blastit/_bin ] && echo "no _bin (correct)"
no _bin (correct)

$ cd app && env -i PATH=PREFIX/bin:/usr/bin:/bin PREFIX/bin/blastit check
.                                                                        [100%]
1 passed in 0.00s
blastit check: 1 request, 0 statements, 1/1 view, clean
exit=0
```

Causal twin (same tree, `__main__.py` moved away so the marker pair is broken —
the pre-fix state):

```
$ env -i PATH=PREFIX/bin:/usr/bin:/bin PREFIX/bin/blastit check
ERROR: Error importing plugin "blastit._check_pytest": No module named 'blastit'
exit=3
```

### `cargo test -p blastit --locked` (default features, Makefile `RUST_ENV`)

```
status=0
suites=31 passed=582 failed=0
test check::tests::an_installed_bundle_is_a_gate_root_and_a_cargo_tree_is_not ... ok
```

### Clippy

```
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.40s
$ cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.43s
```

### `make test-critical`

```
status=0
protected critical tests hold no skip construct
cargo build --locked -p blastit
python -m unittest discover -s python/tests/critical
.....................................................
Ran 53 tests in 9.917s

OK
```

(the staged `_bin/blastit` + `_bin/blastit-dbproxy -> blastit` layout still
passes: the new markers are satisfied by the staged `python/blastit/*.py` too.)

### `make fast-check`

```
status=0
163 "test result: ok" suites; ...
ok   .../target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (critical_handoff)
```

## Follow-ups

* Out of scope here (the node says do not touch the roots): for a staged
  `<dir>/blastit/_bin/blastit`, `exe_dir.join("../../..")` resolves one level
  *above* `<dir>` (three `..` from `_bin`), so that wheel/critical-tier
  candidate can never match; `make test-critical` passes only because it sets
  `PYTHONPATH` itself, and a wheel imports `blastit` from its own site-packages.
  If the wheel root is ever meant to be load-bearing it should be `../..`.
* `bundled_python()` reads `PYTHONPATH` from `prepared.app_env()`, which
  `blastit-env` scrubs — that is the intended secret-free app env, and the
  bundle path is prefixed to it, so nothing to change; recorded for the reader.
