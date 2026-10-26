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

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = os.environ.get("E2E_PORT", "18080")
SQLALCHEMY_PORT = os.environ.get("E2E_SQLALCHEMY_PORT", "18081")


def compose(*args, **kw):
    return subprocess.run(["docker", "compose", "-f", os.path.join(HERE, "compose.yaml"), *args], **kw)


def scenario_self_test():
    """`scenario.py --self-test`: the helpers' own assertions (E1)."""
    return subprocess.run(
        [sys.executable, os.path.join(HERE, "scenario.py"), "--self-test"], text=True
    ).returncode


def main():
    env = {
        **os.environ,
        "E2E_DB_PASSWORD": "e2e-" + secrets.token_hex(16),
        "E2E_PORT": PORT,
        "E2E_SQLALCHEMY_PORT": SQLALCHEMY_PORT,
    }
    os.environ.update(env)
    rc = 1
    try:
        if compose("up", "--build", "--detach", "app", "sqlalchemy").returncode != 0:
            print("compose up failed")
            return 1
        rc = scenario_self_test()
        rc |= subprocess.run([sys.executable, os.path.join(HERE, "scenario.py"), f"http://127.0.0.1:{PORT}",
                             "--expect", "blastit",
                             # The app is published directly here, so the peer is
                             # not a trusted one and a header the client sends is
                             # not read at all (E1).
                             "--forged-proto-ignored"]).returncode
        logs = compose("logs", "--no-color", "--no-log-prefix", "app", capture_output=True, text=True).stdout
        with open(os.path.join(HERE, "app.log"), "w") as f:
            f.write(logs)
        rc |= subprocess.run([sys.executable, os.path.join(HERE, "check_logs.py"), os.path.join(HERE, "app.log")],
                             env={**env, "E2E_SECRET_VAR": "E2E_DB_PASSWORD"}).returncode

        # The second app: Flask + SQLAlchemy, its own manifest, its own port. The
        # HTTP part first, then its log — a log only holds requests that were
        # made, and this one is written outside the checkout: it is a run's
        # artifact, not a file of the repository, and the `logs` dump below has
        # it when something fails.
        rc |= subprocess.run(
            [sys.executable, os.path.join(HERE, "sqlalchemy_check.py"),
             f"http://127.0.0.1:{SQLALCHEMY_PORT}"]
        ).returncode
        sqlalchemy_log = os.path.join(tempfile.gettempdir(), "blastit-e2e-sqlalchemy.log")
        logs = compose(
            "logs", "--no-color", "--no-log-prefix", "sqlalchemy", capture_output=True, text=True
        ).stdout
        with open(sqlalchemy_log, "w") as f:
            f.write(logs)
        rc |= subprocess.run(
            [sys.executable, os.path.join(HERE, "sqlalchemy_check.py"), "--log", sqlalchemy_log]
        ).returncode
    finally:
        if rc:
            compose("logs", "--no-color", "--tail", "200")
        compose("down", "--volumes", "--remove-orphans", capture_output=True)
    return rc


if __name__ == "__main__":
    sys.exit(main())
