# RESULT: MA.2.3.6 — repair the check-contract suite and psycopg guard

STATUS: done (attempt 1/3; F1+F2 fixed, no assertion deleted, relaxed or skipped)

## Root cause

**F1 — the stub `pytest` package in each project shadowed the host's real one.**
`Project` writes `stub/pytest/{__init__,__main__}.py` and puts `stub` first on the child's
`PYTHONPATH`. The empty `pytest_django.py` stub existed only for the Django twins
(`install_django_stub()`), so in the seven non-Django tests `_check_pytest.pytest_configure`
→ `_refuse_unhandshaken_django()` → `import pytest_django` picked up the **real** installed
plugin, whose `@pytest.fixture` decorators ran against the stub `pytest`
(`AttributeError: module 'pytest' has no attribute 'fixture'`) and turned a 0/1/2 run into
exit 3. Reproduced verbatim before the fix (8 failed, 66 passed) with the exact CI package set
(`pytest`, `psycopg`, `django`, `pytest-django`, pytest 9.2 / Django 6.1.3 / pytest-django 4.14).

**F2 — `importorskip` was used as a statement.** `d291078` (MA.2.3.1) dropped the binding of
`psycopg` at `test_plugin_helpers.py:25` while line 61 still referenced
`psycopg.ProgrammingError`, with a `# noqa: F821 - importorskip binds it` that documented the
wrong thing.

## Fix (2 files, 6 hunks, additive/repair only)

1. `python/tests/critical/test_check_contract.py::Project.__init__` now writes the empty
   `stub/pytest_django.py` for **every** project, Django twin or not, and
   `install_django_stub()` no longer duplicates it (only `django/` + `app_settings` now).
   The dedicated unhandshaken twin is unchanged and still fails closed: the gate refuses
   because `app_settings` configures Django *inside* pytest (`CONTRACT_INI_DJANGO_SETTINGS`
   → `DJANGO_SETTINGS_MODULE` + `_django_settings_configured()`), not because the real
   plugin was importable — so it still proves `ControlError` → exit 3, empty stdout, zero
   upstream statements.
2. `_plugin()` only synthesises a stand-in after `import pytest` itself raises
   `ModuleNotFoundError`, and pops it from `sys.modules` immediately after
   `blastit.pytest_plugin` has bound it. A real pytest session can never see (or keep) a fake
   module.
3. `test_plugin_helpers.py`: `psycopg = pytest.importorskip("psycopg", …)`; the misleading
   `# noqa: F821` is gone.

No assertion was added, removed, relaxed, renamed or skipped; the harness is the only thing
that moved.

## Acceptance evidence

```console
$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest --with psycopg \
    --with django --with pytest-django --offline pytest \
    python/tests/critical/test_check_contract.py python/tests/test_plugin_helpers.py -q
........................................................................ [ 97%]
..                                                                       [100%]
74 passed in 8.23s
$ echo $?
0
```

Same command with `--with pytest-xdist` added (the CI venv installs it) — identical:

```console
74 passed in 8.25s
```

`74 = 21 contract (18 `TestTheContract` + 3 `TestTheUnprivilegedLiveServer`) + 53 plugin
helpers`; the node's `21 + 3` parenthetical counts the live-server class separately.

```console
$ make test-critical
cargo test -p blastit-dbproxy --test critical_learn_and_redaction   # 9 passed
cargo test -p blastit-http --test critical_early_abort              # 5 passed
cargo test -p blastit-ingress --test critical_ingress               # 55 passed
cargo test -p blastit-manifest --test critical_manifest_011         # 17 passed
cargo test -p blastit-manifest --test critical_manifest_edit        # 18 passed
cargo test -p blastit-py --test critical_check_schema               # 6 passed
cargo test -p blastit-scheduler --test critical_ingress_enforcement # 9 passed
cargo test -p blastit-sqlpolicy --test critical_literals            # 25 passed
cargo test -p blastit --test critical_check_engine                  # 13 passed
cargo test -p blastit --test critical_config_and_presets            # 11 passed
cargo test -p blastit --test critical_dev_and_fail_closed           # 11 passed
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
python -m unittest discover -s python/tests/critical
.................................................
----------------------------------------------------------------------
Ran 49 tests in 2.945s

OK
$ echo $?
0
```

(The first `make test-critical` of this session hit the **known F4 flake** —
`learn_attributes_queries_per_route_across_concurrent_live_views`, exit 1 from a late
`ctl.chan: Broken pipe (os error 32)`; that is `MA.2.3.8`'s scope and unrelated to these
files. The rerun above is green.)

Lint/format on the two touched files (the lead's `make check` reruns the whole tree):

```console
$ uvx --offline ruff check python/tests/critical/test_check_contract.py python/tests/test_plugin_helpers.py
All checks passed!
$ uvx --offline ruff format --check python/tests/critical/test_check_contract.py python/tests/test_plugin_helpers.py
2 files already formatted
```

Protected-file authorization:

```console
$ uv run --no-project tools/critical_guard.py --base d73ded3 --node MA.2.3.6
critical-guard: d73ded3..working tree, 6 changed file(s)
node MA.2.3.6: paths ['python/tests/critical/test_check_contract.py', 'python/tests/test_plugin_helpers.py']
  M  python/tests/critical/test_check_contract.py  [authorized by python/tests/critical/test_check_contract.py]
critical-guard: OK: all 1 protected change(s) are authorized by node MA.2.3.6
```

## Oracle evidence (each fix is load-bearing, not cosmetic)

- **F1 fix:** re-ran the suite from a scratch copy of `test_check_contract.py` with only the
  `pytest_django.py` write removed → **12 failed, 9 passed**, the same
  `AttributeError: module 'pytest' has no attribute 'fixture'` (it now also hits the Django
  twins, which read the stub from `install_django_stub()` before). Restoring the line → 74
  passed. Scratch copy deleted; the repo was never mutated for this check.
- **F1 sys.modules guard:** under a real pytest session,
  `_plugin()` leaves `sys.modules["pytest"]` and `pytest_plugin.pytest` bound to the real
  module (`keep real pytest: True`, `plugin bound real pytest: True`,
  `…/site-packages/pytest/__init__.py`). Under `make test-critical` (no pytest installed) the
  stand-in is built, bound and unregistered — which is what keeps the three
  `TestTheUnprivilegedLiveServer` tests importable offline.
- **Fail-closed twin:** `pytest -k "unhandshaken or UnprivilegedLiveServer or
  django_project_without_a_proxy"` → `5 passed`, i.e. exit 3 + `wrote no database handshake`
  + zero upstream statements still hold with the empty stub in place.
- **F2 fix:** the test fails before the change (`NameError: name 'psycopg' is not defined`)
  and asserts a real class after it (`psycopg.ProgrammingError`, raised by
  `_psycopg_converter()` on mixed placeholders).

## Follow-ups

1. **Ready-to-append `docs/knowledge/LESSONS.md` bullet** (`docs/**` is outside this node's
   `paths`): *A stub `pytest` package on a child's `PYTHONPATH` shadows the host's real one,
   so any real plugin imported in that child (e.g. `pytest_django`, whose module body runs
   `@pytest.fixture`) dies with `AttributeError: module 'pytest' has no attribute 'fixture'`.
   Stub the sibling plugins for every project the harness builds, not only the ones that name
   them — and only install a stand-in `pytest` in `sys.modules` when `import pytest` itself
   fails, popping it again before returning.*
2. **`make check` still has no Python standard tier** (`AUDIT-MA.2.3.md` follow-up 1): F2 has
   been latent since `d291078` and only CI's `pytest-plugin` job caught it. A `make py-test`
   would close the loop locally.
3. **`make test-critical` flake (F4, `MA.2.3.8`)** reproduced once in two runs here; no
   change made, by design.
