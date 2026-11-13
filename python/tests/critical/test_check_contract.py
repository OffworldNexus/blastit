"""Protected critical tier: `blastit check`'s wire contract (R3..R5, R29, R30, R39).

`blastit check` is one transaction with four observable promises, and each of
them is a security boundary here rather than a formatting preference:

* **`stdout` is a contract.** Under `--json` it carries at most one document, and
  **zero bytes** on exit `2` or `3` — even though the suite prints and writes to
  file descriptor 1 itself. That is enforced by the orchestrator at the OS level
  (`Stdio`), so a `print()` in a test cannot corrupt what an agent parses.
* **Exit `2` is "Blastit could not start".** An invalid manifest, a missing
  pytest and a corrupt `http_obs.jsonl` are all that, and none may emit a
  report: half-audited (or never-run) is worse than no answer.
* **Exit `1` is "the manifest is incomplete".** A statement the manifest does not
  grant is one document with `missing_grant` and the grant nobody used beside
  it — never a failed suite.
* **Exit `3` is "the suite failed", including pytest's `5`** ("no tests
  collected"). It must never be confused with `1` ("findings"), or a broken suite
  would look like a complete manifest. `--learn` writes nothing on that path.
* **The credentials never enter the suite.** The non-Django `pytest` process gets
  the secret-free `swapped_env`; a `pytest-django` suite gets the real inputs
  only in a one-shot `0600` file read and unlinked before Django builds its
  settings, and the gate restores the secret-free values (`R30`, `ADR-0004`).
* **R39**: `_live_server_mode` falls back to unprivileged `blastit dev`, never to
  a skip, and still chooses production when it can.

Everything runs offline and unprivileged: a **stub `pytest`** (a 30-line module
this file writes into the project) stands in for the real one so the critical
tier needs no pytest installation; a **`MockPgServer`** speaks just enough
PostgreSQL for dbproxy's upstream client; and a small Unix-socket client speaks
just enough for the front end. The real binaries under test are the ones the
wheel ships: `blastit._blastit` and `blastit/_bin/blastit-dbproxy`, staged by
`make test-critical`.

The suite the command runs is driven through the gate by hand
(`blastit._check_pytest.wrap`), which is exactly what a raw-WSGI project does:
there is no framework seam to patch for "a WSGI callable".
"""

from __future__ import annotations

import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import types
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

#: The value planted in `DATABASE_URL` and searched for everywhere a suite can
#: see. It must never appear in the suite's environment.
PASSWORD = "contract-password-9f3c1a"

#: The proxy's real user, which the mock upstream records.
USER = "contract-user"

PROTOCOL_VERSION = 196_608
SSL_REQUEST = 80_877_103


def _package() -> Path:
    """The installed `blastit` package directory (the staged wheel, in this tier)."""
    import blastit

    return Path(blastit.__file__).resolve().parent


def _package_root() -> Path:
    """The directory `python -m blastit` and `import blastit` need on `PYTHONPATH`."""
    return _package().parent


def _dbproxy() -> Path:
    """The Python-free `blastit-dbproxy` binary the wheel ships."""
    return _package() / "_bin" / "blastit-dbproxy"


class MockPgServer:
    """A PostgreSQL good enough for dbproxy's upstream client (see `mock_pg.rs`).

    Trust authentication, the simple and extended protocol answered with a
    canned `SELECT 0`, and every statement recorded. Binds `127.0.0.1:0`.
    """

    def __init__(self) -> None:
        self.statements: list[str] = []
        self.startups: list[dict[str, str]] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind(("127.0.0.1", 0))
        self._listener.listen(8)
        self.port = self._listener.getsockname()[1]
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

    def _serve(self) -> None:
        self._listener.settimeout(0.05)
        while not self._stop.is_set():
            try:
                connection, _ = self._listener.accept()
            except TimeoutError:
                continue
            except OSError:
                return
            threading.Thread(
                target=self._session, args=(connection,), daemon=True
            ).start()

    def _session(self, connection: socket.socket) -> None:
        with connection:
            connection.settimeout(10)
            try:
                while True:
                    header = _read_exactly(connection, 4)
                    if header is None:
                        return
                    length = struct.unpack("!I", header)[0]
                    if not 4 <= length <= 16 * 1024:
                        return
                    payload = _read_exactly(connection, length - 4) or b""
                    protocol = struct.unpack("!I", payload[:4])[0]
                    if protocol == SSL_REQUEST:
                        connection.sendall(b"N")
                        continue
                    if protocol != PROTOCOL_VERSION:
                        return
                    startup = _parameters(payload[4:])
                    with self._lock:
                        self.startups.append(startup)
                    # Trust: identity was the unix socket the client reached.
                    connection.sendall(_frame(b"R", struct.pack("!I", 0)))
                    for name, value in [
                        ("client_encoding", "UTF8"),
                        ("standard_conforming_strings", "on"),
                        ("server_version", "16.15"),
                    ]:
                        connection.sendall(
                            _frame(b"S", name.encode() + b"\0" + value.encode() + b"\0")
                        )
                    connection.sendall(_frame(b"K", struct.pack("!II", 1, 2)))
                    connection.sendall(_frame(b"Z", b"I"))
                    self._messages(connection)
                    return
            except OSError:
                return

    def _messages(self, connection: socket.socket) -> None:
        while True:
            header = _read_exactly(connection, 5)
            if header is None:
                return
            tag = header[:1]
            length = struct.unpack("!I", header[1:])[0]
            body = _read_exactly(connection, length - 4) or b""
            if tag == b"Q":
                with self._lock:
                    self.statements.append(
                        body.rstrip(b"\0").decode("utf-8", "replace")
                    )
                connection.sendall(_frame(b"C", b"SELECT 0\0"))
                connection.sendall(_frame(b"Z", b"I"))
            elif tag == b"P":
                statement = body.split(b"\0")[1] if b"\0" in body else b""
                with self._lock:
                    self.statements.append(statement.decode("utf-8", "replace"))
                connection.sendall(_frame(b"1", b""))
            elif tag == b"B":
                connection.sendall(_frame(b"2", b""))
            elif tag == b"D":
                connection.sendall(_frame(b"n", b""))
            elif tag == b"E":
                connection.sendall(_frame(b"C", b"SELECT 0\0"))
            elif tag == b"C":
                connection.sendall(_frame(b"3", b""))
            elif tag == b"S":
                connection.sendall(_frame(b"Z", b"I"))
            elif tag == b"X":
                return

    def close(self) -> None:
        self._stop.set()
        self._listener.close()
        self._thread.join(timeout=5)


def _read_exactly(stream: socket.socket, count: int) -> bytes | None:
    """Reads exactly `count` bytes, or `None` at end of stream."""
    out = b""
    while len(out) < count:
        chunk = stream.recv(count - len(out))
        if not chunk:
            return None
        out += chunk
    return out


def _parameters(payload: bytes) -> dict[str, str]:
    """The startup packet's parameter pairs."""
    parameters: dict[str, str] = {}
    fields = payload.split(b"\0")
    index = 0
    while index + 1 < len(fields) and fields[index]:
        parameters[fields[index].decode("utf-8", "replace")] = fields[index + 1].decode(
            "utf-8", "replace"
        )
        index += 2
    return parameters


def _frame(tag: bytes, body: bytes) -> bytes:
    """One protocol frame."""
    return tag + struct.pack("!I", len(body) + 4) + body


class _Completed:
    """The one attribute `subprocess.run`'s result is read for."""

    def __init__(self, returncode: int) -> None:
        self.returncode = returncode


def _plugin() -> Any:
    """`blastit.pytest_plugin`, with a stand-in pytest when the tier has none.

    The plugin is imported by pytest itself in a project, so its `import pytest`
    is its host, not a dependency. `make test-critical` runs under plain
    `unittest`, where the stand-in supplies the one decorator the module uses at
    import time (`fixture`). When real pytest is the runner, it is used as is —
    the stand-in is only built when `import pytest` itself fails, and it is
    removed from `sys.modules` again right after the plugin has bound it, so a
    later import in the same session (the subprocess helper is not the only
    reader) can never see a fake module instead of the real runner.
    """
    synthesised = False
    try:
        import pytest  # noqa: F401 - the host, when the tier has a real one
    except ModuleNotFoundError:
        fake = types.ModuleType("pytest")

        def fixture(*args: Any, **kwargs: Any) -> Any:
            if args and callable(args[0]):
                return args[0]
            return lambda function: function

        fake.fixture = fixture  # type: ignore[attr-defined]
        sys.modules["pytest"] = fake
        synthesised = True
    try:
        from blastit import pytest_plugin
    finally:
        if synthesised:
            sys.modules.pop("pytest", None)

    return pytest_plugin


#: The mini-runner the audited project runs: it drives the WSGI callable through
#: the gate exactly as a raw-WSGI suite does, and records what it saw for the
#: outer assertions. Twenty-five lines of it are the gate's client contract.
MINI_RUNNER = """\
import json, os, sys
from blastit import _check_pytest as gate
from app import body

application = gate.wrap(body())
request = {"REQUEST_METHOD": "GET", "PATH_INFO": "/", "wsgi.input": None,
           "CONTENT_LENGTH": "0"}
status = {}
def start_response(code, headers):
    status["code"] = code
response = b"".join(application(request, start_response))
assert status["code"] == "200 OK", status
assert response == b"SELECT 1", response
os.write(1, b"noise\\n")
print("noise from print")
lines = open(os.environ["BLASTIT_CHECK_HTTP_OBS"], encoding="utf-8").read().splitlines()
assert len(lines) == 1, lines
entry = json.loads(lines[0])
assert (entry["method"], entry["path"]) == ("GET", "/"), entry
sys.stdout.write(json.dumps({"observed": entry["path"]}) + "\\n")
"""

MINI_APP = '''\
import os
from urllib.parse import parse_qs, urlsplit
import socket, struct

from blastit import _check_pytest as gate


def query():
    parts = urlsplit(os.environ["DATABASE_URL"])
    query = parse_qs(parts.query)
    path = os.path.join(query["host"][0], ".s.PGSQL." + query["port"][0])
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.connect(path)
        payload = struct.pack("!I", 196608) + b"user\\0blastit\\0database\\0app\\0\\0"
        client.sendall(struct.pack("!I", len(payload) + 4) + payload)
        _ready(client)
        message = b"SELECT 1\\0"
        client.sendall(b"Q" + struct.pack("!I", len(message) + 4) + message)
        _ready(client)


def _ready(client):
    while True:
        header = client.recv(5)
        assert len(header) == 5, header
        length = struct.unpack("!I", header[1:])[0]
        body = b""
        while len(body) < length - 4:
            body += client.recv(length - 4 - len(body))
        if header[:1] == b"E":
            raise AssertionError(body)
        if header[:1] == b"Z":
            return


def handle():
    query()
    return b"SELECT 1"


class FlaskLike:
    """Flask's shape: the callable a server reaches is `.wsgi_app`."""

    @staticmethod
    def wsgi_app(environ, start_response):
        return [handle()]


def body():
    """A framework-like layer: the request is served by `.wsgi_app`, gated again.

    The second `wrap` is the re-entrancy twin: it runs inside the first request,
    so the ContextVar guard must keep it to one observation.
    """

    def application(environ, start_response):
        deep = gate.wrap(FlaskLike.wsgi_app)
        payload = b"".join(deep(environ, start_response))
        start_response("200 OK", [("Content-Length", str(len(payload)))])
        return [payload]

    return application
'''


#: `MINI_APP`'s socket round-trip, sending one statement instead of `SELECT 1`.
#: `__STATEMENT__` is the SQL literal a `--learn` run observes, so the same app
#: serves the inline-literal twin and the parameterised one.
LEARN_APP = """\
import os
import socket
import struct
from urllib.parse import parse_qs, urlsplit

STATEMENT = __STATEMENT__


def query():
    parts = urlsplit(os.environ["DATABASE_URL"])
    query = parse_qs(parts.query)
    path = os.path.join(query["host"][0], ".s.PGSQL." + query["port"][0])
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.connect(path)
        payload = struct.pack("!I", 196608) + b"user\\0blastit\\0database\\0app\\0\\0"
        client.sendall(struct.pack("!I", len(payload) + 4) + payload)
        _ready(client)
        message = STATEMENT.encode() + b"\\0"
        client.sendall(b"Q" + struct.pack("!I", len(message) + 4) + message)
        _ready(client)


def _ready(client):
    while True:
        header = client.recv(5)
        assert len(header) == 5, header
        length = struct.unpack("!I", header[1:])[0]
        body = b""
        while len(body) < length - 4:
            body += client.recv(length - 4 - len(body))
        if header[:1] == b"E":
            raise AssertionError(body)
        if header[:1] == b"Z":
            return


def application(environ, start_response):
    query()
    payload = b"done"
    start_response("200 OK", [("Content-Length", str(len(payload)))])
    return [payload]
"""


def _learn_app(statement: str) -> str:
    """`LEARN_APP` sending `statement`, whatever the SQL under test is."""
    return LEARN_APP.replace("__STATEMENT__", repr(statement))


#: One gated request for a path no manifest route covers (`/notes`), so the
#: learn plan has a route to propose beside the statement it may or may not
#: grant. It writes nothing to stdout: the run's document is the only one.
LEARN_RUNNER = """\
import os
from blastit import _check_pytest as gate
from app import application

gated = gate.wrap(application)
request = {"REQUEST_METHOD": "GET", "PATH_INFO": "/notes", "wsgi.input": None,
           "CONTENT_LENGTH": "0"}
status = {}
def start_response(code, headers):
    status["code"] = code
payload = b"".join(gated(request, start_response))
assert status["code"] == "200 OK", status
assert payload == b"done", payload
lines = open(os.environ["BLASTIT_CHECK_HTTP_OBS"], encoding="utf-8").read().splitlines()
assert len(lines) == 1, lines
"""

#: Minimal stub `django` + `app_settings` modules for offline contract tests.
STUB_DJANGO_CONF = """\
class _Settings:
    configured = False
    DATABASES = {}

    def configure(self, **kwargs):
        for key, value in kwargs.items():
            setattr(self, key, value)
        self.configured = True


settings = _Settings()
"""

STUB_DJANGO_DB = """\
from django.conf import settings


class _Connections:
    def __init__(self):
        self.close_calls = 0

    @property
    def databases(self):
        return settings.DATABASES

    def close_all(self):
        if getattr(self, "fail_close", False):
            raise RuntimeError("close_all boom")
        self.close_calls += 1


connections = _Connections()
"""

STUB_APP_SETTINGS = f"""\
import os
from urllib.parse import urlsplit
from django.conf import settings

_raw = os.environ.get("DATABASE_URL", "")
SETTINGS_SAW_REAL = {PASSWORD!r} in _raw
_parts = urlsplit(_raw)
settings.configure(
    DATABASES={{
        "default": {{
            "ENGINE": "django.db.backends.postgresql",
            "NAME": _parts.path.lstrip("/") or "app",
            "USER": _parts.username or "",
            "PASSWORD": _parts.password or "",
            "HOST": _parts.hostname or "",
            "PORT": str(_parts.port or 5432),
            "OPTIONS": {{"sslmode": "require", "server_side_binding": True}},
        }}
    }}
)
"""

STUB_PYTEST_INIT = """\
def hookimpl(*args, **kwargs):
    if args and callable(args[0]):
        return args[0]
    return lambda function: function
"""

#: Stand-in for pytest: honours `-p no:blastit -p blastit._check_pytest`, drives
#: pytest's real hook sequence (`pytest_configure` -> `pytest_sessionstart` ->
#: `pytest_collection_finish` -> optional `pytest_fixture_setup` around
#: `django_db_setup` -> `pytest_runtest_setup` -> test body -> LIFO finalizers
#: -> `pytest_sessionfinish`), and exits with pytest's status codes.
STUB_PYTEST = f'''\
"""A stub pytest for the offline contract test (no pytest is installed here)."""
import importlib, os, runpy, sys

argv = sys.argv[1:]
plugins = [argv[index + 1] for index, word in enumerate(argv[:-1]) if word == "-p"]
assert "-q" in argv, argv
assert "no:blastit" in plugins, plugins
assert "blastit._check_pytest" in plugins, plugins
# `-p no:blastit` blocks the 0.1.0 pytest11 entry point the project's
# pyproject.toml declares; a run that lost it would import the legacy plugin.
if "no:blastit" not in plugins:
    import blastit.pytest_plugin  # noqa: F401
else:
    assert "blastit.pytest_plugin" not in sys.modules, "the 0.1.0 plugin was loaded"
from blastit import _check_pytest


class _FixtureDef:
    def __init__(self, argname):
        self.argname = argname
        self.finalizers = []

    def addfinalizer(self, callback):
        self.finalizers.append(callback)


def _setup_django_from_ini_or_env():
    ini_ds = os.environ.get("CONTRACT_INI_DJANGO_SETTINGS")
    if ini_ds:
        os.environ["DJANGO_SETTINGS_MODULE"] = ini_ds
    ds = os.environ.get("DJANGO_SETTINGS_MODULE")
    if not ds:
        return
    try:
        import pytest_django  # noqa: F401
    except ImportError:
        return
    importlib.import_module(ds)


def _run_django_db_setup(fixturedef):
    from django.db import connections

    default = connections.databases["default"]
    assert default["HOST"] == "127.0.0.1", default
    assert default["PASSWORD"] == {PASSWORD!r}, "setup_databases needs real auth"
    expected_name = os.environ.get("CONTRACT_TEST_DB_NAME", "test_app")
    default["NAME"] = expected_name
    if os.environ.get("CONTRACT_FAIL_CLOSE_ALL") == "1":
        connections.fail_close = True

    def teardown_databases():
        restored = connections.databases["default"]
        assert restored["HOST"] == "127.0.0.1", restored
        assert restored["PASSWORD"] == {PASSWORD!r}, "teardown needs real auth back"
        assert restored["OPTIONS"].get("sslmode") == "require", restored
        assert restored["NAME"] == expected_name, restored
        if expected_name == "test_app" and not getattr(connections, "fail_close", False):
            assert connections.close_calls >= 2, connections.close_calls
            sys.stderr.write("TEARDOWN_RESTORED_OK\\n")

    fixturedef.addfinalizer(teardown_databases)


exitstatus = 0
fixturedef = None
try:
    _setup_django_from_ini_or_env()
    _check_pytest.pytest_configure(None)
    _check_pytest.pytest_sessionstart(None)
    _check_pytest.pytest_collection_finish(None)
    scripted = os.environ.get("CONTRACT_EXIT")
    if scripted:
        sys.exit(int(scripted))
    if os.environ.get("CONTRACT_DJANGO_DB") == "1":
        fixturedef = _FixtureDef("django_db_setup")
        wrapper = _check_pytest.pytest_fixture_setup(fixturedef, None)
        next(wrapper)
        _run_django_db_setup(fixturedef)
        try:
            wrapper.send(None)
        except StopIteration:
            pass
    _check_pytest.pytest_runtest_setup(None)
    runpy.run_path(os.environ["CONTRACT_MINITEST"], run_name="__main__")
except SystemExit as stopped:
    exitstatus = int(stopped.code or 0)
    raise
except BaseException:
    exitstatus = 1
    import traceback

    traceback.print_exc()
    sys.exit(1)
finally:
    if fixturedef is not None:
        while fixturedef.finalizers:
            fixturedef.finalizers.pop()()
    _check_pytest.pytest_sessionfinish(None, exitstatus)
sys.exit(0)
'''

PROJECT_MANIFEST = """\
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

#: The manifest that grants the statement the app never sends (`exit 1`).
DEGRADED_MANIFEST = PROJECT_MANIFEST.replace(
    'queries = ["SELECT 1"]', 'queries = ["SELECT 2"]'
)

#: Django DB runner: never calls `finish_django_handshake()` itself — the real
#: `pytest_fixture_setup` hookwrapper must trigger on `django_db_setup`, scrub
#: `os.environ`, repoint `connections.databases` at dbproxy, and steer the next
#: session to `test_app` before any test body line executes (R30, ADR-0004).
DJANGO_DB_RUNNER = (
    "import os\n"
    "import app_settings\n"
    "from django.db import connections\n"
    "from blastit import _check_pytest as gate\n"
    "assert app_settings.SETTINGS_SAW_REAL is True, "
    "'settings.py had to see the real inputs during initial setup'\n"
    "record = gate.last_django_handshake()\n"
    "assert record is not None, 'a handshake ran'\n"
    "assert record['mode'] & 0o777 == 0o600, record\n"
    "assert record['unlinked'] is True, record\n"
    "assert record['proxy'] == 'db', record\n"
    "assert record['database'] == 'test_app', record\n"
    "assert record['real_variables'] == ['DATABASE_URL'], record\n"
    "assert record['env_restored'] is True, record\n"
    "assert record['finished'] is True, record\n"
    "assert record['restored_connections'] is False, record\n"
    f"assert not any({PASSWORD!r} in value for value in os.environ.values()), "
    "'the real password outlived the handshake'\n"
    "assert 'host=' in os.environ['DATABASE_URL'], os.environ['DATABASE_URL']\n"
    "default = connections.databases['default']\n"
    "assert default['PASSWORD'] == '', default\n"
    "assert '127.0.0.1' not in default['HOST'], default\n"
    "assert default['OPTIONS'] == {'server_side_binding': True}, default\n"
) + MINI_RUNNER

#: Django NO-DB runner: zero DB fixtures run and `finish_django_handshake()` is
#: never called, yet `os.environ` must already hold `swapped` before the test
#: body starts.
DJANGO_NO_DB_RUNNER = (
    "import os, sys\n"
    "import app_settings\n"
    "from blastit import _check_pytest as gate\n"
    "assert app_settings.SETTINGS_SAW_REAL is True, "
    "'settings.py saw the real inputs before collection scrubbed os.environ'\n"
    f"assert not any({PASSWORD!r} in value for value in os.environ.values()), "
    "'a no-DB Django test body must never see the real password'\n"
    "url = os.environ['DATABASE_URL']\n"
    "assert 'host=' in url and '127.0.0.1' not in url, url\n"
    "record = gate.last_django_handshake()\n"
    "assert record is not None and record['unlinked'] is True, record\n"
    "assert record['env_restored'] is True, record\n"
    "assert record['finished'] is False, 'django_db_setup never ran'\n"
    "ping = gate.wrap(lambda env, start: (start('200 OK', []), [b'pong'])[1])\n"
    "payload = b''.join(ping({'REQUEST_METHOD': 'GET', 'PATH_INFO': '/', "
    "'wsgi.input': None, 'CONTENT_LENGTH': '0'}, lambda code, headers: None))\n"
    "assert payload == b'pong', payload\n"
    "sys.stderr.write('NO_DB_BODY_SECRET_FREE\\n')\n"
)

UPLOAD_MANIFEST = """\
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "upload"
path = ['/upload']
[route.ingress]
body = "raw"
allow_raw = true
[route.db]
queries = ["SELECT 1"]
"""


class Project:
    """One throwaway project, with the stub `pytest` and the mini-runner."""

    def __init__(self, manifest: str, runner: str = MINI_RUNNER) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        (self.root / "blastit.toml").write_text(manifest, encoding="utf-8")
        (self.root / "app.py").write_text(MINI_APP, encoding="utf-8")
        tests = self.root / "tests"
        tests.mkdir()
        (tests / "test_contract.py").write_text(runner, encoding="utf-8")
        (tests / "pyproject.toml").write_text(
            '[tool.pytest.ini_options]\nblastit_manifest = "blastit.toml"\n',
            encoding="utf-8",
        )
        stub = self.root / "stub" / "pytest"
        stub.mkdir(parents=True)
        (stub / "__init__.py").write_text(STUB_PYTEST_INIT, encoding="utf-8")
        (stub / "__main__.py").write_text(STUB_PYTEST, encoding="utf-8")
        # The stub `pytest` package shadows the host's real one, so a real
        # `pytest_django` imported in the child would run its `@pytest.fixture`
        # decorators against a module that has no `fixture` and blow up before
        # the gate ever sees an exit code. Every project therefore carries an
        # empty `pytest_django.py`: `_refuse_unhandshaken_django()`'s "is this a
        # Django suite" probe is then deterministic offline, and the dedicated
        # unhandshaken twin still fails closed (it is `app_settings` configuring
        # Django *inside* pytest, not the presence of the real plugin, that the
        # gate must refuse).
        (self.root / "stub" / "pytest_django.py").write_text("", encoding="utf-8")

    def install_django_stub(self) -> None:
        """Installs minimal offline `django` and `app_settings` modules."""
        stub_root = self.root / "stub"
        django_pkg = stub_root / "django"
        django_pkg.mkdir(parents=True, exist_ok=True)
        (django_pkg / "__init__.py").write_text("", encoding="utf-8")
        (django_pkg / "conf.py").write_text(STUB_DJANGO_CONF, encoding="utf-8")
        (django_pkg / "db.py").write_text(STUB_DJANGO_DB, encoding="utf-8")
        (stub_root / "app_settings.py").write_text(STUB_APP_SETTINGS, encoding="utf-8")

    def cleanup(self) -> None:
        self._directory.cleanup()


class CheckRun:
    """One `blastit check` invocation and what it wrote."""

    def __init__(self, completed: subprocess.CompletedProcess[bytes]) -> None:
        self.returncode = completed.returncode
        self.stdout = completed.stdout
        self.stderr = completed.stderr

    def document(self) -> dict[str, Any]:
        """The one JSON document on stdout, parsed."""
        text = self.stdout.decode("utf-8")
        assert text.strip(), f"stdout was empty; stderr:\n{self.stderr.decode()}"
        return json.loads(text)


class TestTheContract(unittest.TestCase):
    """The six promises, each with its allowed and its denied twin."""

    @classmethod
    def setUpClass(cls) -> None:
        # The staged wheel is what this tier tests; a source checkout without it
        # cannot run the contract, and saying so is better than a false pass.
        assert _dbproxy().is_file(), (
            f"{_dbproxy()} is missing: run `make test-critical`, which builds and "
            "stages blastit._blastit and blastit/_bin/blastit-dbproxy"
        )
        import blastit._blastit  # noqa: F401

    def server(self) -> MockPgServer:
        server = MockPgServer()
        self.addCleanup(server.close)
        return server

    def project(self, manifest: str = PROJECT_MANIFEST, **kwargs: Any) -> Project:
        project = Project(manifest, **kwargs)
        self.addCleanup(project.cleanup)
        return project

    def run_check(
        self,
        project: Project,
        *arguments: str,
        server: MockPgServer,
        env: dict[str, str] | None = None,
        runner: str = "tests/test_contract.py",
        pythonpath: list[Path] | None = None,
    ) -> CheckRun:
        """Runs `blastit check` with a real proxied `DATABASE_URL`."""
        url = (
            f"postgresql://{USER}:{PASSWORD}@127.0.0.1:{server.port}/app"
            "?sslmode=disable"
        )
        entries = [str(entry) for entry in (pythonpath or [project.root / "stub"])]
        entries.append(str(_package_root()))
        entries.append(os.environ.get("PYTHONPATH", ""))
        environment = {
            **os.environ,
            "DATABASE_URL": url,
            "CONTRACT_MINITEST": str(project.root / runner),
            "PYTHONPATH": os.pathsep.join(entries),
            "PYTHONDONTWRITEBYTECODE": "1",
            **(env or {}),
        }
        completed = subprocess.run(
            [sys.executable, "-m", "blastit", "check", *arguments],
            cwd=project.root,
            env=environment,
            capture_output=True,
            timeout=180,
            check=False,
        )
        return CheckRun(completed)

    def test_a_clean_run_is_one_document_and_no_findings(self) -> None:
        """The allowed path: one request, one statement, one JSON document."""
        server = self.server()
        project = self.project()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        document = run.document()
        self.assertEqual(document["schema_version"], 1)
        self.assertTrue(document["ok"], document)
        self.assertEqual(document["requests"], 1)
        self.assertEqual(document["statements"], 1)
        self.assertEqual(document["findings"], [])
        self.assertEqual(document["routes"], {"total": 1, "exercised": 1})
        self.assertEqual(document["proxies"]["db"], {"statements": 1, "denied": 0})
        self.assertEqual(
            server.statements, ["SELECT 1"], "the statement reached the database"
        )
        self.assertEqual(server.startups[0]["user"], USER)
        # R5: one document, and everything the suite wrote to fd 1 went to
        # stderr instead — `print()` and `os.write(1, …)` alike.
        self.assertNotIn(b"noise", run.stdout)
        self.assertIn(b"noise", run.stderr)
        self.assertIn(b"observed", run.stderr)
        self.assertTrue(run.stdout.lstrip().startswith(b"{"))
        self.assertTrue(run.stdout.rstrip().endswith(b"}"))

    def test_the_real_password_and_url_never_reach_the_suite(self) -> None:
        """The denied twin of the credential barrier: nothing the suite sees is real."""
        project = self.project()
        # The same mini-runner, with the environment itself asserted first.
        (project.root / "tests" / "test_contract.py").write_text(
            "import os\n"
            f"assert {PASSWORD!r} not in os.environ['DATABASE_URL']\n"
            "url = os.environ['DATABASE_URL']\n"
            "assert 'host=' in url and '127.0.0.1' not in url, url\n"
            f"assert not any({PASSWORD!r} in value for value in os.environ.values())\n"
            "assert os.environ.get('BLASTIT_MANIFEST') is None\n" + MINI_RUNNER,
            encoding="utf-8",
        )
        server = self.server()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        self.assertEqual(run.document()["findings"], [])

    def test_the_body_sample_is_capped_at_sixty_four_kib(self) -> None:
        """`R36`: the gate hands the probe a 64 KiB prefix, never the body."""
        project = self.project(UPLOAD_MANIFEST)
        body = json.dumps({"blob": "x" * (200 * 1024)}).encode()
        (project.root / "tests" / "test_contract.py").write_text(
            "import json, os\n"
            "from blastit import _check_pytest as gate\n"
            "from app import body\n"
            "BODY = " + repr(body) + "\n"
            "core = gate.wrap(body())\n"
            "environ = {'REQUEST_METHOD': 'POST', 'PATH_INFO': '/upload',\n"
            "           'CONTENT_TYPE': 'application/json',\n"
            "           'CONTENT_LENGTH': str(len(BODY)),\n"
            "           'wsgi.input': __import__('io').BytesIO(BODY)}\n"
            "payload = b''.join(core(environ, lambda code, headers: None))\n"
            "assert payload == b'SELECT 1', payload\n"
            "line = json.loads(open(os.environ['BLASTIT_CHECK_HTTP_OBS'], encoding='utf-8').read().splitlines()[-1])\n"
            "assert line['body_len'] == len(BODY), line\n"
            "assert line['sample']['truncated'] is True, line\n"
            "assert line['sample']['valid_json'] is False, 'a truncated body is not validated'\n"
            "assert line['sample']['mime'] == 'application/json', line\n",
            encoding="utf-8",
        )
        server = self.server()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        self.assertEqual(run.document()["findings"], [], run.document())

    def test_a_bad_manifest_exits_two_with_an_empty_stdout(self) -> None:
        """`exit 2`: Blastit could not start, so there is no report at all."""
        server = self.server()
        project = self.project('version = 1\n[[route]\nname = "oops"\n')
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 2, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 2 emits no document")
        self.assertIn(b"blastit check:", run.stderr)
        self.assertEqual(server.statements, [], "the suite never ran")

    def test_a_corrupt_observation_line_exits_two_with_an_empty_stdout(self) -> None:
        """The adversarial twin: the suite scribbles in the file it is handed."""
        project = self.project()
        (project.root / "tests" / "test_contract.py").write_text(
            MINI_RUNNER
            + (
                "with open(os.environ['BLASTIT_CHECK_HTTP_OBS'], 'a', encoding='utf-8') as log:\n"
                "    log.write('{not json}\\n')\n"
            ),
            encoding="utf-8",
        )
        server = self.server()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 2, run.stderr.decode())
        self.assertEqual(
            run.stdout, b"", "a corrupt observation file emits no document"
        )
        self.assertIn(b"http_obs.jsonl", run.stderr)
        self.assertIn(b"line 2", run.stderr)

    def test_a_failing_suite_exits_three_with_an_empty_stdout(self) -> None:
        """`R4`: pytest's `1` is a failed suite, never a findings report."""
        server = self.server()
        project = self.project()
        run = self.run_check(
            project, "--json", server=server, env={"CONTRACT_EXIT": "1"}
        )
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 3 emits no document")

    def test_no_tests_collected_exits_three_with_an_empty_stdout(self) -> None:
        """`R4`: pytest's exit `5` is a failure, not an empty clean run."""
        server = self.server()
        project = self.project()
        run = self.run_check(
            project, "--json", server=server, env={"CONTRACT_EXIT": "5"}
        )
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 3 emits no document")

    def test_a_dead_control_channel_exits_three(self) -> None:
        """A request that cannot be attributed fails the suite (`exit 3`)."""
        project = self.project()
        (project.root / "tests" / "test_contract.py").write_text(
            "import os\n"
            "os.close(int(os.environ['BLASTIT_CHECK_CTL_FD']))\n" + MINI_RUNNER,
            encoding="utf-8",
        )
        server = self.server()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 3 emits no document")

    def test_the_legacy_entry_point_is_blocked_but_the_gate_loads(self) -> None:
        """`-p no:blastit` is what keeps the 0.1.0 plugin out; the gate still runs."""
        server = self.server()
        project = self.project()
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        # The stub asserts the absence itself; the report proves the gate ran.
        self.assertEqual(run.document()["requests"], 1)

    def test_the_human_form_is_the_one_documented_line(self) -> None:
        """Without `--json`, the suite's output and then the §1.3 clean line."""
        server = self.server()
        project = self.project()
        run = self.run_check(project, server=server)
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        lines = run.stdout.decode().splitlines()
        self.assertEqual(
            lines[-1], "blastit check: 1 requests, 1 statements, 1/1 routes, clean"
        )
        self.assertIn("noise", run.stdout.decode(), "pytest output stays on stdout")

    def test_a_denied_statement_exits_one_with_one_document(self) -> None:
        """`exit 1` (findings): the grant covers `SELECT 2`, the app sends `SELECT 1`."""
        server = self.server()
        project = self.project(DEGRADED_MANIFEST)
        run = self.run_check(project, "--json", server=server)
        self.assertEqual(run.returncode, 1, run.stderr.decode())
        document = run.document()
        self.assertFalse(document["ok"], document)
        kinds = {
            (finding["kind"], finding.get("sql")) for finding in document["findings"]
        }
        self.assertIn(("missing_grant", "SELECT 1"), kinds)
        # The grant the manifest declares is dead too: the suite never sent it.
        self.assertIn(("dead_grant", "SELECT 2"), kinds)
        missing = next(
            finding
            for finding in document["findings"]
            if finding["kind"] == "missing_grant"
        )
        self.assertEqual(
            missing["fix"], {"action": "add_query", "to": "[route.db].queries"}
        )
        self.assertTrue(
            run.stdout.rstrip().endswith(b"}"), "one document, nothing else"
        )

    def test_learn_never_writes_when_the_suite_fails(self) -> None:
        """`R29`: a failing suite is exit `3`, no document, and a byte-identical manifest."""
        server = self.server()
        project = self.project()
        before = (project.root / "blastit.toml").read_bytes()
        run = self.run_check(
            project, "--learn", "--json", server=server, env={"CONTRACT_EXIT": "1"}
        )
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(
            run.stdout, b"", "exit 3 emits no document, not even a proposal"
        )
        self.assertEqual(
            (project.root / "blastit.toml").read_bytes(), before, "nothing was written"
        )

    def test_learn_refuses_an_inline_literal_with_exit_one_and_writes_nothing(
        self,
    ) -> None:
        """`R29`/`R24`: a refused statement is exit `1`, `"wrote": false`, no write.

        The denied twin sends a statement the learn policy refuses to grant
        because the value is inlined; the allowed twin sends the parameterised
        statement of the same shape on the same request, and it *is* written.
        So the denial is the literal, never a blanket `--learn` refusal, and the
        exit code cannot be satisfied by both sides.
        """
        literal = "SELECT 1 WHERE 's3cr3t' = 's3cr3t'"
        parameterised = "SELECT 1 WHERE $1 = $2"

        # Denied twin: one unrouted request, so there is a route to propose and
        # the refusal is the only thing standing between the plan and a write.
        server = self.server()
        refused_project = self.project(runner=LEARN_RUNNER)
        (refused_project.root / "app.py").write_text(
            _learn_app(literal), encoding="utf-8"
        )
        before = (refused_project.root / "blastit.toml").read_bytes()
        refused = self.run_check(refused_project, "--learn", "--json", server=server)
        self.assertEqual(refused.returncode, 1, refused.stderr.decode())
        # The statement reaches this test's own server asynchronously: the view
        # sends it to dbproxy, dbproxy forwards it upstream, and only then is the
        # observation drained and the report built. Wait, bounded, for the wire
        # statement to be on the server before asserting what reached it — the
        # assertion is unchanged, and a timeout only means it fails with the
        # message it always had.
        deadline = time.monotonic() + 10
        while server.statements != [literal] and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(
            server.statements,
            [literal],
            "the literal reached the database, so the policy saw real wire SQL",
        )
        document = refused.document()
        self.assertFalse(document["wrote"], document)
        self.assertEqual(
            document["proposed"],
            {"routes": 1, "queries": 0},
            "a route was worth writing; only the refused query was withheld",
        )
        after = (refused_project.root / "blastit.toml").read_bytes()
        self.assertEqual(after, before, "a refusal writes nothing")
        self.assertNotIn(b'name = "notes"', after, "no [[route]] line was added")
        self.assertNotIn(b"SELECT 1 WHERE", after, "no queries line was added")
        self.assertIn(b"learn: refused", refused.stderr)
        self.assertIn(
            b"bind the value with a placeholder instead",
            refused.stderr,
            "the refusal names the fix",
        )
        self.assertIn(
            "SELECT 1 WHERE … = …".encode(),
            refused.stderr,
            "the refused statement is shown redacted, not dropped",
        )
        # R24: the value is gone from every channel the run writes, the report
        # and its diagnostics alike.
        self.assertTrue(
            refused.stdout.rstrip().endswith(b"}"), "one document, nothing else"
        )
        self.assertNotIn(b"s3cr3t", refused.stdout)
        self.assertNotIn(b"s3cr3t", refused.stderr)

        # Allowed twin: the same request, path, and statement shape, with the
        # value bound instead of inlined. It is proposed, written, and clean.
        server = self.server()
        written_project = self.project(runner=LEARN_RUNNER)
        (written_project.root / "app.py").write_text(
            _learn_app(parameterised), encoding="utf-8"
        )
        before = (written_project.root / "blastit.toml").read_bytes()
        written = self.run_check(written_project, "--learn", "--json", server=server)
        self.assertEqual(written.returncode, 0, written.stderr.decode())
        document = written.document()
        self.assertTrue(document["wrote"], document)
        self.assertEqual(document["proposed"], {"routes": 1, "queries": 1}, document)
        text = (written_project.root / "blastit.toml").read_bytes()
        self.assertNotEqual(text, before, "the parameterised grant was written")
        manifest = tomllib.loads(text.decode("utf-8"))
        notes = [route for route in manifest["route"] if route.get("name") == "notes"]
        self.assertEqual(len(notes), 1, text.decode("utf-8"))
        self.assertEqual(notes[0]["path"], ["/notes"], notes)
        self.assertEqual(
            notes[0]["db"]["queries"],
            ["SELECT 1 WHERE $? = $?"],
            "the grant holds the normalised text, placeholders as `$?`",
        )

    def test_a_missing_pytest_is_exit_two_not_a_failed_suite(self) -> None:
        """`R4`: nothing ran, so a missing pytest is "could not start" (exit 2)."""
        server = self.server()
        project = self.project()
        # A `pytest` that cannot import stands in for an environment without it,
        # whatever the host interpreter happens to have installed.
        broken = project.root / "no-pytest"
        broken.mkdir()
        (broken / "pytest.py").write_text(
            "raise ModuleNotFoundError(\"No module named 'pytest'\")\n",
            encoding="utf-8",
        )
        run = self.run_check(project, "--json", server=server, pythonpath=[broken])
        self.assertEqual(run.returncode, 2, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 2 emits no document")
        self.assertIn(b"install pytest: uv add --dev pytest", run.stderr)
        self.assertEqual(server.statements, [], "the suite never ran")

    def test_a_pytest_django_run_hands_over_the_test_database_and_restores_teardown(
        self,
    ) -> None:
        """`R30`/`ADR-0004`: `pytest_fixture_setup` repoints `test_app` and LIFO teardown restores real settings.

        The test body never calls `finish_django_handshake()`: the real
        `pytest_fixture_setup` hookwrapper fires on `django_db_setup`, scrubs
        `os.environ`, repoints `connections.databases` at dbproxy, sends
        `set_db`, and registers the LIFO finalizer that restores the real
        settings right before `teardown_databases` drops `test_app`.
        """
        server = self.server()
        project = self.project(runner=DJANGO_DB_RUNNER)
        project.install_django_stub()
        run = self.run_check(
            project,
            "--json",
            server=server,
            env={
                "DJANGO_SETTINGS_MODULE": "app_settings",
                "CONTRACT_DJANGO_DB": "1",
            },
        )
        self.assertEqual(run.returncode, 0, run.stderr.decode())
        document = run.document()
        self.assertEqual(document["findings"], [], document)
        self.assertEqual(document["requests"], 1)
        self.assertEqual(
            server.startups[0]["database"],
            "test_app",
            "set_db steered the session Django's tests used",
        )
        self.assertEqual(server.startups[0]["user"], USER)
        self.assertEqual(server.statements, ["SELECT 1"])
        self.assertIn(
            b"TEARDOWN_RESTORED_OK",
            run.stderr,
            "LIFO finalizer restored real connection settings before teardown_databases",
        )
        self.assertNotIn(PASSWORD.encode(), run.stdout)
        self.assertNotIn(PASSWORD.encode(), run.stderr)

    def test_a_no_db_django_suite_never_exposes_real_credentials_to_test_bodies(
        self,
    ) -> None:
        """Denied credential-window twin: `django_db_setup` never runs, yet test bodies see only `swapped`."""
        server = self.server()
        project = self.project(runner=DJANGO_NO_DB_RUNNER)
        project.install_django_stub()
        run = self.run_check(
            project,
            "--json",
            server=server,
            env={"DJANGO_SETTINGS_MODULE": "app_settings"},
        )
        # pytest exited 0 (every assertion in DJANGO_NO_DB_RUNNER passed), and
        # the audit reports exit 1 because `[proxy.db]` saw 0 statements.
        self.assertEqual(run.returncode, 1, run.stderr.decode())
        document = run.document()
        self.assertEqual(document["requests"], 1, document)
        self.assertEqual(document["statements"], 0, document)
        self.assertIn(b"NO_DB_BODY_SECRET_FREE", run.stderr)
        self.assertNotIn(PASSWORD.encode(), run.stdout)
        self.assertNotIn(PASSWORD.encode(), run.stderr)
        self.assertEqual(server.statements, [])

    def test_an_unhandshaken_django_suite_fails_closed_in_pytest_configure(
        self,
    ) -> None:
        """Fail-closed twin: `pytest_django` + configured settings + no handshake -> `ControlError` (exit 3)."""
        server = self.server()
        project = self.project()
        project.install_django_stub()
        # Outer env has no `DJANGO_SETTINGS_MODULE` and project has no
        # `manage.py`, so `check.rs` writes no handshake; `pytest.ini`
        # (`CONTRACT_INI_DJANGO_SETTINGS`) configures Django inside pytest,
        # which `pytest_configure` must refuse before any test runs.
        run = self.run_check(
            project,
            "--json",
            server=server,
            env={"CONTRACT_INI_DJANGO_SETTINGS": "app_settings"},
        )
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 3 emits no document")
        self.assertIn(b"wrote no database handshake", run.stderr)
        self.assertEqual(server.statements, [], "no statement reached upstream")

    def test_repoint_failure_raises_control_error_instead_of_bypassing_dbproxy(
        self,
    ) -> None:
        """Fail-closed repoint twin: if no alias matches `test_app` (or `close_all` fails), `ControlError` aborts (`exit 3`)."""
        server = self.server()
        project = self.project(runner=DJANGO_DB_RUNNER)
        project.install_django_stub()
        run = self.run_check(
            project,
            "--json",
            server=server,
            env={
                "DJANGO_SETTINGS_MODULE": "app_settings",
                "CONTRACT_DJANGO_DB": "1",
                "CONTRACT_TEST_DB_NAME": "unmatched_db",
            },
        )
        self.assertEqual(run.returncode, 3, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 3 emits no document")
        self.assertIn(b"could not repoint Django database", run.stderr)
        self.assertEqual(server.statements, [], "traffic never bypassed dbproxy")

        # Second repoint failure mode: alias matches `test_app`, but
        # `connections.close_all()` raises during repointing -> `ControlError`.
        close_fail = self.run_check(
            project,
            "--json",
            server=server,
            env={
                "DJANGO_SETTINGS_MODULE": "app_settings",
                "CONTRACT_DJANGO_DB": "1",
                "CONTRACT_FAIL_CLOSE_ALL": "1",
            },
        )
        self.assertEqual(close_fail.returncode, 3, close_fail.stderr.decode())
        self.assertEqual(close_fail.stdout, b"")
        self.assertIn(b"could not repoint Django database", close_fail.stderr)
        self.assertEqual(server.statements, [])

    def test_a_django_project_without_a_proxy_is_exit_two_before_it_runs(self) -> None:
        """The denied twin: a Django project with nothing to redirect fails closed.

        With `pytest-django` importable and settings named, `blastit check` must
        refuse to start (exit `2`, an exact fix) rather than let
        `django_db_setup` create its test database where the suite's statements
        would land in the developer's own database.
        """
        server = self.server()
        project = self.project(
            "version = 1\n\n[[route]]\nname = \"home\"\npath = ['/']\n"
        )
        project.install_django_stub()
        run = self.run_check(
            project,
            "--json",
            server=server,
            env={"DJANGO_SETTINGS_MODULE": "app_settings"},
        )
        self.assertEqual(run.returncode, 2, run.stderr.decode())
        self.assertEqual(run.stdout, b"", "exit 2 emits no document")
        self.assertIn(b"declares no postgresql proxy", run.stderr)
        self.assertEqual(server.statements, [], "the suite never ran")


class TestTheUnprivilegedLiveServer(unittest.TestCase):
    """`R39`: the live server falls back to `blastit dev`, it never skips.

    Moved here from `python/tests/test_live_server.py` so `make check` — which
    runs the protected critical tier and nothing else of `python/tests` — always
    sees the allowed path (`([], true)`, a dev server on a machine without root)
    beside both production twins.
    """

    def setUp(self) -> None:
        self.plugin = _plugin()

    def test_no_root_and_no_sudo_starts_dev(self) -> None:
        with (
            mock.patch.object(self.plugin.os, "geteuid", return_value=1000),
            mock.patch.object(self.plugin.shutil, "which", return_value=None),
        ):
            self.assertEqual(
                self.plugin._live_server_mode(), ([], True), "blastit dev, unprivileged"
            )

    def test_root_still_serves_production(self) -> None:
        with mock.patch.object(self.plugin.os, "geteuid", return_value=0):
            self.assertEqual(self.plugin._live_server_mode(), ([], False))

    def test_passwordless_sudo_still_serves_production(self) -> None:
        with (
            mock.patch.object(self.plugin.os, "geteuid", return_value=1000),
            mock.patch.object(
                self.plugin.shutil, "which", return_value="/usr/bin/sudo"
            ),
            mock.patch.object(
                self.plugin.subprocess, "run", return_value=_Completed(0)
            ),
        ):
            self.assertEqual(
                self.plugin._live_server_mode(), (["/usr/bin/sudo", "-n"], False)
            )


if __name__ == "__main__":
    unittest.main()
