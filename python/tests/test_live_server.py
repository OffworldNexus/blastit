"""The live server, served by blastit itself (D3c).

A fixture that starts a process can only be tested by starting one: every test
here runs a real `pytest` session in `live_project/`, whose tests make HTTP
requests to `blastit_live_server` (or to `live_server`, with
`--blastit-live-server`), and then reads the report that session wrote.

They need a real PostgreSQL (`DATABASE_URL` or `BLASTIT_TEST_PG_URL`); without
one they skip, with the reason on screen. Privilege is different since R39: the
fixture never skips for lack of root — it starts `blastit dev`, the unprivileged
single-UID simulation, when there is no root and no passwordless `sudo` (the
production/production-sudo decision and its twins live in the protected
`python/tests/critical/test_check_contract.py`).
"""

import functools
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest
from blastit import pytest_plugin as plugin

HERE = Path(__file__).parent
PROJECT = HERE / "live_project"
#: The same thing in the layout `django-admin startproject myshop .` writes:
#: packaged settings, a dotted `WSGI_APPLICATION`, and a project that is not
#: installed.
PACKAGED = HERE / "packaged_project"

DATABASE_URL = os.environ.get("DATABASE_URL") or os.environ.get("BLASTIT_TEST_PG_URL")

requires_database = pytest.mark.skipif(
    not DATABASE_URL,
    reason="needs a real PostgreSQL: set DATABASE_URL or BLASTIT_TEST_PG_URL",
)


@pytest.fixture
def privileges() -> None:
    """Settles how the inner server starts: production, or unprivileged `dev` (R39).

    No skip: a machine with neither root nor passwordless sudo serves the project
    through `blastit dev`, the single-UID simulation, so the tests below still
    exercise a real five-role process tree. What they cannot exercise without
    root is the OS isolation itself, which the Rust critical suite covers.
    """
    plugin._live_server_mode()


class Outcome:
    """One inner pytest session: its exit code, its output and its report."""

    def __init__(
        self, completed: subprocess.CompletedProcess[str], report_path: Path
    ) -> None:
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


@pytest.fixture(scope="session")
def packaged_project(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A pristine copy of the project `startproject` would have written."""
    target = tmp_path_factory.mktemp("blastit-packaged") / "packaged_project"
    shutil.copytree(PACKAGED, target, ignore=shutil.ignore_patterns("__pycache__"))
    return target


def run(
    project: Path,
    *args: str,
    report: str = "blastit-live-report.json",
    tmp_path: Path | None = None,
    env: dict[str, str] | None = None,
) -> Outcome:
    """Runs pytest in a copy of the live project, and returns what it did."""
    target = project
    report_path = project / report
    staged: Path | None = None
    if tmp_path is not None:
        if plugin._privileges() is None:
            # No root and no sudo: the fixture serves the project through
            # `blastit dev`, which never authorises the manifest directory, so a
            # copy in the test user's own temporary directory is enough.
            target = tmp_path / "live_project"
            shutil.copytree(
                project, target, ignore=shutil.ignore_patterns("__pycache__")
            )
            report_path = target / report
        else:
            target, report_path = _stage_for_production(project, tmp_path, report)
            staged = target
    environment = {
        **os.environ,
        "DATABASE_URL": str(DATABASE_URL),
        "PYTHONDONTWRITEBYTECODE": "1",
        **(env or {}),
    }
    try:
        completed = subprocess.run(
            [
                sys.executable,
                "-m",
                "pytest",
                *args,
                "--blastit-report",
                str(report_path),
            ],
            cwd=target,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        if staged is not None:
            _remove_stage(staged)
    return Outcome(completed, report_path)


#: The roots a staged copy may live under, in the order they are tried.
#:
#: `blastit serve` authorises the manifest directory before it binds a listener
#: (`host::Provenance`), and `/tmp` is `1777`: a path under it is refused, because
#: a uid the views do not run as could replace the code they serve. Each of these
#: sits on the root filesystem and is root-owned `0755` on an ordinary host — the
#: layout a container image gives `/app`.
_PROTECTED_CANDIDATES = (Path("/opt"), Path("/usr/local"), Path("/srv"))

#: The write bits another uid would need (`S_IWGRP | S_IWOTH`): the test
#: `host::Facts::directory_writable_by_others` applies to every ancestor.
_GROUP_OR_OTHER_WRITE = 0o022


@functools.cache
def _protected_root() -> Path:
    """The first candidate a production boot will authorise, or a refusal naming each.

    The whole path decides, not only the leaf: `host::Provenance` refuses a
    manifest directory below a directory another uid could write, so a `0755`
    `/opt` under a `0777` `/` protects nothing. Resolved once — the walk is the
    same for every test — and a host with none is reported with every rejected
    candidate and its reason: that is an operator's `chmod`, and the live suite
    must never fall back to `blastit dev` and claim production ran.
    """
    rejected: list[str] = []
    for candidate in _PROTECTED_CANDIDATES:
        reason = _unprotected_reason(candidate)
        if reason is None:
            return candidate
        rejected.append(f"{candidate} ({reason})")
    pytest.fail(
        "blastit: a production live server needs a staging directory no other uid "
        "can write, and none of "
        + ", ".join(rejected)
        + " qualifies — fix the first with `sudo chmod go-w <it>`, and the same on "
        "every directory above it (a root-owned 0755 /opt is what a container "
        "gives /app)",
        pytrace=False,
    )


def _unprotected_reason(candidate: Path) -> str | None:
    """Why `candidate` cannot hold a staged copy, or `None` when it can."""
    if not candidate.is_dir():
        return "does not exist"
    directory: Path | None = candidate
    while directory is not None:
        try:
            mode = os.stat(directory).st_mode
        except OSError as error:
            return f"{directory} cannot be inspected ({error})"
        if mode & _GROUP_OR_OTHER_WRITE:
            return f"{directory} is group- or other-writable (mode {mode & 0o7777:o})"
        directory = directory.parent if directory != directory.parent else None
    return None


def _privileged(prefix: list[str], *args: str) -> None:
    """Runs `args` as root, directly or through the prefix `_privileges` chose."""
    completed = subprocess.run(
        [*prefix, *args], capture_output=True, text=True, check=False
    )
    assert completed.returncode == 0, (
        f"{' '.join([*prefix, *args])} failed ({completed.returncode}):\n"
        f"{completed.stdout}{completed.stderr}"
    )


def _stage_for_production(
    project: Path, tmp_path: Path, report: str
) -> tuple[Path, Path]:
    """A copy of `project` production `blastit` will authorise, and its report path.

    The copy is made under [`_protected_root`] and handed to root with no
    group/other write bits once it is filled, so the manifest directory passes
    `host::Provenance` the way `/app` does in a container; the report stays in
    `tmp_path`, which the test user owns and can write (the project copy itself is
    readable but not writable by it — that is what being root's means).
    """
    prefix = plugin._privileges() or []
    directory = _protected_root() / f"blastit-live-{os.getpid()}-{tmp_path.name}"
    target = directory / project.name
    try:
        _privileged(prefix, "mkdir", "-p", str(directory))
        _privileged(prefix, "chown", f"{os.getuid()}:{os.getgid()}", str(directory))
        shutil.copytree(project, target, ignore=shutil.ignore_patterns("__pycache__"))
        _privileged(prefix, "chown", "-R", "root:root", str(directory))
        _privileged(prefix, "chmod", "-R", "go-w,a+rX", str(directory))
    except BaseException:
        # A half-chowned copy is root's from the first `chown` on, so nothing
        # else may clean it up; the test's own failure is the one to report.
        _remove_stage(target)
        raise
    return target, tmp_path / report


def _remove_stage(target: Path) -> None:
    """Removes a staged copy, without ever replacing the session's own result.

    Only a directory this module created — `blastit-live-*` directly under the
    resolved protected root — is ever removed, and a failure to remove it is
    printed rather than raised: a cleanup path must not turn a test's real
    outcome into a fixture error.
    """
    directory = target.parent
    try:
        root = _protected_root()
    except BaseException:  # noqa: BLE001 - cleanup never reports that refusal instead
        return
    if directory.parent != root or not directory.name.startswith("blastit-live-"):
        return
    prefix = plugin._privileges() or []
    completed = subprocess.run(
        [*prefix, "rm", "-rf", str(directory)],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        print(
            f"blastit: could not remove the staged copy {directory} "
            f"({completed.returncode}): {completed.stderr.strip()}"
        )


@requires_database
def test_a_packaged_settings_module_is_served(
    privileges: None, packaged_project: Path, tmp_path: Path
) -> None:
    """`django-admin startproject myshop .`, served through the live server (D3c).

    Three things about that layout used to stop a live server from starting, and
    this is the one test that walks over all three at once:

    * `WSGI_APPLICATION = "myshop.wsgi.application"` is a *dotted* path, and
      `BLASTIT_APP` takes `module:attribute`;
    * `myshop` is importable only from the directory that holds the package, so
      the server has to run in the project root rather than next to the settings
      module;
    * the project is not installed, and nothing here sets `PYTHONPATH`: the
      working directory blastit was started in is what `sys.path` holds (§5.3).

    The manifest is passed the way a person types it — a relative path — which
    only works if the plugin resolves it where it was invoked.
    """
    outcome = run(
        packaged_project, "--blastit-manifest", "Blastfile", tmp_path=tmp_path
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "1 passed" in outcome.stdout, outcome.stdout
    assert outcome.report["totals"]["requests"] == 1
    assert outcome.missing_sql("packaged") == [], (
        "the manifest grants what the view ran"
    )


@requires_database
def test_a_live_server_is_served_by_blastit(
    privileges: None, project: Path, tmp_path: Path
) -> None:
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
        "Blastfile",
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
    denied = outcome.missing_sql("live-denied")
    assert "SELECT …" in denied, (
        "the statement blastit refused, as the log normalised it and R24 "
        "redacted its literal"
    )
    assert "SELECT 'denied'" not in json.dumps(report), (
        "R24 redacts the literal, so the raw text never reaches the report"
    )
    assert outcome.missing_sql("live-ok") == [], "the manifest covers the granted route"
    assert report["totals"]["missing"] == 2, (
        "`SELECT 'denied'` (redacted to `SELECT …`) and the health check Django "
        "runs after an error, both denied on that route"
    )
    assert [entry["path_info"] for entry in report["unrouted"]] == ["/live/oops/"]
    assert report["totals"]["unrouted"] == 1
    assert report["totals"]["unused"] == 0, (
        "nothing but the log saw these routes, so no grant is called unused"
    )
    assert report["totals"]["unused_global"] == 0
    assert report["totals"]["clean"] is False, "a grant is missing"
    assert not list(outcome.report_path.parent.glob("blastit-live.env")), (
        "the environment file, which holds the test database's URL, is removed"
    )


@requires_database
def test_a_projects_own_live_server_wins(
    privileges: None, project: Path, tmp_path: Path
) -> None:
    """A `conftest.py` fixture is collected after the plugin, so it wins (D3c).

    The live project defines its own `live_server` when the environment asks for
    it (its `conftest.py` says why); `--blastit-live-server` must not take that
    away from it, and the report proves it: nothing went through blastit at all.
    """
    outcome = run(
        project,
        "--blastit-manifest",
        "Blastfile",
        "--blastit-live-server",
        "-k",
        "own_live_server",
        report="own-report.json",
        env={"BLASTIT_TEST_OWN_LIVE_SERVER": "1"},
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "1 passed" in outcome.stdout, outcome.stdout
    assert outcome.report["totals"]["requests"] == 0, (
        "the project's own fixture served it, not blastit"
    )


@requires_database
def test_the_live_server_option_serves_a_live_server_suite(
    privileges: None, project: Path, tmp_path: Path
) -> None:
    """`--blastit-live-server`: `live_server` is blastit's, and the report says so.

    The inner test only passes when `live_server` really is blastit's — it
    asserts that the ungranted statement is refused, which pytest-django's own
    `live_server` would serve happily.
    """
    outcome = run(
        project,
        "--blastit-manifest",
        "Blastfile",
        "--blastit-live-server",
        "-k",
        "override",
        tmp_path=tmp_path,
    )
    assert outcome.returncode == 0, outcome.stdout + outcome.stderr
    assert "1 passed" in outcome.stdout, outcome.stdout
    assert "SELECT …" in outcome.missing_sql("live-denied"), (
        "the statement only blastit could have refused is in the report, "
        "redacted by R24"
    )
    assert "SELECT 'denied'" not in json.dumps(outcome.report), (
        "R24 redacts the literal, so the raw text never reaches the report"
    )
    assert outcome.report["totals"]["requests"] == 1


@requires_database
def test_the_option_without_a_manifest_is_a_usage_error(project: Path) -> None:
    """Nothing to serve with: the plugin says so before any test runs."""
    outcome = run(project, "--blastit-live-server")
    output = (outcome.stdout + outcome.stderr).replace("\n", " ")
    assert outcome.returncode == 4
    assert "--blastit-live-server" in output and "need --blastit-manifest" in output, (
        output
    )
