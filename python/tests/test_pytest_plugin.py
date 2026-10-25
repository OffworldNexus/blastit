"""The plugin's own tests: real pytest sessions over a real Django project.

Every test here starts a `pytest` subprocess in a copy of `python/tests/project`
— a tiny Django app plus the manifest the plugin diffs against — and reads the
report it wrote. That is the only honest way to test a pytest plugin: the
options, the hooks, the terminal summary, the exit code and the xdist merge are
all things only a session has.
"""

import json
import os
import shutil
import subprocess
import sys
import tomllib
from importlib import metadata
from pathlib import Path
from typing import Any

import pytest

HERE = Path(__file__).parent
PROJECT = HERE / "project"

#: PostgreSQL for the test database, in either of the two spellings this
#: repository already uses (CI sets BLASTIT_TEST_PG_URL for dbproxy's tests).
DATABASE_URL = os.environ.get("DATABASE_URL") or os.environ.get("BLASTIT_TEST_PG_URL")

pytestmark = pytest.mark.skipif(
    not DATABASE_URL,
    reason="needs a real PostgreSQL: set DATABASE_URL or BLASTIT_TEST_PG_URL",
)


class Outcome:
    """One inner pytest session: its exit code, its output and its report."""

    def __init__(self, completed: subprocess.CompletedProcess[str], report_path: Path) -> None:
        self.returncode = completed.returncode
        self.stdout = completed.stdout
        self.stderr = completed.stderr
        self.report_path = report_path
        self._report: dict[str, Any] | None = None

    @property
    def report(self) -> dict[str, Any]:
        """The JSON report the session wrote, parsed."""
        if self._report is None:
            assert self.report_path.is_file(), (
                f"the session wrote no report at {self.report_path}:\n"
                f"stdout:\n{self.stdout}\nstderr:\n{self.stderr}"
            )
            self._report = json.loads(self.report_path.read_text(encoding="utf-8"))
        return self._report

    def route(self, name: str) -> dict[str, Any]:
        """One route of the report, by name."""
        for route in self.report["routes"]:
            if route["name"] == name:
                return route
        raise AssertionError(f"no route {name!r} in {[r['name'] for r in self.report['routes']]}")

    def proxy(self, name: str, proxy: str = "db") -> dict[str, Any]:
        """One `(route, proxy)` block of the report."""
        for block in self.route(name)["proxies"]:
            if block["name"] == proxy:
                return block
        raise AssertionError(f"route {name!r} has no proxy {proxy!r} block")

    def missing_sql(self, name: str, proxy: str = "db") -> list[str]:
        """The normalised SQL every `missing` entry of a route names."""
        return [entry["sql"] for entry in self.proxy(name, proxy)["missing"]]

    def setup_statements(self) -> list[dict[str, Any]]:
        """The connection-setup statements of the report."""
        return list(self.report["connection_setup"]["statements"])

    def setup(self, marker: str) -> dict[str, Any]:
        """The connection-setup statement whose normalised SQL contains `marker`."""
        for entry in self.setup_statements():
            if marker in entry["sql"]:
                return entry
        raise AssertionError(
            f"no connection-setup statement contains {marker!r}: "
            f"{[entry['sql'] for entry in self.setup_statements()]}"
        )


@pytest.fixture(scope="session")
def project(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A pristine copy of the tiny Django project."""
    installed = {entry.name for entry in metadata.entry_points(group="pytest11")}
    assert "blastit" in installed, (
        "the plugin is not installed as a pytest plugin; build and install the "
        "wheel first: scripts/build-wheel.sh && uv pip install dist/blastit-*.whl"
    )
    target = tmp_path_factory.mktemp("blastit-project") / "project"
    shutil.copytree(PROJECT, target, ignore=shutil.ignore_patterns("__pycache__"))
    return target


def copy_project(source: Path, tmp_path: Path) -> Path:
    """Copies the tiny project into a fresh directory, for one inner session."""
    target = tmp_path / "project"
    shutil.copytree(source, target, ignore=shutil.ignore_patterns("__pycache__"))
    return target


def run(
    project: Path,
    *args: str,
    report: str = "blastit-report.json",
    env: dict[str, str] | None = None,
) -> Outcome:
    """Runs pytest in a project copy, and returns what the session did.

    `report` is both what the session is asked to write (`--blastit-report`) and
    where it is read back from — pass a distinct name when one directory runs
    several sessions.
    """
    environment = {
        **os.environ,
        "DATABASE_URL": str(DATABASE_URL),
        "PYTHONDONTWRITEBYTECODE": "1",
        **(env or {}),
    }
    completed = subprocess.run(  # noqa: S603 - sys.executable, fixed arguments
        [sys.executable, "-m", "pytest", *args, "--blastit-report", report],
        cwd=project,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    return Outcome(completed, project / report)


@pytest.fixture(scope="session")
def baseline(project: Path, tmp_path_factory: pytest.TempPathFactory) -> Outcome:
    """One full session of the tiny project, against its unfinished manifest."""
    return run(
        copy_project(project, tmp_path_factory.mktemp("baseline")),
        "--blastit-manifest",
        "blastit.toml",
    )


@pytest.fixture(scope="session")
def strict(project: Path, tmp_path_factory: pytest.TempPathFactory) -> Outcome:
    """The same session, with `--blastit-strict`."""
    return run(
        copy_project(project, tmp_path_factory.mktemp("strict")),
        "--blastit-manifest",
        "blastit.toml",
        "--blastit-strict",
    )


def test_the_suite_itself_passes(baseline: Outcome) -> None:
    """The plugin records requests; it does not change what the tests do."""
    assert baseline.returncode == 0, baseline.stdout + baseline.stderr
    assert "14 passed, 1 skipped" in baseline.stdout, baseline.stdout


def test_a_granted_route_is_clean(baseline: Outcome) -> None:
    """`/notes/` is served twice — by the client and by the live server — and covered."""
    assert baseline.route("notes")["requests"] == 2, "the client and the live server"
    assert baseline.proxy("notes")["missing"] == []
    assert baseline.proxy("notes")["unused"] == []
    assert baseline.proxy("notes")["observed"] == 2
    assert baseline.route("notes")["needs_transaction_control"] is False


def test_a_missing_grant_names_the_statement_the_test_and_the_path(baseline: Outcome) -> None:
    """The DELETE is executed and not granted: the report says exactly where."""
    delete = 'DELETE FROM "notes_note" WHERE "notes_note"."id" = $?'
    assert baseline.missing_sql("note-delete") == [delete]
    entry = baseline.proxy("note-delete")["missing"][0]
    assert entry["count"] == 1
    assert entry["wire_sql"] == 'DELETE FROM "notes_note" WHERE "notes_note"."id" = $1'
    assert entry["tests"] == ["tests/test_app.py::test_delete_note"]
    assert entry["paths"] == ["/notes/4/delete/"]
    assert baseline.report["totals"]["missing"] == 12, (
        "the DELETE, the savepoints and psycopg's BEGIN, bulk_create's two shapes, "
        "the type lookup, the named cursor's three statements and the live "
        "server's BEGIN/COMMIT"
    )


def test_a_grant_nothing_matched_is_unused(baseline: Outcome) -> None:
    """A stale grant is reported, and does not fail a run on its own."""
    stale = 'SELECT "notes_note"."id" FROM "notes_note" ORDER BY "notes_note"."id" DESC LIMIT 1'
    assert baseline.proxy("note-delete")["unused"] == [stale]
    assert baseline.proxy("note-create")["unused"] == [
        'SELECT "notes_note"."body" FROM "notes_note" WHERE "notes_note"."id" = $?'
    ]
    assert baseline.proxy("note-create")["missing"] == [], "its own grant matched"
    assert baseline.report["totals"]["unused"] == 2


def test_a_savepoint_needs_transaction_control(baseline: Outcome) -> None:
    """A nested atomic is a SAVEPOINT, whose name changes on every run."""
    route = baseline.route("note-atomic")
    assert route["needs_transaction_control"] is True
    assert route["proxies"][0]["transaction_control"] == {
        "observed": True,
        "granted": False,
        "needed": True,
    }
    missing = baseline.missing_sql("note-atomic")
    assert missing, "the savepoint statements are denied"
    for sql in missing:
        assert sql.startswith(("SAVEPOINT ", "RELEASE SAVEPOINT ", "BEGIN")), sql
    assert "UPDATE" not in " ".join(missing), "the UPDATE it sends is granted"
    assert baseline.report["totals"]["needs_transaction_control"] == [
        "note-atomic",
        "note-atomic-live",
    ]


def test_an_id_list_of_any_length_is_one_grant(baseline: Outcome) -> None:
    """Two requests, one with one id and one with three: the `IN` grant covers both.

    D3a's shape rule: `IN ($1)` and `IN ($1, $2, $3)` normalise to `IN ($?, ...)`,
    so a `prefetch_related` of ten objects does not need a grant of its own.
    """
    assert baseline.route("note-in")["requests"] == 2
    assert baseline.proxy("note-in")["observed"] == 2
    assert baseline.proxy("note-in")["missing"] == [], "one grant, both list lengths"
    assert baseline.proxy("note-in")["unused"] == [], "and the grant was used"


def test_a_run_of_values_rows_is_one_grant(baseline: Outcome) -> None:
    """A two-row and a five-row hand-written `INSERT` match the same collapsed grant."""
    assert baseline.route("note-multi")["requests"] == 2
    assert baseline.proxy("note-multi")["observed"] == 2
    assert baseline.proxy("note-multi")["missing"] == [], "one grant, both row counts"
    assert baseline.proxy("note-multi")["unused"] == []


def test_bulk_create_sends_two_shapes(baseline: Outcome) -> None:
    """`bulk_create` of one object and of three are two statements, not one.

    Django sends a single `VALUES` row for one object and an `UNNEST` array for
    several, so the row count changes the statement itself: both have to be
    granted, and a grant for one never covers the other.
    """
    missing = baseline.missing_sql("note-bulk")
    assert len(missing) == 2, missing
    unnest = [sql for sql in missing if "UNNEST(" in sql]
    values = [sql for sql in missing if "UNNEST(" not in sql and "VALUES (" in sql]
    assert len(unnest) == 1 and len(values) == 1, missing
    assert all(", ..." not in sql for sql in missing), "neither is a collapsed run"
    assert unnest[0] != values[0]
    assert baseline.proxy("note-bulk")["observed"] == 2


def test_a_statement_psycopg_runs_itself_is_reported(baseline: Outcome) -> None:
    """The type lookup goes straight through psycopg, and is still diffed.

    This is what the postgis and hstore backends do at connect; no Django cursor
    of the request sees it, so only the psycopg-level capture can find it.
    """
    missing = baseline.missing_sql("note-types")
    assert len(missing) == 1, missing
    assert "pg_type" in missing[0] and "to_regtype($?)" in missing[0], missing
    entry = baseline.proxy("note-types")["missing"][0]
    assert entry["count"] == 1
    assert entry["tests"] == ["tests/test_app.py::test_type_info_fetch"]
    assert baseline.proxy("note-types")["observed"] == 1


def test_a_named_cursor_is_reported_as_the_declare_it_sends(baseline: Outcome) -> None:
    """`iterator()` gets a server-side cursor: the wire text is not the SELECT.

    Django hands `QuerySet.iterator()` a `chunked_cursor()`, i.e. psycopg's
    `ServerCursor`, so PostgreSQL receives a `DECLARE … CURSOR FOR …`, then a
    `FETCH`, then a `CLOSE` — and a grant has to hold those, not the query the
    view reads.
    """
    missing = baseline.missing_sql("note-stream")
    assert len(missing) == 3, missing
    declare = [sql for sql in missing if sql.startswith("DECLARE ")]
    assert len(declare) == 1, missing
    assert 'CURSOR FOR SELECT "notes_note"."id"' in declare[0], declare
    assert any(sql.startswith("FETCH FORWARD ") for sql in missing), missing
    assert any(sql.startswith("CLOSE ") for sql in missing), missing
    assert all(not sql.startswith("SELECT") for sql in missing), (
        "the plain SELECT never went on the wire"
    )
    entry = baseline.proxy("note-stream")["missing"][0]
    assert entry["tests"] == ["tests/test_app.py::test_named_cursor_stream"]


def test_psycopg_own_transaction_commands_are_reported(baseline: Outcome) -> None:
    """With no test transaction around it, psycopg sends the BEGIN and COMMIT.

    `transaction=True` and the live server together: Django's `atomic()` calls
    `set_autocommit(False)` and `commit()`, and psycopg answers with a `BEGIN`
    and a `COMMIT` of its own — statements no Django cursor sees, and the only
    hint that the route needs `transaction_control` in production.
    """
    route = baseline.route("note-atomic-live")
    assert route["requests"] == 1
    assert route["needs_transaction_control"] is True
    assert baseline.missing_sql("note-atomic-live") == ["BEGIN", "COMMIT"]
    assert baseline.report["totals"]["needs_transaction_control"] == [
        "note-atomic",
        "note-atomic-live",
    ], "the savepoint route and the psycopg-BEGIN route"


def test_a_route_that_opens_connections_needs_egress(baseline: Outcome) -> None:
    """`socket.connect` is where every TCP client ends up, so it is all seen."""
    queue = baseline.route("note-queue")["egress"]
    assert queue["needed"] is True, "no egress = true on this route"
    assert queue["observed"] == [
        {
            "target": "127.0.0.1:9",
            "host": "127.0.0.1",
            "port": 9,
            "family": "AF_INET",
            "count": 1,
            "tests": ["tests/test_app.py::test_egress_routes"],
        }
    ]
    cache = baseline.route("note-cache")["egress"]
    assert cache["observed"] == queue["observed"], "the same view, the same attempt"
    assert cache["needed"] is False, "this route has egress = true"
    assert baseline.report["totals"]["egress"] == 1
    for name in ("notes", "note-delete", "orders"):
        assert baseline.route(name)["egress"] == {"observed": [], "needed": False}, name


def test_connection_setup_names_the_routes_that_do_not_grant_it(baseline: Outcome) -> None:
    """The statements every connection runs are reported apart from the requests.

    Under blastit every request opens its own connection, so these run on every
    route in production — while here they ran once, mostly outside any request.
    """
    assert len(baseline.setup_statements()) == 2, baseline.setup_statements()
    django_setup = baseline.setup("set_config")
    assert django_setup["sql"] == "SELECT set_config('TimeZone', $?, false)"
    assert django_setup["wire_sql"] == "SELECT set_config('TimeZone', $1, false)"
    assert django_setup["proxy"] == "db"
    assert django_setup["count"] >= 1
    assert django_setup["ungranted_in"] == [
        "notes",
        "note-create",
        "note-delete",
        "note-atomic",
        "note-in",
        "note-multi",
        "note-atomic-live",
        "orders",
    ], "every route with a listener, and not the ones with no grant at all"
    assert django_setup["granted_in"] == 0

    app_setup = baseline.setup("application_name")
    assert app_setup["sql"] == "SET application_name = 'blastit-test'"
    assert app_setup["ungranted_in"] == django_setup["ungranted_in"]
    assert baseline.report["connection_setup"]["ungranted"] == 2
    assert baseline.report["totals"]["connection_setup"] == 2
    assert baseline.report["totals"]["connection_setup_ungranted"] == 2


def test_an_unrouted_path_is_reported_with_its_test(baseline: Outcome) -> None:
    """`/oops/` exists in Django; blastit would answer 404 before reaching it."""
    unrouted = baseline.report["unrouted"]
    assert len(unrouted) == 1, unrouted
    assert unrouted[0]["method"] == "GET"
    assert unrouted[0]["path_info"] == "/oops/"
    assert unrouted[0]["count"] == 1
    assert unrouted[0]["tests"] == ["tests/test_app.py::test_unrouted_path"]
    assert baseline.report["totals"]["unrouted"] == 1


def test_a_route_no_request_reached_is_named(baseline: Outcome) -> None:
    """`orders` is declared and nothing exercises it."""
    assert baseline.report["unexercised_routes"] == ["orders"]
    assert baseline.route("orders")["requests"] == 0
    assert baseline.route("orders")["proxies"] == []
    assert baseline.route("orders")["egress"] == {"observed": [], "needed": False}
    assert baseline.report["totals"]["routes_exercised"] == 12


def test_sql_outside_a_request_is_ignored(baseline: Outcome) -> None:
    """The plugin records requests, not statements: pytest-django's own SQL is invisible."""
    assert "4242" not in baseline.report_path.read_text(encoding="utf-8")
    assert baseline.report["totals"]["statements"] == 21, (
        "one per request, plus the savepoints, psycopg's BEGIN, bulk_create's "
        "second shape, the type lookup and the named cursor's three statements"
    )


def test_the_report_counts_what_the_manifest_does_not_cover(baseline: Outcome) -> None:
    """The totals are the summary a human reads first."""
    totals = baseline.report["totals"]
    assert totals["requests"] == 17
    assert totals["routed"] == 16
    assert totals["unrouted"] == 1
    assert totals["routes"] == 13
    assert totals["missing_distinct"] == 12
    assert totals["egress"] == 1
    assert totals["unrecognised"] == 0
    assert totals["plugin_warnings"] == 0
    assert totals["clean"] is False
    assert baseline.report["version"] == 1
    assert baseline.report["manifest"] == {
        "routes": 13,
        "proxies": {"db": "postgresql"},
        "warnings": [],
    }
    assert baseline.report["unrecognised"] == {"statements": [], "count": 0}
    assert baseline.report["plugin_warnings"] == []


def test_the_terminal_summary_is_short_and_actionable(baseline: Outcome) -> None:
    """What the developer sees without opening the JSON."""
    assert "blastit: 17 requests, 21 statements, 12/13 routes exercised" in baseline.stdout
    assert "blastit: route note-delete: 1 statement(s) missing" in baseline.stdout
    assert "blastit: route note-bulk: 2 statement(s) missing" in baseline.stdout
    assert "blastit: route note-atomic-live: 2 statement(s) missing" in baseline.stdout
    assert "blastit: route note-atomic: needs transaction_control = true" in baseline.stdout
    assert (
        "blastit: route note-queue opens outbound connections (127.0.0.1:9) "
        "but has no egress = true" in baseline.stdout
    )
    assert "blastit: 1 unrouted request(s): GET /oops/" in baseline.stdout
    assert (
        "blastit: 2 connection-setup statement(s) some routes do not grant: "
        "put them in a group attached to every route" in baseline.stdout
    )
    assert "blastit-report.json" in baseline.stdout


def test_strict_fails_the_session(strict: Outcome) -> None:
    """`--blastit-strict` is the exit code a CI job can watch."""
    assert strict.returncode == 1, strict.stdout + strict.stderr
    assert (
        "blastit: --blastit-strict: 12 statement execution(s) missing, 1 unrouted "
        "request(s), 2 connection-setup statement(s) not granted in every route, "
        "1 route(s) opening connections without egress = true, 0 unreadable "
        "statement(s) and 0 warning(s)" in strict.stdout
    )
    assert strict.report["totals"]["clean"] is False


def test_the_plugin_is_inert_without_a_manifest(project: Path, tmp_path: Path) -> None:
    """No manifest, no patching, no report, nothing printed — a plain test run.

    The inner suite carries a test of its own (`test_the_plugin_is_inert`) that
    only runs here: it asserts that `BaseHandler.get_response`, psycopg and
    `socket.socket.connect` are all untouched.
    """
    copy = copy_project(project, tmp_path)
    outcome = run(copy, report="never-written.json")
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "15 passed" in outcome.stdout, "its own inert test runs here, and passes"
    assert "blastit:" not in outcome.stdout, outcome.stdout
    assert not outcome.report_path.exists(), "a report with no manifest to diff against"
    assert not list(copy.glob("never-written.json.*"))


def test_the_report_is_enough_to_fill_the_manifest_in(
    baseline: Outcome, project: Path, tmp_path: Path
) -> None:
    """The whole point: what the report says, applied to the manifest, is clean.

    Applying the report happens twice, which is the honest shape of the loop: the
    first pass adds the statements of the routes that ran — including the two
    `bulk_create` shapes, the type lookup psycopg sends and the named cursor's
    three statements — sets `egress = true` where a route opened connections,
    puts the connection-setup statements in a group attached to every route, and
    gives the unrouted path a route of its own. Only then do that route's own
    statements show up as missing, because until it existed nothing could
    attribute them to it.
    """
    assert baseline.report["totals"]["clean"] is False
    copy = copy_project(project, tmp_path)
    source = (copy / "blastit.toml").read_text(encoding="utf-8")

    repaired = repaired_manifest(source, baseline)
    assert "SELECT set_config('TimeZone', $?, false)" in repaired, (
        "the connection-setup statement is in the repair"
    )
    assert 'egress = true' in repaired, "and so is the route that opens connections"
    (copy / "blastit-repaired.toml").write_text(repaired, encoding="utf-8")
    second = run(
        copy,
        "--blastit-manifest",
        "blastit-repaired.toml",
        report="second-report.json",
    )
    assert second.returncode == 0, second.stdout + second.stderr
    assert second.report["totals"]["unrouted"] == 0, "the path has a route now"
    assert second.report["totals"]["needs_transaction_control"] == []
    assert second.report["totals"]["egress"] == 0, "egress = true is in the manifest"
    assert second.report["connection_setup"]["ungranted"] == 0, (
        "the group attached to every route covers the connection setup"
    )
    assert second.missing_sql("fallback") == ["SELECT 1"], "the new route's own SQL"
    named_cursor = second.missing_sql("note-stream")
    assert second.report["totals"]["missing"] == 1 + len(named_cursor), (
        "everything the first report listed is granted, except the named cursor"
    )
    assert len(named_cursor) == 3, named_cursor
    assert all(
        sql.startswith(("DECLARE ", "FETCH ", "CLOSE ")) for sql in named_cursor
    ), named_cursor

    # A named cursor's `DECLARE`/`FETCH`/`CLOSE` carry a name Django generates
    # for each run — `_django_curs_<id>_sync_<n>` — so no grant can ever match
    # them (the README says what to do instead: turn server-side cursors off).
    # Everything else converges, which is what this third run checks.
    (copy / "blastit-repaired.toml").write_text(
        repaired_manifest(repaired, second), encoding="utf-8"
    )
    third = run(
        copy,
        "--blastit-manifest",
        "blastit-repaired.toml",
        "--blastit-strict",
        "-k",
        "not named_cursor",
        report="third-report.json",
    )
    assert third.returncode == 0, third.stdout + third.stderr
    assert third.report["totals"]["clean"] is True, json.dumps(third.report["totals"])
    assert "blastit: the manifest covers every request that was made" in third.stdout


def test_atomic_requests_on_the_alias_needs_transaction_control(
    project: Path, tmp_path: Path
) -> None:
    """`ATOMIC_REQUESTS` is a transaction on the route, granted or not."""
    outcome = run(
        copy_project(project, tmp_path),
        "--blastit-manifest",
        "blastit.toml",
        "-k",
        "test_list_notes",
        env={"BLASTIT_TEST_ATOMIC": "1"},
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert outcome.route("notes")["needs_transaction_control"] is True
    assert outcome.report["totals"]["needs_transaction_control"] == ["notes"]


def test_more_than_one_proxy_needs_a_mapping(project: Path, tmp_path: Path) -> None:
    """With two proxies, the plugin asks which one an alias talks to."""
    copy = copy_project(project, tmp_path)
    (copy / "blastit-two-proxies.toml").write_text(two_proxy_manifest(), encoding="utf-8")
    without = run(
        copy, "--blastit-manifest", "blastit-two-proxies.toml", "-k", "test_list_notes"
    )
    assert without.returncode == 4, "a usage error, before any test runs"
    assert "--blastit-proxy alias=proxy" in (without.stdout + without.stderr).replace("\n", " ")
    with_mapping = run(
        copy,
        "--blastit-manifest",
        "blastit-two-proxies.toml",
        "--blastit-proxy",
        "default=warehouse",
        "-k",
        "test_list_notes",
        report="mapped-report.json",
    )
    assert with_mapping.returncode == 0, with_mapping.stdout + with_mapping.stderr
    assert with_mapping.route("notes")["proxies"][0]["name"] == "warehouse"
    assert with_mapping.missing_sql("notes", "warehouse") == [
        'SELECT "notes_note"."id" AS "id" FROM "notes_note" '
        'WHERE "notes_note"."owner_id" = $?'
    ]
    assert with_mapping.setup("set_config")["ungranted_in"] == ["notes"]


def test_a_typo_in_the_mapping_is_a_usage_error(project: Path, tmp_path: Path) -> None:
    """`--blastit-proxy` is checked against the manifest's own proxies."""
    outcome = run(
        copy_project(project, tmp_path),
        "--blastit-manifest",
        "blastit.toml",
        "--blastit-proxy",
        "default=not-a-proxy",
    )
    assert outcome.returncode == 4
    assert "not-a-proxy" in outcome.stdout + outcome.stderr


def test_strict_without_a_manifest_is_a_usage_error(project: Path, tmp_path: Path) -> None:
    """The plugin is inert, so asking it for anything is worth naming."""
    outcome = run(copy_project(project, tmp_path), "--blastit-strict")
    output = (outcome.stdout + outcome.stderr).replace("\n", " ")
    assert outcome.returncode == 4
    assert "need --blastit-manifest" in output, output


def test_an_invalid_manifest_fails_the_session(project: Path, tmp_path: Path) -> None:
    """The plugin never reports against a manifest it could not parse."""
    copy = copy_project(project, tmp_path)
    (copy / "broken.toml").write_text(
        "version = 1\n"
        "[proxy.db]\n"
        'protocol = "postgresql"\n'
        'input = { url = "DATABASE_URL" }\n'
        "[[route]]\n"
        'name = "notes"\n'
        'path = ["("]\n',
        encoding="utf-8",
    )
    outcome = run(copy, "--blastit-manifest", "broken.toml", "-k", "test_list_notes")
    assert outcome.returncode == 1
    assert "blastit: the audit failed" in outcome.stdout
    assert "invalid" in outcome.stdout, outcome.stdout
    assert not outcome.report_path.exists(), "no report is written for an invalid manifest"


def test_xdist_workers_are_merged(project: Path, tmp_path: Path) -> None:
    """One report for the whole run, whatever `-n` was."""
    copy = copy_project(project, tmp_path)
    outcome = run(
        copy,
        "--blastit-manifest",
        "blastit.toml",
        "-n",
        "2",
        report="xdist-report.json",
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "14 passed, 1 skipped" in outcome.stdout
    totals = outcome.report["totals"]
    assert totals["requests"] == 17, "every request, from both workers"
    assert totals["statements"] == 21
    assert totals["routes_exercised"] == 12
    assert totals["missing"] == 12
    assert totals["connection_setup_ungranted"] == 2, "counted once, not once per worker"
    assert not list(copy.glob("xdist-report.json.gw*")), "the worker files are cleaned up"
    assert "blastit: route note-delete: 1 statement(s) missing" in outcome.stdout


def test_an_alias_without_server_side_binding_is_reported(
    project: Path, tmp_path: Path
) -> None:
    """Django's default inlines the parameters: the `$n` text would be invented.

    blastit needs `OPTIONS["server_side_binding"] = True` (§6), and the plugin
    says so instead of quietly reporting text dbproxy would never see. It is a
    warning, so a run still passes — `--blastit-strict` is what fails.
    """
    copy = copy_project(project, tmp_path)
    outcome = run(
        copy,
        "--blastit-manifest",
        "blastit.toml",
        "-k",
        "test_list_notes",
        env={"BLASTIT_TEST_CLIENT_BINDING": "1"},
    )
    assert outcome.returncode == 0, "a warning, and the tests still run"
    assert "blastit: the database alias 'default' does not use server-side binding" in (
        outcome.stdout
    ), outcome.stdout
    assert "server_side_binding" in outcome.stdout
    assert outcome.report["plugin_warnings"], outcome.report["plugin_warnings"]
    assert outcome.report["totals"]["plugin_warnings"] >= 1
    assert outcome.report["totals"]["clean"] is False

    strict = run(
        copy,
        "--blastit-manifest",
        "blastit.toml",
        "--blastit-strict",
        "-k",
        "test_list_notes",
        report="strict-binding.json",
        env={"BLASTIT_TEST_CLIENT_BINDING": "1"},
    )
    assert strict.returncode == 1, "strict fails over a report that cannot be trusted"
    assert "warning(s)" in strict.stdout


def test_a_stray_file_next_to_the_report_is_not_merged(project: Path, tmp_path: Path) -> None:
    """Only worker reports of this version are merged; anything else is reported."""
    copy = copy_project(project, tmp_path)
    stray = copy / "stray.json.old"
    stray.write_text(
        '{"version": 1, "requests": [{"method": "GET", "path_info": "/strange/"}]}',
        encoding="utf-8",
    )
    outcome = run(copy, "--blastit-manifest", "blastit.toml", "-n", "2", report="stray.json")
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "stray.json.old is not a blastit worker report" in outcome.stdout
    assert stray.is_file(), "left where it was"
    assert outcome.report["totals"]["requests"] == 17, "the workers were merged, nothing else"
    assert outcome.report["totals"]["unrouted"] == 1, "no /strange/ request in the report"
    assert outcome.report["plugin_warnings"], outcome.report["plugin_warnings"]


def test_xdist_reports_are_not_merged_twice(project: Path, tmp_path: Path) -> None:
    """A second run in the same directory does not see the first run's requests."""
    copy = copy_project(project, tmp_path)
    first = run(copy, "--blastit-manifest", "blastit.toml", "-n", "2", report="twice.json")
    second = run(copy, "--blastit-manifest", "blastit.toml", "-n", "2", report="twice.json")
    assert first.returncode == second.returncode == 0
    assert first.report["totals"]["requests"] == second.report["totals"]["requests"] == 17
    assert second.report["connection_setup"]["ungranted"] == 2
    assert not list(copy.glob("twice.json.gw*"))


def is_transaction_control(sql: str) -> bool:
    """Whether a reported statement is transaction control.

    Only this test module needs to know: the report says which routes need
    `transaction_control`, and the statements it lists as missing on those routes
    are the savepoints, whose names change on every run — a grant for their text
    would be meaningless.
    """
    return sql.startswith(
        ("SAVEPOINT ", "RELEASE ", "ROLLBACK", "BEGIN", "COMMIT", "START TRANSACTION", "END")
    )


def repaired_manifest(source: str, outcome: Outcome) -> str:
    """Applies a report to a manifest, the way a human or an agent would.

    * every `missing` statement that is not transaction control is appended to
      its route's grant, on the proxy the report names — the report hands the
      normalised text over, which is exactly what a grant holds;
    * a route that `needs_transaction_control` gets the flag, instead of a grant
      for a savepoint whose name changes on every run;
    * the connection-setup statements go into a group attached to every route
      (`groups = ["connection"]`), which is what the report's
      `ungranted_in` is telling you to do: they run on every route in
      production, so they cannot belong to one;
    * a route whose `egress.needed` is true gets `egress = true`, because it
      opened connections and the manifest does not let it;
    * a path the report calls `unrouted` gets a route of its own: blastit would
      have answered 404 before Django saw it. That route's statements were not
      attributed to anything (they could not be), so the next run is the one that
      reports them.
    """
    document = tomllib.loads(source)
    routes = {route["name"]: route for route in document.get("route", [])}
    for reported in outcome.report["routes"]:
        route = routes.get(reported["name"])
        if route is None:
            continue
        for block in reported["proxies"]:
            queries = route.setdefault(block["name"], {}).setdefault("queries", [])
            for entry in block["missing"]:
                if not is_transaction_control(entry["sql"]) and entry["sql"] not in queries:
                    queries.append(entry["sql"])
            if block["transaction_control"]["needed"]:
                route[block["name"]]["transaction_control"] = True
        if reported["egress"]["needed"]:
            route["egress"] = True

    setup = [
        entry["sql"]
        for entry in outcome.report["connection_setup"]["statements"]
        if entry["ungranted_in"]
    ]
    if setup:
        groups = document.setdefault("groups", [])
        if "connection" not in groups:
            groups.append("connection")
        grant = document.setdefault("group", {}).setdefault("connection", {}).setdefault("db", {})
        queries = grant.setdefault("queries", [])
        queries.extend(sql for sql in setup if sql not in queries)

    for entry in outcome.report["unrouted"]:
        document.setdefault("route", []).append(
            {"name": "fallback", "path": [entry["path_info"]], "db": {"queries": []}}
        )
    return dumps_manifest(document)


def dumps_manifest(document: dict[str, Any]) -> str:
    """Writes the small subset of `blastit.toml` the tiny project uses.

    `version`, top-level `groups`, `[proxy.*]` with an inline `input`,
    `[group.*.<proxy>]`, and `[[route]]` with an inline `[route.<proxy>]` grant —
    so a test can apply a report to a manifest the way a human would, without a
    TOML writer in the plugin's own dependencies.
    """
    lines = [f"version = {document['version']}"]
    if document.get("groups"):
        lines.append(f"groups = {_toml(document['groups'])}")
    for name, proxy in document.get("proxy", {}).items():
        inputs = ", ".join(
            f"{key} = {_toml(value)}" for key, value in proxy.get("input", {}).items()
        )
        lines += [
            "",
            f"[proxy.{name}]",
            f"protocol = {_toml(proxy['protocol'])}",
            f"input = {{ {inputs} }}",
        ]
    for name, group in document.get("group", {}).items():
        for proxy_name, grant in group.items():
            lines += ["", f"[group.{name}.{proxy_name}]"]
            if grant.get("transaction_control"):
                lines.append("transaction_control = true")
            lines.append(f"queries = {_toml(grant.get('queries', []))}")
    for route in document.get("route", []):
        lines += [
            "",
            "[[route]]",
            f"name = {_toml(route['name'])}",
            f"path = {_toml(route['path'])}",
        ]
        for field in ("methods", "groups"):
            if field in route:
                lines.append(f"{field} = {_toml(route[field])}")
        for field in ("egress", "prefork"):
            if route.get(field):
                lines.append(f"{field} = true")
        for name, grant in route.items():
            if not isinstance(grant, dict):
                continue
            lines.append("")
            lines.append(f"[route.{name}]")
            if grant.get("transaction_control"):
                lines.append("transaction_control = true")
            lines.append(f"queries = {_toml(grant.get('queries', []))}")
    return "\n".join(lines) + "\n"


def _toml(value: Any) -> str:
    """One TOML value: what the manifest writer above needs, and nothing more."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ", ".join(_toml(item) for item in value) + "]"
    raise TypeError(f"the manifest writer cannot write {value!r}")


def two_proxy_manifest() -> str:
    """A manifest with two `postgresql` proxies: only the tests can say which is which."""
    return (
        "version = 1\n"
        "\n"
        "[proxy.db]\n"
        'protocol = "postgresql"\n'
        'input = { url = "DATABASE_URL" }\n'
        "\n"
        "[proxy.warehouse]\n"
        'protocol = "postgresql"\n'
        'input = { url = "WAREHOUSE_URL" }\n'
        "\n"
        "[[route]]\n"
        'name = "notes"\n'
        "path = ['/notes/']\n"
        'methods = ["GET"]\n'
        "[route.warehouse]\n"
        "queries = ['SELECT 1']\n"
    )
