"""``blastit check``'s pytest gate: one observation per WSGI request (R30, R36).

`blastit check` runs ``pytest -q -p no:blastit -p blastit._check_pytest`` with
``blastit-dbproxy`` in `DynamicCheck` mode in front of PostgreSQL. The proxy
records the statements; this module records the **requests**, and tells the proxy
which request the statements that follow belong to.

The module is inert unless the orchestrator asked for it: with no
``BLASTIT_CHECK_HTTP_OBS`` or ``BLASTIT_CHECK_CTL_FD`` in the environment
(`crates/blastit/src/check.rs`) importing it starts nothing, patches nothing and
writes nothing. That is what makes ``-p blastit._check_pytest`` safe to name on a
machine where the command is not running.

Two things are recorded per request, and exactly once:

* one ``HttpObsLine`` appended to ``http_obs.jsonl`` — the method, the
  ``PATH_INFO`` the scheduler would route on, the ``Content-Type``, the body's
  length, and a **bounded** probe of the body. The probe is
  :func:`blastit._blastit.probe_ingress`'s, capped at
  :data:`MAX_SAMPLE_BYTES` (64 KiB); the body itself is replayed to the
  application unchanged, never logged;
* one target pushed over the control channel and popped when the response body
  is exhausted, so the proxy attributes the statements of *that* request to it.

The push names the request (``push_unrouted``): which route a path belongs to is
Blastit's decision, and re-implementing the routing table's regex semantics in
Python would be a second, divergent router. ``crates/blastit/src/check.rs``
resolves every observed request with the same table the audit uses.

A request that nests another (Flask's ``.wsgi_app`` called by a Flask view, a
Django handler served by a test client) is observed **once**: a
:class:`~contextvars.ContextVar` marks the outermost call, so the identifier of
the request is not duplicated and its target is pushed and popped exactly once.

If the control channel fails — dbproxy gone, a malformed reply, the 5-second
deadline — the gate raises. That is deliberate: the run cannot attribute any
statement any more, and ``blastit check`` turns the suite's non-zero exit into
exit ``3`` rather than reporting an audit of nothing (`R4`).

The Django handshake (`R30`, `ADR-0004`)
----------------------------------------

A ``pytest-django`` suite is the one case where the secret-free environment is
not enough: ``django_db_setup`` has to create its ``test_*`` database on the
real server before tests run, and without it every migration and flush would
land in the developer's own database. So when ``pytest-django`` is importable
the orchestrator writes the real proxy inputs into a one-shot ``0600`` file
instead of the environment. This module is imported by ``-p`` *before*
``pytest-django`` builds Django's settings, so at import it reads that file,
names it in :data:`DJANGO_SETUP`, unlinks it immediately, and applies the real
values only long enough for Django's ``DATABASES`` dictionary to be evaluated.

Once Django's settings are loaded (and unconditionally at collection finish /
before any test setup, even when zero collected tests use the database),
``os.environ`` is scrubbed back to the secret-free ``swapped`` values so no test
import or test body ever sees a real credential. When ``django_db_setup`` runs,
the ``pytest_fixture_setup`` hook saves the real connection dictionaries, points
matching ``test_*`` aliases at dbproxy's socket, sends
``{"op":"set_db","proxy":…,"database":"test_…"}``, and registers a LIFO
finalizer on ``django_db_setup`` that closes proxied sessions and restores the
saved dictionaries right before ``teardown_databases`` drops ``test_*`` on the
real server.
"""

from __future__ import annotations

import copy
import json
import os
import socket
import stat
import tempfile
from contextvars import ContextVar
from typing import Any
from urllib.parse import parse_qs, urlsplit

__all__ = [
    "MAX_SAMPLE_BYTES",
    "ControlError",
    "finish_django_handshake",
    "last_django_handshake",
    "wrap",
]

#: The largest body prefix the ingress probe sees (`MAX_CHECK_SAMPLE_BYTES`).
MAX_SAMPLE_BYTES = 64 * 1024

#: The largest body the gate is willing to buffer before the application runs.
#: `BLASTIT_MAX_BODY_BYTES`'s default, and a hard ceiling on what one request can
#: make this process hold.
MAX_BODY_BYTES = 100 * 1024 * 1024

#: The environment variable naming the JSONL file `http_obs.jsonl`.
HTTP_OBS = "BLASTIT_CHECK_HTTP_OBS"

#: The environment variable naming the inherited control descriptor.
CTL_FD = "BLASTIT_CHECK_CTL_FD"

#: The environment variable naming the one-shot Django handshake file (`R30`).
DJANGO_SETUP = "BLASTIT_CHECK_DJANGO_SETUP"

#: The largest handshake file this gate reads; the real inputs are two URLs.
MAX_SETUP_BYTES = 128 * 1024

#: How long one control round trip may take, matching the proxy's own deadline.
CONTROL_TIMEOUT = 5.0

#: Connection `OPTIONS` keys that belong to the upstream TCP/TLS leg and must
#: not ride onto dbproxy's local Unix socket while repointed.
_UPSTREAM_ONLY_OPTIONS = frozenset(
    {
        "dbname",
        "host",
        "hostaddr",
        "passfile",
        "password",
        "port",
        "service",
        "sslcert",
        "sslcrl",
        "sslkey",
        "sslmode",
        "sslpassword",
        "sslrootcert",
        "user",
    }
)

#: Set while the outermost gated call is handling a request.
_CURRENT: ContextVar[bool] = ContextVar("blastit_check_request", default=False)

#: The control channel, opened on first use.
_CONTROL: list[Any] = []


class ControlError(RuntimeError):
    """The control channel could not attribute a request (`exit 3`)."""


def _enabled() -> bool:
    """Whether the orchestrator asked for observations."""
    return bool(os.environ.get(HTTP_OBS)) and os.environ.get(CTL_FD) is not None


def _control() -> socket.socket:
    """The connected control channel, adopted from the inherited descriptor."""
    if _CONTROL:
        return _CONTROL[0]
    raw = os.environ.get(CTL_FD)
    if raw is None:
        raise ControlError(
            f"blastit check: {CTL_FD} is not set, so no request can be attributed"
        )
    try:
        number = int(raw)
        channel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM, fileno=number)
    except (ValueError, OSError) as error:
        raise ControlError(
            f"blastit check: the control descriptor {raw!r} is not usable: {error!r}"
        ) from error
    channel.settimeout(CONTROL_TIMEOUT)
    _CONTROL.append(channel)
    return channel


def _send(request: dict[str, Any]) -> None:
    """Sends one control line and requires the proxy's ``{"ok": true}``."""
    channel = _control()
    line = json.dumps(request).encode("utf-8") + b"\n"
    if len(line) > 4096:
        raise ControlError("blastit check: the control line is longer than 4096 bytes")
    try:
        channel.sendall(line)
        reply = _read_line(channel)
        answer = json.loads(reply)
    except (OSError, ValueError) as error:
        raise ControlError(
            f"blastit check: the control channel failed: {error!r}"
        ) from error
    if not isinstance(answer, dict) or answer.get("ok") is not True:
        raise ControlError(
            f"blastit check: the control channel refused a request: {answer!r}"
        )


def _read_line(channel: socket.socket) -> str:
    """Reads one newline-terminated reply, bounded like the proxy's own lines."""
    buffered = b""
    while not buffered.endswith(b"\n"):
        chunk = channel.recv(4096)
        if not chunk:
            raise ControlError("blastit check: the control channel closed")
        buffered += chunk
        if len(buffered) > 4096:
            raise ControlError(
                "blastit check: the control reply is longer than 4096 bytes"
            )
    return buffered.decode("utf-8", "replace").strip()


def _django_settings_configured() -> bool:
    """Whether `django.conf.settings` has already been configured."""
    try:
        from django.conf import settings

        return bool(settings.configured)
    except Exception:  # noqa: BLE001 - absent or broken settings raise on access
        return False


def _materialise_django_databases() -> None:
    """Forces Django's lazy `DATABASES` mapping to evaluate before env scrub."""
    if not _django_settings_configured():
        return
    try:
        from django.conf import settings
        from django.db import connections

        _ = settings.DATABASES
        _ = connections.databases
    except Exception:  # noqa: BLE001 - surfaced by Django itself when used
        return


class DjangoHandshake:
    """The real proxy inputs one `django_db_setup` may use, and what comes after.

    `blastit check` writes them into a `0600` file instead of the suite's
    environment, because `pytest-django` has to load `DATABASES` with the real
    upstream credentials so `django_db_setup` can create `test_*` (`R30`,
    `ADR-0004`). [`restore_env`][DjangoHandshake.restore_env] closes the
    environment window before test modules or bodies run;
    [`finish`][DjangoHandshake.finish] repoints the created `test_*` alias at
    dbproxy and selects `test_*` over `ctl.sock`; and
    [`restore_connections`][DjangoHandshake.restore_connections] puts the real
    settings back right before `teardown_databases` drops `test_*`.
    """

    def __init__(
        self,
        path: str,
        mode: int,
        real: dict[str, str],
        swapped: dict[str, str],
        proxy: str,
        database: str,
        socket_dir: str | None = None,
        port: int | None = None,
        user: str | None = None,
    ) -> None:
        self.path = path
        self.mode = mode
        self.real = real
        self.swapped = swapped
        self.proxy = proxy
        self.database = database
        self.socket_dir = socket_dir
        self.port = port
        self.user = user
        self.env_restored = False
        self.finished = False
        self.restored_connections = False
        self._saved_databases: dict[str, dict[str, Any]] = {}

    def restore_env(self) -> None:
        """Replaces the real environment inputs with the secret-free `swapped` ones."""
        if self.env_restored:
            return
        _materialise_django_databases()
        self.env_restored = True
        for name in self.real:
            value = self.swapped.get(name)
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    def finish(self, *, send: bool) -> None:
        """Repoints Django's test database at dbproxy and sends `set_db`."""
        if self.finished:
            return
        self.restore_env()
        self.finished = True
        self._saved_databases = _repoint_django(self)
        self.restored_connections = False
        if send:
            _send(
                {
                    "op": "set_db",
                    "proxy": self.proxy,
                    "database": self.database,
                }
            )

    def restore_connections(self) -> None:
        """Closes proxied connections and restores saved settings before DB drop."""
        if self.restored_connections or not self._saved_databases:
            return
        self.restored_connections = True
        from django.db import connections

        connections.close_all()
        for alias, saved in self._saved_databases.items():
            restored = copy.deepcopy(saved)
            target = connections.databases.get(alias)
            if isinstance(target, dict):
                target.clear()
                target.update(restored)
            else:
                connections.databases[alias] = restored
            if _django_settings_configured():
                from django.conf import settings

                conf_target = settings.DATABASES.get(alias)
                if isinstance(conf_target, dict) and conf_target is not target:
                    conf_target.clear()
                    conf_target.update(copy.deepcopy(saved))


def _read_django_setup() -> DjangoHandshake | None:
    """Reads, unlinks, and validates the one-shot handshake, if there is one.

    The unlink happens as soon as the bytes are in memory, before anything is
    parsed: from that moment on the real inputs exist only in this process's
    memory, never on disk (`R30`).
    """
    path = os.environ.get(DJANGO_SETUP)
    if not path or not _enabled():
        return None
    with open(path, "rb") as document:
        mode = stat.S_IMODE(os.fstat(document.fileno()).st_mode)
        raw = document.read(MAX_SETUP_BYTES + 1)
    os.unlink(path)
    if len(raw) > MAX_SETUP_BYTES:
        raise ControlError(
            f"blastit check: {DJANGO_SETUP} is larger than {MAX_SETUP_BYTES} bytes"
        )
    if mode != 0o600:
        raise ControlError(
            f"blastit check: {DJANGO_SETUP} was {mode:04o}, not 0600; blastit only "
            "hands the real database inputs to a file no other user can read"
        )
    try:
        document = json.loads(raw)
    except ValueError as error:
        raise ControlError(
            f"blastit check: {DJANGO_SETUP} is not JSON: {error!r}"
        ) from error
    if not isinstance(document, dict):
        raise ControlError(f"blastit check: {DJANGO_SETUP} is not a JSON object")
    real = document.get("real")
    swapped = document.get("swapped")
    proxy = document.get("proxy")
    database = document.get("database")
    if (
        not isinstance(real, dict)
        or not isinstance(swapped, dict)
        or not isinstance(proxy, str)
        or not isinstance(database, str)
        or not real
        or not swapped
    ):
        raise ControlError(
            f"blastit check: {DJANGO_SETUP} does not name the real inputs, the "
            "swapped ones, a proxy and a test database"
        )
    raw_socket_dir = document.get("socket_dir")
    raw_port = document.get("port")
    raw_user = document.get("user")
    return DjangoHandshake(
        path=path,
        mode=mode,
        real={str(name): str(value) for name, value in real.items()},
        swapped={str(name): str(value) for name, value in swapped.items()},
        proxy=proxy,
        database=database,
        socket_dir=str(raw_socket_dir) if isinstance(raw_socket_dir, str) else None,
        port=int(raw_port) if isinstance(raw_port, int) else None,
        user=str(raw_user) if isinstance(raw_user, str) else None,
    )


def begin_django_handshake() -> DjangoHandshake | None:
    """Starts the Django handshake: real inputs for `settings.py`, scrubbed after (`R30`).

    The suite is imported as a `-p` plugin **before** `pytest-django` builds
    Django's settings, so applying the real values here is what populates
    `settings.DATABASES` with the server `django_db_setup` creates `test_*` on.
    """
    handshake = _read_django_setup()
    if handshake is None:
        return None
    os.environ.update(handshake.real)
    return handshake


def finish_django_handshake() -> None:
    """Completes the Django handshake after its test database exists (`R30`)."""
    if _DJANGO is not None and not _DJANGO.finished:
        _DJANGO.finish(send=True)


def last_django_handshake() -> dict[str, Any] | None:
    """What the Django handshake did, as names and no values (a test seam)."""
    if _DJANGO is None:
        return None
    return {
        "path": _DJANGO.path,
        "mode": _DJANGO.mode,
        "unlinked": not os.path.exists(_DJANGO.path),
        "proxy": _DJANGO.proxy,
        "database": _DJANGO.database,
        "real_variables": sorted(_DJANGO.real),
        "swapped_variables": sorted(_DJANGO.swapped),
        "env_restored": _DJANGO.env_restored,
        "finished": _DJANGO.finished,
        "restored_connections": _DJANGO.restored_connections,
    }


def _proxy_endpoint(handshake: DjangoHandshake) -> tuple[str, str, str]:
    """Resolves dbproxy's `(socket_dir, port, user)` from the handshake."""
    if handshake.socket_dir:
        return (
            handshake.socket_dir,
            str(handshake.port or 5432),
            handshake.user or "blastit",
        )
    for candidate in handshake.swapped.values():
        parts = urlsplit(candidate)
        if parts.scheme not in ("postgres", "postgresql"):
            continue
        query = parse_qs(parts.query)
        host = (query.get("host") or [parts.hostname or ""])[0]
        port = (query.get("port") or [str(parts.port or 5432)])[0]
        user = parts.username or "blastit"
        if host:
            return host, port, user
    raise ControlError(
        f"blastit check: proxy {handshake.proxy!r} has no local socket endpoint "
        "in the Django handshake"
    )


def _repoint_django(handshake: DjangoHandshake) -> dict[str, dict[str, Any]]:
    """Points Django's test connections at dbproxy, saving originals for teardown.

    Raises [`ControlError`] whenever `django.db.connections` cannot be inspected
    or has no alias matching `handshake.database`, so traffic can never silently
    stay on the real PostgreSQL server and bypass dbproxy.
    """
    host, port, user = _proxy_endpoint(handshake)
    try:
        from django.db import connections

        databases = connections.databases
        if not isinstance(databases, dict):
            raise TypeError("django.db.connections.databases is not a dict")
        saved: dict[str, dict[str, Any]] = {}
        for alias, settings_dict in list(databases.items()):
            # Only a database created as this run's test database is repointed:
            # an unrelated alias keeps its own server.
            if not isinstance(settings_dict, dict):
                continue
            if settings_dict.get("NAME") != handshake.database:
                continue
            saved[alias] = copy.deepcopy(settings_dict)
            options = settings_dict.get("OPTIONS")
            if isinstance(options, dict):
                settings_dict["OPTIONS"] = {
                    key: value
                    for key, value in options.items()
                    if key not in _UPSTREAM_ONLY_OPTIONS
                }
            settings_dict.update(HOST=host, PORT=port, USER=user, PASSWORD="")
        if not saved:
            raise LookupError(
                f"no Django database alias has NAME={handshake.database!r}"
            )
        connections.close_all()
    except ControlError:
        raise
    except Exception as error:
        raise ControlError(
            f"blastit check: could not repoint Django database "
            f"{handshake.database!r} at proxy {handshake.proxy!r}: {error!r}"
        ) from error
    return saved


def _hookimpl(function: Any) -> Any:
    """Applies pytest's hookwrapper marker when this module's host is pytest.

    Deferred import: the module is also imported by test suites and by the
    offline contract tier, where pytest is not installed. The import is the
    plugin host's, exactly like `blastit.pytest_plugin`'s.
    """
    try:
        import pytest
    except ImportError:
        return function
    hookimpl = getattr(pytest, "hookimpl", None)
    if hookimpl is None:
        return function
    return hookimpl(hookwrapper=True)(function)


def pytest_sessionstart(session: Any) -> None:
    """Closes the real-credential env window before test modules are imported."""
    if not _enabled():
        return
    _refuse_unhandshaken_django()
    if _DJANGO is not None and _django_settings_configured():
        _DJANGO.restore_env()


def pytest_collection_finish(session: Any) -> None:
    """Always restores `swapped` before tests run, even when no item uses the DB."""
    if _DJANGO is not None:
        _DJANGO.restore_env()


def pytest_runtest_setup(item: Any) -> None:
    """Guarantees no test body ever runs before `swapped` is restored."""
    if _DJANGO is not None:
        _DJANGO.restore_env()


@_hookimpl
def pytest_fixture_setup(fixturedef: Any, request: Any) -> Any:
    """Swaps to dbproxy once `django_db_setup` creates `test_*`, restoring before drop."""
    outcome = yield
    if getattr(fixturedef, "argname", None) != "django_db_setup":
        return
    if _DJANGO is None:
        return
    if outcome is not None and getattr(outcome, "excinfo", None) is not None:
        _DJANGO.restore_env()
        return
    finish_django_handshake()
    # `fixturedef` runs its finalizers LIFO, so registering here executes after
    # every dependent per-test fixture (`_django_db_helper` rollback/flush) and
    # immediately before `django_db_setup` resumes to run `teardown_databases`.
    addfinalizer = getattr(fixturedef, "addfinalizer", None) or getattr(
        request, "addfinalizer", None
    )
    if callable(addfinalizer):
        addfinalizer(_DJANGO.restore_connections)


def pytest_sessionfinish(session: Any, exitstatus: int) -> None:
    """Backstop cleanup if the session aborted before normal teardown."""
    if _DJANGO is not None:
        _DJANGO.restore_env()
        _DJANGO.restore_connections()


def _content_type(environ: dict[str, Any]) -> str | None:
    """The request's ``Content-Type``, when it has one."""
    value = environ.get("CONTENT_TYPE")
    return str(value) if value else None


def _body_length(environ: dict[str, Any]) -> int:
    """The declared body length, or zero when the request declares none."""
    raw = environ.get("CONTENT_LENGTH")
    if raw in (None, ""):
        return 0
    try:
        return max(int(str(raw)), 0)
    except ValueError:
        return 0


def _replay_body(environ: dict[str, Any]) -> tuple[bytes, int, Any]:
    """Buffers the request body, replays it, and returns its 64 KiB prefix.

    The whole body goes into a spooled temporary file (memory up to 1 MiB, disk
    above it) and `wsgi.input` is replaced by that file, rewound, so the
    application reads exactly the bytes it would have read — while the gate keeps
    only the bounded prefix the ingress probe is allowed to see. The spool is the
    third value so the request can close it once its response is exhausted.
    """
    stream = environ.get("wsgi.input")
    declared = _body_length(environ)
    if stream is None or declared == 0:
        return b"", 0, None
    limit = min(declared, MAX_BODY_BYTES)
    prefix = b""
    total = 0
    # The file outlives this function: `wsgi.input` is what the application
    # reads, and the request closes it when its iterable is exhausted.
    spool = tempfile.SpooledTemporaryFile(max_size=1024 * 1024)  # noqa: SIM115
    try:
        remaining = limit
        while remaining > 0:
            chunk = stream.read(min(64 * 1024, remaining))
            if not chunk:
                break
            if isinstance(chunk, str):
                chunk = chunk.encode("utf-8", "surrogateescape")
            remaining -= len(chunk)
            total += len(chunk)
            spool.write(chunk)
            if len(prefix) < MAX_SAMPLE_BYTES:
                prefix += chunk[: MAX_SAMPLE_BYTES - len(prefix)]
        spool.seek(0)
        environ["wsgi.input"] = spool
    except BaseException:
        spool.close()
        raise
    return prefix, total, spool


def _probe(
    content_type: str | None, prefix: bytes, total: int
) -> dict[str, Any] | None:
    """`_blastit.probe_ingress`, or `None` when the extension is not there."""
    try:
        from blastit import _blastit
    except ImportError:
        return None
    try:
        return json.loads(_blastit.probe_ingress(content_type, prefix, total))
    except Exception:  # noqa: BLE001 - a probe must never fail a request
        return None


def _append(line: dict[str, Any]) -> None:
    """Appends one ``HttpObsLine`` to the file the orchestrator named."""
    path = os.environ.get(HTTP_OBS)
    if not path:
        raise ControlError(
            f"blastit check: {HTTP_OBS} is not set, so no request can be recorded"
        )
    with open(path, "a", encoding="utf-8") as document:
        document.write(json.dumps(line, sort_keys=True) + "\n")


def _target(environ: dict[str, Any]) -> dict[str, Any]:
    """The control command that attributes this request's statements."""
    method = str(environ.get("REQUEST_METHOD") or "GET")
    path = str(environ.get("PATH_INFO") or "/")
    return {"op": "push_unrouted", "method": method, "path": path}


def _build_obs_line(
    method: str,
    path: str,
    content_type: str | None,
    total: int,
    prefix: bytes,
) -> dict[str, Any]:
    """One `HttpObsLine`, exactly the schema `check_audit` reads."""
    line: dict[str, Any] = {
        "method": method,
        "path": path,
        "content_type": content_type,
        "body_len": total,
    }
    sample = _probe(content_type, prefix, total) if total else None
    if sample is not None:
        line["sample"] = sample
    return line


def _request_line(
    environ: dict[str, Any], content_type: str | None, total: int, prefix: bytes
) -> dict[str, Any]:
    """One WSGI `HttpObsLine`, built from `environ` and its bounded prefix."""
    return _build_obs_line(
        str(environ.get("REQUEST_METHOD") or "GET"),
        str(environ.get("PATH_INFO") or "/"),
        content_type,
        total,
        prefix,
    )


def _iterating(body: Any, done: Any) -> Any:
    """Pops the target when the response body is exhausted, whatever happens."""
    try:
        yield from body
    finally:
        if hasattr(body, "close"):
            body.close()
        done()


def wrap(app: Any) -> Any:
    """Gates one WSGI callable: one observation, one target, per request.

    A callable that is already gated is returned unchanged, so wrapping the same
    application twice — a fixture wrapping a module-level app, a Django handler
    wrapped by the framework — still observes one request once.

    A nested call (the gate reached through another WSGI layer inside the same
    request) passes straight through: the outermost call owns the observation.
    """
    if not _enabled() or getattr(app, "_blastit_gated", False):
        return app

    def gated(environ: dict[str, Any], start_response: Any) -> Any:
        if _CURRENT.get():
            return app(environ, start_response)
        token = _CURRENT.set(True)
        pushed = False
        spool = None
        try:
            content_type = _content_type(environ)
            prefix, total, spool = _replay_body(environ)
            _send(_target(environ))
            pushed = True
            _append(_request_line(environ, content_type, total, prefix))
            body = app(environ, start_response)
        except BaseException:
            if pushed:
                _pop_quietly()
            if spool is not None:
                spool.close()
            _CURRENT.reset(token)
            raise

        def done() -> None:
            _pop_quietly()
            if spool is not None:
                spool.close()
            _CURRENT.reset(token)

        if isinstance(body, (list, tuple)):
            # A one-shot body: the request is over the moment it is returned.
            done()
            return body
        return _iterating(body, done)

    gated._blastit_gated = True  # type: ignore[attr-defined]
    gated.__doc__ = getattr(app, "__doc__", None)
    gated.__wrapped__ = app  # type: ignore[attr-defined]
    return gated


def _pop_quietly() -> None:
    """Pops the pushed target, reporting a dead channel once per request."""
    try:
        _send({"op": "pop"})
    except ControlError:
        # The channel is gone; the orchestrator turns the suite's failure into
        # exit 3. Swallowing it here keeps the application's own exception — if
        # there is one — the one the test reports.
        pass


def _refuse_unhandshaken_django() -> None:
    """Stops a Django suite the orchestrator did not hand a test database to.

    `blastit check` only writes the handshake when it recognised a Django
    project (`DJANGO_SETTINGS_MODULE` in the environment or a `manage.py`). A
    project configured some other way would still run `django_db_setup` — and
    with no `set_db` its migrations would land in the development database. So
    this is a hard failure before any test runs, never a silent fallback.
    """
    if _DJANGO is not None or DJANGO_SETUP in os.environ:
        return
    try:
        import pytest_django  # noqa: F401 - present means this suite is Django's
    except ImportError:
        return
    if _django_settings_configured() or os.environ.get("DJANGO_SETTINGS_MODULE"):
        raise ControlError(
            "blastit check: pytest-django is configured but blastit wrote no "
            "database handshake, so no test may run against the development "
            "database; run blastit check from the directory that holds manage.py, "
            "or export DJANGO_SETTINGS_MODULE, and try again"
        )


def pytest_addoption(parser: Any) -> None:
    """Accepts a legacy `0.1.0` `blastit_manifest` ini key without warning.

    `-p no:blastit` disables the `0.1.0` plugin (`ADR-0004`); registering its
    one ini option here keeps projects that still carry `blastit_manifest` in
    `pyproject.toml` warning-clean under `-W error`.
    """
    addini = getattr(parser, "addini", None)
    if callable(addini):
        addini(
            "blastit_manifest",
            "Legacy 0.1.0 pytest plugin option (ignored under blastit check).",
            default="",
        )


def pytest_configure(config: Any) -> None:
    """Installs the gate on the framework seams this process has (R30).

    Only the two seams a check run can find without a project's cooperation are
    patched: Django's handler (every request a Django test makes ends there) and
    Flask's `wsgi_app` (every WSGI call a Flask app serves, test client
    included). A raw-WSGI suite asks for [`wrap`] explicitly — there is no global
    seam for "a WSGI callable", and inventing one would mean patching every
    server module in the standard library.

    The plugin is inert when the orchestrator did not ask for observations, which
    is why this hook is safe to import anywhere.
    """
    if not _enabled():
        return
    _refuse_unhandshaken_django()
    _patch_django()
    _patch_flask()


def _materialise_test_transactions() -> None:
    """Flushes any outer `TestCase` `BEGIN` before a request target is pushed.

    `pytest-django`'s `@pytest.mark.django_db` enters `TestCase`'s outer
    `atomic()` (`set_autocommit(False)`) during fixture setup, but `psycopg 3`
    defers sending `BEGIN` on the wire until the first `cursor.execute(...)`.
    Without flushing that outer `BEGIN` before `push_unrouted`, a read-only
    view's first `SELECT` would inherit the test harness's `BEGIN` as a routed
    transaction statement. Views that open their own `transaction.atomic()`
    still emit `SAVEPOINT` inside the request and still require
    `transaction_control = true`.
    """
    try:
        from django.db import connections

        for conn in connections.all():
            if not getattr(conn, "in_atomic_block", False):
                continue
            raw_conn = getattr(conn, "connection", None)
            pgconn = getattr(raw_conn, "pgconn", None)
            if raw_conn is None or getattr(pgconn, "transaction_status", None) == 0:
                with conn.cursor() as cursor:
                    cursor.execute("SELECT 1")
    except Exception:  # noqa: BLE001 - non-PostgreSQL or stub connections
        return


def _patch_django() -> None:
    """Gates `BaseHandler.get_response`, the one place a Django request ends."""
    try:
        from django.core.handlers.base import BaseHandler
    except Exception:  # noqa: BLE001 - no Django, or settings are unusable
        return
    original = BaseHandler.get_response
    if getattr(original, "_blastit_gated", False):
        return

    def get_response(self: Any, request: Any) -> Any:
        method = str(getattr(request, "method", None) or "GET")
        path = str(getattr(request, "path_info", None) or getattr(request, "path", "/"))
        if _CURRENT.get():
            return original(self, request)
        token = _CURRENT.set(True)
        pushed = False
        try:
            raw_ct = getattr(request, "content_type", None)
            content_type = str(raw_ct) if raw_ct else None
            raw_body = getattr(request, "body", b"") or b""
            total = len(raw_body)
            prefix = bytes(raw_body[:MAX_SAMPLE_BYTES]) if total else b""
            _materialise_test_transactions()
            _send({"op": "push_unrouted", "method": method, "path": path})
            pushed = True
            _append(_build_obs_line(method, path, content_type, total, prefix))
            return original(self, request)
        finally:
            if pushed:
                _pop_quietly()
            _CURRENT.reset(token)

    get_response._blastit_gated = True  # type: ignore[attr-defined]
    get_response.__doc__ = original.__doc__
    get_response.__wrapped__ = original  # type: ignore[attr-defined]
    BaseHandler.get_response = get_response


def _patch_flask() -> None:
    """Gates `Flask.wsgi_app`, which every request of a Flask app goes through."""
    try:
        from flask import Flask
    except Exception:  # noqa: BLE001 - Flask is optional, and usually absent
        return
    original = Flask.wsgi_app
    if getattr(original, "_blastit_gated", False):
        return

    def wsgi_app(self: Any, environ: dict[str, Any], start_response: Any) -> Any:
        if _CURRENT.get():
            return original(self, environ, start_response)
        token = _CURRENT.set(True)
        pushed = False
        spool = None
        try:
            content_type = _content_type(environ)
            prefix, total, spool = _replay_body(environ)
            _send(_target(environ))
            pushed = True
            _append(_request_line(environ, content_type, total, prefix))
            # Flask renders the whole response before `wsgi_app` returns, so the
            # request is over here rather than when its iterable is consumed.
            return original(self, environ, start_response)
        finally:
            if pushed:
                _pop_quietly()
            if spool is not None:
                spool.close()
            _CURRENT.reset(token)

    wsgi_app._blastit_gated = True  # type: ignore[attr-defined]
    wsgi_app.__doc__ = original.__doc__
    wsgi_app.__wrapped__ = original  # type: ignore[attr-defined]
    Flask.wsgi_app = wsgi_app


#: The Django handshake this run's `0600` file started, if there was one (`R30`).
#:
#: Read at import, before `pytest-django` builds Django's settings: that is the
#: only moment the real proxy inputs may exist in `os.environ` (`ADR-0004`).
_DJANGO: DjangoHandshake | None = begin_django_handshake()
