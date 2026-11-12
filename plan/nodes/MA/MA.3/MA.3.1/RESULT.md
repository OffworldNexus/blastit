# RESULT — MA.3.1 Runnable-docs harness (attempt 3)

STATUS: done

`make docs-test` is green, twice, unprivileged, with zero skipped/xfailed
fences, and `make check` passes end to end. The two obsolete `expect_drift` pins
are gone: both fences now assert the behaviour the page documents (a `queries = []`
route denying with `42501`, and one `dev --learn` pass writing its queries).
The wheel-cache hazard is closed by a per-run `UV_CACHE_DIR` plus a file-by-file
verification of the installed package against the wheel this run built.

## 1. The two pins became positive assertions

`inventory.toml` no longer carries `expect_drift`; the drift machinery
(`Drift`, `DECLARED_DRIFT`, `record_drift`, `Report.drift`, the `run.py`
validation) is deleted, so there is **no key left that can declare either
regression acceptable**. A regression can only turn the fence red.

`docs/local.md` fence 3 (`page-manifest`) now asserts, in one run of the page's
own `queries = []` TOML block:

- the request answers `500` (the page: uncaught denial is a `500`);
- a `WARN sql.denied` line names `products db <phase> SELECT 1`;
- the app receives `SQLSTATE 42501` (the page: PostgreSQL's permission error).
  `fixtures/shop/pgwire.py` now parses the ErrorResponse and raises
  `RuntimeError("SQLSTATE 42501: statement denied for route products")` instead
  of the raw field stream, so the code is readable in the dev log.

`docs/local.md` fence 5 (`dev-learn`) now asserts one SIGINT pass:

- prints `learn: N routes, M queries added → <path>` with `N ≥ 1`, `M ≥ 1`;
- `<path>` resolves to the project's own `blastit.toml` (the page: “prints the
  resolved path”);
- the newly seen `extra` route has non-empty `[route.db].queries` containing the
  statement the app actually issued (`SELECT 1`), not just a route without
  grants.

```console
$ uv run --no-project python python/tests/runnable_docs/run.py docs/local.md
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 0998d1cbef81 · per-run uv cache /tmp/opencode/brd-qkr47ihk/uv-cache · installed package verified against the built wheel
PASS  docs/local.md  fence 1  dev                   0.0s  served on 127.0.0.1:8080
PASS  docs/local.md  fence 2  pgstub                0.0s  substituted: `docker` is not run; the harness's PostgreSQL wire stub answered StartupMessage and SELECT 1 instead
PASS  docs/local.md  fence 3  page-manifest         0.1s  the page's `queries = []` route answered 500 with a `sql.denied` line and SQLSTATE 42501
PASS  docs/local.md  fence 4  learn-git             1.8s  learn wrote the manifest, then check was clean
PASS  docs/local.md  fence 5  dev-learn             0.1s  one pass added 1 route(s) and 1 query(ies), written to the printed manifest
PASS  docs/local.md  fence 6  denial                0.1s  the printed denial matches the documented fields

6 fences: 5 executed, 1 accounted, 0 failed · 2.4s
```

## 2. Wheel-cache hazard closed

Every install (the shared base venv and both `uv add blastit` fences) uses
`UV_CACHE_DIR=<run>/uv-cache`, a directory this run created empty; only
`pytest` reads the global cache, so the wheel can never be served from it.
`verify_wheel_install()` then reads the built wheel's zip and compares every
file (except the installer-rewritten `RECORD`) byte for byte with the installed
venv, failing with *“a wheel cached by an earlier run was installed, not the one
this run built”*.

The build is not reproducible across `make` runs (maturin timestamps), which makes
the proof visible: three consecutive gates built `95271ebf42af`, `83ac6be9ad5e`
and `32d4267c5b7f`, and each run installed and verified its own wheel. The guard
is not decorative:

```console
$ uv run --no-project python /tmp/opencode/probe_cache_guard.py
cache: /tmp/opencode/brd-mbgammeu/uv-cache empty_at_start_requested=True
wheel: blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl 0998d1cbef81
baseline verify: ok
tampered verify: RED as expected -> blastit/__init__.py in the installed environment differs from blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl (sha256 tampered): a wheel cached by an earlie
```

(The probe installs the real wheel, then hands the verifier a same-version copy
with one changed byte, simulating a rebuild the global cache would have hidden.)

## Acceptance

### `make docs-test` run 1

```console
$ make docs-test
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 95271ebf42af · per-run uv cache /tmp/opencode/brd-970ty7uj/uv-cache · installed package verified against the built wheel
PASS  README.md  fence 1  install               0.4s  installed blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
…
PASS  docs/local.md  fence 3  page-manifest         0.2s  the page's `queries = []` route answered 500 with a `sql.denied` line and SQLSTATE 42501
PASS  docs/local.md  fence 5  dev-learn             0.1s  one pass added 1 route(s) and 1 query(ies), written to the printed manifest
…
38 fences: 26 executed, 12 accounted, 0 failed · 21.1s
$ echo $?
0
```

### `make docs-test` run 2 (determinism)

```console
$ make docs-test
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 83ac6be9ad5e · per-run uv cache /tmp/opencode/brd-c9zimr2i/uv-cache · installed package verified against the built wheel
38 fences: 26 executed, 12 accounted, 0 failed · 18.9s
$ echo $?
0
```

The wheel hash and cache path legitimately differ (a new wheel was built; the
cache is run-scoped). The verdicts do not:

```console
$ grep -E '^(PASS|FAIL) ' run1 | sed -E 's/ +[0-9]+\.[0-9]s +/  /' > v1
$ grep -E '^(PASS|FAIL) ' run2 | sed -E 's/ +[0-9]+\.[0-9]s +/  /' > v2
$ diff v1 v2 && echo "VERDICTS IDENTICAL"
VERDICTS IDENTICAL
$ sed -E 's/[0-9]+\.[0-9]s//' run1 | grep -E '^[0-9]+ fences:' > s1
$ sed -E 's/[0-9]+\.[0-9]s//' run2 | grep -E '^[0-9]+ fences:' > s2
$ diff s1 s2 && echo "SUMMARY IDENTICAL (timing stripped)"
SUMMARY IDENTICAL (timing stripped)
```

### No masking construct

```console
$ grep -rnE '\bskip|xfail' python/tests/runnable_docs
$ echo $?
1
```

### Unprivileged

```console
$ id -u
1000
$ test "$(id -u)" -ne 0 && make docs-test > /tmp/opencode/unpriv.out 2>&1
$ echo $?
0
$ tail -1 /tmp/opencode/unpriv.out
38 fences: 26 executed, 12 accounted, 0 failed · 23.5s
```

### `make check`

```console
$ make check
guard: OK (30 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 14 changed file(s)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
…
protected critical tests hold no skip construct
Ran 50 tests in 3.550s
OK
All checks passed!              # ruff check python tools
59 files already formatted      # ruff format --check python tools
No issues found                 # zensical build --strict
scripts/build-wheel.sh
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 32d4267c5b7f · per-run uv cache /tmp/opencode/brd-c6ffb1qp/uv-cache · installed package verified against the built wheel
…
38 fences: 26 executed, 12 accounted, 0 failed · 18.8s
uv run --no-project tools/plan.py validate
plan OK
$ echo $?
0
```

### Inventory stays red on an unaccounted fence

Unchanged from attempt 2; `run.py` still validates the inventory before running
(`README.md: 4 console fences but the inventory names [1, 2, 3]` → exit 1), and
`make check` exercises that validation on every run.

## Accounting

26 fences execute; 12 are accounted with a written reason in `inventory.toml`
and on their verdict line (deployment recipes statically checked, two
third-party installs, the Django migration example, the load-balancer example,
and the Docker PostgreSQL line substituted by the harness wire stub).

## Observations (not this node)

- `make check` passes, but its report-only `critical-guard` step prints
  `crates/blastit/tests/critical_dev_and_fail_closed.rs [VIOLATION: not covered
  by any node path]`. That file belongs to `MA.3.7`'s changes, not this node, and
  the guard is report-only by default (`make check` exit 0).
- No product files, docs, or README were touched. `Makefile` already carried the
  `docs-test` target and its `make check` dependency from attempt 2 and is
  unchanged in this attempt.
