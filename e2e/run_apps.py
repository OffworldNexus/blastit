"""The application and runtime end-to-end suite.

    uv run --no-project e2e/run_apps.py            # every leg, free ports 28080+
    uv run --no-project e2e/run_apps.py --legs wsgi  # only the WSGI family

Real frameworks, a real PostgreSQL 18, a real PHP front controller, and a bare
Debian container that has neither Python nor PHP. Every leg asserts the user
outcome *and* the boundary that makes it safe: per-view SQL grants, ungranted
columns and statements refused, the database password absent from every worker
environment and response, `exit()`/fatal recovery, `php2shell` neutrality, and
a static mount that is a boundary rather than a prefix.

The fixtures live in `e2e/apps/`: `php/` (front controller, per-view grants,
static assets), `python/django/` and `python/flask/` (WSGI views, shared
`connection` traits, static mounts), and `static/` (a site with a fallback and
one without). `e2e/apps/postgres/schema.sql` is the one schema every leg reads.

The binary is the artifact under test: `--binary`, `$E2E_BLASTIT`,
`target/release/blastit`, `target/debug/blastit` or `PATH` — never a rebuilt
copy inside an image. PHP and the static site run in Debian trixie containers
(the distribution's NTS `libphp`, and no interpreter at all respectively); the
Django and Flask legs run on this host's CPython 3.13 and 3.14.

A host fact the suite needs and does not have (Docker, `libphp8.4`, a CPython
minor, the shims) is a loud SKIP naming the command that installs it, never a
silent pass. Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Self
from urllib.parse import urlencode

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
APPS = HERE / "apps"
sys.path.insert(0, str(HERE))
from universal_binary import LegFailure, Skip, http

PORT_BASE = 28080
WORK_ROOT = Path(os.environ.get("TMPDIR", "/tmp/opencode")) / "blastit-e2e-apps"
PASSWORD_ERROR = "the database password reached a view or a response"


# ---------------------------------------------------------------------------
# processes, ports and one summary
# ---------------------------------------------------------------------------


class Ports:
    """Free loopback ports from 28080 upwards (each one is bound before use)."""

    def __init__(self) -> None:
        self.next = PORT_BASE

    def take(self) -> int:
        for port in range(self.next, PORT_BASE + 200):
            self.next = port + 1
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
                try:
                    sock.bind(("127.0.0.1", port))
                except OSError:
                    continue
                return port
        raise LegFailure(f"no free port between {PORT_BASE} and {PORT_BASE + 200}")


PORTS = Ports()


def clean_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """The environment a leg runs under: no inherited venv or `BLASTIT_*`."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "TMPDIR": str(WORK_ROOT),
        "BLASTIT_MEM_HIGH_WATERMARK": "100",
        "BLASTIT_MAX_VIEW_PROCESSES": "8",
    }
    if os.environ.get("LD_LIBRARY_PATH"):
        env["LD_LIBRARY_PATH"] = os.environ["LD_LIBRARY_PATH"]
    env.update(extra or {})
    return env


def run(
    argv: list[str],
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: float = 600,
) -> subprocess.CompletedProcess[str]:
    """Runs one command, never raising on its exit status."""
    return subprocess.run(
        argv,
        cwd=str(cwd) if cwd else None,
        env=clean_env(env),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def docker(*args: str, timeout: float = 600) -> subprocess.CompletedProcess[str]:
    """One `docker` command, with the host's own environment (daemon sockets)."""
    return subprocess.run(
        ["docker", *args],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def tail(text: str, lines: int = 20) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])


class Checks:
    """The assertions of one leg; a run names every promise it broke."""

    def __init__(self) -> None:
        self.failures: list[str] = []

    def that(self, condition: bool, promise: str) -> None:
        if not condition:
            self.failures.append(promise)

    def done(self, leg: str) -> str:
        if self.failures:
            raise LegFailure(
                f"{leg}: {len(self.failures)} promise(s) failed:\n  "
                + "\n  ".join(self.failures)
            )
        return leg


class Dev:
    """One `blastit dev` on the host, its transcript, and a stop on exit."""

    def __init__(self, binary: Path, project: Path, port: int, env: dict[str, str]):
        self.binary = binary
        self.project = project
        self.port = port
        self.env = env
        self.process: subprocess.Popen[str] | None = None
        self.out: list[str] = []
        self.err: list[str] = []
        self._ready = threading.Event()
        self._pumps: list[threading.Thread] = []

    def _pump(self, stream, sink: list[str]) -> None:
        for line in stream:
            sink.append(line)
            # The JSON log (`BLASTIT_LOG_FORMAT=json`) spells the same line as
            # an event; both are the boundary the next request can use.
            if "INFO  ready" in line or '"event":"ready"' in line:
                self._ready.set()

    def __enter__(self) -> Self:
        self.process = subprocess.Popen(
            [str(self.binary), "dev", "--port", str(self.port), "--no-reload"],
            cwd=str(self.project),
            env=clean_env(self.env),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._pumps = [
            threading.Thread(
                target=self._pump, args=(self.process.stdout, self.out), daemon=True
            ),
            threading.Thread(
                target=self._pump, args=(self.process.stderr, self.err), daemon=True
            ),
        ]
        for pump in self._pumps:
            pump.start()
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if self._ready.wait(0.1):
                return self
            if self.process.poll() is not None:
                raise LegFailure(
                    f"`blastit dev` in {self.project.name} exited "
                    f"{self.process.returncode} before ready:\n{self.transcript()}"
                )
        raise LegFailure(
            f"`blastit dev` in {self.project.name} never printed `INFO  ready`:\n"
            f"{self.transcript()}"
        )

    def transcript(self, lines: int = 30) -> str:
        return (
            "stdout:\n"
            + "".join(self.out[-lines:])
            + "stderr:\n"
            + "".join(self.err[-lines:])
        )

    def log(self) -> str:
        return "".join(self.out) + "".join(self.err)

    def __exit__(self, *_exc) -> None:
        if self.process is None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        for pump in self._pumps:
            pump.join(timeout=5)


class Summary:
    def __init__(self) -> None:
        self.rows: list[tuple[str, str, str]] = []

    def add(self, leg: str, state: str, detail: str) -> None:
        self.rows.append((leg, state, detail))
        print(f"{state:4}  {leg:24}  {detail}", flush=True)

    def report(self) -> int:
        passed = sum(1 for _, state, _ in self.rows if state == "PASS")
        skipped = sum(1 for _, state, _ in self.rows if state == "SKIP")
        failed = sum(1 for _, state, _ in self.rows if state == "FAIL")
        print(
            f"\n{passed} passed, {failed} failed, {skipped} skipped "
            "(a SKIP names the host fact it needs)",
            flush=True,
        )
        return 1 if failed else 0


# ---------------------------------------------------------------------------
# host facts and one PostgreSQL
# ---------------------------------------------------------------------------


def require_docker() -> None:
    """A reachable Docker daemon, or a loud SKIP naming how to install it."""
    if shutil.which("docker") is None:
        raise Skip(
            "Docker is not installed; `sudo apt install docker.io` "
            "(or `sudo dnf install docker-ce`)"
        )
    version = docker("version", "--format", "{{.Server.Version}}", timeout=30)
    if version.returncode != 0:
        raise Skip(
            "the Docker daemon is not reachable; "
            "`sudo systemctl enable --now docker` "
            f"({tail(version.stderr, 3)})"
        )


def python_shims(binary: Path) -> Path:
    """The CPython shim directory a WSGI `dev` run can load from."""
    roots = [
        binary.parent,
        ROOT / "target" / "wheel-blobs",
        ROOT / "target" / "py-sapi",
    ]
    found = next(
        (root for root in roots if list(root.glob("libblastit_py31*-*.so"))), None
    )
    if found is None:
        raise Skip(
            "no libblastit_py31*-*.so shim; run `make python-runtime` "
            "(needs CPython 3.13 and 3.14 on the host)"
        )
    return found


class Postgres:
    """One `postgres:18` container, its schema, and its password."""

    def __init__(self, network: str, name: str, port: int):
        self.network = network
        self.name = name
        self.port = port
        self.password = "e2e-" + secrets.token_hex(16)

    def url(self, host: str, port: int | str) -> str:
        return f"postgresql://apps:{self.password}@{host}:{port}/apps?sslmode=disable"

    def start(self) -> None:
        result = docker(
            "run",
            "-d",
            "--name",
            self.name,
            "--network",
            self.network,
            "--network-alias",
            "pg",
            "-e",
            "POSTGRES_USER=apps",
            "-e",
            f"POSTGRES_PASSWORD={self.password}",
            "-e",
            "POSTGRES_DB=apps",
            "-p",
            f"127.0.0.1:{self.port}:5432",
            "postgres:18",
        )
        if result.returncode != 0:
            raise LegFailure(f"`docker run postgres:18` failed: {tail(result.stderr)}")
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            ready = docker(
                "exec", self.name, "pg_isready", "-U", "apps", "-d", "apps", timeout=20
            )
            if ready.returncode == 0:
                return
            time.sleep(0.5)
        raise LegFailure(f"{self.name} never became ready:\n{tail(self.logs())}")

    def apply(self, sql: Path) -> None:
        with sql.open("rb") as handle:
            result = subprocess.run(
                [
                    "docker",
                    "exec",
                    "-i",
                    self.name,
                    "psql",
                    "-q",
                    "-v",
                    "ON_ERROR_STOP=1",
                    "-U",
                    "apps",
                    "-d",
                    "apps",
                ],
                stdin=handle,
                capture_output=True,
                text=True,
                timeout=120,
                check=False,
            )
        if result.returncode != 0:
            raise LegFailure(f"the schema did not apply: {tail(result.stderr)}")

    def logs(self) -> str:
        return docker("logs", "--tail", "60", self.name, timeout=30).stdout


# ---------------------------------------------------------------------------
# the promises shared by every app leg
# ---------------------------------------------------------------------------


def assert_products(checks: Checks, port: int, names: list[str]) -> None:
    status, headers, body = http(port, "/products")
    checks.that(status == 200, f"GET /products is 200, not {status}: {body[:200]!r}")
    checks.that(
        headers.get("content-type", "").startswith("application/json"),
        f"GET /products is not JSON ({headers.get('content-type')!r})",
    )
    try:
        document = json.loads(body)
    except ValueError:
        checks.that(False, f"GET /products answered no JSON: {body[:200]!r}")
        return
    if not isinstance(document, dict):
        checks.that(False, f"GET /products answered {document!r}, not an object")
        return
    got = [row.get("name") for row in document.get("products", [])]
    checks.that(got == names, f"GET /products is {got!r}, not {names!r}")


def assert_orders(checks: Checks, port: int, total: str) -> None:
    body = urlencode({"customer_id": "1", "total": total}).encode()
    status, _headers, response = http(
        port,
        "/orders",
        "POST",
        body,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    checks.that(status == 201, f"POST /orders is 201, not {status}: {response[:200]!r}")
    try:
        document = json.loads(response)
    except ValueError:
        checks.that(False, f"POST /orders answered no JSON: {response[:200]!r}")
        return
    if not isinstance(document, dict):
        checks.that(False, f"POST /orders answered {document!r}, not an object")
        return
    checks.that(
        document.get("inserted") is True, f"POST /orders did not insert: {document}"
    )
    totals = [row.get("total") for row in document.get("orders", [])]
    checks.that(total in totals, f"POST /orders read back {totals!r}, not {total!r}")


def assert_env(
    checks: Checks, port: int, password: str, minor: str | None = None
) -> None:
    status, _headers, body = http(port, "/env")
    checks.that(status == 200, f"GET /env is 200, not {status}")
    checks.that(password.encode() not in body, f"GET /env: {PASSWORD_ERROR}")
    try:
        document = json.loads(body)
    except ValueError:
        checks.that(False, f"GET /env answered no JSON: {body[:200]!r}")
        return
    if not isinstance(document, dict):
        checks.that(False, f"GET /env answered {document!r}, not an object")
        return
    url = document.get("database_url") or ""
    checks.that("host=" in url, f"the app was not handed a socket URL: {url!r}")
    checks.that(
        document.get("url_has_password") is False, "the rendered URL carries a password"
    )
    checks.that(
        document.get("db_password_in_env") is False,
        "DB_PASSWORD reached the worker environment",
    )
    if minor is not None:
        checks.that(
            document.get("python") == minor,
            f"CPython {minor} was expected, the body says {document.get('python')!r}",
        )


def assert_denied(
    checks: Checks, port: int, target: str, password: str, method: str = "GET"
) -> None:
    """A refused statement: `403`, SQLSTATE `42501`, and no credential in the body."""
    body = b""
    headers = None
    if method == "POST":
        body = urlencode({"customer_id": "1", "total": "1.00"}).encode()
        headers = {"Content-Type": "application/x-www-form-urlencoded"}
    try:
        status, _headers, response = http(port, target, method, body, headers=headers)
    except (OSError, LegFailure) as error:
        checks.that(False, f"{method} {target} never answered ({error})")
        return
    checks.that(
        status == 403,
        f"{method} {target} is {status}, not the documented 403: {response[:200]!r}",
    )
    checks.that(
        password.encode() not in response, f"{method} {target}: {PASSWORD_ERROR}"
    )
    try:
        document = json.loads(response)
    except ValueError:
        checks.that(False, f"{method} {target} answered no JSON: {response[:200]!r}")
        return
    checks.that(
        isinstance(document, dict) and document.get("sqlstate") == "42501",
        f"{method} {target} did not report SQLSTATE 42501: {response[:200]!r}",
    )


def assert_static_mount(checks: Checks, port: int, asset: str) -> None:
    status, headers, _body = http(port, asset)
    checks.that(status == 200, f"GET {asset} is 200 from the mount, not {status}")
    checks.that(
        "immutable" in headers.get("cache-control", ""),
        f"GET {asset} is not immutable ({headers.get('cache-control')!r})",
    )


# ---------------------------------------------------------------------------
# the legs
# ---------------------------------------------------------------------------


def leg_django(
    binary: Path, workspace: Path, venv: Path, minor: str, pg: Postgres
) -> str:
    project = workspace / f"django-{minor}"
    shutil.copytree(APPS / "python" / "django", project)
    (project / ".venv").symlink_to(venv, target_is_directory=True)
    database_url = pg.url("127.0.0.1", pg.port)
    collected = run(
        [str(venv / "bin" / "python"), "manage.py", "collectstatic", "--no-input"],
        cwd=project,
        env={"DATABASE_URL": database_url},
    )
    port = PORTS.take()
    checks = Checks()
    checks.that(
        collected.returncode == 0,
        "`manage.py collectstatic --no-input` failed: "
        f"{tail(collected.stdout + collected.stderr, 10)}",
    )
    with Dev(
        binary,
        project,
        port,
        {
            "DATABASE_URL": database_url,
            "BLASTIT_PY_SHIM_DIR": str(python_shims(binary)),
            "BLASTIT_LOG_FORMAT": "json",
        },
    ) as server:
        assert_products(checks, port, ["alpha", "beta", "gamma"])
        status, _headers, body = http(port, "/products/2")
        checks.that(status == 200, f"GET /products/2 is 200, not {status}")
        checks.that(b'"beta"' in body, f"GET /products/2 is not beta: {body[:200]!r}")
        assert_orders(checks, port, "42.50")
        assert_env(checks, port, pg.password, minor)
        assert_static_mount(checks, port, "/static/app.9f2c1d7a.css")
        assert_denied(checks, port, "/customers/peek", pg.password)
        assert_denied(checks, port, "/customers/insert", pg.password)
        assert_denied(checks, port, "/orders/note-insert", pg.password)
        assert_denied(checks, port, "/orders/purge", pg.password, "POST")
        assert_denied(checks, port, "/orders/note", pg.password, "POST")
        checks.that("sql.denied" in server.log(), "no sql.denied event in the log")
    return checks.done(f"django {minor}")


def leg_flask(
    binary: Path, workspace: Path, venv: Path, minor: str, pg: Postgres
) -> str:
    project = workspace / f"flask-{minor}"
    shutil.copytree(APPS / "python" / "flask", project)
    (project / ".venv").symlink_to(venv, target_is_directory=True)
    port = PORTS.take()
    checks = Checks()
    with Dev(
        binary,
        project,
        port,
        {
            "DATABASE_URL": pg.url("127.0.0.1", pg.port),
            "BLASTIT_PY_SHIM_DIR": str(python_shims(binary)),
            "BLASTIT_LOG_FORMAT": "json",
        },
    ) as server:
        assert_products(checks, port, ["alpha", "beta", "gamma"])
        assert_orders(checks, port, "7.25")
        assert_env(checks, port, pg.password, minor)
        assert_static_mount(checks, port, "/static/app.9f2c1d7a.css")
        assert_denied(checks, port, "/customers/peek", pg.password)
        assert_denied(checks, port, "/customers/insert", pg.password)
        assert_denied(checks, port, "/orders/note-insert", pg.password)
        assert_denied(checks, port, "/orders/purge", pg.password, "POST")
        assert_denied(checks, port, "/orders/note", pg.password, "POST")
        checks.that("sql.denied" in server.log(), "no sql.denied event in the log")
    return checks.done(f"flask {minor}")


def php_image() -> str:
    image = f"blastit-apps-php:{os.getpid()}"
    built = docker(
        "build",
        "-q",
        "-f",
        str(APPS / "php" / "Dockerfile"),
        "-t",
        image,
        str(ROOT),
        timeout=1800,
    )
    if built.returncode != 0:
        raise LegFailure(f"the PHP image did not build: {tail(built.stderr, 30)}")
    return image


def start_php(
    binary: Path,
    network: str,
    image: str,
    name: str,
    port: int,
    manifest: str,
    environment: dict[str, str],
) -> None:
    """One PHP container: the binary mounted, the shard's manifest selected."""
    started = docker(
        "run",
        "-d",
        "--name",
        name,
        "--network",
        network,
        "-p",
        f"127.0.0.1:{port}:{port}",
        "-v",
        f"{binary}:/usr/local/bin/blastit:ro",
        "-w",
        "/fixture",
        "-e",
        "BLASTIT_DEV_EXPOSE=1",
        "-e",
        "BLASTIT_MEM_HIGH_WATERMARK=100",
        "-e",
        "BLASTIT_LOG_FORMAT=json",
        *(
            arg
            for key, value in environment.items()
            for arg in ("-e", f"{key}={value}")
        ),
        image,
        "sh",
        "-c",
        "BLASTIT_MANIFEST=/fixture/"
        + manifest
        + f" exec /usr/local/bin/blastit dev --host 0.0.0.0 --port {port} --no-reload",
    )
    if started.returncode != 0:
        raise LegFailure(f"{name} did not start: {tail(started.stderr, 20)}")


def wait_php(
    checks: Checks, name: str, port: int, body_path: str = "/products"
) -> bytes:
    deadline = time.monotonic() + 180
    while True:
        try:
            status, _headers, body = http(port, body_path, timeout=5)
            break
        except (OSError, LegFailure) as error:
            if time.monotonic() > deadline:
                raise LegFailure(
                    f"{name} never answered: {error}\n{tail(docker('logs', name).stdout, 40)}"
                ) from error
            time.sleep(0.25)
    checks.that(status == 200, f"GET {body_path} is 200, not {status}: {body[:200]!r}")
    return body


def assert_split_env(checks: Checks, port: int, password: str) -> None:
    """`/env` from the `$DB_*` spelling: no password, no rendered URL, but a store."""
    status, _headers, body = http(port, "/env")
    checks.that(status == 200, f"GET /env is 200 on the split store, not {status}")
    checks.that(
        password.encode() not in body, f"GET /env (split store): {PASSWORD_ERROR}"
    )
    try:
        document = json.loads(body)
    except ValueError:
        checks.that(False, f"GET /env (split store) answered no JSON: {body[:200]!r}")
        return
    if not isinstance(document, dict):
        checks.that(
            False, f"GET /env (split store) answered {document!r}, not an object"
        )
        return
    checks.that(
        document.get("database_url") is None, "the split store rendered a DATABASE_URL"
    )
    checks.that(
        document.get("db_password_in_env") is False,
        "DB_PASSWORD reached the split-store worker",
    )


def leg_php(binary: Path, network: str, pg: Postgres) -> str:
    image = php_image()
    port = PORTS.take()
    split_port = PORTS.take()
    name = f"blastit-apps-php-{os.getpid()}"
    split_name = f"blastit-apps-php-split-{os.getpid()}"
    checks = Checks()
    try:
        start_php(
            binary,
            network,
            image,
            name,
            port,
            "Blastfile",
            {"DATABASE_URL": pg.url("pg", 5432)},
        )
        wait_php(checks, name, port)
        assert_products(checks, port, ["alpha", "beta", "gamma"])
        status, _headers, body = http(port, "/products/1", timeout=15)
        checks.that(
            status == 200 and b"alpha" in body,
            f"GET /products/1: {status} {body[:120]!r}",
        )
        assert_orders(checks, port, "9.99")
        assert_env(checks, port, pg.password)
        assert_static_mount(checks, port, "/assets/app.4f1c2a9b7d3e.css")
        assert_denied(checks, port, "/customers/peek", pg.password)
        assert_denied(checks, port, "/customers/insert", pg.password)
        assert_denied(checks, port, "/orders/note-insert", pg.password)
        assert_denied(checks, port, "/orders/purge", pg.password, "POST")
        assert_denied(checks, port, "/orders/note", pg.password, "POST")
        # `exit()` and a fatal end their own request, not the route.
        status, _headers, body = http(port, "/crash/exit", timeout=15)
        checks.that(status == 200, f"GET /crash/exit is 200, not {status}")
        status, _headers, body = http(port, "/crash/fatal", timeout=15)
        checks.that(status >= 500, f"GET /crash/fatal is 5xx, not {status}")
        checks.that(b"<?php" not in body, "the fatal response carried PHP source")
        checks.that(
            http(port, "/products", timeout=15)[0] == 200,
            "the route stopped serving after a fatal",
        )

        # The same views with the five `$DB_*` variables instead of one URL.
        start_php(
            binary,
            network,
            image,
            split_name,
            split_port,
            "Blastfile.splitstore",
            {
                "DB_HOST": "pg",
                "DB_PORT": "5432",
                "DB_USERNAME": "apps",
                "DB_PASSWORD": pg.password,
                "DB_DATABASE": "apps",
            },
        )
        wait_php(checks, split_name, split_port)
        assert_products(checks, split_port, ["alpha", "beta", "gamma"])
        assert_split_env(checks, split_port, pg.password)

        probes = docker(
            "exec",
            name,
            "python3",
            "/opt/e2e/php2shell.py",
            "--fixture",
            "/fixture",
            "--site",
            "/tmp/php2shell-site",
            "--page",
            "/opt/e2e/php-docs/index.md",
            "--matrix",
            "/opt/e2e/probes.md",
            "--port",
            "28090",
            timeout=1200,
        )
        checks.that(
            probes.returncode == 0,
            "the php2shell matrix failed:\n" + tail(probes.stdout + probes.stderr, 40),
        )
        checks.that(
            "sql.denied" in docker("logs", name).stdout,
            "no sql.denied event in the PHP log",
        )
    finally:
        if checks.failures:
            print(tail(docker("logs", name).stdout, 80), flush=True)
        for container in (name, split_name):
            docker("rm", "-f", container, timeout=60)
        docker("rmi", "-f", image, timeout=120)
    return checks.done("php 8.4")


def static_argv(
    image: str,
    binary: Path,
    site: Path,
    port: int,
    network: str,
    name: str,
) -> list[str]:
    """One static-only container: the binary mounted, no interpreter present."""
    return [
        "run",
        "-d",
        "--rm",
        "--name",
        name,
        "--network",
        network,
        "-p",
        f"127.0.0.1:{port}:{port}",
        "-v",
        f"{binary}:/usr/local/bin/blastit:ro",
        "-v",
        f"{site}:/site:ro",
        "-w",
        "/site",
        "-e",
        "BLASTIT_DEV_EXPOSE=1",
        image,
        "sh",
        "-c",
        f"exec /usr/local/bin/blastit dev --host 0.0.0.0 --port {port} --no-reload",
    ]


def leg_static(binary: Path, workspace: Path, network: str) -> str:
    image = f"blastit-apps-static:{os.getpid()}"
    built = docker(
        "build",
        "-q",
        "-f",
        str(APPS / "static" / "Dockerfile"),
        "-t",
        image,
        str(ROOT),
        timeout=600,
    )
    if built.returncode != 0:
        raise LegFailure(f"the static image did not build: {tail(built.stderr, 20)}")
    clean = docker(
        "run",
        "--rm",
        image,
        "sh",
        "-c",
        "! command -v python3 && ! command -v php && echo no-interpreter",
    )
    checks = Checks()
    checks.that(
        "no-interpreter" in clean.stdout,
        f"the static image has an interpreter: {clean.stdout + clean.stderr}",
    )

    site = workspace / "static-site"
    shutil.copytree(APPS / "static" / "site", site)
    # Real decoys under the served root: a tracked dotfile directory, a secret
    # file the `..` probes aim at, and a symlink that leaves the root. They are
    # installed here because git refuses a nested `.git` and a committed `.env`
    # would be ignored.
    (site / "public" / ".git").mkdir()
    (site / "public" / ".git" / "config").write_text(
        "[core]\n\trepositoryformatversion = 0\n", encoding="utf-8"
    )
    (site / "public" / ".env").write_text("SECRET=leaked\n", encoding="utf-8")
    (site / "public" / "secret").write_text("top-secret\n", encoding="utf-8")
    (site / "public" / "escape").symlink_to("/etc/passwd")
    np_site = workspace / "static-nospa"
    shutil.copytree(APPS / "static" / "nospa", np_site)

    port = PORTS.take()
    np_port = PORTS.take()
    names = [
        f"blastit-apps-static-{os.getpid()}-{port}",
        f"blastit-apps-static-{os.getpid()}-{np_port}",
    ]
    try:
        for name, site_dir, bind in (
            (names[0], site, port),
            (names[1], np_site, np_port),
        ):
            result = docker(*static_argv(image, binary, site_dir, bind, network, name))
            if result.returncode != 0:
                raise LegFailure(f"{name} did not start: {tail(result.stderr, 20)}")
        for bind in (port, np_port):
            deadline = time.monotonic() + 120
            while True:
                try:
                    http(bind, "/", timeout=5)
                    break
                except (OSError, LegFailure) as error:
                    if time.monotonic() > deadline:
                        raise LegFailure(
                            f"the static site on {bind} never answered: {error}"
                        ) from error
                    time.sleep(0.25)
        index = (site / "public" / "index.html").read_bytes()
        status, headers, body = http(port, "/")
        checks.that(
            status == 200 and body == index, f"GET / is not index.html ({status})"
        )
        etag = headers.get("etag")
        checks.that(etag is not None, "GET / carries no ETag")
        if etag:
            status, _headers, body = http(port, "/", headers={"If-None-Match": etag})
            checks.that(
                status == 304 and body == b"", f"If-None-Match is {status}, not 304"
            )
        status, headers, _body = http(port, "/dashboard/settings")
        checks.that(status == 200, f"the SPA fallback is {status}, not 200")
        checks.that(
            "no-cache" in headers.get("cache-control", ""),
            f"the fallback is cached ({headers.get('cache-control')!r})",
        )
        assert_static_mount(checks, port, "/assets/app.4f1c2a9b7d3e.css")
        status, _headers, _body = http(port, "/robots.txt")
        checks.that(status == 200, f"GET /robots.txt is {status}, not 200")
        status, _headers, body = http(port, "/robots.txt", "HEAD")
        checks.that(
            status == 200 and body == b"", f"HEAD /robots.txt is {status}/{len(body)}B"
        )
        status, _headers, _body = http(
            port, "/.well-known/acme-challenge/fixture-token"
        )
        checks.that(status == 200, f"the .well-known exception is {status}, not 200")
        status, _headers, body = http(port, "/secret")
        checks.that(
            status == 200 and body == b"top-secret\n",
            f"GET /secret is {status} {body[:40]!r}, so the traversal rows prove nothing",
        )
        # The documented per-class statuses (`docs/frameworks/static-and-spa.md`):
        # traversal and control bytes are 400, a dotfile/symlink/`.php` is 404.
        for target, expected in (
            ("/%00.js", 400),
            ("/foo%5c..%5cbar", 400),
            ("/assets/%2e%2e/%2e%2e/secret", 400),
            ("/../etc/passwd", 400),
            ("/%2e%2e/%2e%2e/etc/passwd", 400),
            ("/assets/../secret", 400),
            ("/.git/", 404),
            ("/.git/config", 404),
            ("/.env", 404),
            ("/escape", 404),
            ("/index.php", 404),
        ):
            status, _headers, _body = http(port, target)
            checks.that(
                status == expected,
                f"GET {target} is {status}, expected the documented {expected}",
            )
        checks.that(
            http(np_port, "/deep/route")[0] == 404,
            "a mount without fallback answered a deep path",
        )
        checks.that(
            http(port, "/", "POST", b"x")[0] != 200, "a POST reached a static mount"
        )
        banner = docker("logs", names[0]).stdout + docker("logs", names[0]).stderr
        checks.that(
            "static  /  GET, HEAD  public" in banner,
            f"the banner does not name the mount as the page shows it:\n{tail(banner, 12)}",
        )
        checked = docker(
            "exec",
            "-w",
            "/site",
            names[0],
            "/usr/local/bin/blastit",
            "check",
            "--json",
        )
        checks.that(
            checked.returncode == 0,
            f"`blastit check --json` on the static site exits {checked.returncode}: "
            f"{tail(checked.stdout + checked.stderr, 10)}",
        )
        try:
            json.loads(checked.stdout)
        except ValueError:
            checks.that(
                False,
                f"`blastit check --json` wrote no document: {checked.stdout[:200]!r}",
            )
    finally:
        for name in names:
            docker("rm", "-f", name, timeout=60)
        docker("rmi", "-f", image, timeout=120)
    return checks.done("static (no interpreter)")


# ---------------------------------------------------------------------------
# the host: the binary, a venv per CPython, and the run
# ---------------------------------------------------------------------------


def binary_argument() -> Path:
    for candidate in (
        os.environ.get("E2E_BLASTIT"),
        ROOT / "target" / "release" / "blastit",
        ROOT / "target" / "debug" / "blastit",
        shutil.which("blastit"),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate).resolve()
    raise Skip(
        "no blastit binary; build one with `cargo build --release -p blastit --locked` "
        "or pass --binary /path/to/blastit"
    )


def venv_for(minor: str) -> Path:
    """One CPython `minor` venv with both frameworks, cached under TMPDIR."""
    target = WORK_ROOT / f"venv-{minor}"
    python = target / "bin" / "python"
    if python.exists():
        present = run(
            [
                str(python),
                "-c",
                "import django, dj_database_url, flask, sqlalchemy, psycopg",
            ]
        )
        if present.returncode == 0:
            return target
    if shutil.which("uv") is None:
        raise Skip(
            f"no `uv` to build a CPython {minor} venv; install `uv` "
            "(https://docs.astral.sh/uv/)"
        )
    if not python.exists():
        created = run(["uv", "venv", "--python", minor, str(target)])
        if created.returncode != 0:
            raise Skip(
                f"CPython {minor} is not installed; `uv python install {minor}` "
                f"({tail(created.stderr, 4)})"
            )
    installed = run(
        [
            "uv",
            "pip",
            "install",
            "--python",
            str(python),
            "Django==5.2.*",
            "dj-database-url==3.*",
            "psycopg[binary]==3.*",
            "Flask==3.1.*",
            "SQLAlchemy==2.1.*",
        ],
        timeout=1800,
    )
    if installed.returncode != 0:
        raise LegFailure(
            f"the {minor} venv did not install: {tail(installed.stderr, 20)}"
        )
    return target


def fmt_check(binary: Path, manifest: Path) -> None:
    result = run([str(binary), "fmt", "--check", str(manifest)], cwd=manifest.parent)
    if result.returncode != 0:
        raise LegFailure(
            f"{manifest.relative_to(ROOT)} is not canonical: "
            f"{tail(result.stdout + result.stderr, 12)}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, help="the blastit artifact under test")
    parser.add_argument(
        "--legs",
        default="",
        help=(
            "comma-separated families to run: wsgi (Django & Flask on 3.13 and "
            "3.14), php, static (default: every family)"
        ),
    )
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    arguments = parser.parse_args()
    families = [name.strip() for name in arguments.legs.split(",") if name.strip()]
    unknown = sorted(set(families) - {"wsgi", "php", "static"})
    if unknown:
        parser.error(f"--legs names {unknown}; the families are wsgi, php, static")

    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix="run-", dir=WORK_ROOT))
    summary = Summary()
    docs = run([sys.executable, str(HERE / "docs_deploy.py")], timeout=180)
    summary.add(
        "docs deploy",
        "PASS" if docs.returncode == 0 else "FAIL",
        "documented pooler promises hold"
        if docs.returncode == 0
        else tail(docs.stdout + docs.stderr, 8),
    )
    network = f"blastit-apps-{os.getpid()}"
    owned: list[str] = []
    pg: Postgres | None = None
    try:
        try:
            binary = (
                arguments.binary.resolve() if arguments.binary else binary_argument()
            )
            require_docker()
        except Skip as skipped:
            summary.add("host", "SKIP", str(skipped))
            return summary.report()
        summary.add("binary", "PASS", f"{binary} ({binary.stat().st_size} bytes)")

        manifests = [
            APPS / "php" / "Blastfile",
            APPS / "php" / "Blastfile.splitstore",
            APPS / "php" / "Blastfile.php2shell",
            APPS / "python" / "django" / "Blastfile",
            APPS / "python" / "flask" / "Blastfile",
            APPS / "static" / "site" / "Blastfile",
            APPS / "static" / "nospa" / "Blastfile",
        ]
        for manifest in manifests:
            fmt_check(binary, manifest)
        summary.add("fmt --check", "PASS", f"{len(manifests)} manifests are canonical")

        created = docker("network", "create", network)
        if created.returncode != 0:
            raise LegFailure(f"`docker network create` failed: {tail(created.stderr)}")
        pg = Postgres(network, f"blastit-apps-pg-{os.getpid()}", PORTS.take())
        owned.append(pg.name)
        pg.start()
        pg.apply(APPS / "postgres" / "schema.sql")
        summary.add(
            "postgres", "PASS", f"postgres:18 on 127.0.0.1:{pg.port} (schema applied)"
        )

        legs = [
            (
                "wsgi",
                "django 3.13",
                lambda: leg_django(binary, workspace, venv_for("3.13"), "3.13", pg),
            ),
            (
                "wsgi",
                "flask 3.13",
                lambda: leg_flask(binary, workspace, venv_for("3.13"), "3.13", pg),
            ),
            (
                "wsgi",
                "django 3.14",
                lambda: leg_django(binary, workspace, venv_for("3.14"), "3.14", pg),
            ),
            (
                "wsgi",
                "flask 3.14",
                lambda: leg_flask(binary, workspace, venv_for("3.14"), "3.14", pg),
            ),
            ("php", "php 8.4", lambda: leg_php(binary, network, pg)),
            ("static", "static", lambda: leg_static(binary, workspace, network)),
        ]
        for family, name, leg in legs:
            if families and family not in families:
                continue
            try:
                summary.add(name, "PASS", leg())
            except Skip as skipped:
                summary.add(name, "SKIP", str(skipped))
            except LegFailure as failure:
                summary.add(name, "FAIL", str(failure))
            except Exception as error:  # noqa: BLE001 - one broken leg must not end the run
                summary.add(name, "FAIL", f"{type(error).__name__}: {error}")
    except LegFailure as failure:
        summary.add("harness", "FAIL", str(failure))
    except Skip as skipped:
        summary.add("harness", "SKIP", str(skipped))
    finally:
        for name in owned:
            docker("rm", "-f", name, timeout=60)
        docker("network", "rm", network, timeout=60)
        if not arguments.keep:
            shutil.rmtree(workspace, ignore_errors=True)
    return summary.report()


if __name__ == "__main__":
    sys.exit(main())
