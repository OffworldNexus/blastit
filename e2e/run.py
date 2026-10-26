"""Run the end-to-end test locally or in CI: build, start, drive, check, tear down.

    run.py            # docker compose; needs Docker with the compose plugin

Stdlib only. The database password is random per run, and the checks prove it
never reaches the app or the logs.
"""

import os
import secrets
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = os.environ.get("E2E_PORT", "18080")


def compose(*args, **kw):
    return subprocess.run(["docker", "compose", "-f", os.path.join(HERE, "compose.yaml"), *args], **kw)


def scenario_self_test():
    """`scenario.py --self-test`: the helpers' own assertions (E1)."""
    return subprocess.run(
        [sys.executable, os.path.join(HERE, "scenario.py"), "--self-test"], text=True
    ).returncode


def main():
    env = {**os.environ, "E2E_DB_PASSWORD": "e2e-" + secrets.token_hex(16), "E2E_PORT": PORT}
    os.environ.update(env)
    rc = 1
    try:
        if compose("up", "--build", "--detach", "app").returncode != 0:
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
    finally:
        if rc:
            compose("logs", "--no-color", "--tail", "200")
        compose("down", "--volumes", "--remove-orphans", capture_output=True)
    return rc


if __name__ == "__main__":
    sys.exit(main())
