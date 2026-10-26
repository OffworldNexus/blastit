"""The live server, served by blastit itself (D3c).

A fixture that starts a process can only be tested by starting one: every test
here runs a real `pytest` session in `live_project/`, whose tests make HTTP
requests to `blastit_live_server` (or to `live_server`, with
`--blastit-live-server`), and then reads the report that session wrote.

They need a real PostgreSQL (`DATABASE_URL` or `BLASTIT_TEST_PG_URL`) *and*
root, or passwordless sudo: blastit is a sandbox — listeners, view uids, private
directories — and the fixture skips, with the reason, when a machine has
neither. Without them the tests are skipped, not failed: what they test is the
manifest, and it cannot be tested without blastit running at all.
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

HERE = Path(__file__).parent
PROJECT = HERE / "live_project"

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

    @property
    def report(self) -> dict[str, Any]:
        """The JSON report the session wrote, parsed."""
        assert self.report_path.is_file(), (
            f"the session wrote no report at {self.report_path}:\n"
            f"stdout:\n{self.stdout}\nstderr:\n{self.stderr}"
        )
        return json.loads(self.report_path.read_text(encoding="utf-8"))

    def route(self, name: str) -> dict[str, Any]:
        """One route of the report, by name."""
        for route in self.report["routes"]:
            if route["name"] == name:
                return route
        raise AssertionError(f"no route {name!r} in the report")

    def missing_sql(self, name: str, proxy: str = "db") -> list[str]:
        """The normalised SQL every `missing` entry of a route names."""
        for block in self.route(name)["proxies"]:
            if block["name"] == proxy:
                return [entry["sql"] for entry in block["missing"]]
        raise AssertionError(f"route {name!r} has no proxy {proxy!r} block")


@pytest.fixture(scope="session")
def project(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A pristine copy of the live project, so a session never dirties it."""
    target = tmp_path_factory.mktemp("blastit-live") / "live_project"
    shutil.copytree(PROJECT, target, ignore=shutil.ignore_patterns("__pycache__"))
    return target


def run(
    project: Path,
    *args: str,
    report: str = "blastit-live-report.json",
    tmp_path: Path | None = None,
) -> Outcome:
    """Runs pytest in a copy of the live project, and returns what it did."""
    target = project
    if tmp_path is not None:
        target = tmp_path / "live_project"
        shutil.copytree(project, target, ignore=shutil.ignore_patterns("__pycache__"))
    environment = {
        **os.environ,
        "DATABASE_URL": str(DATABASE_URL),
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    completed = subprocess.run(  # noqa: S603 - sys.executable, fixed arguments
        [sys.executable, "-m", "pytest", *args, "--blastit-report", report],
        cwd=target,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    return Outcome(completed, target / report)


def test_a_live_server_is_served_by_blastit(project: Path, tmp_path: Path) -> None:
    """The three requests the tests make are in the report, straight from the log.

    The plugin sees nothing of them itself — they are made to another process —
    so what is in the report is what the live server logged (§8.1): the routing
    decisions as `request` events, and the statement the manifest does not grant
    as a `sql.denied` one. The health probe the fixture itself makes is not
    traffic, and does not appear as an unrouted request.
    """
    outcome = run(
        project,
        "--blastit-manifest",
        "blastit.toml",
        "-k",
        "not override",
        tmp_path=tmp_path,
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "3 passed" in outcome.stdout, outcome.stdout
    report = outcome.report
    assert report["totals"]["requests"] == 3, "one per test"
    assert outcome.route("live-ok")["requests"] == 1
    assert outcome.route("live-denied")["requests"] == 1
    assert "SELECT 'denied'" in outcome.missing_sql("live-denied"), (
        "the statement blastit refused, as the log normalised it"
    )
    assert outcome.missing_sql("live-ok") == [], "the manifest covers the granted route"
    assert report["totals"]["missing"] == 2, (
        "`SELECT 'denied'` and the health check Django runs after an error, both "
        "denied on that route"
    )
    assert [entry["path_info"] for entry in report["unrouted"]] == ["/live/oops/"]
    assert report["totals"]["unrouted"] == 1
    assert report["totals"]["unused"] == 0, (
        "nothing but the log saw these routes, so no grant is called unused"
    )
    assert report["totals"]["unused_global"] == 0
    assert report["totals"]["clean"] is False, "a grant is missing"


def test_the_live_server_option_serves_a_live_server_suite(
    project: Path, tmp_path: Path
) -> None:
    """`--blastit-live-server`: `live_server` is blastit's, and the report says so.

    The inner test only passes when `live_server` really is blastit's — it
    asserts that the ungranted statement is refused, which pytest-django's own
    `live_server` would serve happily.
    """
    outcome = run(
        project,
        "--blastit-manifest",
        "blastit.toml",
        "--blastit-live-server",
        "-k",
        "override",
        tmp_path=tmp_path,
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "1 passed" in outcome.stdout, outcome.stdout
    assert "SELECT 'denied'" in outcome.missing_sql("live-denied"), (
        "the statement only blastit could have refused is in the report"
    )
    assert outcome.report["totals"]["requests"] == 1


def test_the_option_without_a_manifest_is_a_usage_error(project: Path) -> None:
    """Nothing to serve with: the plugin says so before any test runs."""
    outcome = run(project, "--blastit-live-server")
    output = (outcome.stdout + outcome.stderr).replace("\n", " ")
    assert outcome.returncode == 4
    assert "--blastit-live-server" in output and "need --blastit-manifest" in output, output
