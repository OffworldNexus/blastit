"""The plugin's pure helpers: the wire form, and reading the manifest's proxies.

No Django, no database, no session: these are the parts of the plugin that can
be tested directly, and the ones where a mistake would quietly report the wrong
SQL.
"""

import contextlib
import functools
import json
import os
import shlex
import shutil
import tempfile
import threading
from pathlib import Path
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


class TestLiveServerManifest:
    """What the live server needs from the manifest, read without Django (D3c)."""

    def manifest(self, tmp_path: Path, text: str) -> Path:
        path = tmp_path / "blastit.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def test_a_url_input_is_the_variable_to_point_at_the_test_database(
        self, tmp_path: Path
    ) -> None:
        path = self.manifest(
            tmp_path,
            "version = 1\n"
            "[proxy.db]\n"
            'protocol = "postgresql"\n'
            'input = { url = "DATABASE_URL" }\n',
        )
        assert plugin._postgresql_url_variable(path) == "DATABASE_URL"

    def test_a_proxy_taking_its_upstream_in_pieces_has_no_url_variable(
        self, tmp_path: Path
    ) -> None:
        path = self.manifest(
            tmp_path,
            "version = 1\n"
            "[proxy.db]\n"
            'protocol = "postgresql"\n'
            'input = { host = "DB_HOST", database = "DB_NAME" }\n',
        )
        assert plugin._postgresql_url_variable(path) is None

    def test_a_manifest_without_a_postgresql_proxy_has_no_url_variable(
        self, tmp_path: Path
    ) -> None:
        path = self.manifest(tmp_path, "version = 1\n")
        assert plugin._postgresql_url_variable(path) is None


class TestLiveObservations:
    """A live server's log, folded into the observations (D3c)."""

    LOG = "\n".join(
        [
            '2026-10-25T14:02:11.384Z app[web.1]: {"event":"request","method":"GET",'
            '"path":"/_blastit/health","route":null,"status":200}',
            '{"event":"request","method":"GET","path":"/notes/","route":"notes","status":200}',
            '{"event":"sql.denied","route":"notes","proxy":"db",'
            '"sql":"SELECT \\"notes\\".\\"body\\" FROM \\"notes\\" WHERE \\"notes\\".\\"id\\" = $?",'
            '"reason":"select on notes.body not granted"}',
            '{"event":"request","method":"GET","path":"/oops/","route":null,"status":404}',
        ]
    )

    def test_a_live_request_carries_its_own_proxy_and_the_live_source(self) -> None:
        assert plugin._live_request("GET", "/notes/", "tests/test_x.py::test_it", []) == {
            "method": "GET",
            "path_info": "/notes/",
            "nodeid": "tests/test_x.py::test_it",
            "transaction_control": False,
            "statements": [],
            "egress": [],
            "source": "live",
        }

    def test_the_log_becomes_requests_with_the_denials_attached(self) -> None:
        session = _FakeSession()
        plugin._Session.record_live(
            session, self.LOG, "tests/test_x.py::test_it", (plugin.HEALTH_PATH,)
        )
        assert len(session.live) == 2, "the health probe is not traffic"
        unrouted, routed = session.live[0], session.live[1]
        assert unrouted["path_info"] == "/oops/", "the 404 blastit answered itself"
        assert routed["path_info"] == "/notes/"
        assert routed["nodeid"] == "tests/test_x.py::test_it"
        statements = routed["statements"]
        assert len(statements) == 1, statements
        assert statements[0]["proxy"] == "db"
        # The log's text is normalised; what the extension gets is its shape, so
        # the table-grant analysis has something to parse (`blastit.logs`).
        assert statements[0]["sql"] == 'SELECT "notes"."body" FROM "notes" WHERE "notes"."id" = $1'

    def test_the_health_path_the_fixture_polls_is_not_traffic(self) -> None:
        session = _FakeSession()
        plugin._Session.record_live(
            session, self.LOG, None, (plugin.HEALTH_PATH,)
        )
        assert [entry["path_info"] for entry in session.live] == ["/oops/", "/notes/"]

    def test_a_denial_with_no_request_event_is_a_problem(self) -> None:
        log = '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SELECT 1"}'
        session = _FakeSession()
        plugin._Session.record_live(session, log, None, ())
        assert session.live == []
        assert session.problems and "notes" in session.problems[0], session.problems


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

    def test_a_connection_that_would_inline_its_parameters_is_named(self, session: _FakeSession) -> None:
        """The `$n` text is invented when Django does not bind on the server."""
        connection = _FakeConnection()
        connection.settings_dict = {"OPTIONS": {}}
        plugin._wrap_connection(connection)
        assert len(session.warnings) == 1
        assert "server_side_binding" in session.warnings[0]
        assert "'default'" in session.warnings[0], session.warnings[0]
        plugin._wrap_connection(connection)
        assert len(session.warnings) == 1, "said once per alias, not once per connect"

    def test_a_cursor_factory_that_binds_client_side_is_named(self, session: _FakeSession) -> None:
        from psycopg import ClientCursor

        connection = _FakeConnection()
        connection.settings_dict = {
            "OPTIONS": {"server_side_binding": True, "cursor_factory": ClientCursor}
        }
        plugin._wrap_connection(connection)
        assert len(session.warnings) == 1
        assert "cursor_factory" in session.warnings[0], session.warnings[0]

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
    """The attributes `_wrap_connection` touches on a Django connection."""

    def __init__(self, alias: str = "default") -> None:
        self.alias = alias
        self.execute_wrappers: list[Any] = []
        # Server-side binding, which is what blastit needs; the tests that check
        # the warning change this.
        self.settings_dict: dict[str, Any] = {"OPTIONS": {"server_side_binding": True}}
        self.connection = None


class _FakeSession:
    """The parts of a session the capture asks for."""

    def __init__(self) -> None:
        self.setup: list[tuple[str | None, str]] = []
        self.warnings: list[str] = []
        self.reported_bindings: set[str] = set()
        self.lock = threading.Lock()
        # What a live server's log folds into (D3c).
        self.live: list[dict[str, Any]] = []
        self.problems: list[str] = []

    def record_connection_setup(self, alias: str | None, sql: str) -> None:
        self.setup.append((alias, sql))

    def record_unrecognised(self, alias: str | None, query: str) -> None:
        self.setup.append((alias, query))


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
            egress=[("AF_INET", "redis", 6379)],
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
            "egress": [{"family": "AF_INET", "host": "redis", "port": 6379}],
        }

    def test_the_document_is_json_serialisable(self) -> None:
        request = plugin._ObservedRequest(
            method="GET",
            path_info="/",
            nodeid=None,
            transaction_control=False,
            statements=[],
            egress=[],
        )
        assert json.loads(json.dumps({"version": 1, "requests": [request.as_json(lambda _: "db")]}))[
            "version"
        ] == 1

    def test_the_context_records_statements_in_order(self) -> None:
        context = plugin._RequestContext()
        context.add("default", "SELECT 1")
        context.add("replica", "SELECT 2")
        assert context.statements == [("default", "SELECT 1"), ("replica", "SELECT 2")]
        assert context.manages_transactions is False, "nothing asked for a transaction"

    def test_the_context_records_egress(self) -> None:
        context = plugin._RequestContext()
        context.add_egress("AF_INET", "redis", 6379)
        assert context.egress == [("AF_INET", "redis", 6379)]


class TestTheLiveServerFilesystem:
    """What the live server is served from, without touching what is not ours (D3c)."""

    def make_project(self, root: Path) -> Path:
        project = root / "project"
        project.mkdir()
        (project / "app.py").write_text("x = 1\n", encoding="utf-8")
        (project / "sub").mkdir()
        (project / "sub" / "mod.py").write_text("y = 2\n", encoding="utf-8")
        (project / ".git").mkdir()
        (project / ".git" / "config").write_text("[core]\n", encoding="utf-8")
        (project / "__pycache__").mkdir()
        return project

    def test_a_traversable_project_is_served_where_it_is(self) -> None:
        # A directory under `/tmp` itself: pytest's own `tmp_path` sits under a
        # `0700` basetemp, which is exactly the case that has to be copied.
        root = Path(tempfile.mkdtemp(prefix="blastit-live-test-", dir="/tmp"))
        try:
            project = self.make_project(root)
            root.chmod(0o755)
            project.chmod(0o755)
            (project / "sub").chmod(0o755)
            assert plugin._traversable(project) is True
            served, temporary = plugin._serveable_project(project, root)
            assert served == project, "no copy is made when the views can read it"
            assert temporary is None
        finally:
            shutil.rmtree(root, ignore_errors=True)

    def test_a_private_project_is_copied_and_nothing_is_loosened(self, tmp_path: Path) -> None:
        """pytest's `tmp_path` is `0700`: the views could not read the code at all."""
        project = self.make_project(tmp_path)
        assert plugin._traversable(project) is False
        before = {path: path.stat().st_mode for path in [project, tmp_path, project / "sub"]}
        served, temporary = plugin._serveable_project(project, tmp_path)
        assert served != project
        assert temporary is not None and temporary.is_dir()
        assert plugin._traversable(served) is True, "the copy is readable by the view uids"
        assert (served / "app.py").read_text(encoding="utf-8") == "x = 1\n"
        assert (served / "sub" / "mod.py").is_file()
        assert not (served / ".git").exists(), "a checkout is not what a server reads"
        assert not (served / "__pycache__").exists()
        after = {path: path.stat().st_mode for path in before}
        assert after == before, "not one mode of the project or its parents changed"

    def test_the_copy_is_removed_by_the_fixture(self, tmp_path: Path) -> None:
        """Teardown removes it; the modes it took are the temporary directory's."""
        project = self.make_project(tmp_path)
        served, temporary = plugin._serveable_project(project, tmp_path)
        assert temporary is not None
        assert temporary.stat().st_mode & 0o001, "the copy's root stays traversable"
        shutil.rmtree(temporary)
        assert not temporary.exists()
        assert served.parent.exists() is False

    def test_the_environment_file_is_readable_by_its_owner_only(self, tmp_path: Path) -> None:
        url = "postgresql://user:it's-a-secret@host/db"
        path = tmp_path / "blastit-live.env"
        plugin._write_environment(path, {"DATABASE_URL": url})
        assert path.stat().st_mode & 0o777 == 0o600, "only its owner and root may read it"
        assert path.read_text(encoding="utf-8") == f"export DATABASE_URL={shlex.quote(url)}\n"

    def test_only_bytecode_this_user_does_not_own_is_removed(self, tmp_path: Path) -> None:
        project = self.make_project(tmp_path)
        cache = project / "__pycache__"
        cache.mkdir(exist_ok=True)
        plugin._clean_bytecode([], project)
        assert cache.is_dir(), "a cache this run wrote is this run's, not root's"

    def test_the_find_only_looks_at_what_this_user_does_not_own(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        seen: list[list[str]] = []

        def run(command: list[str], **kwargs: Any) -> Any:
            seen.append(command)
            return None

        monkeypatch.setattr(plugin.subprocess, "run", run)
        plugin._clean_bytecode(["/usr/bin/sudo", "-n"], tmp_path / "project")
        assert seen == [
            [
                "/usr/bin/sudo",
                "-n",
                "find",
                str(tmp_path / "project"),
                "-name",
                "__pycache__",
                "-type",
                "d",
                "!",
                "-uid",
                str(os.getuid()),
                "-exec",
                "rm",
                "-rf",
                "--",
                "{}",
                "+",
            ]
        ]


class TestLiveServerPrivileges:
    """When blastit cannot run here, the fixture says so instead of failing (D3c)."""

    def test_root_starts_it_directly(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(plugin.os, "geteuid", lambda: 0)
        assert plugin._live_server_prefix() == []

    def test_passwordless_sudo_is_the_other_way(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(plugin.os, "geteuid", lambda: 1000)
        monkeypatch.setattr(plugin.shutil, "which", lambda name: "/usr/bin/sudo")
        monkeypatch.setattr(
            plugin.subprocess, "run", lambda *args, **kwargs: _Completed(0)
        )
        assert plugin._live_server_prefix() == ["/usr/bin/sudo", "-n"]

    def test_sudo_that_wants_a_password_skips(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(plugin.os, "geteuid", lambda: 1000)
        monkeypatch.setattr(plugin.shutil, "which", lambda name: "/usr/bin/sudo")
        monkeypatch.setattr(
            plugin.subprocess, "run", lambda *args, **kwargs: _Completed(1)
        )
        with pytest.raises(pytest.skip.Exception) as skipped:
            plugin._live_server_prefix()
        assert "root" in str(skipped.value) and "sudo" in str(skipped.value)

    def test_no_root_and_no_sudo_skips(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(plugin.os, "geteuid", lambda: 1000)
        monkeypatch.setattr(plugin.shutil, "which", lambda name: None)
        with pytest.raises(pytest.skip.Exception) as skipped:
            plugin._live_server_prefix()
        assert "passwordless sudo" in str(skipped.value)


class TestLiveAppSpec:
    """Which WSGI callable the live server serves (D3c)."""

    def test_the_option_wins(self) -> None:
        config = _FakeConfig({"blastit_live_app": "myproject.wsgi:application"})
        assert plugin._live_app_spec(config) == "myproject.wsgi:application"

    def test_django_without_settings_is_a_skip_not_a_raise(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """No Django configured at all: the fixture skips, it never breaks (D3c)."""
        import django.conf

        class Unconfigured:
            def __getattr__(self, name: str) -> Any:
                raise RuntimeError("settings are not configured")

        monkeypatch.setattr(django.conf, "settings", Unconfigured())
        monkeypatch.delenv("DJANGO_SETTINGS_MODULE", raising=False)
        assert plugin._live_app_spec(_FakeConfig({})) == ""


class _Completed:
    """A `subprocess.run` result with just a return code."""

    def __init__(self, returncode: int) -> None:
        self.returncode = returncode


class _FakeConfig:
    """The two calls `_live_app_spec` makes on a pytest config."""

    def __init__(self, options: dict[str, Any]) -> None:
        self.options = options

    def getoption(self, name: str) -> Any:
        return self.options.get(name)
