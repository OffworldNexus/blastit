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
    assert "7 passed" in baseline.stdout, baseline.stdout


def test_a_granted_route_is_clean(baseline: Outcome) -> None:
    """`/notes/` is served twice — by the client and by the live server — and covered."""
    assert baseline.route("notes")["requests"] == 2, "the client and the live server"
    assert baseline.proxy("notes")["missing"] == []
    assert baseline.proxy("notes")["unused"] == []
    assert baseline.proxy("notes")["observed"] == 2
    assert baseline.route("notes")["needs_transaction_control"] is False


def test_a_missing_grant_names_the_statement_the_test_and_the_path(baseline: Outcome) -> None:
    """The DELETE is executed and not granted: the report says exactly where."""
    delete = 'DELETE FROM "notes_note" WHERE "notes_note"."id" = $1'
    assert baseline.missing_sql("note-delete") == [delete]
    entry = baseline.proxy("note-delete")["missing"][0]
    assert entry["count"] == 1
    assert entry["wire_sql"] == delete, "the wire text, placeholders included"
    assert entry["tests"] == ["tests/test_app.py::test_delete_note"]
    assert entry["paths"] == ["/notes/4/delete/"]
    assert baseline.report["totals"]["missing"] == 3, "the DELETE and the two savepoints"


def test_a_grant_nothing_matched_is_unused(baseline: Outcome) -> None:
    """A stale grant is reported, and does not fail a run on its own."""
    stale = 'SELECT "notes_note"."id" FROM "notes_note" ORDER BY "notes_note"."id" DESC LIMIT 1'
    assert baseline.proxy("note-delete")["unused"] == [stale]
    assert baseline.proxy("note-create")["unused"] == [
        'SELECT "notes_note"."body" FROM "notes_note" WHERE "notes_note"."id" = $1'
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
        assert sql.startswith(("SAVEPOINT ", "RELEASE SAVEPOINT ")), sql
    assert "UPDATE" not in " ".join(missing), "the UPDATE it sends is granted"
    assert baseline.report["totals"]["needs_transaction_control"] == ["note-atomic"]


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
    assert baseline.report["totals"]["routes_exercised"] == 4


def test_sql_outside_a_request_is_ignored(baseline: Outcome) -> None:
    """The plugin records requests, not statements: pytest-django's own SQL is invisible."""
    assert "4242" not in baseline.report_path.read_text(encoding="utf-8")
    assert baseline.report["totals"]["statements"] == 7, (
        "one per request, plus the two savepoints and the UPDATE"
    )


def test_the_report_counts_what_the_manifest_does_not_cover(baseline: Outcome) -> None:
    """The totals are the summary a human reads first."""
    totals = baseline.report["totals"]
    assert totals["requests"] == 6
    assert totals["routed"] == 5
    assert totals["unrouted"] == 1
    assert totals["missing_distinct"] == 3
    assert totals["clean"] is False
    assert baseline.report["version"] == 1
    assert baseline.report["manifest"] == {
        "routes": 5,
        "proxies": {"db": "postgresql"},
        "warnings": [],
    }
    assert baseline.report["script_name"] is None


def test_the_terminal_summary_is_short_and_actionable(baseline: Outcome) -> None:
    """What the developer sees without opening the JSON."""
    assert "blastit: route note-delete: 1 statement(s) missing" in baseline.stdout
    assert "blastit: route note-atomic: needs transaction_control = true" in baseline.stdout
    assert "blastit: 1 unrouted request(s): GET /oops/" in baseline.stdout
    assert "blastit-report.json" in baseline.stdout


def test_strict_fails_the_session(strict: Outcome) -> None:
    """`--blastit-strict` is the exit code a CI job can watch."""
    assert strict.returncode == 1, strict.stdout + strict.stderr
    assert "blastit: --blastit-strict: 3 statement execution(s) missing" in strict.stdout
    assert strict.report["totals"]["clean"] is False


def test_the_report_is_enough_to_fill_the_manifest_in(
    baseline: Outcome, project: Path, tmp_path: Path
) -> None:
    """The whole point: what the report says, applied to the manifest, is clean.

    Applying the report happens twice, which is the honest shape of the loop: the
    first pass adds the statements of the routes that ran, and gives the
    unrouted path a route of its own — and only then do that route's own
    statements show up as missing, because until it existed nothing could
    attribute them to it.
    """
    assert baseline.report["totals"]["clean"] is False
    copy = copy_project(project, tmp_path)
    source = (copy / "blastit.toml").read_text(encoding="utf-8")

    repaired = repaired_manifest(source, baseline)
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
    assert second.missing_sql("fallback") == ["SELECT 1"], "the new route's own SQL"
    assert second.report["totals"]["missing"] == 1

    (copy / "blastit-repaired.toml").write_text(
        repaired_manifest(repaired, second), encoding="utf-8"
    )
    third = run(
        copy,
        "--blastit-manifest",
        "blastit-repaired.toml",
        "--blastit-strict",
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
        'WHERE "notes_note"."owner_id" = $1'
    ]


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
    assert "7 passed" in outcome.stdout
    totals = outcome.report["totals"]
    assert totals["requests"] == 6, "every request, from both workers"
    assert totals["statements"] == 7
    assert totals["routes_exercised"] == 4
    assert totals["missing"] == 3
    assert not list(copy.glob("xdist-report.json.gw*")), "the worker files are cleaned up"
    assert "blastit: route note-delete: 1 statement(s) missing" in outcome.stdout


def test_xdist_reports_are_not_merged_twice(project: Path, tmp_path: Path) -> None:
    """A second run in the same directory does not see the first run's requests."""
    copy = copy_project(project, tmp_path)
    first = run(copy, "--blastit-manifest", "blastit.toml", "-n", "2", report="twice.json")
    second = run(copy, "--blastit-manifest", "blastit.toml", "-n", "2", report="twice.json")
    assert first.returncode == second.returncode == 0
    assert first.report["totals"]["requests"] == second.report["totals"]["requests"] == 6
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
    for entry in outcome.report["unrouted"]:
        document.setdefault("route", []).append(
            {"name": "fallback", "path": [entry["path_info"]], "db": {"queries": []}}
        )
    return dumps_manifest(document)


def dumps_manifest(document: dict[str, Any]) -> str:
    """Writes the small subset of `blastit.toml` the tiny project uses.

    `version`, `[proxy.*]` with an inline `input`, and `[[route]]` with an inline
    `[route.<proxy>]` grant — so a test can apply a report to a manifest the way
    a human would, without a TOML writer in the plugin's own dependencies.
    """
    lines = [f"version = {document['version']}"]
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
        "queries = []\n"
    )
