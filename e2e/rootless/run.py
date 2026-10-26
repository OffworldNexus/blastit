"""Rootless end-to-end test for blastit (ROADMAP D6, ARCHITECTURE.md §10).

Runs the e2e scenario against the demo app served by an **unprivileged** blastit
holding only the capabilities it needs, either on this machine (Docker Compose)
or on a DigitalOcean Droplet created for the run and destroyed afterwards, and
checks the logs afterwards. Stdlib only.

    run.py --local                       # Docker Compose on this machine
    run.py --droplet                     # create, test, destroy (ams3)
    run.py --droplet --keep              # leave the resources for debugging
    run.py --cleanup --name NAME         # delete a named droplet and its key
    run.py --janitor [--older-than-minutes 120]

Both modes run the same compose file (`e2e/rootless/compose.yml`), the same
scenario (`e2e/scenario.py`) and the same log checks (`e2e/check_logs.py`, plus
`check_rootless_logs` here — D6's own part: the boot event says which mode init
runs in, and it is `capabilities`).

Authentication: doctl's (DIGITALOCEAN_ACCESS_TOKEN, or `--context` locally). The
SSH key a droplet is created with is generated in `/tmp/opencode` and imported
under a `blastit-d6-` name, then deleted with the droplet.
"""

import argparse
import datetime as dt
import json
import os
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
COMPOSE = os.path.join(HERE, "compose.yml")
PREFIX = "blastit-d6-"
TAG = "blastit-d6"
REGION = "ams3"
SIZE = "s-2vcpu-4gb"
# The capabilities D6 asks for, in the order the kernel numbers them. The
# image's `setcap` line, the compose `cap_add` list and this list are one set,
# and the log check fails when blastit reports anything else.
CAPABILITIES = [
    "cap_chown",
    "cap_dac_override",
    "cap_fowner",
    "cap_kill",
    "cap_setgid",
    "cap_setuid",
    "cap_setpcap",
]
CONTEXT = []


def log(msg):
    print(f"[rootless {dt.datetime.now(dt.timezone.utc):%H:%M:%S}] {msg}", flush=True)


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    return bool(cond)


def run(cmd, *, check_rc=True, cwd=None, **kw):
    p = subprocess.run(cmd, text=True, capture_output=True, cwd=cwd, **kw)
    if check_rc and p.returncode != 0:
        raise RuntimeError(
            f"{' '.join(str(part) for part in cmd[:6])}… failed ({p.returncode}): "
            f"{p.stderr.strip()[-800:]}"
        )
    return p


def doctl(*args, parse=True, check_rc=True):
    p = run(["doctl", *CONTEXT, *args] + (["-o", "json"] if parse else []), check_rc=check_rc)
    if not parse:
        return p.stdout
    try:
        return json.loads(p.stdout or "null")
    except ValueError:
        return None


# --------------------------------------------------------------------------
# the shared test: compose, scenario, log checks
# --------------------------------------------------------------------------


def compose_env(project, port, image, password, dockerfile="Dockerfile"):
    return {
        **os.environ,
        "E2E_PROJECT": project,
        "E2E_IMAGE": image,
        "E2E_PORT": str(port),
        "E2E_DB_PASSWORD": password,
        "E2E_ROOTLESS_DOCKERFILE": dockerfile,
    }


def compose_local(env, *args):
    return subprocess.run(["docker", "compose", "-f", COMPOSE, *args], env=env, text=True,
                          capture_output=("-" in args or "logs" in args))


def compose_remote(ssh, env, *args, check_rc=True):
    """`docker compose …` on the droplet, over ssh."""
    variables = " ".join(
        f"{name}={env[name]}"
        for name in ("E2E_PROJECT", "E2E_IMAGE", "E2E_PORT", "E2E_DB_PASSWORD", "E2E_ROOTLESS_DOCKERFILE")
    )
    command = f"cd /root/blastit && env {variables} docker compose -f e2e/rootless/compose.yml {' '.join(args)}"
    return ssh_run(ssh, command, check_rc=check_rc)


def scenario(port, timeout=180):
    """The shared e2e scenario, against the app on `port`."""
    return subprocess.run(
        [sys.executable, os.path.join(ROOT, "e2e", "scenario.py"),
         f"http://127.0.0.1:{port}", "--expect", "blastit", "--wait", str(timeout)],
        text=True,
    ).returncode


def check_rootless_logs(path, password):
    """D6's own log checks, on top of the shared ones."""
    rc = subprocess.run(
        [sys.executable, os.path.join(ROOT, "e2e", "check_logs.py"), path],
        env={**os.environ, "E2E_SECRET_VAR": "E2E_DB_PASSWORD", "E2E_DB_PASSWORD": password},
        text=True,
    ).returncode
    events = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    events.append(json.loads(line))
                except ValueError:
                    pass

    print("D6: the boot event says which mode init runs in")
    ok = True
    stage0 = [e for e in events if e.get("event") == "boot" and e.get("stage") == "stage0"]
    preload = [e for e in events if e.get("event") == "boot" and e.get("stage") == "preload"]
    ok &= check(len(stage0) == 1, f"one stage 0 boot event ({len(stage0)})")
    for event, where in ((stage0[0] if stage0 else {}), "stage 0"), \
                        ((preload[0] if preload else {}), "preload"):
        ok &= check(event.get("privilege") == "capabilities",
                    f"the {where} boot event says `privilege: capabilities` ({event.get('privilege')})")
    reported = (stage0[0].get("capabilities") if stage0 else None) or []
    ok &= check(reported == CAPABILITIES,
                f"and names exactly the capabilities it holds ({reported})")

    print("D6: the sandboxes are unchanged")
    spawned = [e for e in events if e.get("event") == "process.spawned"]
    ok &= check(bool(spawned), f"view processes were forked ({len(spawned)})")
    ok &= check(all(e.get("uid", 0) >= 20_000 for e in spawned),
                f"every view runs under a uid of its own, above the layout base "
                f"({sorted({e.get('uid') for e in spawned})})")
    ok &= check(any(e.get("seccomp") is True for e in events),
                "the seccomp filters are on in the boot event")
    return rc | (0 if ok else 1)


def dump_logs(text, out):
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, "rootless-app.log")
    with open(path, "w") as f:
        f.write(text)
    return path


# --------------------------------------------------------------------------
# local run
# --------------------------------------------------------------------------


def local(args):
    password = "e2e-" + secrets.token_hex(16)
    project = args.project or "blastit-d6-rootless"
    image = args.image or "blastit-d6-rootless:local"
    env = compose_env(project, args.port, image, password, args.dockerfile)
    rc = 1
    try:
        log(f"docker compose up --build ({project}, port {args.port})")
        if compose_local(env, "up", "--build", "--detach", "app").returncode != 0:
            compose_local(env, "logs", "--no-color", "--tail", "80", "app")
            return 1
        rc = scenario(args.port)
        logs = subprocess.run(["docker", "compose", "-f", COMPOSE, "logs", "--no-color",
                               "--no-log-prefix", "app"], env=env, text=True,
                              capture_output=True).stdout
        rc |= check_rootless_logs(dump_logs(logs, args.out), password)
        caps = subprocess.run(["docker", "compose", "-f", COMPOSE, "exec", "-T", "app", "sh", "-c",
                               'getcap "$(readlink -f "$(command -v python3)")"'], env=env,
                              text=True, capture_output=True)
        log(f"the interpreter's file capabilities: {caps.stdout.strip() or caps.stderr.strip()}")
        rc |= 0 if "cap_setuid" in caps.stdout and "cap_setpcap" in caps.stdout else 1
        limits = subprocess.run(
            ["docker", "inspect", "-f", "User={{.Config.User}} CapDrop={{.HostConfig.CapDrop}} "
             "CapAdd={{.HostConfig.CapAdd}}",
             subprocess.run(["docker", "compose", "-f", COMPOSE, "ps", "-q", "app"], env=env,
                            text=True, capture_output=True).stdout.strip()],
            text=True, capture_output=True)
        log(f"the app container's limits: {limits.stdout.strip()}")
    finally:
        if rc:
            compose_local(env, "logs", "--no-color", "--tail", "200", "app")
        subprocess.run(["docker", "compose", "-f", COMPOSE, "down", "--volumes",
                        "--remove-orphans"], env=env, text=True, capture_output=True)
    return rc


# --------------------------------------------------------------------------
# droplet run
# --------------------------------------------------------------------------


def names(args):
    """The droplet and SSH key name a run uses."""
    name = args.name or f"{PREFIX}{os.getpid()}"
    key_name = name if name.startswith(PREFIX) else PREFIX + name
    return name, key_name


def ssh_key_path(key_name):
    return os.path.join("/tmp/opencode", f"{key_name}_ed25519")


def generate_key(key_name):
    path = ssh_key_path(key_name)
    run(["rm", "-f", path, path + ".pub"])
    run(["ssh-keygen", "-t", "ed25519", "-N", "", "-C", key_name, "-f", path])
    log(f"generated an SSH key at {path}")
    return path


def ssh_argv(key, ip):
    return ["ssh", "-i", key, "-o", "StrictHostKeyChecking=no", "-o",
            "UserKnownHostsFile=/dev/null", "-o", "IdentitiesOnly=yes", "-o",
            "ConnectTimeout=15", "-o", "ServerAliveInterval=30", "-o", "LogLevel=ERROR",
            f"root@{ip}"]


def ssh_run(ssh, command, *, check_rc=False, timeout=None, stdin=None):
    return subprocess.run([*ssh, command], text=True, capture_output=True, timeout=timeout,
                          stdin=stdin, check=False)


def wait_for_ssh(ssh, timeout=300):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ssh_run(ssh, "true").returncode == 0:
            return True
        time.sleep(5)
    return False


def cloud_init():
    """Docker from Docker's own repository, before anything else runs."""
    return """#cloud-config
package_update: true
runcmd:
  - [ sh, -c, "curl -fsSL https://get.docker.com | sh" ]
  - [ systemctl, enable, --now, docker ]
"""


def droplet_image(args):
    """The image to create the droplet with.

    A DigitalOcean Docker image saves the cloud-init step, but the only one on
    offer here is `docker-20-04` (Ubuntu 22.04, an old Docker and a compose v1),
    so a current Ubuntu with Docker installed by cloud-init is the safer
    default: the compose file uses syntax only compose v2 understands.
    """
    if args.droplet_image:
        return args.droplet_image
    images = doctl("compute", "image", "list", "--public") or []
    modern = sorted(
        str(image.get("slug", ""))
        for image in images
        if str(image.get("slug", "")).startswith("docker-")
        and ("24.04" in str(image.get("name", "")) or "26.04" in str(image.get("name", "")))
    )
    if modern:
        log(f"droplet image: {modern[-1]} (Docker and Compose are already there)")
        return modern[-1]
    log("droplet image: ubuntu-24-04-x64 (Docker is installed by cloud-init)")
    return "ubuntu-24-04-x64"


def require_clean():
    """The droplet run ships `git archive HEAD`, so HEAD has to be the whole
    story."""
    dirty = run(["git", "status", "--porcelain"], cwd=ROOT).stdout.strip()
    if dirty:
        raise RuntimeError(
            "the worktree is dirty, and the droplet run ships `git archive HEAD`: "
            f"commit first\n{dirty}"
        )


def ship_checkout(ssh):
    """`git archive HEAD` into `/root/blastit` on the droplet."""
    with tempfile.TemporaryDirectory() as tmp:
        archive = os.path.join(tmp, "blastit.tar")
        with open(archive, "wb") as out:
            run(["git", "archive", "--format=tar", "HEAD"], cwd=ROOT, stdout=out, check_rc=False)
        remote = subprocess.run([*ssh, "rm -rf /root/blastit && mkdir -p /root/blastit && "
                                        "tar -x -C /root/blastit"], stdin=open(archive, "rb"),
                                check=False)
        if remote.returncode != 0:
            raise RuntimeError("shipping the checkout failed")
    commit = run(["git", "rev-parse", "HEAD"], cwd=ROOT).stdout.strip()
    log(f"commit {commit} is on the droplet")


def droplet(args):
    name, key_name = names(args)
    require_clean()
    key = generate_key(key_name)
    password = "e2e-" + secrets.token_hex(16)
    rc = 1
    try:
        log(f"importing the SSH key {key_name}")
        imported = doctl("compute", "ssh-key", "import", key_name, "--public-key-file", key + ".pub")
        imported = imported[0] if isinstance(imported, list) else imported
        fingerprint = imported["fingerprint"]

        with tempfile.NamedTemporaryFile("w", suffix=".yaml", delete=False) as f:
            f.write(cloud_init())
            user_data = f.name
        image = droplet_image(args)
        log(f"creating the droplet {name} in {REGION} ({image}, {args.droplet_size})")
        try:
            created = doctl("compute", "droplet", "create", name, "--region", REGION,
                            "--size", args.droplet_size, "--image", image,
                            "--ssh-keys", fingerprint, "--tag-name", TAG,
                            "--user-data-file", user_data, "--wait")
        finally:
            os.unlink(user_data)
        created = created[0] if isinstance(created, list) else created
        ip = next((n.get("ip_address") for n in created.get("networks", {}).get("v4", [])
                   if n.get("type") == "public"), None)
        if not ip:
            raise RuntimeError(f"the droplet has no public IPv4: {created.get('networks')}")
        log(f"droplet {created['id']} is up at {ip}")

        ssh = ssh_argv(key, ip)
        if not wait_for_ssh(ssh):
            raise TimeoutError(f"ssh to {ip} never worked")
        log("waiting for cloud-init (Docker)")
        ssh_run(ssh, "cloud-init status --wait || true", timeout=1800)
        version = ssh_run(ssh, "docker version --format '{{.Server.Version}}'")
        log(f"docker on the droplet: {version.stdout.strip() or version.stderr.strip()}")

        ship_checkout(ssh)

        env = compose_env(args.project or "blastit-d6-rootless", args.port,
                          args.image or "blastit-d6-rootless:droplet", password)
        log("docker compose up --build (a full Rust build on the droplet, this takes a while)")
        started = time.monotonic()
        up = compose_remote(ssh, env, "up", "--build", "--detach", "app")
        log(f"compose up returned {up.returncode} after {time.monotonic() - started:.0f}s")
        if up.returncode != 0:
            print(up.stdout[-4000:], up.stderr[-4000:])
            compose_remote(ssh, env, "logs", "--no-color", "--tail", "200", "app")
            return 1

        # The app publishes on the droplet's loopback; the scenario runs here,
        # through a tunnel, so nothing is exposed to the internet.
        log(f"tunnelling 127.0.0.1:{args.port} from the droplet")
        tunnel = subprocess.Popen(
            [*ssh_argv(key, ip), "-N", "-L", f"127.0.0.1:{args.port}:127.0.0.1:{args.port}",
             "-o", "ExitOnForwardFailure=yes"])
        try:
            wait_http(args.port)
            rc = scenario(args.port)
            logs = compose_remote(ssh, env, "logs", "--no-color", "--no-log-prefix", "app").stdout
            rc |= check_rootless_logs(dump_logs(logs, args.out), password)
            caps = ssh_run(ssh, 'getcap "$(readlink -f "$(command -v python3)")"')
            log(f"the interpreter's file capabilities on the droplet: {caps.stdout.strip()}")
            rc |= 0 if "cap_setuid" in caps.stdout and "cap_setpcap" in caps.stdout else 1
            limits = ssh_run(ssh, "docker inspect -f User={{.Config.User}} "
                                  "CapDrop={{.HostConfig.CapDrop}} CapAdd={{.HostConfig.CapAdd}} "
                                  "CapAddAmbient={{.HostConfig.CapAddAmbient}} "
                                  "$(docker compose -f /root/blastit/e2e/rootless/compose.yml ps -q app)")
            log(f"the app container's limits: {limits.stdout.strip()}")
            log(f"the run said {'PASS' if rc == 0 else 'FAIL'}")
        finally:
            tunnel.terminate()
            tunnel.wait()
    except KeyboardInterrupt:
        log("interrupted")
        rc = 130
    except Exception as e:  # noqa: BLE001
        log(f"FAILED: {type(e).__name__}: {e}")
        rc = 1
    finally:
        if args.keep:
            log(f"--keep: leaving {name} and the key {key_name}; delete with "
                f"`run.py --cleanup --name {name}`")
        else:
            destroy(name, key_name)
    return rc


def wait_http(port, timeout=900):
    """The app answers its health path through the tunnel."""
    url = f"http://127.0.0.1:{port}/_blastit/health"
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=10) as response:
                if response.status == 200:
                    log(f"{url} answers 200")
                    return
        except Exception:  # noqa: BLE001
            pass
        time.sleep(3)
    raise TimeoutError(f"{url} never answered")


# --------------------------------------------------------------------------
# cleanup and janitor
# --------------------------------------------------------------------------


def find_droplet(name):
    for droplet in doctl("compute", "droplet", "list", "--tag-name", TAG) or []:
        if droplet.get("name") == name:
            return droplet
    return None


def find_key(key_name):
    for key in doctl("compute", "ssh-key", "list") or []:
        if key.get("name") == key_name:
            return key
    return None


def destroy(name, key_name):
    """Delete the droplet and the key, then say what is left."""
    droplet = find_droplet(name)
    if droplet is None:
        log(f"no droplet named {name}")
    else:
        log(f"deleting droplet {droplet['id']} ({name})")
        doctl("compute", "droplet", "delete", str(droplet["id"]), "--force", parse=False, check_rc=False)
    key = find_key(key_name)
    if key is None:
        log(f"no SSH key named {key_name}")
    else:
        log(f"deleting SSH key {key['id']} ({key_name})")
        doctl("compute", "ssh-key", "delete", str(key["id"]), "--force", parse=False, check_rc=False)
    verify_gone(name, key_name)


def verify_gone(name, key_name):
    """Say, loudly, whether anything of this run is left behind.

    DigitalOcean's list calls are eventually consistent, so a deletion is
    confirmed over a few seconds rather than once.
    """
    for attempt in range(6):
        left = [d["name"] for d in doctl("compute", "droplet", "list", "--tag-name", TAG) or []
                if d.get("name") == name]
        keys = [k["name"] for k in doctl("compute", "ssh-key", "list") or []
                if k.get("name") == key_name]
        if not left and not keys:
            log(f"verified: neither the droplet nor the key named {name} exists")
            return
        time.sleep(2 + attempt)
    print(f"!! still there after cleanup: droplets={left} keys={keys}", flush=True)


def janitor(minutes):
    """Delete the resources this script leaves behind.

    Droplets are the expensive resource: anything tagged `blastit-d6` that is
    older than `--older-than-minutes` goes, whichever run created it. An SSH key
    goes with the droplet of the same name: doctl does not report when a key was
    created, so the safe rule is that a `blastit-d6-` key is deleted when no
    surviving droplet carries its name.
    """
    now = dt.datetime.now(dt.timezone.utc)

    def old(created):
        try:
            stamp = dt.datetime.fromisoformat(created.replace("Z", "+00:00").rstrip(" "))
        except (AttributeError, ValueError):
            return True
        return (now - stamp).total_seconds() > minutes * 60

    droplets = doctl("compute", "droplet", "list", "--tag-name", TAG) or []
    surviving = set()
    for droplet in droplets:
        if old(droplet.get("created_at", "")):
            log(f"stale droplet {droplet['name']} ({droplet['id']}, created {droplet.get('created_at')})")
            doctl("compute", "droplet", "delete", str(droplet["id"]), "--force", parse=False, check_rc=False)
        else:
            surviving.add(droplet.get("name"))
    for key in doctl("compute", "ssh-key", "list") or []:
        key_name = key.get("name", "")
        if key_name.startswith(PREFIX) and key_name not in surviving:
            log(f"stale SSH key {key_name} ({key['id']})")
            doctl("compute", "ssh-key", "delete", str(key["id"]), "--force", parse=False, check_rc=False)
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--local", action="store_true", help="run on this machine with Docker Compose")
    ap.add_argument("--droplet", action="store_true", help="create a droplet, test it, destroy it")
    ap.add_argument("--cleanup", action="store_true", help="delete --name's droplet and key")
    ap.add_argument("--janitor", action="store_true", help="delete stale blastit-d6 resources")
    ap.add_argument("--older-than-minutes", type=int, default=120)
    ap.add_argument("--name", help=f"droplet name (default {PREFIX}<pid>)")
    ap.add_argument("--project", help="compose project name")
    ap.add_argument("--image", help="image tag for the app service")
    ap.add_argument("--dockerfile", default="Dockerfile", help="Dockerfile under e2e/rootless/")
    ap.add_argument("--port", type=int, default=int(os.environ.get("E2E_PORT", "18080")))
    ap.add_argument("--droplet-size", default=SIZE)
    ap.add_argument("--droplet-image", help="droplet image slug (default: the newest docker-* one)")
    ap.add_argument("--context", help="doctl auth context (local runs)")
    ap.add_argument("--keep", action="store_true", help="do not delete the droplet (debugging)")
    ap.add_argument("--out", default=os.path.join(HERE, "out"))
    args = ap.parse_args()
    if args.context:
        CONTEXT.extend(["--context", args.context])
    if args.janitor:
        return janitor(args.older_than_minutes)
    if args.cleanup:
        name, key_name = names(args)
        destroy(name, key_name)
        return 0
    if args.local and args.droplet:
        ap.error("--local and --droplet are two different runs")
    if args.droplet:
        return droplet(args)
    return local(args)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(130))
    signal.signal(signal.SIGINT, lambda *_: sys.exit(130))
    sys.exit(main())
