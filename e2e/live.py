"""Live end-to-end test on DigitalOcean App Platform (ARCHITECTURE.md §10).

Creates a fresh managed PostgreSQL cluster and an App Platform app in ams3,
running the e2e image that CI built from the commit under test (pushed to
GHCR), then runs the same scenario and log checks as e2e/run.py against the
public URL, and deletes both, whatever happened.

    live.py --name NAME --image ghcr.io/OWNER/REPO:TAG   # full run
    live.py --cleanup --name NAME                        # delete NAME's app and cluster
    live.py --janitor --older-than-minutes 90            # delete stale CI resources

Authentication: doctl's (DIGITALOCEAN_ACCESS_TOKEN in CI, `--context` locally).
GHCR_CREDENTIALS ("user:token") lets App Platform pull a private image.
Stdlib only.
"""

import argparse
import datetime as dt
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
PREFIX = "blastit-ci-"
TAG = "blastit-ci"
CONTEXT = []


def doctl(*args, parse=True, check=True):
    cmd = ["doctl", *CONTEXT, *args] + (["-o", "json"] if parse else [])
    p = subprocess.run(cmd, capture_output=True, text=True)
    if check and p.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd[:4])}… failed: {p.stderr.strip()[-800:]}")
    if not parse:
        return p.stdout
    try:
        return json.loads(p.stdout or "null")
    except ValueError:
        return None


def log(msg):
    print(f"[live {dt.datetime.now(dt.timezone.utc):%H:%M:%S}] {msg}", flush=True)


def app_spec(name, cluster, image):
    # E1: the App Platform trust decision is a preset. It sets
    # `BLASTIT_CLIENT_IP_HEADER=do-connecting-ip` and trusts every peer, which
    # §10 explains: a service is only reachable through DO's load balancer and
    # the app's own internal services, and the edge *overwrites* that header with
    # the real client address (measured; see §10's table), so no client can forge
    # it. It also turns on fair queuing and JSON logs.
    ref, tag = image.rsplit(":", 1)
    _, owner, repo = ref.split("/", 2)
    creds = os.environ.get("GHCR_CREDENTIALS", "")
    image_block = f"""  image:
    registry_type: GHCR
    registry: {owner.lower()}
    repository: {repo.lower()}
    tag: "{tag}"
""" + (f'    registry_credentials: "{creds}"\n' if creds else "")
    common_env = """  - key: DATABASE_URL
    scope: RUN_TIME
    value: ${db.DATABASE_URL}
  - key: DJANGO_SECRET_KEY
    scope: RUN_TIME
    type: SECRET
    value: live-e2e-not-secret
"""
    return f"""name: {name}
region: ams
databases:
- name: db
  engine: PG
  production: true
  cluster_name: {cluster}
jobs:
- name: migrate
  kind: PRE_DEPLOY
{image_block}  instance_size_slug: apps-s-1vcpu-1gb
  run_command: sh -c 'python manage.py migrate -v0 && python manage.py seed'
  envs:
{common_env}services:
- name: web
{image_block}  http_port: 8080
  instance_count: 1
  instance_size_slug: apps-s-1vcpu-1gb
  health_check:
    http_path: /_blastit/health
    initial_delay_seconds: 20
  envs:
{common_env}  - key: BLASTIT_PRESET
    scope: RUN_TIME
    value: digitalocean-paas
"""


def find_app(name):
    for a in doctl("apps", "list") or []:
        if a.get("spec", {}).get("name") == name:
            return a
    return None


def find_cluster(name):
    for c in doctl("databases", "list") or []:
        if c.get("name") == name:
            return c
    return None


def cleanup(name):
    app = find_app(name)
    if app:
        log(f"deleting app {name} ({app['id']})")
        doctl("apps", "delete", app["id"], "--force", parse=False, check=False)
    cluster = find_cluster(name)
    if cluster:
        log(f"deleting database cluster {name} ({cluster['id']})")
        doctl("databases", "delete", cluster["id"], "--force", parse=False, check=False)
    if not app and not cluster:
        log(f"nothing to delete for {name}")


def print_failed_steps(steps, depth=0):
    for step in steps:
        if step.get("status") not in ("SUCCESS", "PENDING"):
            reason = step.get("reason") or {}
            print(f"{'  ' * depth}- {step.get('name')}: {step.get('status')} "
                  f"{reason.get('code', '')} {reason.get('message', '')}".rstrip(), flush=True)
        print_failed_steps(step.get("steps") or [], depth + 1)


def wait_deployment(app_id, timeout=25 * 60):
    deadline = time.monotonic() + timeout
    phase = None
    while time.monotonic() < deadline:
        deps = doctl("apps", "list-deployments", app_id) or []
        if deps:
            dep = deps[0]
            if dep.get("phase") != phase:
                phase = dep.get("phase")
                log(f"deployment {dep['id'][:8]}: {phase}")
            if phase == "ACTIVE":
                return dep
            if phase in ("ERROR", "CANCELED"):
                full = doctl("apps", "get-deployment", app_id, dep["id"], check=False)
                full = full[0] if isinstance(full, list) and full else (full or {})
                print_failed_steps((full.get("progress") or {}).get("steps") or [])
                for component in ("migrate", "web"):
                    for kind in ("deploy", "run"):
                        out = doctl("apps", "logs", app_id, component, "--type", kind,
                                    "--deployment", dep["id"], parse=False, check=False)
                        print(f"--- {component} {kind} log tail ---\n{out[-4000:]}")
                raise RuntimeError(f"deployment {phase}")
        time.sleep(15)
    raise TimeoutError("deployment did not become ACTIVE in time")


def wait_http(url, timeout=5 * 60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url + "/_blastit/health", timeout=15) as r:
                if r.status == 200:
                    return
        except Exception:  # noqa: BLE001
            pass
        time.sleep(5)
    raise TimeoutError(f"{url} never answered its health path")


def run_logs(app_id):
    """The web component's stdout, one blastit event per line (DO's prefix removed)."""
    raw = doctl("apps", "logs", app_id, "web", "--type", "run", "--tail", "100000", parse=False, check=False)
    lines = []
    for line in raw.splitlines():
        brace = line.find("{")
        if brace >= 0:
            lines.append(line[brace:])
        elif line.strip():
            lines.append(line)  # kept, so check_logs reports it as non-JSON
    return "\n".join(lines) + "\n"


def full_run(args):
    name = args.name
    if not name.startswith(PREFIX) or len(name) > 32:
        sys.exit(f"--name must start with {PREFIX!r} and be at most 32 characters")

    def on_term(signum, _frame):
        raise KeyboardInterrupt(f"signal {signum}")

    signal.signal(signal.SIGTERM, on_term)
    rc = 1
    try:
        log(f"creating PostgreSQL 18 cluster {name} in ams3 (a few minutes)")
        t0 = time.monotonic()
        cluster = doctl("databases", "create", name, "--engine", "pg", "--version", "18",
                        "--region", "ams3", "--size", "db-s-1vcpu-1gb", "--num-nodes", "1",
                        "--tag", TAG, "--wait")
        cluster = cluster[0] if isinstance(cluster, list) else cluster
        log(f"cluster online after {time.monotonic() - t0:.0f}s")
        conn = doctl("databases", "connection", cluster["id"])
        conn = conn[0] if isinstance(conn, list) else conn
        password = conn.get("password", "")
        if os.environ.get("GITHUB_ACTIONS") and password:
            print(f"::add-mask::{password}", flush=True)

        with tempfile.NamedTemporaryFile("w", suffix=".yaml", delete=False) as f:
            f.write(app_spec(name, name, args.image))
            spec_path = f.name
        log(f"creating app {name} from {args.image}")
        try:
            created = doctl("apps", "create", "--spec", spec_path)
        finally:
            os.unlink(spec_path)
        app = created[0] if isinstance(created, list) else created
        wait_deployment(app["id"])
        app = doctl("apps", "get", app["id"])[0]
        url = (app.get("default_ingress") or app.get("live_url")).rstrip("/")
        log(f"app live at {url}")
        wait_http(url)

        rc = subprocess.run(
            [sys.executable, os.path.join(HERE, "scenario.py"), url, "--expect", "blastit",
             # E1: the client address must be a client's, not the platform
             # ingress's — the driver cannot know the runner's public address, so
             # the scenario checks what it can: that `REMOTE_ADDR` is nobody's
             # forged header and is not an ingress.
             "--remote-addr", "public"],
        ).returncode
        time.sleep(20)  # let App Platform's log forwarding catch up
        os.makedirs(args.out, exist_ok=True)
        log_path = os.path.join(args.out, "live-app.log")
        with open(log_path, "w") as f:
            f.write(run_logs(app["id"]))
        log(f"app log saved to {log_path}")
        rc |= subprocess.run(
            [sys.executable, os.path.join(HERE, "check_logs.py"), log_path],
            env={**os.environ, "E2E_SECRET": password, "E2E_SECRET_VAR": "E2E_SECRET",
                 "E2E_EXPECT_PRESET": "digitalocean-paas"},
        ).returncode
    except KeyboardInterrupt as e:
        log(f"interrupted ({e}), cleaning up")
        rc = 130
    except Exception as e:  # noqa: BLE001
        log(f"FAILED: {e}")
        rc = 1
    finally:
        if args.keep:
            log(f"--keep: leaving {name} in place; delete with: live.py --cleanup --name {name}")
        else:
            cleanup(name)
    return rc


def janitor(minutes):
    now = dt.datetime.now(dt.timezone.utc)

    def old(created):
        try:
            t = dt.datetime.fromisoformat(created.replace("Z", "+00:00").replace(" +0000 UTC", "+00:00"))
        except (AttributeError, ValueError):
            return True
        return (now - t).total_seconds() > minutes * 60

    for a in doctl("apps", "list") or []:
        name = a.get("spec", {}).get("name", "")
        if name.startswith(PREFIX) and old(a.get("created_at", "")):
            log(f"stale app {name}")
            doctl("apps", "delete", a["id"], "--force", parse=False, check=False)
    for c in doctl("databases", "list") or []:
        if c.get("name", "").startswith(PREFIX) and TAG in (c.get("tags") or []) and old(c.get("created_at", "")):
            log(f"stale cluster {c['name']}")
            doctl("databases", "delete", c["id"], "--force", parse=False, check=False)
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--name")
    ap.add_argument("--image")
    ap.add_argument("--out", default=os.path.join(HERE, "out"))
    ap.add_argument("--context", help="doctl auth context (local runs)")
    ap.add_argument("--keep", action="store_true", help="don't delete the resources (debugging)")
    ap.add_argument("--cleanup", action="store_true")
    ap.add_argument("--janitor", action="store_true")
    ap.add_argument("--older-than-minutes", type=int, default=90)
    args = ap.parse_args()
    if args.context:
        CONTEXT.extend(["--context", args.context])
    if args.janitor:
        return janitor(args.older_than_minutes)
    if not args.name:
        ap.error("--name is required")
    if args.cleanup:
        cleanup(args.name)
        return 0
    if not args.image:
        ap.error("--image is required for a full run")
    return full_run(args)


if __name__ == "__main__":
    sys.exit(main())
