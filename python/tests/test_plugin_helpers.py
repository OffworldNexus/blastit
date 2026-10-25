"""The plugin's pure helpers: the wire form, and reading the manifest's proxies.

No Django, no database, no session: these are the parts of the plugin that can
be tested directly, and the ones where a mistake would quietly report the wrong
SQL.
"""

import contextlib
import functools
import json
from typing import Any, Iterator

import pytest

from blastit import pytest_plugin as plugin

# The wire form of a statement is psycopg's, so these tests need the driver the
# tiny project is served with (it is in the same test environment).
pytest.importorskip("psycopg", reason="the wire form is psycopg's")


class TestWireForm:
    """psycopg's own converter, or the regular expression that stands in for it."""

    def test_positional_placeholders_are_numbered(self) -> None:
        assert (
            plugin.to_wire_sql('SELECT "t"."id" FROM "t" WHERE "t"."a" = %s AND "t"."b" = %s')
            == 'SELECT "t"."id" FROM "t" WHERE "t"."a" = $1 AND "t"."b" = $2'
        )

    def test_named_placeholders_are_numbered_in_order_of_first_use(self) -> None:
        # psycopg numbers a named parameter the first time it appears, and reuses
        # the number afterwards: the grant has to be written the same way.
        assert (
            plugin.to_wire_sql("SELECT %(b)s, %(a)s, %(b)s")
            == "SELECT $1, $2, $1"
        )

    def test_a_doubled_percent_is_one_percent(self) -> None:
        assert plugin.to_wire_sql("SELECT '100%%'") == "SELECT '100%'"

    def test_the_fallbacks_agree_with_psycopg(self) -> None:
        for sql in [
            "SELECT %s",
            "SELECT %s, %s",
            "SELECT %(name)s, %s",
            "SELECT '100%%' , %s",
            'SELECT "t"."id" FROM "t" WHERE "t"."a" = %s',
        ]:
            assert plugin._regex_wire(sql) == plugin.to_wire_sql(sql), sql

    def test_the_regular_expression_handles_what_psycopg_refuses(self) -> None:
        # Mixing positional and named placeholders is a programming error in
        # psycopg; the plugin must not turn that into a failing test run.
        mixed = "SELECT %s, %(name)s"
        with pytest.raises(Exception):
            plugin._psycopg_converter()(mixed.encode(), "utf-8")  # type: ignore[misc]
        assert plugin.to_wire_sql(mixed) == "SELECT $1, $2"

    def test_nothing_to_convert_is_left_alone(self) -> None:
        assert plugin.to_wire_sql("SELECT 1") == "SELECT 1"
        assert plugin.to_wire_sql("") == ""

    def test_unicode_survives(self) -> None:
        assert plugin.to_wire_sql("SELECT 'héllo' , %s") == "SELECT 'héllo' , $1"


class TestProxyOptions:
    """`--blastit-proxy alias=proxy`, one per database alias."""

    def test_pairs_are_read(self) -> None:
        assert plugin._proxy_options(["default=db", "replica=warehouse"]) == {
            "default": "db",
            "replica": "warehouse",
        }

    def test_nothing_given_is_no_mapping(self) -> None:
        assert plugin._proxy_options([]) == {}

    @pytest.mark.parametrize("value", ["default", "=db", "default=", "=", ""])
    def test_anything_else_is_a_usage_error(self, value: str) -> None:
        with pytest.raises(pytest.UsageError):
            plugin._proxy_options([value])

    def test_a_later_pair_wins(self) -> None:
        assert plugin._proxy_options(["default=db", "default=warehouse"]) == {
            "default": "warehouse"
        }


class TestManifestProxies:
    """The manifest is read with `tomllib`: the plugin needs to know its proxies."""

    def test_the_postgresql_proxies_are_found(self) -> None:
        manifest = (
            "version = 1\n"
            "[proxy.db]\n"
            'protocol = "postgresql"\n'
            "[proxy.warehouse]\n"
            'protocol = "postgresql"\n'
        )
        assert plugin._postgresql_proxies(manifest) == ["db", "warehouse"]

    def test_a_manifest_without_proxies_has_none(self) -> None:
        assert plugin._postgresql_proxies("version = 1\n") == []

    def test_broken_toml_is_a_usage_error(self) -> None:
        with pytest.raises(pytest.UsageError):
            plugin._postgresql_proxies("version = ")


class TestStatementCapture:
    """The wrapper that sits on a Django connection, with no Django in sight."""

    @pytest.fixture(autouse=True)
    def session(self, monkeypatch: pytest.MonkeyPatch) -> _FakeSession:
        """A stand-in session: the capture only records while one is active."""
        fake = _FakeSession()
        monkeypatch.setattr(plugin, "_SESSION", fake)
        return fake

    def test_a_wrapper_is_installed_once(self) -> None:
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        plugin._wrap_connection(connection)
        assert len(connection.execute_wrappers) == 1

    def test_a_statement_inside_a_request_is_recorded_in_wire_form(self) -> None:
        connection = _FakeConnection(alias="replica")
        plugin._wrap_connection(connection)
        with _request() as context:
            _execute(connection, "SELECT %s", [1])
        assert context.statements == [("replica", "SELECT $1")]

    def test_executemany_is_one_statement(self) -> None:
        # psycopg repeats the statement per row on the wire; a grant is one
        # statement, so the report counts it once.
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        with _request() as context:
            _execute(connection, "INSERT INTO t (a) VALUES (%s)", [(1,), (2,), (3,)], many=True)
        assert context.statements == [("default", "INSERT INTO t (a) VALUES ($1)")]

    def test_a_query_without_parameters_is_left_as_it_is(self) -> None:
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        with _request() as context:
            _execute(connection, "SET TIME ZONE 'UTC'", None)
        assert context.statements == [("default", "SET TIME ZONE 'UTC'")]

    def test_statements_outside_a_request_are_dropped(self) -> None:
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        assert plugin._CURRENT.get() is None
        assert _execute(connection, "SELECT %s", [1]) == ("SELECT %s", [1])

    def test_one_request_does_not_see_another(self) -> None:
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        with _request() as first:
            _execute(connection, "SELECT %s", [1])
        with _request() as second:
            _execute(connection, "SELECT %s", [2])
        assert len(first.statements) == 1
        assert len(second.statements) == 1

    def test_the_outermost_layer_records_each_statement_once(self) -> None:
        """psycopg's layer sees the same statement as Django's: only one line."""
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        seen: list[str] = []

        def executor(sql: str, params: Any, many: bool, context: Any) -> Any:
            # What psycopg's own wrapper does with the very same statement, from
            # inside Django's.
            plugin._capture("default", sql, params)
            seen.append(sql)
            return (sql, params)

        wrapped = functools.partial(connection.execute_wrappers[0], executor)
        with _request() as context:
            wrapped("SELECT %s", [1], False, {})
        assert seen == ["SELECT %s"], "the statement was executed"
        assert context.statements == [("default", "SELECT $1")], "and recorded once"

    def test_a_query_that_cannot_be_rendered_is_skipped(self) -> None:
        assert plugin._sql_text("SELECT 1", None) == "SELECT 1"
        assert plugin._sql_text(object(), None) is None
        assert plugin._sql_text(None, None) is None

    def test_a_statement_during_a_connect_is_connection_setup(self, session: _FakeSession) -> None:
        """It is the connection's SQL, not a route's, wherever it runs."""
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        token = plugin._CONNECTING.set("default")
        try:
            _execute(connection, "SET application_name = 'x'", None)
        finally:
            plugin._CONNECTING.reset(token)
        assert session.setup == [("default", "SET application_name = 'x'")]

    def test_the_connection_that_creates_the_test_database_is_ignored(self, session: _FakeSession) -> None:
        """Django's `__no_db__` connection is test machinery, not the app's."""
        connection = _FakeConnection()
        plugin._wrap_connection(connection)
        token = plugin._CONNECTING.set(plugin.NO_DB_ALIAS)
        try:
            _execute(connection, "SET application_name = 'x'", None)
        finally:
            plugin._CONNECTING.reset(token)
        assert session.setup == []


class _FakeConnection:
    """The two attributes `_wrap_connection` touches on a Django connection."""

    def __init__(self, alias: str = "default") -> None:
        self.alias = alias
        self.execute_wrappers: list[Any] = []


class _FakeSession:
    """The one thing the capture asks of a session."""

    def __init__(self) -> None:
        self.setup: list[tuple[str | None, str]] = []

    def record_connection_setup(self, alias: str | None, sql: str) -> None:
        self.setup.append((alias, sql))


@contextlib.contextmanager
def _request() -> Iterator[Any]:
    """Installs a request context, the way `get_response` does around a request."""
    context = plugin._RequestContext()
    token = plugin._CURRENT.set(context)
    try:
        yield context
    finally:
        plugin._CURRENT.reset(token)


def _execute(connection: _FakeConnection, sql: str, params: Any, many: bool = False) -> Any:
    """Runs one statement through every wrapper of a connection, as Django does."""

    def executor(inner_sql: str, inner_params: Any, _many: bool, _context: Any) -> Any:
        return (inner_sql, inner_params)

    wrapped: Any = executor
    for wrapper in reversed(connection.execute_wrappers):
        wrapped = functools.partial(wrapper, wrapped)
    return wrapped(sql, params, many, {})


class TestObservationDocument:
    """The observations document the extension reads (crates/blastit-py)."""

    def test_a_request_is_rendered_with_its_statements(self) -> None:
        request = plugin._ObservedRequest(
            method="POST",
            path_info="/notes/",
            nodeid="tests/test_app.py::test_it",
            transaction_control=True,
            statements=[("default", "SELECT 1"), ("replica", "SELECT 2")],
        )
        assert request.as_json(lambda alias: {"default": "db"}.get(alias)) == {
            "method": "POST",
            "path_info": "/notes/",
            "nodeid": "tests/test_app.py::test_it",
            "transaction_control": True,
            "statements": [
                {"alias": "default", "proxy": "db", "sql": "SELECT 1"},
                {"alias": "replica", "proxy": None, "sql": "SELECT 2"},
            ],
        }

    def test_the_document_is_json_serialisable(self) -> None:
        request = plugin._ObservedRequest(
            method="GET",
            path_info="/",
            nodeid=None,
            transaction_control=False,
            statements=[],
        )
        assert json.loads(json.dumps({"version": 1, "requests": [request.as_json(lambda _: "db")]}))[
            "version"
        ] == 1

    def test_the_context_records_statements_in_order(self) -> None:
        context = plugin._RequestContext()
        context.add("default", "SELECT 1")
        context.add("replica", "SELECT 2")
        assert context.statements == [("default", "SELECT 1"), ("replica", "SELECT 2")]
