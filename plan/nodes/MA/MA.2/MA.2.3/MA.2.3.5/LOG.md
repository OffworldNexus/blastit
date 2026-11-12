# Log MA.2.3.5

## 2026-11-12T13:12 [planner]

created (task) Leaf 12: check orchestrator, WSGI gate and python check tests

## 2026-11-12T19:15 [lead]

status ready -> active

## 2026-11-12T19:15 [lead]

attempt 1/3 started

## 2026-11-12T19:15 [lead]

base 865050e8f97402af820e18c71c5aa31aeb48ae55

## 2026-11-12T19:29 [implementer]

check.rs: std::process::Command only inherits descriptors without FD_CLOEXEC (clear it on the gate socketpair end) and swapped_env rewrites away every BLASTIT_* variable, so BLASTIT_CHECK_HTTP_OBS/CTL_FD are added after build_with_socket_dir, never before

## 2026-11-12T19:29 [implementer]

this host's `uv run --no-project` has no pytest: the node's pytest acceptance lines need `--with pytest` plus PYTHONPATH=target/python-critical:python (make test-critical's staging) to be runnable; test_check.py was also run for real against a temporary postgres:15-alpine on 127.0.0.1:55432 with psycopg 3.3.9

## 2026-11-12T19:32 [lead]

status active -> review

## 2026-11-12T19:38 [lead]

REVIEW FAIL (1/3): [BLOCKER] R30 Django path missing: no one-shot 0600 setup file + no {"op":"set_db"} send, so pytest-django migrations hit the dev database; dbproxy set_db exists (MA.2.2.4) so the gap is check.rs/_check_pytest.py. [MAJOR] check.rs:506-548 missing pytest exits 3, R4/§1.1 demand 2. [MINOR] check.rs:143-147 CLOEXEC cleared before dbproxy spawn; no exit-1 twin in contract tests; learn write gate unasserted for failing suite; pytest_plugin.py:1506 dead _live_server_prefix + R39 twins only in standard tier; NODE acceptance lines need PYTHONPATH form. See LOG of reviewer run.

## 2026-11-12T19:38 [lead]

status review -> ready

## 2026-11-12T19:38 [lead]

attempt 2/3 started

## 2026-11-12T19:38 [lead]

retry base 865050e8f97402af820e18c71c5aa31aeb48ae55

## 2026-11-12T19:46 [implementer]

attempt 2: R30 Django handshake implemented — check.rs writes a 0600 django-setup.json (proxy/database/test_db/real/swapped) only when pytest_django imports AND DJANGO_SETTINGS_MODULE or manage.py is present; _check_pytest reads+unlinks it at -p import (before django.setup), applies the real env for django_db_setup, then restores swapped_env, repoints django connections at dbproxy and sends set_db on the pytest_fixture_setup hookwrapper for django_db_setup; pytest_sessionfinish restores the env if no Django fixture ran; pytest_configure fails closed when pytest-django is configured without a handshake

## 2026-11-12T19:46 [implementer]

attempt 2: pytest preflight (python -c import pytest in swapped_env) is exit 2 with 'install pytest: uv add --dev pytest'; FD_CLOEXEC on the suite's control end is cleared only after Dbproxy::start; contract tier gained exit-1 denied-statement, failing-suite --learn byte-identical, missing-pytest exit-2, Django handshake + no-proxy exit-2 twins and the three R39 _live_server_mode twins (fake pytest injected for unittest-only runs)

## 2026-11-12T19:46 [implementer]

attempt 2: host fact (not LESSONS.md, out of paths): the critical tier's only protected test runner is 'PYTHONPATH=target/python-critical:python uv run --no-project python -m unittest discover -s python/tests/critical'; a bare 'uv run --no-project pytest' needs --with pytest on a host without a project venv, and pytest_plugin needs --with psycopg for tests/test_plugin_helpers.py (module-level importorskip)

## 2026-11-12T19:48 [implementer]

attempt 2 done: all five acceptance commands re-run (pytest lines need --with pytest + PYTHONPATH=target/python-critical:python), real PG run 5 passed, critical_guard rc=0, make check rc=0 with 46 critical tests; RESULT.md documents the Django handshake, the one out-of-path test_plugin_helpers edit, and the detection boundary

## 2026-11-12T19:49 [lead]

status ready -> review

## 2026-11-12T19:55 [lead]

REVIEW FAIL (2/3): [MAJOR] _check_pytest.py:290,365-368 real credentials in os.environ for whole session when no DB-using test (no-DB Django body sees real DSN); [MAJOR] _repoint_django:316-337 leaves connections.databases on dbproxy so pytest-django teardown cannot drop test DB (warning, test DB leaks); [MINOR] contract Django twin never runs pytest-django; fail-closed branch untested; _repoint_django swallows exceptions (should ControlError), test_plugin_helpers.py:580 out of path justified. Prior findings 2-7 fixed.

## 2026-11-12T19:55 [lead]

edited fields

## 2026-11-12T19:55 [lead]

edited fields

## 2026-11-12T19:55 [lead]

status review -> active

## 2026-11-12T19:55 [lead]

attempt 1/4 started

## 2026-11-12T19:55 [lead]

escalated to expert; base 865050e8f97402af820e18c71c5aa31aeb48ae55

## 2026-11-12T20:30 [expert]

expert attempt 1: fixed all 5 R30 pytest-django review findings plus 2 real-Django traps found against Django 6.1.3 / pytest-django 4.14 / PG 15: (1) separated restore_env() from finish() and scrub os.environ in pytest_sessionstart/pytest_collection_finish/pytest_runtest_setup after materialising settings.DATABASES so no-DB and mixed suites never expose real credentials to test imports or bodies; (2) save original connections.databases settings_dicts in _repoint_django and register a LIFO fixturedef finalizer on django_db_setup to close proxied sessions and restore real upstream settings before teardown_databases drops test_* (dbproxy ignores startup database claims, so an un-restored _nodb_cursor routed DROP DATABASE into test_* itself); (3) raised ControlError on any _repoint_django failure or unmatched alias and passed explicit socket_dir/port/user in django-setup.json; (4) registered legacy blastit_manifest ini stub in _check_pytest.pytest_addoption and flushed outer TestCase deferred psycopg BEGIN before push_unrouted; (5) upgraded STUB_PYTEST to drive real hooks (49 critical tests green) and added 5 real pytest-django PG tests in test_check.py (10/10 green under -W error::pytest.PytestWarning).

## 2026-11-12T20:31 [lead]

status active -> review

## 2026-11-12T20:37 [lead]

edited fields

## 2026-11-12T20:37 [lead]

status review -> done
