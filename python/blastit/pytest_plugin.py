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
a connection opens; a backend looks a type's OID up at connect with
``TypeInfo.fetch`` (postgis, hstore — psycopg's ``Connection.execute``, which no
Django cursor of that request sees); and, most importantly, with
``autocommit = False`` **psycopg sends the ``BEGIN`` and the ``COMMIT``** through
``Connection._exec_command``, so a request that runs an ``atomic()`` — Django's
``ATOMIC_REQUESTS``, or a handler that wraps its work in one — would otherwise
look like it never needed ``transaction_control``.

So the plugin also wraps ``psycopg.Cursor.execute``/``executemany``/``stream``/
``copy``, ``Connection.execute``/``copy``, ``ServerCursor.execute`` (whose
statement is the ``DECLARE`` psycopg really sends) and ``_exec_command`` (the
internal command channel, whose ``BEGIN``/``COMMIT``/``FETCH`` are recorded even
in the middle of a statement the caller wrote). One statement is recorded once:
each capture layer marks the call it is recording, and the inner layers skip a
*statement they have already seen* — the gate is per call, so a third-party
``execute_wrapper`` that runs a statement of its own is recorded too.

Under blastit every request opens its own connection (``CONN_MAX_AGE = 0``,
§6), so connection-setup statements run on **every** route, once per request; in
a test suite the connection is reused and they run once, usually outside any
request at all. The plugin records what runs while
``DatabaseWrapper.connect()`` is in progress as a separate ``connection_setup``
set, and the report does not diff it against one route: it lists the routes that
do **not** grant it, and the fix is a group attached to every route
(``groups = ["connection"]`` at the top level, §6).

Egress
------

A route that opens outbound connections needs ``egress = true`` (§7.3); without
it the seccomp filter refuses them. The plugin wraps ``socket.socket.connect``
(and ``connect_ex``, which ``socket.create_connection`` goes through), so every
outbound connection a request tries is reported per route, with the destination,
how often, and an example test. A unix socket is not egress: under blastit the
database itself is one.

Mocked HTTP clients — pytest-httpx, responses, a stub transport — never open a
socket, so the list is a **lower bound**: a route the report says nothing about
may still need ``egress = true`` in production.

A live server, served by blastit itself
--------------------------------------

Everything above records what happens *inside this process*. A test suite that
drives a live server — pytest-django's ``live_server``, Playwright against it —
makes its real traffic in a server process of its own, so the plugin cannot see
it, and the front-end paths nobody asserts on are exactly the ones that surprise
a deployment. The ``blastit_live_server`` fixture (D3c) closes that gap: it
starts the real ``blastit`` binary on a free port, with the project's manifest
and the *test* database, waits for its health path, yields the URL to point a
browser or an HTTP client at, and on teardown folds what the server logged — the
``request`` events and every ``sql.denied`` — into the same observations the
report is built from.

```python
from urllib.request import urlopen


def test_notes_page(blastit_live_server):
    with urlopen(f"{blastit_live_server.url}/notes/") as response:
        assert response.status == 200
```

or, when the suite already uses ``live_server`` everywhere:

```console
$ pytest --blastit-live-server --blastit-manifest blastit.toml
```

``--blastit-live-server`` makes ``live_server`` *be* blastit's, so a suite that
was written against pytest-django's is served through blastit without touching
the tests. A project can also say it in its own ``conftest.py``::

    from blastit.pytest_plugin import blastit_live_server as live_server  # noqa: F401

Two things it needs, because blastit is a sandbox and not a thread:

* **root**, or passwordless ``sudo``: init binds listeners, makes the view
  directories under ``/run/blastit`` and drops privileges to the view uids
  (§2.1, §4). A machine with neither is skipped, with the reason, rather than
  failed: what the fixture tests is the manifest, and it cannot be tested
  without those privileges at all.
* the **WSGI callable** to serve: ``WSGI_APPLICATION`` in the Django settings,
  or ``--blastit-live-app module:attribute``. Without either, ``myproject.wsgi:
  application`` is guessed from ``DJANGO_SETTINGS_MODULE``.

What the log cannot say is as important as what it says. A `sql.denied` event
carries the route, the proxy and the *normalised* statement — not the request
that made it — so the plugin attaches it to the first request the log shows for
that route; that is what puts a path next to the statement, and a route with no
request event at all is reported as a problem rather than dropped. The statement
is normalised, so it is what a grant has to hold — and
[`blastit.logs.parseable_sql`](logs.py) turns it back into SQL PostgreSQL's
parser accepts, so the route's `table_grants` snippet covers the denial too.

A log also says nothing about the statements that *ran*: it records the requests
that were served and the statements that were denied, never an allowed one. So a
route whose only evidence is the log — the common case for a suite whose traffic
all goes through the live server — reports its `missing` grants and no `unused`
ones, and `unused_global` stays empty for the proxies such a route has a grant
on: the report never calls a grant dead when nothing observed whether it was
used. The suite's own requests, the ones this process served, are evidence like
any other.

Options
-------

``--blastit-manifest PATH``
    The ``blastit.toml`` to diff against. Without it (and without the
    ``blastit_manifest`` ini option) the plugin does nothing at all: nothing is
    patched, nothing is written, no test sees a difference.

``--blastit-report PATH``
    Where to write the JSON report. Default ``blastit-report.json``, relative to
    the working directory (like pytest's own ``--junitxml``).

``--blastit-proxy ALIAS=PROXY``
    Maps a Django database alias to a manifest proxy. Needed when the manifest
    declares more than one ``postgresql`` proxy; with exactly one, every alias
    is that one and the option is not needed.

``--blastit-strict``
    Fails the session (non-zero exit code) when anything is missing, unrouted, a
    route needs ``transaction_control`` it was not granted, a route opens
    connections without ``egress = true``, a connection-setup statement is not
    granted in every route, a statement could not be read, or the plugin itself
    had to warn about something.

``--blastit-prefer-tables``
    Print the ``table_grants`` snippet of a route with missing statements, so the
    fix reads as a table grant rather than as a list of statements. It does not
    change what is *missing*: the audit already checks table grants as well as
    exact ones, so a statement a route's table grants cover is not missing at
    all.

``--blastit-live-server``
    Serve pytest-django's ``live_server`` fixture through blastit itself, so a
    suite written against it checks real front-end traffic against the manifest.
    Off by default: pytest-django's ``live_server`` runs the app in a thread of
    this process, which is faster and needs no privileges.

``--blastit-live-app MODULE:ATTR``
    The WSGI callable ``blastit_live_server`` should serve. Default: the Django
    setting ``WSGI_APPLICATION``, or ``<package>.wsgi:application`` derived from
    ``DJANGO_SETTINGS_MODULE``.

`pytest-xdist <https://pytest-xdist.readthedocs.io/>`_ works: each worker writes
the requests it saw next to the report path, and the controller merges them and
reports once, so ``pytest -n auto --blastit-manifest blastit.toml`` gives one
report for the whole run. Only files that say which worker wrote them *and*
carry the version this build knows are merged; anything else is left alone and
reported.

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
     "routes": [{"name": ..., "path": [...], "requests": N,
                 "egress": {"observed": [{"target": ..., "host": ..., "port": N,
                                          "family": ..., "count": N,
                                          "tests": [...]}],
                            "needed": bool},
                 "proxies": [{"name": ..., "observed": N,
                              "missing": [{"sql": ..., "count": N,
                                           "wire_sql": ..., "tests": [...],
                                           "paths": [...]}],
                              "unused": [...],
                              "transaction_control": {"observed": bool,
                                                      "granted": bool,
                                                      "needed": bool},
                              "table_grants": {
                                  "tables": {"orders_order": {"select": ["id"],
                                                              "delete": true}},
                                  "functions": ["unaccent"],
                                  "toml": "[route.db.tables]\\n…",
                                  "unanalysable": [{"sql": ...,
                                                    "reason": ...}]}}]}],
     "unrouted": [{"method": ..., "path_info": ..., "count": N,
                   "statements": N, "tests": [...]}],
     "unexercised_routes": [...],
     "unused_global": {"statements": [{"proxy": ..., "sql": ...,
                                       "groups": [...]}],
                       "tables": [{"proxy": ..., "group": ...,
                                   "tables": {...}, "functions": [...]}],
                       "count": N},
     "connection_setup": {"statements": [{"sql": ..., "proxy": ..., "count": N,
                                          "wire_sql": ...,
                                          "ungranted_in": [...],
                                          "granted_in": N}],
                          "ungranted": N},
     "unrecognised": {"statements": [{"query": ..., "alias": ..., "count": N}],
                      "count": N},
     "plugin_warnings": [...],
     "totals": {"requests": N, "routed": N, "unrouted": N, "routes": N,
                "routes_exercised": N, "statements": N, "missing": N,
                "missing_distinct": N, "unused": N, "unused_global": N,
                "needs_transaction_control": [...], "connection_setup": N,
                "connection_setup_ungranted": N, "egress": N, "unrecognised": N,
                "plugin_warnings": N, "clean": bool}}

``missing[].sql`` is the *normalised* text: it is what a grant has to contain,
so it can be pasted into the manifest as it is. ``wire_sql`` is one raw
occurrence, for a human who wants to see the placeholders as they were sent.

``table_grants`` is the other way to fix a route (D7): the smallest
``[route.<proxy>.tables]`` grant that covers every statement the route ran,
with the TOML to paste in. It is reported per ``(route, proxy)`` pair, because
that is what a grant is, and it is computed from **every** statement the pair
ran — the ones that matched a grant included — so a route can be rewritten from
exact queries to table grants in one step. A statement a table grant cannot
describe at all (a ``SET``, a ``COPY``, a statement this build does not
understand) is listed in ``unanalysable``, with the same reason a ``sql.denied``
event carries: the covering grant is then not a complete replacement for the
route's exact queries.
"""

from __future__ import annotations

import contextlib
import json
import os
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
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
    egress: list[tuple[str, str, int]] = field(default_factory=list)
    #: Whether this request went through Django's transaction API itself —
    #: `set_autocommit`, `commit`, `rollback`, a savepoint. A transaction that
    #: was already open when the request began is someone else's (in a test
    #: suite, pytest-django's), and under blastit there is no such thing.
    manages_transactions: bool = False

    def add(self, alias: str, sql: str) -> None:
        """Records one statement as executed by `alias`."""
        self.statements.append((alias, sql))

    def add_egress(self, family: str, host: str, port: int) -> None:
        """Records one outbound connection attempt."""
        self.egress.append((family, host, port))


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

#: The statement the outermost capture layer is recording right now, as
#: `(text, id(params))`, or `None`.
#:
#: Django's cursor wrapper, psycopg's own methods and `Connection.execute`
#: delegating to `Cursor.execute` all see the *same* call, and only one of them
#: may record it. The gate is per call, not a counter: another layer — a
#: third-party `execute_wrapper`, a `ServerCursor`'s `DECLARE`, psycopg's own
#: `BEGIN` — runs a *different* statement inside the marked call, and that
#: statement is recorded like any other.
_MARK: ContextVar[tuple[str, int] | None] = ContextVar("blastit_mark", default=None)


@dataclass
class _ObservedRequest:
    """One request, as the observations document carries it."""

    method: str
    path_info: str
    nodeid: str | None
    transaction_control: bool
    statements: list[tuple[str, str]]
    egress: list[tuple[str, str, int]]

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
            "egress": [
                {"family": family, "host": host, "port": port}
                for family, host, port in self.egress
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
        self.prefer_tables = bool(config.getoption("blastit_prefer_tables"))
        self.proxies = _proxy_options(config.getoption("blastit_proxy") or [])
        self.declared = _postgresql_proxies(self.manifest)
        _check_proxy_mapping(self)
        self.observations: list[_ObservedRequest] = []
        #: What a live server's log recorded (D3c), already in the document's
        #: shape: a denial there carries its proxy itself, which a request
        #: recorded in this process has to resolve from a database alias.
        self.live: list[dict[str, Any]] = []
        self.connection_setup: dict[tuple[str, str], int] = {}
        self.unrecognised: dict[tuple[str, str], int] = {}
        self.lock = threading.Lock()
        self.nodeid: str | None = None
        self.installed = False
        self.deferred: str | None = None
        self.unmapped: set[str] = set()
        self.problems: list[str] = []
        self.warnings: list[str] = []
        self.reported_bindings: set[str] = set()

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
            egress=list(context.egress),
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

    def record_unrecognised(self, alias: str | None, query: str) -> None:
        """Counts one statement whose text could not be read.

        psycopg takes `sql.Composable` objects and t-string `Template`s, and a
        `Template` has no text until it is rendered with values the plugin does
        not have. Inventing one would be worse than saying so, and dropping it
        silently would hide a statement that goes on the wire.
        """
        key = (alias or "", query)
        with self.lock:
            self.unrecognised[key] = self.unrecognised.get(key, 0) + 1

    def record_live(
        self, text: str, nodeid: str | None, ignore_paths: tuple[str, ...] = ()
    ) -> None:
        """Adds what a live server's log recorded to the observations (D3c).

        A server started with [`blastit_live_server`] is another process: the
        plugin sees nothing of its requests through Django, so the log is the
        only record of what the front end really sent. Every `request` event
        becomes an observation, so a route the traffic hit is exercised and a
        path blastit would not have routed is reported as `unrouted`; every
        `sql.denied` becomes a statement of its route's first observation, so
        the grant the manifest is missing comes out as `missing`.

        A denial carries a route and no request id (§8.1, "Correlation"), so the
        route is the only link between a statement and a path. One with no
        request event at all — a log cut in half, a route no request reached —
        cannot be checked against the manifest, and is reported as a problem
        rather than dropped.

        `ignore_paths` is what the server answers itself and no suite asked for:
        the health path (§2.4), which the fixture polls and which is in the log
        because the scheduler logs every request it answers. Counting it as
        traffic would report an unrouted request that nobody made.
        """
        from blastit import logs

        events = logs.parse_events(text.splitlines())
        by_route: dict[str, list[dict[str, Any]]] = {}
        for denial in logs.denials(events):
            by_route.setdefault(denial["route"], []).append(denial)

        observations: list[dict[str, Any]] = [
            _live_request(entry["method"], entry["path"], nodeid, [])
            for entry in logs.unrouted_requests(events)
            if entry["path"] not in ignore_paths
        ]
        attached: set[str] = set()
        for entry in logs.routed_requests(events):
            route = entry["route"]
            statements: list[dict[str, Any]] = []
            if route not in attached:
                attached.add(route)
                statements = [
                    {
                        "alias": None,
                        "proxy": denial["proxy"] or None,
                        # The log holds the normalised text, which no parser can
                        # read; `parseable_sql` is the same statement as SQL
                        # again, so the table-grant analysis has something to
                        # work with, and normalising it gives the log's text back.
                        "sql": logs.parseable_sql(denial["sql"]),
                    }
                    for denial in by_route.get(route, [])
                ]
            observations.append(
                _live_request(entry["method"], entry["path"], nodeid, statements)
            )

        unattached = sorted(set(by_route) - attached)
        with self.lock:
            self.live.extend(observations)
            if unattached:
                self.problems.append(
                    "the live server's log has sql.denied events for the route(s) "
                    + ", ".join(unattached)
                    + " but no `request` event for them: a denied statement cannot be "
                    "checked against the manifest without the request that ran it"
                )

    def document(self) -> dict[str, Any]:
        """The observations document the extension reads."""
        with self.lock:
            requests = [observed.as_json(self.proxy_for) for observed in self.observations]
            requests.extend(self.live)
            setup = sorted(self.connection_setup.items())
            unrecognised = sorted(self.unrecognised.items())
            warnings = list(self.warnings)
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
            "unrecognised": [
                {"alias": alias or None, "query": query, "count": count}
                for (alias, query), count in unrecognised
            ],
            "warnings": warnings,
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


def _capture(alias: str | None, sql: Any, params: Any, connection: Any = None) -> str | None:
    """Records one statement, wherever it came from.

    Called by every capture layer; the outermost one records, and the inner ones
    see the mark it left. A statement run while a connection is being
    established goes to the session's connection-setup set — whether or not a
    request is in flight, because it is not the request's — and any other
    statement goes to the request that is running, or nowhere when none is.

    Returns the rendered text, so the caller can mark it as the call the inner
    layers will see, or `None` when the statement could not be read — which is
    counted as `unrecognised` rather than dropped.
    """
    session = _SESSION
    if session is None:
        return None
    text = _sql_text(sql, connection)
    if text is None:
        if _CONNECTING.get() is not None or _CURRENT.get() is not None:
            session.record_unrecognised(alias, query_repr(sql))
        return None
    if _already_recorded(text, params):
        return text
    wire = to_wire_sql(text) if params is not None else text

    connecting = _CONNECTING.get()
    if connecting == NO_DB_ALIAS:
        # Django opens one connection *without* a database to create the test
        # database. It is test machinery, it never runs under blastit (the
        # database is already there), and its alias names no proxy.
        return text
    if connecting is not None:
        session.record_connection_setup(connecting or alias, wire)
        return text
    current = _CURRENT.get()
    if current is not None:
        current.add(alias or "", wire)
    return text


def query_repr(sql: Any) -> str:
    """A short, honest rendering of a query the plugin could not read."""
    text = repr(sql)
    return text if len(text) <= 200 else text[:197] + "..."


def _already_recorded(text: str, params: Any) -> bool:
    """Whether the outermost layer is recording this very call right now."""
    mark = _MARK.get()
    return mark is not None and mark == (text, id(params))


def _capture_egress(sock: Any, address: Any) -> None:
    """Records one outbound connection attempt, for the route's `egress` (§7.3).

    `socket.socket.connect` is where every TCP client ends up
    (`socket.create_connection` calls it), so whatever opens the socket —
    redis, httpx, a queue client — is seen. A unix socket is not egress under
    blastit: the database itself is one, and dbproxy's database socket is not a
    network destination.
    """
    session = _SESSION
    current = _CURRENT.get()
    if session is None or current is None:
        return
    target = _egress_target(sock, address)
    if target is None:
        return
    family, host, port = target
    if (host, port) in _database_addresses():
        # The connection to the database itself: under blastit it is dbproxy's
        # unix socket, and the database is not an egress destination.
        return
    current.add_egress(family, host, port)


def _egress_target(sock: Any, address: Any) -> tuple[str, str, int] | None:
    """`(family, host, port)` of a connect attempt, or `None` when it is not one."""
    family = getattr(getattr(sock, "family", None), "name", None)
    if family is None or "AF_UNIX" in family:
        return None
    if not isinstance(address, tuple) or len(address) < 2:
        # AF_UNIX addresses are paths, AF_NETLINK ones a pid: neither is egress.
        return None
    host, port = address[0], address[1]
    if not isinstance(host, str) or not isinstance(port, int):
        return None
    return family, host, port


def _database_addresses() -> set[tuple[str, int]]:
    """Every `(host, port)` a Django database of this process is configured with."""
    try:
        from django.db import connections

        addresses = set()
        for wrapper in connections.all():
            settings_dict = wrapper.settings_dict
            host = str(settings_dict.get("HOST") or "")
            port = settings_dict.get("PORT") or 5432
            if host:
                addresses.add((host, int(port)))
        return addresses
    except Exception:  # noqa: BLE001 - no Django, or Django without settings
        return set()


def _sql_text(sql: Any, connection: Any) -> str | None:
    """The text of a query, whichever form it came in.

    A `str` is what Django and most application code pass, and `bytes` is what
    psycopg's own internal commands are. psycopg also accepts `sql.Composable`
    objects, and renders them against the connection — which is what psycopg
    itself does before it converts the placeholders; the result still has its
    `%s`, so the same conversion and the same grant comparison apply. Anything
    else (a t-string `Template`, which has no text without the values it was
    built for) is reported as unrecognised rather than guessed at.
    """
    if isinstance(sql, str):
        return sql
    if isinstance(sql, bytes):
        return sql.decode("utf-8", "replace")
    render = getattr(sql, "as_string", None)
    if render is None or connection is None:
        return None
    try:
        return str(render(connection))
    except Exception:  # noqa: BLE001 - a query we cannot render is one we report
        return None


def _wrap_connection(connection: Any) -> None:
    """Records every statement executed on one database connection.

    The wrapper is installed permanently (`connection.execute_wrappers`), so it
    covers the whole lifetime of the connection, reconnects included. It is also
    where a connection that would inline its parameters is noticed: the SQL such
    a connection sends is not the text a grant holds, so the report cannot be
    trusted and says so.
    """
    if getattr(connection, "_blastit_wrapped", False):
        return
    alias = str(connection.alias)
    _warn_if_binding_is_client_side(connection)

    def wrapper(execute: Any, sql: str, params: Any, many: bool, context: Any) -> Any:
        # `params is None` means there is nothing to bind, and psycopg sends the
        # text as it is; with parameters, the text that goes on the wire has
        # `$n` placeholders, which is what a grant has to match. `executemany`
        # is one statement, however many rows it carries.
        dbapi = getattr(context.get("cursor"), "cursor", None)
        statement = sql
        if _is_server_cursor(dbapi):
            # A named cursor does not send `SELECT …`: it sends one `DECLARE …
            # CURSOR FOR …`, which is what dbproxy sees and what a grant has to
            # hold.
            statement = _declared_statement(dbapi, sql, _psycopg_connection(dbapi))
        text = _capture(alias, statement, params, getattr(connection, "connection", None))
        token = _MARK.set((text, id(params)) if text is not None else None)
        try:
            return execute(sql, params, many, context)
        finally:
            _MARK.reset(token)

    connection.execute_wrappers.append(wrapper)
    connection._blastit_wrapped = True  # type: ignore[attr-defined]


def _is_server_cursor(cursor: Any) -> bool:
    """Whether a DBAPI cursor is one of psycopg's named (server-side) ones."""
    if cursor is None:
        return False
    try:
        import psycopg

        return isinstance(cursor, psycopg.ServerCursor)
    except Exception:  # noqa: BLE001 - no psycopg, no psycopg cursors
        return False


def _warn_if_binding_is_client_side(connection: Any) -> None:
    """Says so when an alias would inline its parameters into the SQL (§6).

    With `OPTIONS["server_side_binding"] = True` psycopg sends the query and its
    parameters separately, which is the only way an allowlist can match
    anything. Django's default is client-side binding, and a `cursor_factory` a
    project sets itself can turn it back on; the `$n` text the report shows
    would then be one the plugin made up, so it is reported as a session problem
    — and `--blastit-strict` fails over it.
    """
    session = _SESSION
    if session is None:
        return
    alias = str(connection.alias)
    if alias == NO_DB_ALIAS:
        # The copy Django opens to create the test database: test machinery, and
        # its SQL never reaches a route.
        return
    with session.lock:
        if alias in session.reported_bindings:
            return
        session.reported_bindings.add(alias)
    problem = _binding_problem(connection, alias)
    if problem is not None:
        with session.lock:
            session.warnings.append(problem)


def _binding_problem(connection: Any, alias: str) -> str | None:
    """Why this connection's parameters would be inlined, or `None`."""
    options = connection.settings_dict.get("OPTIONS") or {}
    if options.get("server_side_binding") is not True:
        return (
            f"the database alias {alias!r} does not use server-side binding: set "
            f'DATABASES[{alias!r}]["OPTIONS"]["server_side_binding"] = True, or the '
            "statements in this report are not the text dbproxy would check "
            '(ARCHITECTURE.md §6, "What the app must do")'
        )
    factory = options.get("cursor_factory") or getattr(
        getattr(connection, "connection", None), "cursor_factory", None
    )
    if factory is not None and _binds_client_side(factory):
        return (
            f"the database alias {alias!r} uses a cursor factory that binds parameters "
            "client-side: remove cursor_factory from the alias' OPTIONS, or the "
            "statements in this report are not the text dbproxy would check"
        )
    return None


def _binds_client_side(factory: Any) -> bool:
    """Whether a psycopg cursor class inlines the parameters into the SQL."""
    try:
        from psycopg.client_cursor import ClientCursorMixin
    except ImportError:  # pragma: no cover - psycopg 3 always has it
        return False
    return isinstance(factory, type) and issubclass(factory, ClientCursorMixin)


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
    statement: Any = None,
    command: bool = False,
) -> None:
    """Wraps one psycopg method so a statement is recorded as it is sent.

    Recorded when the method is *called*, not when its result is consumed, so
    `stream` and `copy` — which return iterators — are not lost when a caller
    asks for the first row lazily. `command` marks psycopg's own command channel
    (`_exec_command`), whose `BEGIN`/`COMMIT`/`FETCH` are never the statement the
    caller wrote and are recorded even in the middle of one — with
    [`_capture_command`] deciding what a transaction command means.
    """
    original = getattr(target, name, None)
    if original is None or getattr(original, "_blastit_patched", False):
        return

    def wrapper(self: Any, *args: Any, **kwargs: Any) -> Any:
        connection = _psycopg_connection(self)
        query, params = query_params(args, kwargs)
        if statement is not None:
            query = statement(self, query, connection)
        if command:
            _capture_command(_alias_of(connection), query, params, connection)
            token = None
        else:
            text = _capture(_alias_of(connection), query, params, connection)
            token = _MARK.set((text, id(params)) if text is not None else None)
        try:
            return original(self, *args, **kwargs)
        finally:
            if token is not None:
                _MARK.reset(token)

    wrapper._blastit_patched = True  # type: ignore[attr-defined]
    wrapper.__doc__ = original.__doc__
    wrapper.__wrapped__ = original  # type: ignore[attr-defined]
    setattr(target, name, wrapper)


def _capture_unconditionally(alias: str | None, sql: Any, params: Any, connection: Any) -> None:
    """Records a statement even when the outermost layer marked this call.

    Used for the commands psycopg sends by itself: `BEGIN`, `COMMIT`,
    `ROLLBACK`, the `FETCH`/`CLOSE`/`MOVE` of a named cursor. They go on the wire
    in the middle of a statement the caller *did* write, and a manifest has to
    grant them.
    """
    token = _MARK.set(None)
    try:
        _capture(alias, sql, params, connection)
    finally:
        _MARK.reset(token)


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
    and hstore backends use to look up a type's OID at connect; the `SET`s of a
    connection; and, crucially, the transaction commands — with
    `autocommit = False`, psycopg sends the `BEGIN` and the `COMMIT` through
    `Connection._exec_command`, so a route that runs an `atomic()` or uses
    `ATOMIC_REQUESTS` would otherwise look like it never needed transaction
    control.

    `Cursor.execute`, `executemany`, `stream` and `copy`, and
    `Connection.execute`/`copy`, are the public entry points;
    `ServerCursor.execute` sends its own `DECLARE`; `_exec_command` is psycopg's
    internal command channel, and what it sends is recorded unconditionally,
    because those commands are never the statement the caller wrote.
    """
    try:
        import psycopg
    except ImportError:
        return
    _wrap_psycopg_method(psycopg.Cursor, "execute")
    _wrap_psycopg_method(psycopg.Cursor, "executemany")
    _wrap_psycopg_method(psycopg.Cursor, "stream")
    _wrap_psycopg_method(psycopg.Cursor, "copy")
    _wrap_psycopg_method(psycopg.Connection, "execute")
    _wrap_psycopg_method(psycopg.Connection, "copy")
    server_cursor = getattr(psycopg, "ServerCursor", None)
    if server_cursor is not None:
        _wrap_psycopg_method(server_cursor, "execute", statement=_declared_statement)
    _wrap_psycopg_method(
        getattr(psycopg, "Connection", psycopg.Connection), "_exec_command", command=True
    )


def _capture_command(alias: str | None, sql: Any, params: Any, connection: Any) -> None:
    """Records a command psycopg sent through its own command channel.

    `BEGIN`, `COMMIT` and `ROLLBACK` are what make a route need
    `transaction_control`, but they are also how *someone else's* transaction
    shows up: pytest-django wraps every test in one, and psycopg only sends its
    `BEGIN` when the first statement runs — which is usually the request's.
    Under blastit each request gets a connection of its own and no test
    transaction exists, so a transaction command counts only when the request
    went through Django's transaction API itself (`atomic()`,
    `ATOMIC_REQUESTS`, an explicit `commit()`), and the rest of the channel
    (`FETCH`, `CLOSE`, `MOVE`, `DEALLOCATE`…) always counts.
    """
    text = _sql_text(sql, connection)
    if isinstance(text, str) and _TRANSACTION_COMMAND.match(text):
        current = _CURRENT.get()
        if _CONNECTING.get() is None and (current is None or not current.manages_transactions):
            return
    _capture_unconditionally(alias, sql, params, connection)


#: A transaction-control command, as §5.2 spells them out — the same list the
#: policy accepts with `transaction_control = true`.
_TRANSACTION_COMMAND = re.compile(
    r"^\s*(BEGIN|START\s+TRANSACTION|COMMIT|END|ROLLBACK|SAVEPOINT|RELEASE)\b",
    re.IGNORECASE,
)


def _patch_transactions() -> None:
    """Notes the requests that use Django's transaction API themselves.

    `atomic()` and `ATOMIC_REQUESTS` go through `set_autocommit`, `commit`,
    `rollback` and the savepoint methods; a transaction that is already open
    when the request starts was opened elsewhere (pytest-django's `django_db`,
    an outer `atomic()`), and the plugin does not put it on the route's account.
    """
    from django.db.backends.base.base import BaseDatabaseWrapper

    names = (
        "set_autocommit",
        "commit",
        "rollback",
        "savepoint",
        "savepoint_commit",
        "savepoint_rollback",
    )
    for name in names:
        original = getattr(BaseDatabaseWrapper, name, None)
        if original is None or getattr(original, "_blastit_patched", False):
            continue

        def wrapper(self: Any, *args: Any, _original: Any = original, **kwargs: Any) -> Any:
            current = _CURRENT.get()
            if current is not None:
                current.manages_transactions = True
            return _original(self, *args, **kwargs)

        wrapper._blastit_patched = True  # type: ignore[attr-defined]
        wrapper.__doc__ = original.__doc__
        wrapper.__wrapped__ = original  # type: ignore[attr-defined]
        setattr(BaseDatabaseWrapper, name, wrapper)


def _patch_sockets() -> None:
    """Records outbound connection attempts, which `egress = true` is about (§7.3).

    `socket.socket.connect` is the one place every TCP client ends up —
    `socket.create_connection`, and therefore requests, httpx, redis and friends,
    go through it — so whatever opens the socket is seen. A unix socket is not
    egress: under blastit the database itself is one.
    """
    original_connect = socket.socket.connect
    original_connect_ex = socket.socket.connect_ex
    if getattr(original_connect, "_blastit_patched", False):
        return

    def connect(self: Any, address: Any) -> Any:
        _capture_egress(self, address)
        return original_connect(self, address)

    def connect_ex(self: Any, address: Any) -> Any:
        _capture_egress(self, address)
        return original_connect_ex(self, address)

    for wrapper, original in ((connect, original_connect), (connect_ex, original_connect_ex)):
        wrapper._blastit_patched = True  # type: ignore[attr-defined]
        wrapper.__doc__ = original.__doc__
        wrapper.__wrapped__ = original  # type: ignore[attr-defined]
    socket.socket.connect = connect  # type: ignore[method-assign]
    socket.socket.connect_ex = connect_ex  # type: ignore[method-assign]


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
        _patch_transactions()
        _patch_connect()
        _patch_psycopg()
        _patch_sockets()
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


#
# A live server, served by blastit itself (D3c).
#


#: How long a live server gets to answer on its health path before it is a
#: failure. Generous: the first request pays for the interpreter's import of the
#: project, which is what a cold boot of the server costs.
LIVE_SERVER_TIMEOUT = 60.0

#: How long a live server gets to stop after `SIGTERM` before it is killed.
LIVE_SERVER_GRACE = 20.0

#: The health path the fixture asks for; the server is told to use it, so a
#: project that sets `BLASTIT_HEALTH_PATH` for production does not confuse it.
HEALTH_PATH = "/_blastit/health"

#: How many times a live server is started before the fixture gives up. A port
#: that something else grabbed between the probe and the bind is worth one more
#: try; anything else fails with the server's own log.
LIVE_SERVER_ATTEMPTS = 3


@dataclass
class _LiveServer:
    """A real `blastit` serving this project, as a test sees it."""

    #: The base URL, without a trailing slash.
    url: str
    #: The address it answers on (`127.0.0.1`).
    host: str
    #: The port it bound.
    port: int
    #: The process, for a test that wants to watch it.
    process: Any
    #: The file its stdout (and stderr) went to.
    log_path: Path


def _live_request(
    method: str, path: str, nodeid: str | None, statements: list[dict[str, Any]]
) -> dict[str, Any]:
    """One observation, in the document's shape, built from a live server's log.

    The shape is the extension's (`crates/blastit-py/src/audit.rs`): a statement
    carries its own proxy here, because a `sql.denied` event names one and there
    is no database alias in this process to resolve it from. `source` is for a
    human reading the document; the extension ignores keys it does not know.
    """
    return {
        "method": method,
        "path_info": path,
        "nodeid": nodeid,
        "transaction_control": False,
        "statements": statements,
        "egress": [],
        "source": "live",
    }


def _live_server_prefix() -> list[str]:
    """How a live server has to be started on this machine, or a skip.

    blastit's init binds the listeners, makes the view directories under
    `/run/blastit` and drops privileges to the view uids (§2.1, §4), so it runs
    as root. Tests that already run as root start it directly; otherwise
    passwordless `sudo -n` is used, and the environment travels in a file that
    only root can read — `sudo` resets the environment, and the test database's
    password has no business in `ps`.
    """
    if os.geteuid() == 0:
        return []
    sudo = shutil.which("sudo")
    if sudo is not None:
        check = subprocess.run(  # noqa: S603 - an absolute path, no shell
            [sudo, "-n", "true"], capture_output=True, check=False, text=True
        )
        if check.returncode == 0:
            return [sudo, "-n"]
    pytest.skip(
        "blastit: a live server needs root (view uids, listeners, /run/blastit) — "
        "run the tests as root, or give this user passwordless sudo (`sudo -n true`)"
    )


def _live_app_spec(config: pytest.Config) -> str:
    """The WSGI callable the live server should serve.

    `--blastit-live-app` wins; then the project's own `WSGI_APPLICATION`; then
    the layout `django-admin startproject` writes, where the settings module
    `myproject.settings` comes with `myproject.wsgi:application`. Empty when
    none of them says anything, which the fixture turns into a skip.
    """
    from_option = config.getoption("blastit_live_app")
    if from_option:
        return str(from_option)
    from django.conf import settings

    declared = getattr(settings, "WSGI_APPLICATION", None)
    if declared:
        return str(declared)
    module = os.environ.get("DJANGO_SETTINGS_MODULE", "")
    if "." in module:
        return f"{module.rsplit('.', 1)[0]}.wsgi:application"
    return ""


def _project_directory() -> Path:
    """The directory a live server should run in, so the app is importable.

    A wheel's stage 1 gets the console script's `sys.path` back (`-I` drops it),
    and that path holds the working directory — so starting the server where the
    settings module lives is what puts `wsgi` on it, exactly as
    `manage.py runserver` does.
    """
    module = os.environ.get("DJANGO_SETTINGS_MODULE", "")
    imported = sys.modules.get(module) if module else None
    path = getattr(imported, "__file__", None)
    if path:
        return Path(path).resolve().parent
    return Path.cwd()


def _test_database_url(alias: str = "default") -> str:
    """The test database as a URL a `postgresql` proxy can read (§5.2).

    pytest-django's `django_db_setup` has already renamed the connection to the
    test database, so this is the database the suite's own tests use — which is
    what the live server has to be pointed at, or it would serve production's
    (or whatever `DATABASE_URL` names) instead of the fixtures' data.

    A Unix-socket host is spelled the way libpq spells one in a URL: the host is
    the percent-encoded directory (`postgresql://user@%2Fvar%2Frun%2Fpostgresql:5432/db`).
    """
    from django.db import connections

    settings_dict = connections[alias].settings_dict
    user = str(settings_dict.get("USER") or "")
    password = str(settings_dict.get("PASSWORD") or "")
    host = str(settings_dict.get("HOST") or "")
    port = str(settings_dict.get("PORT") or 5432)
    name = str(settings_dict.get("NAME") or "")
    credentials = urllib.parse.quote(user, safe="")
    if password:
        credentials += f":{urllib.parse.quote(password, safe='')}"
    authority = urllib.parse.quote(host, safe="") if host else "localhost"
    return f"postgresql://{credentials}@{authority}:{port}/{urllib.parse.quote(name, safe='')}"


def _free_port() -> int:
    """A port nothing is using right now, for the live server to bind."""
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _live_server_environment(
    config: pytest.Config, manifest: Path, port: int, app_spec: str
) -> dict[str, str]:
    """The environment a live server is started with.

    Deliberately small: the app's environment under blastit is not the test
    runner's (§5.3 builds it from the manifest's outputs), so passing the whole
    environment would only hand the app variables it will not have in
    production. What it needs is the WSGI callable, the manifest, the port, the
    settings module and the proxy's own input variable.
    """
    environment = {
        "BLASTIT_APP": app_spec,
        "BLASTIT_MANIFEST": str(manifest),
        "BLASTIT_LOG_FORMAT": "json",
        "BLASTIT_HEALTH_PATH": HEALTH_PATH,
        "PORT": str(port),
        "PYTHONPATH": str(_project_directory()),
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    settings_module = os.environ.get("DJANGO_SETTINGS_MODULE")
    if settings_module:
        environment["DJANGO_SETTINGS_MODULE"] = settings_module
    preload = _preload_modules()
    if preload:
        # §6, "Load lazily-loaded code before the fork": Django imports its root
        # URLconf on the first request, which under blastit is *after* the fork —
        # so the import would happen in one view process, from a directory the
        # app's own uid may not be able to read. Naming it here is what the
        # architecture tells a Django project to do, and the fixture can do it
        # for the project rather than making every suite remember.
        environment["BLASTIT_PRELOAD"] = ",".join(preload)
    variable = _postgresql_url_variable(manifest)
    if variable is not None:
        environment[variable] = _test_database_url()
    return environment


def _preload_modules() -> list[str]:
    """The modules a Django project imports lazily, for `BLASTIT_PRELOAD` (§6).

    The root URLconf, which is the one the architecture names: Django resolves
    it on the first request, so without this it is imported after the fork and
    the URL configuration lives in one view process only. Empty when Django is
    not usable or declares none.
    """
    try:
        from django.conf import settings
    except Exception:  # noqa: BLE001 - not a Django project, or settings are unusable
        return []
    root = getattr(settings, "ROOT_URLCONF", None)
    return [root] if isinstance(root, str) and root else []


def _make_reachable(path: Path) -> None:
    """Makes the project's directory tree traversable by the view uids (§2.1).

    Every view runs as a uid of its own, so it has to be able to *read* the
    application's code — `/app` in a container is world-readable for exactly
    that reason. A test project in a private directory (pytest's own `tmp_path`
    is `0700`) would otherwise fail to import anything it did not already have
    in memory when blastit forked it, which looks like an application bug.

    Only the missing `o+x`/`o+r` on directories is added, and the walk stops at
    the first directory the views can already traverse.
    """
    directory = path.resolve()
    while True:
        mode = directory.stat().st_mode & 0o777
        if mode & 0o005 == 0o005:
            return
        directory.chmod(mode | 0o005)
        if directory.parent == directory:
            return
        directory = directory.parent


def _postgresql_url_variable(manifest: Path) -> str | None:
    """The variable a `postgresql` proxy reads its URL from, or `None`.

    `input = { url = "DATABASE_URL" }` is the usual declaration (§5.1). A proxy
    that takes its upstream in pieces (`host`, `port`, `user`, `password`,
    `database`) is not something this fixture can fill in from one URL, and the
    caller says so.
    """
    import tomllib

    document = tomllib.loads(manifest.read_text(encoding="utf-8"))
    proxies = document.get("proxy")
    if not isinstance(proxies, dict):
        return None
    for declaration in proxies.values():
        if not isinstance(declaration, dict):
            continue
        if declaration.get("protocol") != "postgresql":
            continue
        inputs = declaration.get("input")
        if isinstance(inputs, dict) and isinstance(inputs.get("url"), str):
            return str(inputs["url"])
        return None
    return None


def _start_live_server(
    prefix: list[str],
    config: pytest.Config,
    log_path: Path,
) -> _LiveServer:
    """Starts a live server, or fails with what it said on the way out.

    A port that something else grabbed between the probe and the bind is the one
    failure worth retrying; anything else is the server's own explanation, which
    is why its output goes to a file rather than a pipe — a server that fails to
    boot must not be able to hang the test run by holding one open.
    """
    app_spec = _live_app_spec(config)
    if not app_spec:
        pytest.skip(
            "blastit: a live server needs the WSGI callable to serve — set "
            "WSGI_APPLICATION in the Django settings, or pass --blastit-live-app "
            "module:attribute"
        )
    manifest = Path(str(config.getoption("blastit_manifest") or config.getini("blastit_manifest")))
    variable = _postgresql_url_variable(manifest)
    if variable is None and _postgresql_proxies(manifest.read_text(encoding="utf-8")):
        pytest.skip(
            "blastit: a live server needs the manifest's postgresql proxy to take its "
            "URL from one variable (`input = { url = \"DATABASE_URL\" }`, §5.1) to be "
            "pointed at the test database"
        )
    failure = ""
    for _ in range(LIVE_SERVER_ATTEMPTS):
        port = _free_port()
        environment = _live_server_environment(config, manifest, port, app_spec)
        log_path.write_text("", encoding="utf-8")
        with log_path.open("w", encoding="utf-8") as log:
            if prefix:
                environment_file = log_path.with_suffix(".env")
                environment_file.write_text(
                    "".join(
                        f"export {key}={shlex.quote(value)}\n"
                        for key, value in environment.items()
                    ),
                    encoding="utf-8",
                )
                command = [
                    *prefix,
                    "/bin/sh",
                    "-c",
                    f". {shlex.quote(str(environment_file))} && exec \"$@\"",
                    "blastit",
                    sys.executable,
                    "-m",
                    "blastit",
                ]
                process = subprocess.Popen(
                    command,  # noqa: S603 - fixed argv, no shell
                    cwd=str(_project_directory()),
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    text=True,
                    start_new_session=True,
                )
            else:
                process = subprocess.Popen(  # noqa: S603 - sys.executable, fixed argv
                    [sys.executable, "-m", "blastit"],
                    cwd=str(_project_directory()),
                    env=environment,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    text=True,
                    start_new_session=True,
                )
        live = _LiveServer(
            url=f"http://127.0.0.1:{port}",
            host="127.0.0.1",
            port=port,
            process=process,
            log_path=log_path,
        )
        if _wait_for_health(live):
            return live
        failure = _stop_live_server(live).strip()
    _clean_bytecode(prefix, _project_directory())
    pytest.fail(
        f"blastit: the live server never answered on {HEALTH_PATH} "
        f"(started {LIVE_SERVER_ATTEMPTS} time(s)); its last words:\n{failure[-4000:]}"
    )


def _wait_for_health(live: _LiveServer) -> bool:
    """Waits for the server to answer its health path, or for it to exit."""
    deadline = time.monotonic() + LIVE_SERVER_TIMEOUT
    url = f"{live.url}{HEALTH_PATH}"
    while time.monotonic() < deadline:
        if live.process.poll() is not None:
            return False
        try:
            with urllib.request.urlopen(url, timeout=2) as response:  # noqa: S310 - 127.0.0.1
                if response.status == 200:
                    return True
        except (urllib.error.URLError, OSError):
            pass
        time.sleep(0.05)
    return False


def _stop_live_server(live: _LiveServer) -> str:
    """Stops a live server, and returns everything it logged.

    `SIGTERM` is what blastit's own graceful shutdown listens for (§4); the whole
    process group is signalled, because the view processes and dbproxy are its
    children and must not outlive the test.
    """
    process = live.process
    if process.poll() is None:
        _signal_group(process.pid, signal.SIGTERM)
        with contextlib.suppress(subprocess.TimeoutExpired):
            process.wait(timeout=LIVE_SERVER_GRACE)
    if process.poll() is None:
        _signal_group(process.pid, signal.SIGKILL)
        with contextlib.suppress(subprocess.TimeoutExpired):
            process.wait(timeout=5)
    with contextlib.suppress(OSError):
        return live.log_path.read_text(encoding="utf-8", errors="replace")
    return ""


def _signal_group(pid: int, number: int) -> None:
    """Signals every process of a live server's own process group."""
    with contextlib.suppress(ProcessLookupError, PermissionError, OSError):
        os.killpg(os.getpgid(pid), number)


def _clean_bytecode(prefix: list[str], project: Path) -> None:
    """Removes the bytecode caches a live server left in the project (D3c).

    A wheel's stage 1 runs `python -I -m blastit`, and `-I` implies `-E`: the
    interpreter ignores `PYTHONDONTWRITEBYTECODE` — which blastit itself injects
    (§5.3) — so every module it imports *as root* before the fork leaves a
    `__pycache__` owned by root in the project directory: the WSGI callable, the
    settings, and the URLconf `BLASTIT_PRELOAD` names. The test process cannot
    remove what root left, and pytest's own cleanup of its temporary directory
    then fails on it. Cleaning it up is the fixture's mess to clean, and it
    needs the same privileges the server did.
    """
    subprocess.run(  # noqa: S603 - a fixed argv, no shell
        [
            *prefix,
            "find",
            str(project),
            "-name",
            "__pycache__",
            "-type",
            "d",
            "-exec",
            "rm",
            "-rf",
            "--",
            "{}",
            "+",
        ],
        capture_output=True,
        check=False,
    )


@pytest.fixture
def blastit_live_server(request: pytest.FixtureRequest, tmp_path: Path) -> Any:
    """A real `blastit`, serving this project, for a live-server test suite (D3c).

    Yields the server — `.url`, `.host`, `.port`, like pytest-django's
    `live_server` — after it answers its health path, and on teardown collects
    the log's `request` and `sql.denied` events into the observations the report
    is built from, so front-end traffic is diffed against the manifest like any
    other request.

    It needs a manifest (`--blastit-manifest`), and the privileges blastit needs
    to start (root, or passwordless sudo): without either it skips with the
    reason rather than failing a suite that has nothing to do with blastit's own
    requirements.
    """
    session = _SESSION
    if session is None:
        pytest.skip(
            "blastit: a live server needs --blastit-manifest (or blastit_manifest in "
            "the configuration) to serve the project with"
        )
    prefix = _live_server_prefix()
    if _postgresql_url_variable(session.manifest_path) is not None:
        # The test database has to exist, and reading its name from the
        # connection is only right once pytest-django has created it.
        try:
            request.getfixturevalue("django_db_setup")
        except pytest.FixtureLookupError as error:
            pytest.skip(
                "blastit: a live server is pointed at the *test* database, which "
                f"pytest-django's django_db_setup creates: {error}"
            )
    # The views run as their own uids: they have to be able to read the code.
    _make_reachable(_project_directory())
    live = _start_live_server(prefix, request.config, tmp_path / "blastit-live.log")
    try:
        yield live
    finally:
        text = _stop_live_server(live)
        _clean_bytecode(prefix, _project_directory())
        state = _SESSION
        if state is not None:
            try:
                state.record_live(text, state.nodeid, (HEALTH_PATH,))
            except Exception as error:  # noqa: BLE001 - reported, never raised in teardown
                state.problems.append(f"the live server's log could not be read: {error!r}")


class _LiveServerPlugin:
    """The `live_server` a `--blastit-live-server` session gets (D3c).

    Registered only when the option is given, and registered at
    `pytest_configure` — after pytest-django — so that a project asking for it
    gets blastit's server under the name its tests already use, without touching
    the tests. A `conftest.py` fixture, or a `pytest_plugins` entry, still wins:
    it is collected later.
    """

    @pytest.fixture
    def live_server(self, blastit_live_server: Any) -> Any:
        """pytest-django's `live_server`, served by blastit itself.

        pytest-django's own `_live_server_helper` — autouse, and part of the
        fixture's contract — enables an `ALLOWED_HOSTS` override on whatever
        `live_server` yields, so ours carries one too. It is the *test*
        process's settings, not the server's: what the server accepts is its own
        `ALLOWED_HOSTS`, in its own process, which is why a project whose
        settings are stricter than `["*"]` has to accept the address it is
        served on.
        """
        try:
            from django.test.utils import modify_settings
        except Exception:  # noqa: BLE001 - no Django settings: nothing to carry
            return blastit_live_server
        blastit_live_server._live_server_modified_settings = modify_settings(  # noqa: SLF001
            ALLOWED_HOSTS={"append": blastit_live_server.host}
        )
        return blastit_live_server


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
    group.addoption(
        "--blastit-prefer-tables",
        action="store_true",
        default=False,
        help="print the table-grant snippet of a route with missing statements, "
        "instead of only naming the statements (D7)",
    )
    group.addoption(
        "--blastit-live-server",
        action="store_true",
        default=False,
        help="serve pytest-django's live_server through blastit itself, so real "
        "front-end traffic is checked against the manifest (D3c; needs root or "
        "passwordless sudo)",
    )
    group.addoption(
        "--blastit-live-app",
        metavar="MODULE:ATTR",
        default=None,
        help="the WSGI callable blastit_live_server serves (default: the Django "
        "setting WSGI_APPLICATION, or <package>.wsgi:application)",
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
        if (
            config.getoption("blastit_proxy")
            or config.getoption("blastit_strict")
            or config.getoption("blastit_prefer_tables")
            or config.getoption("blastit_live_server")
            or config.getoption("blastit_live_app")
        ):
            raise pytest.UsageError(
                "blastit: --blastit-proxy, --blastit-strict and "
                "--blastit-prefer-tables need --blastit-manifest (or "
                "blastit_manifest in the configuration); --blastit-live-server and "
                "--blastit-live-app need it too, to know which manifest to serve "
                "the project with"
            )
        return
    session = _Session(config)
    _SESSION = session
    # The option asks for blastit's own live server under the name the suite
    # already uses; nothing is registered without it, so a project keeps
    # pytest-django's `live_server` — which is what most suites want.
    if config.getoption("blastit_live_server"):
        config.pluginmanager.register(_LiveServerPlugin(), "blastit-live-server")
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

    Workers write next to the report (`<report>.<workerid>`, the convention
    pytest-xdist's own `--junitxml` uses). A file is only merged when it says
    which worker wrote it and carries the document version this build knows:
    anything else in that directory belongs to someone else, and is left where
    it is and reported.
    """
    document = session.document()
    report = session.report_path
    workers = sorted(path for path in report.parent.glob(f"{report.name}.*"))
    merged = []
    for path in workers:
        try:
            theirs = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            session.warnings.append(
                f"{path.name} is not a blastit worker report ({error!r}): left in place"
            )
            continue
        if not isinstance(theirs, dict) or not theirs.get("worker"):
            session.warnings.append(
                f"{path.name} is not a blastit worker report (no `worker` key): left in place"
            )
            continue
        if theirs.get("version") != _OBSERVATIONS_VERSION:
            session.warnings.append(
                f"{path.name} is a blastit worker report of version "
                f"{theirs.get('version')!r}, this build writes "
                f"{_OBSERVATIONS_VERSION}: left in place"
            )
            continue
        for request in theirs.get("requests", []):
            document["requests"].append(request)
        for statement in theirs.get("connection_setup", []):
            document["connection_setup"].append(statement)
        for entry in theirs.get("unrecognised", []):
            document["unrecognised"].append(entry)
        for warning in theirs.get("warnings", []):
            document["warnings"].append(warning)
        merged.append(path)
    for path in merged:
        try:
            path.unlink()
        except OSError as error:  # pragma: no cover - best effort cleanup
            session.warnings.append(f"could not remove the worker report {path}: {error!r}")
    document["workers"] = [path.name for path in merged]
    document["warnings"] = list(dict.fromkeys(session.warnings))
    return document


def _write_json(path: Path, document: dict[str, Any]) -> None:
    """Writes one JSON document, creating its directory if it has one."""
    if path.parent != Path():
        path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def _audit(session: _Session, document: dict[str, Any]) -> dict[str, Any]:
    """Runs blastit's own audit, in the compiled extension.

    The observations carry the `PATH_INFO` the app saw, and blastit's view had
    already stripped `BLASTIT_SCRIPT_NAME` from it before WSGI (§2.5), so it is
    the same string the scheduler routed on and there is no mount prefix to hand
    over.
    """
    from blastit import _blastit

    report = _blastit.audit(session.manifest, json.dumps(document))
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
        lines = _summary(report, str(state.report_path), state.prefer_tables)
    for warning in state.warnings:
        lines.append((f"blastit: {warning}", True))
    for problem in state.problems:
        lines.append((f"blastit: {problem}", True))

    strict_failure = report is not None and state.strict and not report["totals"]["clean"]
    if strict_failure:
        totals = report["totals"]
        lines.append(
            (
                "blastit: --blastit-strict: "
                f"{totals['missing']} statement execution(s) missing, "
                f"{totals['unrouted']} unrouted request(s), "
                f"{totals['connection_setup_ungranted']} connection-setup "
                f"statement(s) not granted in every route, "
                f"{totals['egress']} route(s) opening connections without egress = true, "
                f"{totals['unrecognised']} unreadable statement(s) and "
                f"{totals['plugin_warnings']} warning(s)",
                True,
            )
        )

    _print_lines(session.config, lines)
    if report is None or state.problems or strict_failure:
        session.exitstatus = pytest.ExitCode.TESTS_FAILED


def _summary(report: dict[str, Any], path: str, prefer_tables: bool = False) -> list[tuple[str, bool]]:
    """The terminal summary: short, and actionable.

    Each line says what to do — add the statements a route is missing, look at
    the routes nothing exercised — and where the detail is. With
    ``--blastit-prefer-tables``, a route with missing statements also prints the
    table grant that covers everything it ran (D7), which is the other way to
    fix it: a grant of tables and columns instead of a list of statements.
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
            if prefer_tables:
                for proxy in route["proxies"]:
                    snippet = proxy.get("table_grants", {}).get("toml", "")
                    if proxy["missing"] and snippet:
                        lines.append(
                            (
                                f"blastit: route {route['name']} (proxy "
                                f"{proxy['name']}): a table grant covering every "
                                f"statement it ran:\n{snippet}",
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
    if totals.get("unused_global"):
        lines.append(
            (
                f"blastit: {totals['unused_global']} grant(s) of a global group nothing "
                f"matched (see {path})",
                False,
            )
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
    for route in report["routes"]:
        egress = route["egress"]
        if egress["needed"]:
            targets = ", ".join(entry["target"] for entry in egress["observed"][:3])
            lines.append(
                (
                    f"blastit: route {route['name']} opens outbound connections "
                    f"({targets}) but has no egress = true",
                    True,
                )
            )
    if report["unrecognised"]["count"]:
        lines.append(
            (
                f"blastit: {report['unrecognised']['count']} statement(s) could not be "
                f"read (a psycopg template has no text before it is rendered): "
                f"see {path}",
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
