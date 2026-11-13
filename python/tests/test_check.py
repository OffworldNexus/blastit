"""`blastit check` against a real PostgreSQL and a real pytest (PG CI).

This is the standard-tier twin of `python/tests/critical/test_check_contract.py`:
same command, same exit codes, same JSON contract, but the suite the command runs
is a **real** `pytest` session and the database behind the proxy is a real
PostgreSQL. The critical tier cannot assume either is installed, so it stages a
stub pytest and a mock server; here both are the genuine article.

Run it with a database:

```console
$ DATABASE_URL=postgresql://user:pass@127.0.0.1:5432/app uv run pytest python/tests/test_check.py
```

Without `DATABASE_URL` (or `BLASTIT_TEST_PG_URL`), every test below skips **with
the reason on screen** — never silently, and never as a pass. `make check` does
not run this file: its critical tier is offline and unprivileged by design
(§7.1), and PostgreSQL lives in the `pytest-plugin` CI job.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlsplit

import blastit
import pytest

DATABASE_URL = os.environ.get("DATABASE_URL") or os.environ.get("BLASTIT_TEST_PG_URL")

#: The directory `python -m blastit` must import the package from: the installed
#: wheel's `site-packages`, or this repository's staged package (the critical
#: tier builds it). Absolute, because the command runs in a throwaway project.
PACKAGE_ROOT = str(Path(blastit.__file__).resolve().parents[1])
PYTHONPATH = os.pathsep.join(
    dict.fromkeys(
        [PACKAGE_ROOT]
        + [
            os.path.abspath(entry)
            for entry in os.environ.get("PYTHONPATH", "").split(os.pathsep)
            if entry
        ]
    )
)

pytestmark = pytest.mark.skipif(
    not DATABASE_URL,
    reason=(
        "needs a real PostgreSQL: set DATABASE_URL (or BLASTIT_TEST_PG_URL) to a "
        "database the tests may create tables in"
    ),
)

APP = """\
import os

import psycopg


def run(statement):
    with psycopg.connect(os.environ["DATABASE_URL"], autocommit=True) as connection:
        with connection.cursor() as cursor:
            cursor.execute(statement)
            if cursor.description is not None:
                cursor.fetchall()


def application(environ, start_response):
    run("SELECT 1")
    start_response("200 OK", [("Content-Length", "2")])
    return [b"ok"]
"""

MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "home"
path = ['/']
[route.db]
queries = ["SELECT 1"]
"""


#: `MANIFEST` plus Django's own connection-setup statement, for the servers that
#: actually send it: `_django_manifest` picks it per server.
#:
#: `CONN_MAX_AGE = 0` opens the connection *inside* the request, and Django's
#: PostgreSQL backend re-states the session time zone whenever the server's
#: startup `TimeZone` differs from `settings_dict["TIME_ZONE"]` (`None`, so
#: `UTC`): Django 6.1 then sends `SELECT set_config('TimeZone', $1, false)`.
#: Whether that happens is **decided by the server's startup zone spelling**:
#: `postgres:18` reports `Etc/UTC` and sends the statement,
#: `postgres:15-alpine` reports `UTC` and never does. Where it runs it is part
#: of what the route must grant, because in production the same connection is
#: opened inside a request and dbproxy would deny it there; where it never runs
#: a grant for it is a `dead_grant` error, not extra safety. The documented fix
#: is a group attached to every route (the statement runs on whichever route
#: opens the connection first), not one query per route, so the group sits
#: before `[[route]]` and `[route.db]` stays the manifest's last table.
DJANGO_MANIFEST = MANIFEST.replace(
    "\n[[route]]",
    "\n[group.connection.db]\n"
    "queries = [\"SELECT set_config('TimeZone', $1, false)\"]\n"
    "\n[[route]]",
    1,
).replace("version = 1\n", 'version = 1\ngroups = ["connection"]\n', 1)


def project(root: Path, manifest: str = MANIFEST, body: str = "") -> Path:
    """A pytest project: the manifest, the app, and one test that drives it."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "blastit.toml").write_text(manifest, encoding="utf-8")
    (root / "app.py").write_text(APP, encoding="utf-8")
    tests = root / "tests"
    tests.mkdir()
    (tests / "test_app.py").write_text(
        textwrap.dedent(
            """\
            import io

            from blastit import _check_pytest as gate
            from app import application


            def request(method="GET", path="/", body=b"", content_type=None):
                environ = {
                    "REQUEST_METHOD": method,
                    "PATH_INFO": path,
                    "CONTENT_LENGTH": str(len(body)),
                    "wsgi.input": io.BytesIO(body),
                }
                if content_type:
                    environ["CONTENT_TYPE"] = content_type
                status = {}
                payload = b"".join(
                    gate.wrap(application)(
                        environ,
                        lambda code, headers: status.update(code=code),
                    )
                )
                assert status["code"] == "200 OK"
                return payload
            """
        )
        + body,
        encoding="utf-8",
    )
    return root


def run(
    root: Path,
    *arguments: str,
    extra_env: dict[str, str] | None = None,
    unset_env: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[str]:
    """One `blastit check` invocation, in the project's own directory."""
    environment = {
        **os.environ,
        "DATABASE_URL": str(DATABASE_URL),
        "PYTHONPATH": PYTHONPATH,
        **(extra_env or {}),
    }
    for key in unset_env:
        environment.pop(key, None)
    completed = subprocess.run(
        [sys.executable, "-m", "blastit", "check", *arguments],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        timeout=300,
        check=False,
    )
    return completed


def document(completed: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    """The one JSON document on stdout, parsed."""
    assert completed.returncode in (0, 1), completed.stderr
    return json.loads(completed.stdout)


def test_a_clean_suite_is_one_document_with_no_findings(tmp_path: Path) -> None:
    """Exit `0`, one request, one statement, `findings == []` (R4, R38)."""
    root = project(tmp_path, body="def test_home():\n    assert request() == b'ok'\n")
    completed = run(root, "--json")
    assert completed.returncode == 0, completed.stderr
    report = document(completed)
    assert report["ok"] is True, report
    assert report["requests"] == 1, report
    assert report["statements"] >= 1, report
    assert report["findings"] == [], report
    assert report["routes"] == {"total": 1, "exercised": 1}


def test_a_denied_statement_is_a_finding_and_exit_one(tmp_path: Path) -> None:
    """A statement no grant allows fails the run with a bootable fix (R33)."""
    manifest = MANIFEST.replace('queries = ["SELECT 1"]', 'queries = ["SELECT 2"]')
    root = project(
        tmp_path, manifest, body="def test_home():\n    assert request() == b'ok'\n"
    )
    completed = run(root, "--json")
    assert completed.returncode == 1, completed.stderr
    report = document(completed)
    assert report["ok"] is False
    missing = [
        entry
        for entry in report["findings"]
        if entry["kind"] == "missing_grant" and entry["sql"] == "SELECT 1"
    ]
    assert missing, report["findings"]
    assert missing[0]["fix"] == {"action": "add_query", "to": "[route.db].queries"}
    # The grant the manifest did declare is dead too: the suite never sent it.
    assert any(entry["kind"] == "dead_grant" for entry in report["findings"])


def test_a_failing_suite_is_exit_three_with_no_document(tmp_path: Path) -> None:
    """`R4`: pytest failed, so there is no report at all — empty stdout."""
    root = project(
        tmp_path, body="def test_broken():\n    assert False, 'the suite failed'\n"
    )
    completed = run(root, "--json")
    assert completed.returncode == 3, completed.stderr
    assert completed.stdout == ""


def test_learn_dry_run_proposes_without_writing(tmp_path: Path) -> None:
    """`R29`: the proposal is printed, the manifest is untouched."""
    root = project(
        tmp_path, body="def test_extra():\n    assert request(path='/extra') == b'ok'\n"
    )
    before = (root / "blastit.toml").read_text(encoding="utf-8")
    completed = run(root, "--learn", "--dry-run", "--json")
    assert completed.returncode == 0, completed.stderr
    proposal = json.loads(completed.stdout)
    assert proposal["wrote"] is False, proposal
    assert proposal["proposed"]["routes"] >= 1, proposal
    assert (root / "blastit.toml").read_text(encoding="utf-8") == before


def test_learn_writes_only_after_a_passing_suite(tmp_path: Path) -> None:
    """`R29`: a real `--learn` replaces the manifest atomically, once."""
    root = project(
        tmp_path, body="def test_extra():\n    assert request(path='/extra') == b'ok'\n"
    )
    completed = run(root, "--learn", "--json")
    assert completed.returncode == 0, completed.stderr
    proposal = json.loads(completed.stdout)
    assert proposal["wrote"] is True, proposal
    written = (root / "blastit.toml").read_text(encoding="utf-8")
    assert 'name = "extra"' in written, written
    assert "SELECT 1" in written.split('name = "extra"')[1], written
    assert not list(root.glob(".*.tmp")), "the atomic write leaves no temp file"


DJANGO_SETTINGS = """\
import os
from urllib.parse import parse_qs, unquote, urlsplit

_raw = os.environ.get("DATABASE_URL", "")
_url = urlsplit(_raw)
_query = parse_qs(_url.query)

SECRET_KEY = "test-only-not-a-secret"
DEBUG = False
ALLOWED_HOSTS = ["*"]
INSTALLED_APPS = []
MIDDLEWARE = []
ROOT_URLCONF = "djapp.urls"
USE_TZ = True

DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "NAME": unquote(_url.path).lstrip("/"),
        "USER": unquote(_url.username or ""),
        "PASSWORD": unquote(_url.password or ""),
        "HOST": (_query.get("host") or [_url.hostname or ""])[0],
        "PORT": (_query.get("port") or [str(_url.port or 5432)])[0],
        "OPTIONS": {"server_side_binding": True},
        "CONN_MAX_AGE": 0,
    }
}
"""

DJANGO_URLS = """\
from django.db import connection, transaction
from django.http import HttpResponse
from django.urls import path


def db_view(request):
    with connection.cursor() as cursor:
        cursor.execute("SELECT 1")
        cursor.fetchall()
    return HttpResponse(b"ok")


def atomic_view(request):
    with transaction.atomic(), connection.cursor() as cursor:
        cursor.execute("SELECT 1")
        cursor.fetchall()
    return HttpResponse(b"atomic")


def ping_view(request):
    return HttpResponse(b"pong")


urlpatterns = [
    path("", db_view),
    path("atomic/", atomic_view),
    path("ping/", ping_view),
]
"""


def _upstream_db_and_password() -> tuple[str, str]:
    """Extracts `(database_name, password)` from the configured `DATABASE_URL`."""
    parts = urlsplit(str(DATABASE_URL))
    return unquote(parts.path).lstrip("/"), unquote(parts.password or "")


def _django_manifest() -> str:
    """The Django manifest this server needs: `MANIFEST` or `DJANGO_MANIFEST`.

    Django's PostgreSQL backend only re-states the session time zone when the
    server's startup `TimeZone` spelling differs from Django's `UTC`
    (`django/db/backends/postgresql/base.py::_configure_timezone`, called from
    `init_connection_state`), so the statement exists on some images and not on
    others — and a grant that never runs is an audit error in its own right.
    Reading the same `parameter_status` Django reads keeps `findings == []` on
    both `postgres:18` (`Etc/UTC`) and `postgres:15-alpine` (`UTC`).
    """
    import psycopg

    with psycopg.connect(str(DATABASE_URL)) as connection:
        time_zone = connection.info.parameter_status("TimeZone")
    # Anything but an explicit `UTC` makes Django send the statement (`None`
    # included: `None != "UTC"`).
    return MANIFEST if time_zone == "UTC" else DJANGO_MANIFEST


def _database_exists(datname: str) -> bool:
    """Checks `pg_database` directly on the real PostgreSQL server."""
    import psycopg

    with (
        psycopg.connect(str(DATABASE_URL), autocommit=True) as connection,
        connection.cursor() as cursor,
    ):
        cursor.execute("SELECT 1 FROM pg_database WHERE datname = %s", (datname,))
        return cursor.fetchone() is not None


def django_project(
    root: Path,
    *,
    manifest: str = MANIFEST,
    tests_body: str,
    manage_py: bool = True,
    ini_ds: bool = False,
) -> Path:
    """Writes a minimal `pytest-django` project into `root`."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "blastit.toml").write_text(manifest, encoding="utf-8")
    if manage_py:
        (root / "manage.py").write_text("#!/usr/bin/env python\n", encoding="utf-8")
    ini_lines = ["[tool.pytest.ini_options]", 'blastit_manifest = "blastit.toml"']
    if ini_ds:
        ini_lines.append('DJANGO_SETTINGS_MODULE = "djapp.settings"')
    (root / "pyproject.toml").write_text("\n".join(ini_lines) + "\n", encoding="utf-8")
    pkg = root / "djapp"
    pkg.mkdir()
    (pkg / "__init__.py").write_text("", encoding="utf-8")
    (pkg / "settings.py").write_text(DJANGO_SETTINGS, encoding="utf-8")
    (pkg / "urls.py").write_text(DJANGO_URLS, encoding="utf-8")
    tests = root / "tests"
    tests.mkdir()
    (tests / "test_dj.py").write_text(
        textwrap.dedent(tests_body),
        encoding="utf-8",
    )
    return root


def test_pytest_django_db_suite_has_zero_teardown_warnings_and_drops_test_db(
    tmp_path: Path,
) -> None:
    """`R30`: real `pytest-django` creates `test_*`, proxies SQL, and drops `test_*` cleanly."""
    db_name, real_password = _upstream_db_and_password()
    test_db = f"test_{db_name}"
    sentinel_check = (
        f"assert not any({real_password!r} in v for v in os.environ.values())"
        if real_password
        else "assert 'host=' in os.environ['DATABASE_URL']"
    )
    root = django_project(
        tmp_path,
        manifest=_django_manifest(),
        tests_body=f"""\
        import os
        import pytest
        from django.db import connection

        {sentinel_check}


        @pytest.mark.django_db
        def test_db_route(client):
            {sentinel_check}
            assert connection.settings_dict["NAME"] == {test_db!r}
            assert connection.settings_dict["PASSWORD"] == ""
            response = client.get("/")
            assert response.status_code == 200
            assert response.content == b"ok"
        """,
    )
    completed = run(
        root,
        "--json",
        "--",
        "-W",
        "error::pytest.PytestWarning",
        extra_env={"DJANGO_SETTINGS_MODULE": "djapp.settings"},
    )
    assert completed.returncode == 0, f"{completed.stderr}\n{completed.stdout}"
    assert "Error when trying to teardown test databases" not in completed.stderr
    assert "cannot drop the currently open database" not in completed.stderr
    if real_password:
        assert real_password not in completed.stdout
        assert real_password not in completed.stderr
    report = document(completed)
    assert report["ok"] is True, report
    assert report["requests"] == 1, report
    assert report["statements"] >= 1, report
    assert report["findings"] == [], report
    assert not _database_exists(test_db), f"{test_db} was left behind after teardown"


def test_pytest_django_denied_grant_exits_one_and_still_drops_test_db(
    tmp_path: Path,
) -> None:
    """Denied twin of the `pytest-django` DB flow: `missing_grant` (`exit 1`) + clean DB drop."""
    db_name, _ = _upstream_db_and_password()
    test_db = f"test_{db_name}"
    manifest = MANIFEST.replace('queries = ["SELECT 1"]', 'queries = ["SELECT 2"]')
    root = django_project(
        tmp_path,
        manifest=manifest,
        tests_body="""\
        import pytest


        @pytest.mark.django_db
        def test_db_route(client):
            response = client.get("/")
            assert response.status_code == 200
        """,
    )
    completed = run(
        root,
        "--json",
        "--",
        "-W",
        "error::pytest.PytestWarning",
        extra_env={"DJANGO_SETTINGS_MODULE": "djapp.settings"},
    )
    assert completed.returncode == 1, f"{completed.stderr}\n{completed.stdout}"
    assert "Error when trying to teardown test databases" not in completed.stderr
    report = document(completed)
    assert report["ok"] is False
    assert any(
        entry["kind"] == "missing_grant" and entry["sql"] == "SELECT 1"
        for entry in report["findings"]
    ), report["findings"]
    assert not _database_exists(test_db), f"{test_db} was left behind after teardown"


def test_pytest_django_no_db_suite_never_exposes_real_credentials(
    tmp_path: Path,
) -> None:
    """No-DB `pytest-django` twin: `django_db_setup` never runs, yet imports/bodies see only `swapped`."""
    db_name, real_password = _upstream_db_and_password()
    test_db = f"test_{db_name}"
    sentinel_check = (
        f"assert not any({real_password!r} in v for v in os.environ.values())"
        if real_password
        else "assert True"
    )
    ping_manifest = MANIFEST.replace("path = ['/']", "path = ['/ping/']")
    root = django_project(
        tmp_path,
        manifest=ping_manifest,
        tests_body=f"""\
        import os

        {sentinel_check}
        assert "host=" in os.environ["DATABASE_URL"]
        assert "127.0.0.1" not in os.environ["DATABASE_URL"]


        def test_ping_without_db(client):
            {sentinel_check}
            assert "host=" in os.environ["DATABASE_URL"]
            assert "127.0.0.1" not in os.environ["DATABASE_URL"]
            response = client.get("/ping/")
            assert response.status_code == 200
            assert response.content == b"pong"
        """,
    )
    completed = run(
        root,
        "--json",
        "--",
        "-W",
        "error::pytest.PytestWarning",
        extra_env={"DJANGO_SETTINGS_MODULE": "djapp.settings"},
    )
    # pytest passed (`exit 0`, so every secret-absence assertion held); the
    # audit exits `1` because `[proxy.db]` observed `0` statements.
    assert completed.returncode == 1, completed.stderr
    if real_password:
        assert real_password not in completed.stdout
        assert real_password not in completed.stderr
    report = document(completed)
    assert report["requests"] == 1, report
    assert report["statements"] == 0, report
    assert any(entry["kind"] == "proxy_unused" for entry in report["findings"])
    assert not _database_exists(test_db)


def test_pytest_django_atomic_view_still_enforces_transaction_control(
    tmp_path: Path,
) -> None:
    """Flushing `TestCase`'s outer `BEGIN` never masks a view's own `transaction.atomic()`."""
    db_name, _ = _upstream_db_and_password()
    test_db = f"test_{db_name}"
    atomic_manifest = _django_manifest().replace("path = ['/']", "path = ['/atomic/']")
    denied_root = django_project(
        tmp_path / "denied",
        manifest=atomic_manifest,
        tests_body="""\
        import pytest


        @pytest.mark.django_db
        def test_atomic(client):
            assert client.get("/atomic/").status_code == 200
        """,
    )
    denied = run(
        denied_root,
        "--json",
        "--",
        "-W",
        "error::pytest.PytestWarning",
        extra_env={"DJANGO_SETTINGS_MODULE": "djapp.settings"},
    )
    assert denied.returncode == 1, f"{denied.stderr}\n{denied.stdout}"
    denied_report = document(denied)
    assert any(
        entry["kind"] == "missing_grant" and entry["sql"] == "SAVEPOINT"
        for entry in denied_report["findings"]
    ), denied_report["findings"]
    assert not _database_exists(test_db)

    granted_manifest = atomic_manifest + "transaction_control = true\n"
    granted_root = django_project(
        tmp_path / "granted",
        manifest=granted_manifest,
        tests_body="""\
        import pytest


        @pytest.mark.django_db
        def test_atomic(client):
            assert client.get("/atomic/").status_code == 200
        """,
    )
    granted = run(
        granted_root,
        "--json",
        "--",
        "-W",
        "error::pytest.PytestWarning",
        extra_env={"DJANGO_SETTINGS_MODULE": "djapp.settings"},
    )
    assert granted.returncode == 0, f"{granted.stderr}\n{granted.stdout}"
    assert document(granted)["findings"] == []
    assert not _database_exists(test_db)


def test_pytest_django_unhandshaken_ini_settings_fails_closed(
    tmp_path: Path,
) -> None:
    """Fail-closed twin: `pytest-django` configured via `pyproject.toml` without `manage.py` or env -> `exit 3`."""
    db_name, _ = _upstream_db_and_password()
    test_db = f"test_{db_name}"
    root = django_project(
        tmp_path,
        manage_py=False,
        ini_ds=True,
        tests_body="""\
        import pytest


        @pytest.mark.django_db
        def test_never_runs(client):
            client.get("/")
        """,
    )
    completed = run(
        root,
        "--json",
        unset_env=("DJANGO_SETTINGS_MODULE",),
    )
    assert completed.returncode == 3, completed.stderr
    assert completed.stdout == ""
    assert "wrote no database handshake" in completed.stderr
    assert not _database_exists(test_db)
