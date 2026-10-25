"""What psycopg sends by itself: no Django, one real connection.

Django's `execute_wrapper` only sees the statements that go through its own
cursors. psycopg runs some of its own — a backend's `TypeInfo.fetch`, a `SET`
while a connection opens — and they go on the wire, so a manifest has to grant
them. These tests drive the psycopg-level capture directly, with a session
stand-in, so what is captured is visible without a whole Django request.
"""

import os

import pytest

from blastit import pytest_plugin as plugin

psycopg = pytest.importorskip("psycopg")

#: In either of the two spellings this repository uses.
DATABASE_URL = os.environ.get("DATABASE_URL") or os.environ.get("BLASTIT_TEST_PG_URL")

pytestmark = pytest.mark.skipif(
    not DATABASE_URL,
    reason="needs a real PostgreSQL: set DATABASE_URL or BLASTIT_TEST_PG_URL",
)


class _FakeSession:
    """The two things the capture asks of a session."""

    def __init__(self) -> None:
        self.setup: list[tuple[str | None, str]] = []

    def record_connection_setup(self, alias: str | None, sql: str) -> None:
        self.setup.append((alias, sql))


@pytest.fixture
def session(monkeypatch: pytest.MonkeyPatch) -> _FakeSession:
    """Installs the psycopg-level capture and a session that keeps its output."""
    plugin._patch_psycopg()
    fake = _FakeSession()
    monkeypatch.setattr(plugin, "_SESSION", fake)
    return fake


@pytest.fixture
def connection() -> object:
    """A psycopg connection of its own, which Django knows nothing about."""
    with psycopg.connect(str(DATABASE_URL)) as conn:
        yield conn


@pytest.fixture
def in_request() -> object:
    """A request context, as `get_response` installs one."""
    context = plugin._RequestContext()
    token = plugin._CURRENT.set(context)
    try:
        yield context
    finally:
        plugin._CURRENT.reset(token)


def test_connection_execute_is_recorded_once(in_request, connection, session) -> None:
    """`Connection.execute` delegates to a cursor: one statement, not two."""
    connection.execute("SELECT %s", [1])
    assert in_request.statements == [("", "SELECT $1")]
    assert session.setup == []


def test_cursor_execute_is_recorded(in_request, connection, session) -> None:
    with connection.cursor() as cursor:
        cursor.execute("SELECT %s, %s", [1, 2])
    assert in_request.statements == [("", "SELECT $1, $2")]


def test_executemany_is_one_statement(in_request, connection, session) -> None:
    with connection.cursor() as cursor:
        cursor.executemany("SELECT %s::int", [(1,), (2,), (3,)])
    assert in_request.statements == [("", "SELECT $1::int")]


def test_stream_is_one_statement(in_request, connection, session) -> None:
    with connection.cursor() as cursor:
        assert list(cursor.stream("SELECT %s::int", [1])) == [(1,)]
    assert in_request.statements == [("", "SELECT $1::int")]


def test_a_server_cursor_is_recorded_as_the_declare_it_sends(
    in_request, connection, session
) -> None:
    """A server-side cursor sends a `DECLARE`, not the query the caller wrote."""
    with connection.cursor(name="blastit_test") as cursor:
        cursor.execute("SELECT %s::int", [1])
        assert cursor.fetchall() == [(1,)]
    assert len(in_request.statements) == 1, in_request.statements
    sql = in_request.statements[0][1]
    assert sql.startswith('DECLARE "blastit_test" CURSOR FOR '), sql
    assert sql.endswith("SELECT $1::int"), sql


def test_type_info_fetch_is_recorded(in_request, connection, session) -> None:
    """What the postgis and hstore backends do at connect, through psycopg."""
    from psycopg.types import TypeInfo

    info = TypeInfo.fetch(connection, "text")
    assert info is not None
    assert len(in_request.statements) == 1, in_request.statements
    sql = in_request.statements[0][1]
    assert "pg_type" in sql and "to_regtype($1)" in sql, sql


def test_a_composable_query_is_rendered_before_it_is_recorded(in_request, connection, session) -> None:
    """psycopg renders a `Composable` itself; the placeholders survive it."""
    from psycopg import sql

    with connection.cursor() as cursor:
        cursor.execute(sql.SQL("SELECT 1 AS {}").format(sql.Identifier("x")))
    assert in_request.statements == [("", 'SELECT 1 AS "x"')]


def test_a_statement_outside_a_request_is_dropped(connection, session) -> None:
    assert plugin._CURRENT.get() is None
    connection.execute("SELECT %s", [1])
    assert session.setup == []


def test_a_statement_during_a_connect_is_connection_setup(connection, session) -> None:
    """No request in flight, and none wanted: this is the connection's SQL, not a route's."""
    token = plugin._CONNECTING.set("default")
    try:
        connection.execute("SET application_name = 'blastit-test'")
    finally:
        plugin._CONNECTING.reset(token)
    assert session.setup == [("default", "SET application_name = 'blastit-test'")]


def test_nothing_is_recorded_without_a_session(monkeypatch, connection, in_request) -> None:
    monkeypatch.setattr(plugin, "_SESSION", None)
    connection.execute("SELECT %s", [1])
    assert in_request.statements == []


def test_the_conversion_is_psycopg_own() -> None:
    """The recorded text is the `$n` text dbproxy checks, `%%` included."""
    assert plugin.to_wire_sql("SELECT %s, %(name)s, 100%%") == "SELECT $1, $2, 100%"
