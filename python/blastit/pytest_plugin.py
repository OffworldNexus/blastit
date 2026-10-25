"""blastit's pytest plugin (D3b): what the tests do, against what the manifest allows.

Writing ``blastit.toml`` by hand from production denial logs is a loop:
deploy, hit an endpoint, read ``sql.denied``, add a grant, repeat — and each
pass only reveals the first denial of each request (ARCHITECTURE.md §6,
"Writing the manifest"). This plugin closes the loop before the deploy: it
records every request the test suite makes, with the SQL each one sent, and
diffs that against the manifest with **blastit's own** routing and SQL
matching — the same ``RoutingTable``, the same effective grants, the same
normaliser. What comes out is the exact list of statements to add, and the list
of grants nothing used.

It is inert unless it is asked for::

    pytest --blastit-manifest blastit.toml

or, in ``pytest.ini``/``pyproject.toml``::

    blastit_manifest = blastit.toml

With the manifest in place, in a project already using pytest-django:

* every request Django handles is recorded — ``django.test.Client``,
  ``live_server`` in its own thread, a view called directly as long as it goes
  through ``BaseHandler.get_response`` — with its method, its ``PATH_INFO`` and
  the pytest node id of the test that made it;
* every SQL statement executed during that request is recorded, on every
  database alias, **as it goes on the wire**: psycopg's own converter turns
  ``%s``/``%(name)s`` into ``$n`` exactly as it does with
  ``OPTIONS = {"server_side_binding": True}``, which is what blastit needs
  (§6, "What the app must do");
* statements executed outside a request — fixture setup, teardown, a
  ``manage.py``-style command — are ignored, because no route is responsible
  for them;
* at the end of the session, the diff is written as JSON (``--blastit-report``,
  default ``blastit-report.json``) and summarised on the terminal.

The plugin does not patch ``django.test.Client``: it patches
``BaseHandler.get_response``, so anything that ends up in Django is seen,
whatever called it. Per-request state lives in a :mod:`contextvars` variable, so
requests served by ``live_server`` in another thread — the case a test suite
that also uses Playwright always has — never mix with the test thread's own.

Statements psycopg runs by itself
--------------------------------

Django's ``execute_wrapper`` only sees the statements that go through its own
cursors, and those are not all of them: psycopg sends ``SET``s of its own while
a connection opens, and a backend looks a type's OID up at connect with
``TypeInfo.fetch`` (postgis, hstore — psycopg's ``Connection.execute``, which no
Django cursor of that request sees). Under blastit every request opens its own
connection (``CONN_MAX_AGE = 0``, §6), so in production those statements run on
**every** route, once per request; in a test suite the connection is reused and
they run once, usually outside any request at all.

So the plugin also wraps ``psycopg.Cursor.execute``/``executemany``/``stream``
and ``psycopg.Connection.execute`` — the outermost wrapper records and the inner
ones stay quiet, so a statement is counted once — and it records what runs while
``DatabaseWrapper.connect()`` is in progress as a separate ``connection_setup``
set. They are not a route's statements, so the report does not diff them against
one route: it lists the routes that do **not** grant them, and the fix is a
group attached to every route (``groups = ["connection"]`` at the top level,
§6). ``--blastit-strict`` fails while one is missing.

Options
-------

``--blastit-manifest PATH``
    The ``blastit.toml`` to diff against. Without it (and without the
    ``blastit_manifest`` ini option) the plugin does nothing at all.

``--blastit-report PATH``
    Where to write the JSON report. Default ``blastit-report.json``.

``--blastit-proxy ALIAS=PROXY``
    Maps a Django database alias to a manifest proxy. Needed when the manifest
    declares more than one ``postgresql`` proxy; with exactly one, every alias
    is that one and the option is not needed.

``--blastit-strict``
    Fails the session (non-zero exit code) when anything is missing or
    unrouted: the manifest is not finished yet.

`pytest-xdist <https://pytest-xdist.readthedocs.io/>`_ works: each worker writes
the requests it saw next to the report path, and the controller merges them and
reports once, so ``pytest -n auto --blastit-manifest blastit.toml`` gives one
report for the whole run.

Transaction control
-------------------

pytest-django runs each test inside a transaction, so a request never sends its
own ``BEGIN``/``COMMIT``: what a nested ``atomic`` sends is a ``SAVEPOINT``, and
that is what the report looks for. A route whose requests ran a savepoint (or
any other transaction-control statement) without the grant allowing it is
reported as ``needs_transaction_control``. When the alias itself has
``ATOMIC_REQUESTS = True`` the request is marked as needing a transaction even
if the test's own atomic swallowed the savepoint, because under blastit there is
no test transaction in the way.

The report
----------

See `blastit._blastit.audit` — the Rust module
``crates/blastit-py/src/audit.rs`` — for the full JSON schema. The stable keys
are::

    {"version": 1,
     "manifest": {"routes": N, "proxies": {...}, "warnings": [...]},
     "script_name": null,
     "routes": [{"name": ..., "path": [...], "requests": N,
                 "needs_transaction_control": bool,
                 "proxies": [{"name": ..., "observed": N,
                              "missing": [{"sql": ..., "count": N,
                                           "wire_sql": ..., "tests": [...],
                                           "paths": [...]}],
                              "unused": [...],
                              "transaction_control": {"observed": bool,
                                                      "granted": bool,
                                                      "needed": bool}}]}],
     "unrouted": [{"method": ..., "path_info": ..., "count": N,
                   "statements": N, "tests": [...]}],
     "unexercised_routes": [...],
     "connection_setup": {"statements": [{"sql": ..., "proxy": ..., "count": N,
                                          "wire_sql": ...,
                                          "ungranted_in": [...],
                                          "granted_in": N}],
                          "ungranted": N},
     "totals": {"requests": N, "routed": N, "unrouted": N, "routes": N,
                "routes_exercised": N, "statements": N, "missing": N,
                "missing_distinct": N, "unused": N,
                "needs_transaction_control": [...], "connection_setup": N,
                "connection_setup_ungranted": N, "clean": bool}}

``missing[].sql`` is the *normalised* text: it is what a grant has to contain,
so it can be pasted into the manifest as it is. ``wire_sql`` is one raw
occurrence, for a human who wants to see the placeholders as they were sent.
"""

from __future__ import annotations

import json
import re
import sys
import threading
from contextvars import ContextVar
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

import pytest

__all__ = ["pytest_addoption", "pytest_configure", "pytest_unconfigure"]

# The observations document and the report are versioned together, and the
# extension refuses to read a version it does not know (crates/blastit-py).
_OBSERVATIONS_VERSION = 1

#: Django's alias for the connection it opens *without* a database, to create the
#: test database (`django.db.backends.base.base.NO_DB_ALIAS`). What it runs is
#: test machinery, never request traffic, and it names no manifest proxy.
NO_DB_ALIAS = "__no_db__"

#: Where the JSON report goes when `--blastit-report` is not given.
DEFAULT_REPORT = "blastit-report.json"

#: `%s`, `%(name)s`, `%b` and `%t` into `$n`, and `%%` into `%`, for the
#: fallback used when psycopg's own converter is not importable. It scans the
#: text without knowing SQL, like psycopg does, so a literal `%` is `%%` on both
#: sides.
_PLACEHOLDER = re.compile(r"%%|%\(([^)]*)\)[sbt]|%[sbt]")


def _regex_wire(sql: str) -> str:
    """Rewrites a query's placeholders the way psycopg does, without psycopg."""
    index = 0
    names: dict[str, int] = {}

    def replace(match: re.Match[str]) -> str:
        nonlocal index
        if match.group(0) == "%%":
            return "%"
        name = match.group(1)
        if name is None:
            index += 1
            return f"${index}"
        if name not in names:
            index += 1
            names[name] = index
        return f"${names[name]}"

    return _PLACEHOLDER.sub(replace, sql)


def _psycopg_converter() -> Callable[..., Any] | None:
    """psycopg's query-to-wire converter, or `None` when psycopg is not there.

    psycopg 3 keeps it private (`psycopg._queries._query2pg`, `psycopg._query`
    before 3.3), and it is the only thing that converts a query exactly the way
    psycopg sends it with server-side binding. The fallback is
    :func:`_regex_wire`.
    """
    try:
        from psycopg import _queries as module  # type: ignore[attr-defined]
    except ImportError:  # pragma: no cover - psycopg 3.0 to 3.2
        try:
            from psycopg import _query as module  # type: ignore[attr-defined]
        except ImportError:
            return None
    for name in ("_query2pg_nocache", "_query2pg"):
        converter = getattr(module, name, None)
        if converter is not None:
            return converter
    return None


_CONVERTER: list[Any] = []


def to_wire_sql(sql: str, encoding: str = "utf-8") -> str:
    """Renders `sql` as psycopg sends it with server-side binding.

    ``%s`` and ``%(name)s`` become ``$1``, ``$2``… — named parameters numbered
    in order of first appearance, which is what psycopg does and therefore the
    order a grant has to be written in — and ``%%`` becomes ``%``. psycopg's own
    converter is used when it can be imported, so the result is what libpq
    really receives; :func:`_regex_wire` is the fallback.

    Raises nothing: a query psycopg's converter refuses (a mix of positional and
    named placeholders, say) falls back to the regular expression rather than
    failing a test run.
    """
    if not _CONVERTER:
        _CONVERTER.append(_psycopg_converter())
    converter = _CONVERTER[0]
    if converter is not None:
        try:
            return converter(sql.encode(encoding), encoding)[0].decode(encoding)
        except Exception:  # noqa: BLE001 - any converter failure falls back
            pass
    return _regex_wire(sql)


@dataclass
class _RequestContext:
    """What one request did, kept per thread and per request."""

    statements: list[tuple[str, str]] = field(default_factory=list)

    def add(self, alias: str, sql: str) -> None:
        """Records one statement as executed by `alias`."""
        self.statements.append((alias, sql))


#: The request being handled in this thread, or `None` outside a request.
_CURRENT: ContextVar[_RequestContext | None] = ContextVar("blastit_request", default=None)

#: The Django alias whose connection is being established, or `None`.
#:
#: A connection-setup statement is not a route's: under blastit every request
#: opens its own connection (§6, "What the app must do"), so these run on every
#: route, once per request — while a test suite reuses one connection and runs
#: them outside any request. They are reported separately, and the report says
#: which routes do not grant them.
_CONNECTING: ContextVar[str | None] = ContextVar("blastit_connecting", default=None)

#: How many capture wrappers are on the stack for the statement being run.
#:
#: Django's cursor wrapper and psycopg's own methods see the same statement, and
#: `Connection.execute` delegates to `Cursor.execute`: the outermost wrapper
#: records it, the inner ones see a non-zero depth and stay quiet, so one
#: statement is one line however many layers saw it.
_DEPTH: ContextVar[int] = ContextVar("blastit_depth", default=0)


@dataclass
class _ObservedRequest:
    """One request, as the observations document carries it."""

    method: str
    path_info: str
    nodeid: str | None
    transaction_control: bool
    statements: list[tuple[str, str]]

    def as_json(self, proxy_for: Callable[[str], str | None]) -> dict[str, Any]:
        """The request as JSON, with every statement's proxy resolved."""
        return {
            "method": self.method,
            "path_info": self.path_info,
            "nodeid": self.nodeid,
            "transaction_control": self.transaction_control,
            "statements": [
                {"alias": alias, "proxy": proxy_for(alias), "sql": sql}
                for alias, sql in self.statements
            ],
        }


class _Session:
    """Everything one pytest session recorded, and the options it runs with."""

    def __init__(self, config: pytest.Config) -> None:
        """Reads the options and the manifest; writes nothing yet."""
        self.config = config
        self.manifest_path = _manifest_path(config)
        self.manifest = self.manifest_path.read_text(encoding="utf-8")
        report = config.getoption("blastit_report")
        self.report_path = Path(report)
        self.strict = bool(config.getoption("blastit_strict"))
        self.proxies = _proxy_options(config.getoption("blastit_proxy") or [])
        self.declared = _postgresql_proxies(self.manifest)
        _check_proxy_mapping(self)
        self.observations: list[_ObservedRequest] = []
        self.connection_setup: dict[tuple[str, str], int] = {}
        self.lock = threading.Lock()
        self.nodeid: str | None = None
        self.installed = False
        self.deferred: str | None = None
        self.unmapped: set[str] = set()
        self.problems: list[str] = []

    def proxy_for(self, alias: str) -> str | None:
        """The manifest proxy a Django database alias talks to.

        `--blastit-proxy` wins; with exactly one `postgresql` proxy in the
        manifest, every alias is that one. Anything else is a request to say
        which proxy an alias uses, and is reported at the end of the session.
        """
        if alias in self.proxies:
            return self.proxies[alias]
        if len(self.declared) == 1:
            return self.declared[0]
        self.unmapped.add(alias)
        return None

    def record(self, request: Any, context: _RequestContext) -> None:
        """Adds one handled request to the observations."""
        transaction_control = any(
            _atomic_requests(alias) for alias, _ in context.statements
        )
        observed = _ObservedRequest(
            method=_request_method(request),
            path_info=_request_path_info(request),
            nodeid=self.nodeid,
            transaction_control=transaction_control,
            statements=list(context.statements),
        )
        with self.lock:
            self.observations.append(observed)

    def record_connection_setup(self, alias: str | None, sql: str) -> None:
        """Counts one statement psycopg ran as a connection was established.

        Aggregated here rather than kept per occurrence: these are the same few
        statements over and over, and what matters is which ones a route does not
        grant, not when they ran.
        """
        key = (alias or "", sql)
        with self.lock:
            self.connection_setup[key] = self.connection_setup.get(key, 0) + 1

    def document(self) -> dict[str, Any]:
        """The observations document the extension reads."""
        with self.lock:
            requests = [observed.as_json(self.proxy_for) for observed in self.observations]
            setup = sorted(self.connection_setup.items())
        return {
            "version": _OBSERVATIONS_VERSION,
            "requests": requests,
            "connection_setup": [
                {
                    "alias": alias or None,
                    "proxy": self.proxy_for(alias) if alias else None,
                    "sql": sql,
                    "count": count,
                }
                for (alias, sql), count in setup
            ],
        }


#: The session in progress, or `None` when the plugin is inert.
_SESSION: _Session | None = None


def _manifest_path(config: pytest.Config) -> Path:
    """The manifest to use: `--blastit-manifest`, or the ini option.

    Raises `pytest.UsageError` when the configured file is not there, because
    the alternative is a report that silently diffs against nothing.
    """
    from_option = config.getoption("blastit_manifest")
    from_ini = config.getini("blastit_manifest")
    raw = from_option or from_ini
    path = Path(str(raw))
    if not path.is_file():
        raise pytest.UsageError(
            f"blastit: the manifest {str(path)!r} is not a file (from "
            f"{'--blastit-manifest' if from_option else 'blastit_manifest'})"
        )
    return path


def _proxy_options(values: list[str]) -> dict[str, str]:
    """Reads `--blastit-proxy alias=proxy` values.

    Raises `pytest.UsageError` on anything that is not `alias=proxy`, because a
    typo there would silently attribute statements to the wrong proxy.
    """
    mapping: dict[str, str] = {}
    for value in values:
        alias, separator, proxy = value.partition("=")
        if not separator or not alias.strip() or not proxy.strip():
            raise pytest.UsageError(
                f"blastit: --blastit-proxy expects alias=proxy, got {value!r}"
            )
        mapping[alias.strip()] = proxy.strip()
    return mapping


def _postgresql_proxies(manifest: str) -> list[str]:
    """The names of the `postgresql` proxies a manifest declares."""
    import tomllib

    try:
        document = tomllib.loads(manifest)
    except tomllib.TOMLDecodeError as error:
        raise pytest.UsageError(f"blastit: the manifest is not valid TOML: {error}") from error
    proxies = document.get("proxy")
    if not isinstance(proxies, dict):
        return []
    return [
        str(name)
        for name, declaration in proxies.items()
        if isinstance(declaration, dict) and declaration.get("protocol") == "postgresql"
    ]


def _check_proxy_mapping(session: _Session) -> None:
    """Refuses a mapping that cannot be resolved, before any test runs.

    Raises `pytest.UsageError` when the manifest declares several `postgresql`
    proxies and `--blastit-proxy` was not used at all, or when one of the names
    it maps to does not exist.
    """
    if not session.declared:
        raise pytest.UsageError(
            "blastit: the manifest declares no postgresql proxy, so no observed "
            "statement could be checked against a grant"
        )
    for alias, proxy in session.proxies.items():
        if proxy not in session.declared:
            raise pytest.UsageError(
                f"blastit: --blastit-proxy maps {alias!r} to {proxy!r}, which is not "
                f"a postgresql proxy of the manifest (it declares: "
                f"{', '.join(session.declared)})"
            )
    if len(session.declared) > 1 and not session.proxies:
        raise pytest.UsageError(
            "blastit: the manifest declares "
            f"{len(session.declared)} postgresql proxies "
            f"({', '.join(session.declared)}); pass --blastit-proxy alias=proxy for "
            "each database alias the tests use"
        )


def _request_method(request: Any) -> str:
    """The method of a handled request, whatever kind of request it is."""
    method = getattr(request, "method", None)
    return str(method) if method else "GET"


def _request_path_info(request: Any) -> str:
    """The `PATH_INFO` of a handled request, as the app saw it.

    blastit's view strips `BLASTIT_SCRIPT_NAME` before WSGI (§2.5), so this is
    the same string the scheduler routed on — which is why the report needs no
    mount prefix.
    """
    path = getattr(request, "path_info", None)
    if path is None:
        path = getattr(request, "path", "")
    return str(path)


def _atomic_requests(alias: str) -> bool:
    """Whether a database alias runs every request in a transaction."""
    from django.db import connections

    try:
        return bool(connections[alias].settings_dict.get("ATOMIC_REQUESTS"))
    except Exception:  # noqa: BLE001 - an unknown alias just is not atomic
        return False


def _patch_handler() -> None:
    """Records every request Django handles through `BaseHandler.get_response`.

    The patch is on the class, so `django.test.Client`, `live_server`'s own
    thread and a view called by hand are all covered by construction. The
    context is a contextvar, so a request served on another thread cannot see
    (or pollute) the thread that made it.
    """
    from django.core.handlers.base import BaseHandler

    original = BaseHandler.get_response
    if getattr(original, "_blastit_patched", False):
        return

    def get_response(self: Any, request: Any) -> Any:
        context = _RequestContext()
        token = _CURRENT.set(context)
        try:
            return original(self, request)
        finally:
            _CURRENT.reset(token)
            session = _SESSION
            if session is not None:
                try:
                    session.record(request, context)
                except Exception as error:  # noqa: BLE001
                    # Never break the application under test because of the
                    # plugin: report it, and carry on.
                    session.problems.append(
                        f"could not record a request to {_request_path_info(request)!r}: "
                        f"{error!r}"
                    )

    get_response._blastit_patched = True  # type: ignore[attr-defined]
    get_response.__doc__ = original.__doc__
    get_response.__wrapped__ = original  # type: ignore[attr-defined]
    BaseHandler.get_response = get_response


def _patch_connect() -> None:
    """Marks the statements run while a Django connection is being established.

    `DatabaseWrapper.connect()` calls the backend's `get_new_connection`, then
    `init_connection_state()`, then sends `connection_created` — and psycopg
    sends `SET`s of its own as the connection opens. All of that is
    connection-setup SQL, and none of it goes through a request's cursors in
    production and tests the same way, so it is recorded apart from the requests.
    """
    from django.db.backends.base.base import BaseDatabaseWrapper

    original = BaseDatabaseWrapper.connect
    if getattr(original, "_blastit_patched", False):
        return

    def connect(self: Any) -> Any:
        token = _CONNECTING.set(str(self.alias))
        try:
            return original(self)
        finally:
            _CONNECTING.reset(token)

    connect._blastit_patched = True  # type: ignore[attr-defined]
    connect.__doc__ = original.__doc__
    connect.__wrapped__ = original  # type: ignore[attr-defined]
    BaseDatabaseWrapper.connect = connect


def _capture(alias: str | None, sql: Any, params: Any, connection: Any = None) -> None:
    """Records one statement, wherever it came from.

    Called by both capture layers, outermost first; only the outermost one
    records. A statement run while a connection is being established goes to the
    session's connection-setup set — whether or not a request is in flight,
    because it is not the request's — and any other statement goes to the request
    that is running, or nowhere when none is.
    """
    if _DEPTH.get():
        return
    session = _SESSION
    if session is None:
        return
    text = _sql_text(sql, connection)
    if text is None:
        return
    wire = to_wire_sql(text) if params is not None else text

    connecting = _CONNECTING.get()
    if connecting == NO_DB_ALIAS:
        # Django opens one connection *without* a database to create the test
        # database. It is test machinery, it never runs under blastit (the
        # database is already there), and its alias names no proxy.
        return
    if connecting is not None:
        session.record_connection_setup(connecting or alias, wire)
        return
    current = _CURRENT.get()
    if current is not None:
        current.add(alias or "", wire)


def _sql_text(sql: Any, connection: Any) -> str | None:
    """The text of a query, whichever form it came in.

    A `str` is what Django and most application code pass. psycopg also accepts
    `sql.Composable` objects, and renders them against the connection — which is
    what psycopg itself does before it converts the placeholders; the result
    still has its `%s`, so the same conversion and the same grant comparison
    apply. Anything else (a `Template`) has no text to compare: it is left alone
    rather than guessed at.
    """
    if isinstance(sql, str):
        return sql
    render = getattr(sql, "as_string", None)
    if render is None or connection is None:
        return None
    try:
        return str(render(connection))
    except Exception:  # noqa: BLE001 - a query we cannot render is one we skip
        return None


def _wrap_connection(connection: Any) -> None:
    """Records every statement executed on one database connection.

    The wrapper is installed permanently (`connection.execute_wrappers`), so it
    covers the whole lifetime of the connection, reconnects included.
    """
    if getattr(connection, "_blastit_wrapped", False):
        return
    alias = str(connection.alias)

    def wrapper(execute: Any, sql: str, params: Any, many: bool, context: Any) -> Any:
        # `params is None` means there is nothing to bind, and psycopg sends the
        # text as it is; with parameters, the text that goes on the wire has
        # `$n` placeholders, which is what a grant has to match. `executemany`
        # is one statement, however many rows it carries.
        _capture(alias, sql, params, getattr(connection, "connection", None))
        token = _DEPTH.set(_DEPTH.get() + 1)
        try:
            return execute(sql, params, many, context)
        finally:
            _DEPTH.reset(token)

    connection.execute_wrappers.append(wrapper)
    connection._blastit_wrapped = True  # type: ignore[attr-defined]


def _psycopg_connection(target: Any) -> Any:
    """The psycopg connection behind a cursor, or `target` itself when it is one."""
    import psycopg

    return target if isinstance(target, psycopg.Connection) else target.connection


def _alias_of(connection: Any) -> str | None:
    """The Django alias of a psycopg connection, when Django owns it.

    Looked up rather than remembered by `id()`: psycopg connections are created
    and dropped (one per request under blastit), and an address can be reused by
    a connection of another alias. An app that talks to psycopg directly, or a
    run with no Django at all, simply has no alias.
    """
    try:
        from django.db import connections

        for wrapper in connections.all():
            if getattr(wrapper, "connection", None) is connection:
                return str(wrapper.alias)
    except Exception:  # noqa: BLE001 - no Django, or Django without settings
        return None
    return None


def _declared_statement(cursor: Any, query: Any, connection: Any) -> Any:
    """What psycopg sends for a server-side cursor: a `DECLARE … CURSOR FOR …`.

    `ServerCursor.execute` does not go through a cursor's `execute` at all: it
    builds one `DECLARE` statement and sends that. Recording the query the
    caller wrote would name a statement that never reached PostgreSQL — a grant
    for it could not match — so psycopg's own rendering of the statement is
    used, and the caller's query only if it cannot be built.
    """
    make = getattr(cursor, "_make_declare_statement", None)
    if make is None:
        return query
    try:
        return make(query)
    except Exception:  # noqa: BLE001 - a DECLARE we cannot build is not one we guess
        return query


def _wrap_psycopg_method(
    target: Any,
    name: str,
    *,
    streaming: bool = False,
    statement: Any = None,
) -> None:
    """Wraps one psycopg method so a statement is recorded as it is sent."""
    original = getattr(target, name, None)
    if original is None or getattr(original, "_blastit_patched", False):
        return

    if streaming:

        def wrapper(self: Any, *args: Any, **kwargs: Any) -> Any:
            connection = _psycopg_connection(self)
            query, params = query_params(args, kwargs)
            if statement is not None:
                query = statement(self, query, connection)
            _capture(_alias_of(connection), query, params, connection)
            yield from original(self, *args, **kwargs)

    else:

        def wrapper(self: Any, *args: Any, **kwargs: Any) -> Any:
            connection = _psycopg_connection(self)
            query, params = query_params(args, kwargs)
            if statement is not None:
                query = statement(self, query, connection)
            _capture(_alias_of(connection), query, params, connection)
            token = _DEPTH.set(_DEPTH.get() + 1)
            try:
                return original(self, *args, **kwargs)
            finally:
                _DEPTH.reset(token)

    wrapper._blastit_patched = True  # type: ignore[attr-defined]
    wrapper.__doc__ = original.__doc__
    wrapper.__wrapped__ = original  # type: ignore[attr-defined]
    setattr(target, name, wrapper)


def query_params(args: tuple[Any, ...], kwargs: dict[str, Any]) -> tuple[Any, Any]:
    """The `(query, params)` of a psycopg call, whatever the signature is.

    psycopg's methods take `(query, params=None, ...)` positionally, but changes
    between versions (and keyword-only options) mean the only safe reading is
    "the first argument is the query, the second — if there is one — the
    parameters".
    """
    query = args[0] if args else kwargs.get("query")
    params = args[1] if len(args) > 1 else kwargs.get("params")
    return query, params


def _patch_psycopg() -> None:
    """Records the statements psycopg runs itself, not only Django's cursors.

    Django's `execute_wrapper` only sees statements that go through its own
    cursors. psycopg runs some of its own: `TypeInfo.fetch`, which the PostGIS
    and hstore backends use to look up a type's OID at connect, and the
    connection's own `SET`s. Those are exactly the statements a manifest has to
    grant — they go on the wire, so dbproxy checks them — and the plugin could
    not see them before.

    `Cursor.execute`, `executemany` and `stream` are short methods with stable
    signatures; the wrapper reads the query and the parameters positionally and
    leaves the call itself alone. `ServerCursor` inherits from `Cursor`, and
    `Connection.execute` delegates to a cursor, so the outermost wrapper records
    and the inner ones stay quiet.
    """
    try:
        import psycopg
    except ImportError:
        return
    _wrap_psycopg_method(psycopg.Cursor, "execute")
    _wrap_psycopg_method(psycopg.Cursor, "executemany")
    _wrap_psycopg_method(psycopg.Cursor, "stream", streaming=True)
    _wrap_psycopg_method(psycopg.Connection, "execute")
    server_cursor = getattr(psycopg, "ServerCursor", None)
    if server_cursor is not None:
        _wrap_psycopg_method(server_cursor, "execute", statement=_declared_statement)


def _on_connection_created(sender: Any = None, connection: Any = None, **_: Any) -> None:
    """Wraps every connection Django opens, in whichever thread opens it.

    A module-level function, and not a closure over the session: Django's signals
    hold receivers weakly, so a receiver built inside `_install` would be
    collected as soon as `_install` returned, and `live_server`'s connection
    (opened on its own thread, after the fact) would never be wrapped.
    """
    if connection is not None and _SESSION is not None:
        _wrap_connection(connection)


def _install(session: _Session) -> None:
    """Patches Django and its connections, once Django is usable.

    Called from the hooks that run early and often: pytest-django may not have
    configured Django settings by the time `pytest_configure` runs, in which case
    the next hook tries again. Reconnectable, because `connection_created` fires
    for every new connection, `live_server`'s thread included.
    """
    if session.installed:
        return
    try:
        from django.core.handlers.base import BaseHandler  # noqa: F401
        from django.db.backends.signals import connection_created
    except ImportError as error:
        raise pytest.UsageError(
            "blastit: the plugin needs Django in the test environment to record "
            f"requests: {error}"
        ) from error
    except Exception as error:  # noqa: BLE001 - usually an unconfigured Django
        session.deferred = str(error)
        return

    try:
        _patch_handler()
        _patch_connect()
        _patch_psycopg()
        connection_created.connect(
            _on_connection_created, dispatch_uid="blastit_pytest_plugin"
        )
        _wrap_existing_connections()
    except Exception as error:  # noqa: BLE001 - usually an unconfigured Django
        session.deferred = str(error)
        return
    session.installed = True


def _wrap_existing_connections() -> None:
    """Wraps the connections pytest-django may have opened before us.

    `connection_created` only fires on new connections; anything already there
    (a `django_db_setup` fixture that ran before the first request) would stay
    unwrapped.
    """
    from django.db import connections

    for connection in connections.all():
        _wrap_connection(connection)


def pytest_addoption(parser: pytest.Parser) -> None:
    """Declares the plugin's options; registers no behaviour on its own."""
    group = parser.getgroup("blastit")
    group.addoption(
        "--blastit-manifest",
        metavar="PATH",
        default=None,
        help="blastit.toml to diff the requests of this test suite against; "
        "without it the plugin does nothing",
    )
    group.addoption(
        "--blastit-report",
        metavar="PATH",
        default=DEFAULT_REPORT,
        help=f"where to write the JSON report (default: {DEFAULT_REPORT})",
    )
    group.addoption(
        "--blastit-proxy",
        metavar="ALIAS=PROXY",
        action="append",
        default=[],
        help="map a Django database alias to a manifest proxy (repeatable; only "
        "needed when the manifest declares several postgresql proxies)",
    )
    group.addoption(
        "--blastit-strict",
        action="store_true",
        default=False,
        help="fail the session when anything is missing or unrouted",
    )
    parser.addini(
        "blastit_manifest",
        "path of the blastit.toml to diff the test suite against",
        default="",
    )


def pytest_configure(config: pytest.Config) -> None:
    """Starts recording, when a manifest was given. Otherwise: nothing."""
    global _SESSION
    _SESSION = None
    if not config.getoption("blastit_manifest") and not config.getini("blastit_manifest"):
        if config.getoption("blastit_proxy") or config.getoption("blastit_strict"):
            raise pytest.UsageError(
                "blastit: --blastit-proxy and --blastit-strict need "
                "--blastit-manifest (or blastit_manifest in the configuration)"
            )
        return
    session = _Session(config)
    _SESSION = session
    _install(session)


def pytest_unconfigure(config: pytest.Config) -> None:
    """Forgets the session, so a later session in the same process starts clean."""
    global _SESSION
    _SESSION = None


def pytest_runtest_logstart(nodeid: str, location: tuple[str, int | None, str]) -> None:
    """Remembers which test is running, for the requests it makes.

    `live_server` serves requests on its own thread and cannot know; the node id
    is a property of the test, not of the thread.
    """
    if _SESSION is not None:
        _SESSION.nodeid = nodeid
        _install(_SESSION)


def pytest_runtest_logfinish(nodeid: str, location: tuple[str, int | None, str]) -> None:
    """Forgets the test, so nothing after it is attributed to it."""
    if _SESSION is not None:
        _SESSION.nodeid = None


def pytest_collection_finish(session: pytest.Session) -> None:
    """Installs the recording once Django is certainly usable."""
    if _SESSION is not None:
        _install(_SESSION)


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> None:
    """Writes the report and the summary, or hands the observations to the controller."""
    if _SESSION is None:
        return
    if hasattr(session.config, "workerinput"):
        _write_worker_observations(_SESSION)
        return
    _finish(session, _SESSION)


def _worker_id(config: pytest.Config) -> str:
    """The xdist worker id, `gw0` and friends."""
    return str(config.workerinput.get("workerid", "worker"))  # type: ignore[attr-defined]


def _write_worker_observations(session: _Session) -> None:
    """Saves one xdist worker's observations for the controller to merge.

    The file sits next to the report and is named after the worker; the
    controller reads them all, merges them in worker order and deletes them, so
    a run reports once.
    """
    document = session.document()
    document["worker"] = _worker_id(session.config)
    path = Path(f"{session.report_path}.{document['worker']}")
    _write_json(path, document)


def _merged_observations(session: _Session) -> dict[str, Any]:
    """This process's observations, plus every xdist worker's, merged.

    Workers write next to the report (`<report>.<workerid>`); they are read,
    deleted, and their requests appended in worker order, so the controller
    reports once for the whole run.
    """
    document = session.document()
    report = session.report_path
    workers = sorted(path for path in report.parent.glob(f"{report.name}.*"))
    for path in workers:
        try:
            theirs = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            session.problems.append(f"could not read the worker report {path}: {error!r}")
            continue
        for request in theirs.get("requests", []):
            document["requests"].append(request)
        for statement in theirs.get("connection_setup", []):
            document["connection_setup"].append(statement)
    for path in workers:
        try:
            path.unlink()
        except OSError as error:  # pragma: no cover - best effort cleanup
            session.problems.append(f"could not remove the worker report {path}: {error!r}")
    document["workers"] = [path.name for path in workers]
    return document


def _write_json(path: Path, document: dict[str, Any]) -> None:
    """Writes one JSON document, creating its directory if it has one."""
    if path.parent != Path():
        path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def _audit(session: _Session, document: dict[str, Any]) -> dict[str, Any]:
    """Runs blastit's own audit, in the compiled extension.

    The mount prefix is left to the report: the observations carry the
    `PATH_INFO` the app saw, and blastit's view already stripped
    `BLASTIT_SCRIPT_NAME` from it before WSGI (§2.5), so it is the same string
    the scheduler routed on.
    """
    from blastit import _blastit

    report = _blastit.audit(session.manifest, json.dumps(document), None)
    return json.loads(report)


def _finish(session: pytest.Session, state: _Session) -> None:
    """Writes the JSON report, prints the summary, and honours `--blastit-strict`."""
    document = _merged_observations(state)
    if not state.installed:
        state.problems.append(
            "the plugin never managed to record Django's requests"
            + (f": {state.deferred}" if state.deferred else " (Django was never usable)")
        )
    if state.unmapped:
        state.problems.append(
            "no proxy for the database alias(es) "
            + ", ".join(sorted(repr(alias) for alias in state.unmapped))
            + ": the manifest declares several postgresql proxies, so pass "
            "--blastit-proxy alias=proxy"
        )
    try:
        report = _audit(state, document)
    except Exception as error:  # noqa: BLE001 - reported, never raised mid-session
        state.problems.append(f"the audit failed: {error}")
        report = None

    lines: list[tuple[str, bool]] = []
    if report is not None:
        _write_json(state.report_path, report)
        lines = _summary(report, str(state.report_path))
    for problem in state.problems:
        lines.append((f"blastit: {problem}", True))

    strict_failure = report is not None and state.strict and not report["totals"]["clean"]
    if strict_failure:
        totals = report["totals"]
        lines.append(
            (
                "blastit: --blastit-strict: "
                f"{totals['missing']} statement execution(s) missing, "
                f"{totals['unrouted']} unrouted request(s) and "
                f"{totals['connection_setup_ungranted']} connection-setup "
                "statement(s) not granted in every route",
                True,
            )
        )

    _print_lines(session.config, lines)
    if report is None or state.problems or strict_failure:
        session.exitstatus = pytest.ExitCode.TESTS_FAILED


def _summary(report: dict[str, Any], path: str) -> list[tuple[str, bool]]:
    """The terminal summary: short, and actionable.

    Each line says what to do — add the statements a route is missing, look at
    the routes nothing exercised — and where the detail is.
    """
    totals = report["totals"]
    lines: list[tuple[str, bool]] = [
        (
            f"blastit: {totals['requests']} requests, {totals['statements']} statements, "
            f"{totals['routes_exercised']}/{totals['routes']} routes exercised — "
            f"report in {path}",
            False,
        )
    ]
    if totals["clean"]:
        lines.append(("blastit: the manifest covers every request that was made", False))
    for route in report["routes"]:
        missing = sum(len(proxy["missing"]) for proxy in route["proxies"])
        unused = sum(len(proxy["unused"]) for proxy in route["proxies"])
        if missing:
            lines.append(
                (
                    f"blastit: route {route['name']}: {missing} statement(s) missing "
                    f"(see {path})",
                    True,
                )
            )
        if route["needs_transaction_control"]:
            lines.append(
                (
                    f"blastit: route {route['name']}: needs transaction_control = true",
                    True,
                )
            )
        if unused:
            lines.append(
                (f"blastit: route {route['name']}: {unused} grant(s) unused", False)
            )
    if totals["unrouted"]:
        examples = ", ".join(
            f"{entry['method']} {entry['path_info']}" for entry in report["unrouted"][:3]
        )
        lines.append(
            (
                f"blastit: {totals['unrouted']} unrouted request(s): {examples} "
                f"(see {path})",
                True,
            )
        )
    if report["unexercised_routes"]:
        lines.append(
            (
                f"blastit: {len(report['unexercised_routes'])} route(s) never "
                f"exercised: {', '.join(report['unexercised_routes'])}",
                False,
            )
        )
    setup = report["connection_setup"]
    if setup["ungranted"]:
        lines.append(
            (
                f"blastit: {setup['ungranted']} connection-setup statement(s) some "
                "routes do not grant: put them in a group attached to every route "
                f"(see {path})",
                True,
            )
        )
    return lines


def _print_lines(config: pytest.Config, lines: list[tuple[str, bool]]) -> None:
    """Writes the summary to the terminal, when there is a terminal."""
    reporter = config.pluginmanager.getplugin("terminalreporter")
    for line, problem in lines:
        if reporter is not None:
            reporter.write_line(line, red=problem)
        else:  # pragma: no cover - a session without a terminal reporter
            print(line, file=sys.stderr)
