# RESULT: MA.2.3.5 (Leaf 12) — check orchestrator, WSGI gate and python check tests

STATUS: done (expert attempt 1/4: all 5 review findings + 2 real-Django traps resolved, zero regressions)

## Root-cause diagnosis & fixes (`R30` `pytest-django` handshake)

1. **[MAJOR] Real credentials remained in `os.environ` across no-DB or non-DB-first Django suites (`_check_pytest.py`):**
   - **Root cause:** `begin_django_handshake()` populated `os.environ` with `real` at `-p` import time so `settings.py` could parse `DATABASES`, and only scrubbed `os.environ` inside `finish_django_handshake()` (triggered by `django_db_setup`) or `pytest_sessionfinish`. Whenever no collected test requested `db`/`django_db`/`transactional_db` (or a non-DB test ran first), `django_db_setup` had not run, leaving the real DSN in `os.environ` during test module imports and test bodies.
   - **Fix:** Split `DjangoHandshake.restore_env()` from `DjangoHandshake.finish()`. Once `pytest-django` configures Django, `pytest_sessionstart` materialises `settings.DATABASES` and `connections.databases` in memory and immediately scrubs `os.environ` back to `swapped` **before collection imports any test module**; `pytest_collection_finish`, `pytest_runtest_setup`, `finish()`, and `pytest_sessionfinish` also call `restore_env()` idempotently. No test import or test body ever sees `real`, even when `django_db_setup` never runs.
2. **[MAJOR] `pytest-django` teardown failed with `OperationalError('cannot drop the currently open database')` and leaked `test_*` (`_check_pytest.py`):**
   - **Root cause:** `blastit-dbproxy` ignores whatever `database` name a client claims in its PostgreSQL startup packet and connects every session to the `set_db` database (`test_app`). Because `_repoint_django` left `connections.databases` pointing at `dbproxy` through session teardown, Django's `teardown_databases()` opened its maintenance `_nodb_cursor` (`dbname="postgres"`) *through `dbproxy`*, which steered it straight into `test_app` itself, where PostgreSQL refuses `DROP DATABASE "test_app"`.
   - **Fix:** `_repoint_django` deep-copies every matching alias's original `settings_dict` (and scrubs upstream-only `OPTIONS` keys such as `sslmode` while on the local Unix leg). Inside `pytest_fixture_setup`, right after `django_db_setup` yields (`outcome.excinfo is None`), the gate registers `_DJANGO.restore_connections` via `fixturedef.addfinalizer(...)`. Because `fixturedef` runs finalizers **LIFO**, `restore_connections()` runs *after* all dependent per-test `_django_db_helper` rollbacks/flushes finish and *immediately before* `django_db_setup` resumes to run `teardown_databases()`, closing proxied sessions and restoring the real upstream settings so `_nodb_cursor` drops `test_*` cleanly on the real server with zero warnings.
3. **[MINOR] Contract Django twin called `finish_django_handshake()` from the test body (`test_check_contract.py`) & missing real `pytest-django` PG cases (`test_check.py`):**
   - **Fix:** Upgraded `STUB_PYTEST` in `python/tests/critical/test_check_contract.py` to drive the real hook sequence (`pytest_configure` -> `pytest_sessionstart` -> `pytest_collection_finish` -> `pytest_fixture_setup` generator hookwrapper around `django_db_setup` -> `pytest_runtest_setup` -> test body -> LIFO `fixturedef` finalizers -> `pytest_sessionfinish`) with zero manual `finish_django_handshake()` calls in test bodies, covering both DB and no-DB critical twins. Added 5 real `pytest` 9.2 + `Django` 6.1.3 + `pytest-django` 4.14 + PostgreSQL 15 tests to `python/tests/test_check.py` running under `-- -W error::pytest.PytestWarning` and verifying in `pg_database` that `test_*` is dropped.
4. **[MINOR] Fail-closed `_refuse_unhandshaken_django` branch untested:**
   - **Fix:** Called `_refuse_unhandshaken_django()` in both `pytest_configure` and `pytest_sessionstart` (catching `pyproject.toml`/`pytest.ini` `DJANGO_SETTINGS_MODULE` as well as `conftest.py` `settings.configure(...)` without `manage.py` or outer env) and added both an offline protected critical twin and a real `pytest-django` PG twin verifying `ControlError` -> `exit 3`, `stdout == b""`, and zero upstream statements.
5. **[MINOR] `_repoint_django` swallowed exceptions (`_check_pytest.py` + `check.rs`):**
   - **Fix:** `crates/blastit/src/check.rs` now writes explicit `socket_dir`, `port`, and `user` into `django-setup.json` (supporting both `url` and split `host`/`port`/`password` proxy inputs, covered by Rust unit tests), and `_repoint_django` raises `ControlError` whenever `django.db.connections` fails, `close_all()` raises, or zero aliases match `handshake.database` (covered by two protected critical failure-mode twins).
6. **Two real-Django traps caught & fixed while running live against Django 6.1.3 / pytest-django 4.14:**
   - Registered an inert `blastit_manifest` ini stub in `_check_pytest.pytest_addoption` so `0.1.0` projects keeping `blastit_manifest = "blastit.toml"` in `pyproject.toml` do not fail under `-p no:blastit` + `-W error` with `PytestConfigWarning: Unknown config option: blastit_manifest`.
   - Flushed `pytest-django` `TestCase`'s outer `atomic()` deferred `psycopg 3` `BEGIN` before `push_unrouted` in `_patch_django` (`_materialise_test_transactions`) so read-only `@pytest.mark.django_db` `GET` views are not falsely charged with the test harness's `BEGIN`, while views opening their own `transaction.atomic()` still emit `SAVEPOINT` inside the request and still enforce `transaction_control = true` (verified by both allowed and denied twins in `python/tests/test_check.py`).

## Acceptance evidence

This host has no project virtualenv installed, so bare `uv run --no-project pytest ...` cannot spawn `pytest`; the runnable offline form uses the staged package from `make test-critical` (`PYTHONPATH=target/python-critical:python UV_OFFLINE=1 uv run --no-project --with pytest ...`), shown plainly alongside all acceptance commands below.

```console
$ CARGO_NET_OFFLINE=1 cargo build --locked -p blastit-dbproxy -p blastit-py
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```

```console
$ CARGO_NET_OFFLINE=1 cargo test -p blastit --locked
unittests src/lib.rs         test result: ok. 164 passed; 0 failed; 1 ignored
unittests src/main.rs        test result: ok. 0 passed; 0 failed; 0 ignored
critical_check_engine        test result: ok. 13 passed; 0 failed; 0 ignored
critical_config_and_presets  test result: ok. 11 passed; 0 failed; 0 ignored
critical_dev_and_fail_closed test result: ok. 11 passed; 0 failed; 0 ignored
smoke                        test result: ok. 31 passed; 0 failed; 0 ignored
Doc-tests blastit            test result: ok. 0 passed; 0 failed; 0 ignored
```

```console
$ PYTHONPATH=target/python-critical:python UV_OFFLINE=1 uv run --no-project --with pytest pytest python/tests/critical -q
.................................................                        [100%]
49 passed in 3.62s
```

```console
$ PYTHONPATH=target/python-critical:python UV_OFFLINE=1 uv run --no-project --with pytest pytest python/tests/test_check.py -q -rs
ssssssssss                                                               [100%]
SKIPPED [10] python/tests/test_check.py: needs a real PostgreSQL: set DATABASE_URL (or BLASTIT_TEST_PG_URL) to a database the tests may create tables in
10 skipped in 0.02s
```

Live PostgreSQL 15 + `pytest` 9.2.0 + `psycopg` 3.3.9 + `Django` 6.1.3 + `pytest-django` 4.14.0 verification (throwaway container started and removed via `trap ... EXIT`):

```console
$ docker run -d --rm --name blastit-check-pg \
    -e POSTGRES_USER=blastit -e POSTGRES_PASSWORD=test-only-placeholder -e POSTGRES_DB=app \
    -p 127.0.0.1:55432:5432 postgres:15-alpine
$ DATABASE_URL='postgresql://blastit:test-only-placeholder@127.0.0.1:55432/app?sslmode=disable' \
  PYTHONPATH=target/python-critical:python \
  UV_OFFLINE=1 uv run --no-project --with pytest --with psycopg --with django --with pytest-django \
  pytest python/tests/test_check.py -v
python/tests/test_check.py::test_a_clean_suite_is_one_document_with_no_findings PASSED [ 10%]
python/tests/test_check.py::test_a_denied_statement_is_a_finding_and_exit_one PASSED [ 20%]
python/tests/test_check.py::test_a_failing_suite_is_exit_three_with_no_document PASSED [ 30%]
python/tests/test_check.py::test_learn_dry_run_proposes_without_writing PASSED [ 40%]
python/tests/test_check.py::test_learn_writes_only_after_a_passing_suite PASSED [ 50%]
python/tests/test_check.py::test_pytest_django_db_suite_has_zero_teardown_warnings_and_drops_test_db PASSED [ 60%]
python/tests/test_check.py::test_pytest_django_denied_grant_exits_one_and_still_drops_test_db PASSED [ 70%]
python/tests/test_check.py::test_pytest_django_no_db_suite_never_exposes_real_credentials PASSED [ 80%]
python/tests/test_check.py::test_pytest_django_atomic_view_still_enforces_transaction_control PASSED [ 90%]
python/tests/test_check.py::test_pytest_django_unhandshaken_ini_settings_fails_closed PASSED [100%]
============================== 10 passed in 12.30s =============================
```

```console
$ uv run --no-project tools/critical_guard.py --base 865050e --node MA.2.3.5
critical-guard: 865050e..working tree, 13 changed file(s)
node MA.2.3.5: paths ['crates/blastit/src/lib.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/check.rs', 'python/blastit/_check_pytest.py', 'python/blastit/pytest_plugin.py', 'python/tests/test_check.py', 'python/tests/test_live_server.py', 'python/tests/critical/**']
  no change to a protected critical test
```

```console
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
guard: OK
critical-guard: no change to a protected critical test
cargo fmt --all -- --check                       # clean
cargo clippy --workspace --all-targets --locked  # clean (-D warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace # clean
cargo test --workspace --all-targets --locked    # all green
cargo test --workspace --doc --locked            # all green
protected critical tests hold no skip construct
python -m unittest discover -s python/tests/critical
Ran 49 tests in 3.514s — OK
uvx --offline ruff check python tools            # All checks passed!
uvx --offline ruff format --check python tools   # 43 files already formatted
uvx --offline zensical build --strict            # No issues found
uv run --no-project tools/plan.py validate       # plan OK
```

## Follow-ups

1. **Ready-to-append `docs/knowledge/LESSONS.md` bullet (for `MA.3`, which owns `docs/**`; logged in `MA.2.3.5` `LOG.md` because `docs/**` is outside `MA.2.3.5`'s `paths`):**
   - **`pytest-django` + `dbproxy` `set_db` lifecycle traps (`_check_pytest.py`):** (a) Scrub `os.environ` back to `swapped` right after materialising `django.conf.settings.DATABASES` (`pytest_sessionstart` / `pytest_collection_finish`), never waiting for `django_db_setup` (which never runs on no-DB suites); (b) `dbproxy` ignores startup database names and routes every connection to `set_db` (`test_<db>`), so restore the saved real `connections.databases` dicts in a LIFO `fixturedef.addfinalizer` on `django_db_setup` before `teardown_databases()` opens `_nodb_cursor` to `DROP DATABASE test_<db>`; (c) flush `TestCase`'s outer `atomic()` deferred `psycopg 3` `BEGIN` before `push_unrouted` so read-only `@pytest.mark.django_db` views aren't charged with the harness's `BEGIN` (`transaction.atomic()` inside a view still emits `SAVEPOINT`).
2. **Pre-existing `python/tests/test_plugin_helpers.py:25` unbound `psycopg` (for `MA.3`, which owns `python/tests/**`):** Leaf 8 (`865050e`) changed `psycopg = pytest.importorskip("psycopg", ...)` at line 25 to bare `pytest.importorskip(...)`, leaving `psycopg.ProgrammingError` at line 61 (`# noqa: F821 - importorskip binds it`) raising `NameError`. Keep attempt 2's accepted `_privileges()` update at line 582 untouched.
3. **`MA.3` docs sync:** Document the one-shot `0600` Django setup handshake (`DJANGO_SETTINGS_MODULE` or `manage.py` detection; `pytest.ini`-only settings fail closed unless `manage.py` or `DJANGO_SETTINGS_MODULE` is present) alongside `docs/frameworks/django.md` and `docs/ci.md`.
