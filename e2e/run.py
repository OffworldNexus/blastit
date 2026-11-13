"""Run the end-to-end test locally or in CI: build, start, drive, check, tear down.

    run.py            # docker compose; needs Docker with the compose plugin

Stdlib only. The database password is random per run, and the checks prove it
never reaches the app or the logs.

Two apps run on the same image and the same database:

* the Django demo (`e2e/demo/`), driven by `scenario.py` and checked by
  `check_logs.py`;
* the Flask + SQLAlchemy app (`e2e/sqlalchemy/`, E2b), driven and checked by
  `sqlalchemy_check.py` — the proof that a non-Django WSGI app with its own URL
  parsing and its own connection statements is served by the same manifest
  design.
"""

import os
import secrets
import subprocess
import sys
import tempfile

import docs_deploy
import prebuilt_wheel

HERE = os.path.dirname(os.path.abspath(__file__))
DIST = os.path.join(os.path.dirname(HERE), "dist")
PORT = os.environ.get("E2E_PORT", "18080")
SQLALCHEMY_PORT = os.environ.get("E2E_SQLALCHEMY_PORT", "18081")
# The static leg's `blastit dev` binds this port *inside* its container: the page
# documents 127.0.0.1:8080 and the driver runs in that same network namespace.
STATIC_PORT = os.environ.get("E2E_STATIC_PORT", "8080")
# The compose project, as `compose.yaml` derives it: a run's artifacts are named
# after it, so two checkouts running at once do not write the same file.
PROJECT = os.environ.get("E2E_PROJECT", "blastit-e2e")


def compose(*args, **kw):
    return subprocess.run(
        ["docker", "compose", "-f", os.path.join(HERE, "compose.yaml"), *args],
        check=False,
        **kw,
    )


def scenario_self_test():
    """`scenario.py --self-test`: the helpers' own assertions (E1)."""
    return subprocess.run(
        [sys.executable, os.path.join(HERE, "scenario.py"), "--self-test"],
        check=False,
        text=True,
    ).returncode


def static_self_test():
    """`static_assets.py --self-test`: the page parser and the page's shape."""
    return subprocess.run(
        [sys.executable, os.path.join(HERE, "static_assets.py"), "--self-test"],
        check=False,
        text=True,
    ).returncode


def static_checks():
    """Replay `docs/frameworks/static-and-spa.md` against the `static` service.

    The driver runs *inside* that container: the page's commands say
    `127.0.0.1:8080`, and `blastit dev` binds exactly that (a published port
    would need `0.0.0.0` and change the documented banner). The service's own
    stdout is the banner fence's evidence; `compose logs` names it without a
    prefix. `env -u` drops the e2e image's Django `BLASTIT_APP`, which an
    all-static manifest refuses.
    """
    logs = compose(
        "logs",
        "--no-color",
        "--no-log-prefix",
        "static",
        capture_output=True,
        text=True,
    ).stdout
    return compose(
        "exec",
        "-T",
        "static",
        "env",
        "-u",
        "BLASTIT_APP",
        "-u",
        "BLASTIT_PRELOAD",
        "python",
        "/opt/e2e/static_assets.py",
        "--base",
        f"http://127.0.0.1:{STATIC_PORT}",
        "--nospa-base",
        "http://static_nospa:8080",
        "--site",
        "/site",
        "--page",
        "/opt/e2e/static-and-spa.md",
        "--banner",
        logs,
    ).returncode


def main():
    # The two deploy pages promise a pooler between blastit and PostgreSQL; a
    # regression there is a docs defect, not a container one, so it is asserted
    # before Docker starts (no Docker, no network).
    if docs_deploy.main() != 0:
        return 1
    env = {
        **os.environ,
        "E2E_DB_PASSWORD": "e2e-" + secrets.token_hex(16),
        "E2E_PORT": PORT,
        "E2E_SQLALCHEMY_PORT": SQLALCHEMY_PORT,
        "E2E_STATIC_PORT": STATIC_PORT,
    }
    # A prebuilt image installs the wheel this host built, so its base image is
    # the interpreter that wheel targets (the extension is not abi3). The
    # in-image `Dockerfile` builds its own wheel on its own interpreter and
    # needs none of this.
    if "prebuilt" in os.environ.get("E2E_DOCKERFILE", "Dockerfile"):
        try:
            env["E2E_PYTHON_VERSION"] = prebuilt_wheel.version(DIST)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 1
    os.environ.update(env)
    rc = 1
    try:
        if (
            compose(
                "up",
                "--build",
                "--detach",
                "app",
                "sqlalchemy",
                "static",
                "static_nospa",
            ).returncode
            != 0
        ):
            print("compose up failed")
            return 1
        # The static containers are healthy before the driver execs into one of
        # them (no fixed sleep; the healthcheck is a request to its own root).
        if (
            compose(
                "up",
                "--detach",
                "--wait",
                "--wait-timeout",
                "90",
                "static",
                "static_nospa",
            ).returncode
            != 0
        ):
            print("the static services did not become healthy")
            return 1
        rc = scenario_self_test()
        rc |= static_self_test()
        rc |= static_checks()
        rc |= subprocess.run(
            [
                sys.executable,
                os.path.join(HERE, "scenario.py"),
                f"http://127.0.0.1:{PORT}",
                "--expect",
                "blastit",
                # The app is published directly here, so the peer is
                # not a trusted one and a header the client sends is
                # not read at all (E1).
                "--forged-proto-ignored",
            ],
            check=False,
        ).returncode
        logs = compose(
            "logs",
            "--no-color",
            "--no-log-prefix",
            "app",
            capture_output=True,
            text=True,
        ).stdout
        with open(os.path.join(HERE, "app.log"), "w") as f:
            f.write(logs)
        rc |= subprocess.run(
            [
                sys.executable,
                os.path.join(HERE, "check_logs.py"),
                os.path.join(HERE, "app.log"),
            ],
            check=False,
            env={**env, "E2E_SECRET_VAR": "E2E_DB_PASSWORD"},
        ).returncode

        # The second app: Flask + SQLAlchemy, its own manifest, its own port. The
        # HTTP part first, then its log — a log only holds requests that were
        # made, and this one is written outside the checkout: it is a run's
        # artifact, not a file of the repository, and the `logs` dump below has
        # it when something fails.
        rc |= subprocess.run(
            [
                sys.executable,
                os.path.join(HERE, "sqlalchemy_check.py"),
                f"http://127.0.0.1:{SQLALCHEMY_PORT}",
            ],
            check=False,
        ).returncode
        sqlalchemy_log = os.path.join(
            tempfile.gettempdir(), f"{PROJECT}-sqlalchemy.log"
        )
        logs = compose(
            "logs",
            "--no-color",
            "--no-log-prefix",
            "sqlalchemy",
            capture_output=True,
            text=True,
        ).stdout
        with open(sqlalchemy_log, "w") as f:
            f.write(logs)
        rc |= subprocess.run(
            [
                sys.executable,
                os.path.join(HERE, "sqlalchemy_check.py"),
                "--log",
                sqlalchemy_log,
            ],
            check=False,
        ).returncode
    finally:
        if rc:
            compose("logs", "--no-color", "--tail", "200")
        compose("down", "--volumes", "--remove-orphans", capture_output=True)
    return rc


if __name__ == "__main__":
    sys.exit(main())
