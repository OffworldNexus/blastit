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

E1: the compose file puts Traefik v3 in front of the app and starts blastit with
`BLASTIT_PRESET=compose-traefik`, so this driver also checks the preset's own
claims: the boot event names it and the settings it supplied, `REMOTE_ADDR` is
the client Traefik saw (and not a header the client forged), and
`wsgi.url_scheme` follows Traefik's `X-Forwarded-Proto` — `http` over its
plaintext entry point, `https` behind its TLS one.

Authentication: doctl's (DIGITALOCEAN_ACCESS_TOKEN, or `--context` locally). The
SSH key a droplet is created with is generated in `/tmp/opencode` and imported
under a `blastit-d6-` name, then deleted with the droplet.
"""

import argparse
import datetime as dt
import glob
import json
import os
import secrets
import shlex
import shutil
import signal
import ssl
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
ENV_FILE = ".e2e-compose.env"
# The uids a deployment uses (§2 and D6): init's own, and the one everything
# else that touches the image — a worker, a migration, an operator's shell — must
# use instead, because init's can run the file the capabilities are on.
UID_BASE = 20_000
APP_UID = 10_000
WORKER_UID = 10_001
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
# E1: the preset the compose file gives the app, and the settings it supplies —
# the whole trust decision for this deployment. The driver asserts that the boot
# event reports exactly these, and that no `BLASTIT_*` variable overrode one.
PRESET = "compose-traefik"
PRESET_SETTINGS = {
    "BLASTIT_CLIENT_IP_HEADER": "x-forwarded-for",
    "BLASTIT_TRUSTED_PROXIES": "*",
    "BLASTIT_FAIR_QUEUING": "on",
    "BLASTIT_LOG_FORMAT": "json",
}
CONTEXT = []


def log(msg):
    print(f"[rootless {dt.datetime.now(dt.timezone.utc):%H:%M:%S}] {msg}", flush=True)


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    return bool(cond)


def run(cmd, *, check_rc=True, cwd=None, capture=True, **kw):
    """Run a command, capturing its output unless `capture=False`."""
    p = subprocess.run(cmd, text=True, capture_output=capture, cwd=cwd, **kw)
    if check_rc and p.returncode != 0:
        raise RuntimeError(
            f"{' '.join(str(part) for part in cmd[:6])}… failed ({p.returncode}): "
            f"{(p.stderr or '')[-800:]}"
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


def compose_env(project, port, tls_port, image, password, dockerfile="Dockerfile"):
    """The compose variables.

    On the droplet these are written to an **env file** (over ssh, from stdin),
    so the database password never appears in an `ssh` command line, where `ps`
    would show it.
    """
    return {
        **os.environ,
        "E2E_PROJECT": project,
        "E2E_IMAGE": image,
        "E2E_PORT": str(port),
        "E2E_TLS_PORT": str(tls_port),
        "E2E_DB_PASSWORD": password,
        "E2E_ROOTLESS_DOCKERFILE": dockerfile,
    }



def compose_local(env, *args):
    # `logs` is captured for the checks; everything else streams to the
    # terminal, so a build failure is visible while it happens.
    return subprocess.run(["docker", "compose", "-f", COMPOSE, *args], env=env, text=True,
                          capture_output=("logs" in args))


def compose_remote(ssh, env, *args, check_rc=True):
    """`docker compose …` on the droplet, over ssh, with the env file.

    `--env-file` is what substitutes `${E2E_DB_PASSWORD}`: the value is in the
    file the droplet received over stdin, never in a command line that `ps` on
    the droplet could read.
    """
    command = (f"cd /root/blastit && docker compose --env-file {ENV_FILE} "
               f"-f e2e/rootless/compose.yml {' '.join(args)}")
    return ssh_run(ssh, command, check_rc=check_rc)


def write_env_file(ssh, env):
    """Writes the compose variables to the droplet, from stdin (never argv)."""
    variables = ("E2E_PROJECT", "E2E_IMAGE", "E2E_PORT", "E2E_TLS_PORT", "E2E_DB_PASSWORD",
                 "E2E_ROOTLESS_DOCKERFILE")
    text = "".join(f"{name}={env[name]}\n" for name in variables if name in env)
    written = subprocess.run([*ssh, f"cat > /root/blastit/{ENV_FILE}"], input=text, text=True,
                             capture_output=True, check=False)
    if written.returncode != 0:
        raise RuntimeError(f"writing the env file failed: {written.stderr.strip()}")


def remove_env_file(ssh):
    """Removes the env file (it holds the database password) — best effort."""
    ssh_run(ssh, f"rm -f /root/blastit/{ENV_FILE}")


def scenario(port, timeout=180, remote_addr=None, expect_scheme=None, forged_proto_ignored=False):
    """The shared e2e scenario, against the app on `port`.

    `remote_addr` is what the client address must be — the docker network's
    gateway, here, because that is what a published port's clients look like
    from inside the container — and `expect_scheme` is the scheme the ingress's
    own `X-Forwarded-Proto` implies (E1). `forged_proto_ignored` says the
    ingress sets `X-Forwarded-Proto` itself, which this stack's Traefik does
    (measured; see ARCHITECTURE §10's Traefik subsection).
    """
    command = [sys.executable, os.path.join(ROOT, "e2e", "scenario.py"),
               f"http://127.0.0.1:{port}", "--expect", "blastit", "--wait", str(timeout)]
    if remote_addr:
        command += ["--remote-addr", remote_addr]
    if expect_scheme:
        command += ["--expect-scheme", expect_scheme]
    if forged_proto_ignored:
        command += ["--forged-proto-ignored"]
    return subprocess.run(command, text=True).returncode


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
    # D6: init's uid is dedicated, and here nothing else runs as it — the compose
    # gives the worker and the migrations a uid of their own. The warning is for
    # the deployments that forgot, and it must not be a false positive here.
    siblings = [e for e in events
                if e.get("event") == "app.output" and "run as uid" in e.get("line", "")]
    ok &= check(not siblings,
                f"nothing else runs as init's uid at boot ({[e.get('line') for e in siblings]})")
    return rc | (0 if ok else 1)


def check_preset_logs(path):
    """E1's own log checks: the boot event names the preset and its settings."""
    events = read_events(path)
    print("E1: the boot event names the preset the deployment uses")
    ok = True
    boots = [e for e in events if e.get("event") == "boot"]
    named = [e for e in boots if e.get("preset") == PRESET]
    ok &= check(bool(named), f"a boot event says `preset: {PRESET}` "
                             f"({[e.get('stage') for e in boots]})")
    for stage in ("config", "preload"):
        event = next((e for e in boots if e.get("stage") == stage), {})
        ok &= check(event.get("preset") == PRESET,
                    f"the {stage} boot event names it ({event.get('preset')})")
        ok &= check(event.get("preset_settings") == PRESET_SETTINGS,
                    f"and the settings it supplied ({event.get('preset_settings')})")
        ok &= check(not event.get("preset_overridden"),
                    f"with nothing overriding them ({event.get('preset_overridden')})")
    return 0 if ok else 1


def read_events(path):
    """Every JSON event in a log file, skipping lines that are not events."""
    events = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except ValueError:
                pass
    return events


def tls_request_probe(port):
    """The same probe through Traefik's TLS entry point.

    Certificate verification is off: Traefik generates a self-signed certificate
    and this run is about which *scheme* reaches the app, not about the chain.
    """
    context = ssl._create_unverified_context()
    url = f"https://127.0.0.1:{port}/debug/request/"
    with urllib.request.urlopen(url, context=context, timeout=30) as response:
        return json.loads(response.read().decode())


def check_proxy_headers(tls_port, gateway):
    """E1: the ingress decides the scheme and the client address, not the client."""
    print("E1: the ingress decides the scheme and the client address")
    ok = True
    try:
        facts = tls_request_probe(tls_port)
    except Exception as e:  # noqa: BLE001
        return 0 if check(False, f"Traefik's TLS entry point answers ({e!r})") else 1
    ok &= check(facts.get("scheme") == "https",
                f"a request Traefik TLS-terminated is `https` to the app "
                f"({facts.get('scheme')})")
    ok &= check(facts.get("remote_addr") == gateway,
                f"and the client address is the one Traefik saw "
                f"({facts.get('remote_addr')}, expected {gateway})")
    ok &= check(facts.get("route") == "debug", f"the probe ran on the debug route ({facts})")
    return 0 if ok else 1


def network_gateway(env, project, ssh=None):
    """The docker network's gateway address.

    A connection from the host to a published port arrives in the container from
    the bridge's gateway, so this is the address Traefik sees for the scenario's
    client — what `REMOTE_ADDR` must be (E1).
    """
    name = f"{project}_default"
    if ssh is None:
        p = subprocess.run(["docker", "network", "inspect", name, "-f", GATEWAY_FORMAT],
                           env=env, text=True, capture_output=True)
    else:
        p = ssh_run(ssh, f"docker network inspect {name} -f {shlex.quote(GATEWAY_FORMAT)}")
    gateway = p.stdout.strip()
    if not gateway:
        raise RuntimeError(f"no gateway for the network {name}: {p.stderr.strip()[-400:]}")
    return gateway


GATEWAY_FORMAT = "{{(index .IPAM.Config 0).Gateway}}"


def dump_logs(text, out):
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, "rootless-app.log")
    with open(path, "w") as f:
        f.write(text)
    return path


# --------------------------------------------------------------------------
# the capabilities of the *processes*, read out of `/proc` inside the container
# --------------------------------------------------------------------------

# Prints one line per process of the container, except init: id, uid, the five
# capability sets and `no_new_privs` (§7.2, D6). `awk` over `/proc/<pid>/status`
# is readable across uids on Linux (`/proc` is world-readable; under gVisor it
# always is, §9.1 #3), which is what makes this checkable from outside the
# server.
PROCESS_PROBE = r"""
for dir in /proc/[0-9]*; do
  pid=${dir#/proc/}
  [ "$pid" = 1 ] && continue
  awk -v pid="$pid" '
    /^Uid:/ {uid = $2}
    /^CapInh:/ {inh = $2}
    /^CapPrm:/ {prm = $2}
    /^CapEff:/ {eff = $2}
    /^CapBnd:/ {bnd = $2}
    /^CapAmb:/ {amb = $2}
    /^NoNewPrivs:/ {nnp = $2}
    END {printf "%s %s %s %s %s %s %s %s\n", pid, uid, inh, prm, eff, bnd, amb, nnp}
  ' "$dir/status" 2>/dev/null
done
"""

# The capability sets and the uid of *this* process, plus whether it can become
# root: the B1 check, for a process that is not init (a plain `python3`, a second
# `manage.py`). Written to a file inside the container and run from there — a
# `-c` one-liner cannot hold the `try` that `setuid(0)` needs.
SELF_PROBE = """\
import os

caps = {
    line.split(":")[0]: line.split(":")[1].strip()
    for line in open("/proc/self/status")
    if line.startswith(("Cap", "NoNewPrivs"))
}


def attempt():
    try:
        os.setuid(0)
        return "ok"
    except OSError as error:
        return "refused"


print(
    "probe",
    os.getuid(),
    caps["CapInh"],
    caps["CapPrm"],
    caps["CapEff"],
    caps["CapBnd"],
    caps["CapAmb"],
    caps["NoNewPrivs"],
    attempt(),
)
"""

ZERO = "0000000000000000"


def check_container(env, project, ssh=None):
    """D6's checks inside the running container: who holds what.

    * every process of the container that runs under a **layout** uid — http, the
      scheduler, dbproxy and every view (`§2`) — holds no capability at all, not
      even in its bounding set, and has `no_new_privs` set;
    * the three component uids are among them;
    * a plain `python3`, and a second `manage.py`, run as the service uid hold
      nothing and cannot become root: the *shared* interpreter has no file
      capabilities, only the dedicated copy init starts from does (B1).

    Processes outside the layout — root, the service uid, and this check's own
    shell — are not asserted on: the service uid's own processes are what the two
    probes above are for, and `docker compose exec` starts a process of that uid
    that is not init and has no `no_new_privs`, which is the point (it is the
    interpreter, not the uid, that must stay capability-free).
    """
    print("D6: what the container's own processes hold")
    ok = True

    def in_container(script):
        if ssh is None:
            return subprocess.run(["docker", "compose", "-f", COMPOSE, "exec", "-T", "app",
                                   "sh", "-c", script], env=env, text=True,
                                  capture_output=True).stdout
        return compose_remote(ssh, env, "exec", "-T", "app", "sh", "-c",
                              shlex.quote(script)).stdout

    def as_uid(uid, script):
        """The same, as another uid: what the container lets a foreign process do."""
        if ssh is None:
            return subprocess.run(["docker", "compose", "-f", COMPOSE, "exec", "-T", "-u",
                                   str(uid), "app", "sh", "-c", script], env=env, text=True,
                                  capture_output=True).stdout
        return compose_remote(ssh, env, "exec", "-T", "-u", str(uid), "app", "sh", "-c",
                              shlex.quote(script)).stdout

    def run_probe(what, command, uid):
        """Writes the probe into the container, runs it as `uid`, reads the line."""
        script = (f"cat > /tmp/blastit-probe.py <<'PROBE'\n{SELF_PROBE}PROBE\n{command}\n")
        out = as_uid(uid, script)
        line = next((text for text in out.splitlines() if text.startswith("probe ")), "")
        fields = line.split()
        if len(fields) != 9:
            return check(False, f"{what} did not report: {out.strip()[:200]}")
        _, uid, inh, prm, eff, bnd, amb, nnp, setuid0 = fields
        # The shared interpreter grants nothing: a process started from it holds
        # no capability and cannot become root. It is *not* protected by its uid —
        # `no_new_privs` is not set on it (nothing set one) and it has the
        # container's bounding set — which is why init's uid is dedicated (D6).
        held = (inh, prm, eff, amb) == (ZERO,) * 4
        return check(
            held and setuid0 == "refused",
            f"{what} (uid {uid}) holds nothing and cannot become root "
            f"(inh={inh} prm={prm} eff={eff} amb={amb} nnp={nnp} setuid={setuid0})",
        )

    seen = {}
    for line in in_container(PROCESS_PROBE).splitlines():
        fields = line.split()
        if len(fields) != 8:
            continue
        pid, uid, inh, prm, eff, bnd, amb, nnp = fields
        seen[int(uid)] = (pid, inh, prm, eff, bnd, amb, nnp)
    layout = {uid: state for uid, state in seen.items() if uid >= UID_BASE}
    ok &= check(len(layout) >= 4, f"the layout runs more than one process ({len(layout)})")
    for uid, (pid, inh, prm, eff, bnd, amb, nnp) in sorted(layout.items()):
        ok &= check(
            (inh, prm, eff, bnd, amb) == (ZERO,) * 5 and nnp == "1",
            f"uid {uid} (pid {pid}) holds nothing and has no_new_privs "
            f"(inh={inh} prm={prm} eff={eff} bnd={bnd} amb={amb} nnp={nnp})",
        )
    for uid, what in ((UID_BASE + 1, "http"), (UID_BASE + 2, "scheduler"),
                      (UID_BASE + 3, "dbproxy")):
        ok &= check(uid in layout, f"{what} runs under uid {uid} (D6 keeps the layout)")

    # A worker, a migration and a shell all run as their own uid (the compose has
    # the `migrate` service on it), and that uid is the one the checks below use:
    # what protects the capable copy is that *they* cannot run it, not that they
    # are somehow capability-free.
    ok &= run_probe(f"a plain python3, as the worker's uid ({WORKER_UID})",
                    "python3 /tmp/blastit-probe.py", WORKER_UID)
    ok &= run_probe(f"a manage.py, as the worker's uid ({WORKER_UID})",
                    'python3 manage.py shell -c "$(cat /tmp/blastit-probe.py)"', WORKER_UID)

    # The capable copy is init's alone: mode `0500`, owned by the uid init runs
    # as, runnable by that uid, refused to every other one (B1, D6 — the uid is
    # dedicated precisely because it *can* run it).
    facts = in_container(
        'stat -c "mode=%a uid=%u" "$(readlink -f "$(command -v python3)")-init"')
    ok &= check(f"mode=500 uid={APP_UID}" in facts,
                f"the capable copy is mode 0500 and owned by init's uid ({facts.strip()})")
    runnable = as_uid(APP_UID,
                      'copy="$(readlink -f "$(command -v python3)")-init"; "$copy" -V 2>&1 || true')
    ok &= check("Permission denied" not in runnable,
                f"init's own uid can run it — which is why nothing else may use that uid "
                f"({runnable.strip()})")
    refused = as_uid(WORKER_UID,
                     'copy="$(readlink -f "$(command -v python3)")-init"; "$copy" -V 2>&1 || true')
    ok &= check("Permission denied" in refused,
                f"the worker's uid cannot run it ({refused.strip()})")
    return 0 if ok else 1


# --------------------------------------------------------------------------
# local run
# --------------------------------------------------------------------------


def caps_and_limits(env, ssh=None):
    """The two facts a deployment is about: which file has the capabilities, and
    what the container was allowed to give it (D6/B1)."""
    if ssh is None:
        caps = subprocess.run(
            ["docker", "compose", "-f", COMPOSE, "exec", "-T", "app", "sh", "-c",
             'getcap "$(readlink -f "$(command -v python3)")-init"; '
             'echo "shared: $(getcap "$(readlink -f "$(command -v python3)")")"'],
            env=env, text=True, capture_output=True)
        limits = subprocess.run(
            ["docker", "inspect", "-f", "User={{.Config.User}} CapDrop={{.HostConfig.CapDrop}} "
             "CapAdd={{.HostConfig.CapAdd}}",
             subprocess.run(["docker", "compose", "-f", COMPOSE, "ps", "-q", "app"], env=env,
                            text=True, capture_output=True).stdout.strip()],
            text=True, capture_output=True)
    else:
        caps = compose_remote(
            ssh, env, "exec", "-T", "app", "sh", "-c",
            shlex.quote('getcap "$(readlink -f "$(command -v python3)")-init"; '
                        'echo "shared: $(getcap "$(readlink -f "$(command -v python3)")")"'))
        limits = ssh_run(ssh, f"cd /root/blastit && docker compose --env-file {ENV_FILE} "
                              "-f e2e/rootless/compose.yml ps -q app | "
                              "xargs docker inspect -f 'User={{.Config.User}} "
                              "CapDrop={{.HostConfig.CapDrop}} CapAdd={{.HostConfig.CapAdd}}'")
    capabilities = caps.stdout.strip()
    log(f"the dedicated interpreter's file capabilities: {capabilities}")
    ok = "cap_setuid" in capabilities and "cap_setpcap" in capabilities
    # B1: the *shared* interpreter must have none — every process of that uid can
    # run it, and the capabilities would turn any of them into init.
    shared = next((line for line in caps.stdout.splitlines() if line.startswith("shared:")), "")
    ok &= "cap_" not in shared
    if not ok:
        print(f"  FAIL the capabilities are on the wrong file:\n{caps.stdout}{caps.stderr}")
    log(f"the app container's limits: {limits.stdout.strip()}")
    return 0 if ok else 1


def local(args):
    password = "e2e-" + secrets.token_hex(16)
    project = args.project or "blastit-d6-rootless"
    image = args.image or "blastit-d6-rootless:local"
    env = compose_env(project, args.port, args.tls_port, image, password, args.dockerfile)
    rc = 1
    try:
        log(f"docker compose up --build ({project}, port {args.port})")
        # Every service, not just `app`: Traefik depends on the app, not the
        # other way round, and `up app` would leave the ingress out — which is
        # the only way in (E1).
        if compose_local(env, "up", "--build", "--detach").returncode != 0:
            compose_local(env, "logs", "--no-color", "--tail", "80", "app")
            return 1
        gateway = network_gateway(env, project)
        log(f"the compose network's gateway: {gateway}")
        rc = scenario(args.port, remote_addr=gateway, expect_scheme="http",
                      forged_proto_ignored=True)
        logs = subprocess.run(["docker", "compose", "-f", COMPOSE, "logs", "--no-color",
                               "--no-log-prefix", "app"], env=env, text=True,
                              capture_output=True).stdout
        path = dump_logs(logs, args.out)
        rc |= check_rootless_logs(path, password)
        rc |= check_preset_logs(path)
        rc |= check_proxy_headers(args.tls_port, gateway)
        rc |= caps_and_limits(env)
        rc |= check_container(env, project)
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


def generate_key(key_name):
    """Generates the run's SSH key in a directory of its own.

    A `TemporaryDirectory` the driver removes when it is done (M2): the private
    key of a droplet that no longer exists has no reason to sit in `/tmp`
    forever. `--keep` prints the path instead, because the droplet it opens still
    does.
    """
    directory = tempfile.mkdtemp(prefix=f"{key_name}-")
    path = os.path.join(directory, "id_ed25519")
    run(["ssh-keygen", "-t", "ed25519", "-N", "", "-C", key_name, "-f", path])
    log(f"generated an SSH key in {directory}")
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
            subprocess.run(["git", "archive", "--format=tar", "HEAD"], cwd=ROOT, stdout=out,
                           check=True)
        with open(archive, "rb") as source:
            remote = subprocess.run(
                [*ssh, "rm -rf /root/blastit && mkdir -p /root/blastit && tar -x -C /root/blastit"],
                stdin=source, check=False)
        if remote.returncode != 0:
            raise RuntimeError("shipping the checkout failed")
    commit = run(["git", "rev-parse", "HEAD"], cwd=ROOT).stdout.strip()
    log(f"commit {commit} is on the droplet")


def droplet(args):
    name, key_name = names(args)
    require_clean()
    password = "e2e-" + secrets.token_hex(16)
    ssh = None
    key = None
    rc = 1
    try:
        # Inside the `try`: a key that was generated and then failed to import is
        # the `finally`'s to remove too (M2).
        key = generate_key(key_name)
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

        env = compose_env(args.project or "blastit-d6-rootless", args.port, args.tls_port,
                          args.image or "blastit-d6-rootless:droplet", password)
        write_env_file(ssh, env)
        log("docker compose up --build (a full Rust build on the droplet, this takes a while)")
        started = time.monotonic()
        up = compose_remote(ssh, env, "up", "--build", "--detach")
        log(f"compose up returned {up.returncode} after {time.monotonic() - started:.0f}s")
        if up.returncode != 0:
            print(up.stdout[-4000:], up.stderr[-4000:])
            compose_remote(ssh, env, "logs", "--no-color", "--tail", "200", "app")
            return 1

        # The app publishes on the droplet's loopback; the scenario runs here,
        # through a tunnel, so nothing is exposed to the internet.
        log(f"tunnelling 127.0.0.1:{args.port} and {args.tls_port} from the droplet")
        tunnel = subprocess.Popen(
            [*ssh_argv(key, ip), "-N",
             "-L", f"127.0.0.1:{args.port}:127.0.0.1:{args.port}",
             "-L", f"127.0.0.1:{args.tls_port}:127.0.0.1:{args.tls_port}",
             "-o", "ExitOnForwardFailure=yes"])
        try:
            wait_http(args.port)
            gateway = network_gateway(env, env["E2E_PROJECT"], ssh)
            log(f"the compose network's gateway on the droplet: {gateway}")
            rc = scenario(args.port, remote_addr=gateway, expect_scheme="http",
                          forged_proto_ignored=True)
            logs = compose_remote(ssh, env, "logs", "--no-color", "--no-log-prefix", "app").stdout
            path = dump_logs(logs, args.out)
            rc |= check_rootless_logs(path, password)
            rc |= check_preset_logs(path)
            rc |= check_proxy_headers(args.tls_port, gateway)
            # The mechanisms, read out of the containers rather than out of the
            # log: which file has the capabilities, what the container was
            # allowed to give it, and what every process of the container holds.
            rc |= caps_and_limits(env, ssh)
            rc |= check_container(env, env["E2E_PROJECT"], ssh)
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
            log(f"--keep: leaving {name}, the key {key_name} and the key file "
                f"({os.path.dirname(key) if key else 'none was generated'}); delete with "
                f"`run.py --cleanup --name {name}`")
        else:
            # M2: the droplet, the key and the private key file all go together
            # (`destroy` removes the file), and the env file — which holds the
            # database password — with them.
            if ssh is not None:
                remove_env_file(ssh)
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
    """Delete the droplet, the key, and any private key file left for them."""
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
    # M2: the private key of a droplet that no longer exists has no reason to
    # outlive it (`--keep` is the caller's business, and it does not call this).
    for directory in glob.glob(os.path.join(tempfile.gettempdir(), f"{key_name}-*")):
        shutil.rmtree(directory, ignore_errors=True)
        if os.path.exists(directory):
            log(f"could not remove the private key file ({directory}): it is still there")
        else:
            log(f"removed the private key file ({directory})")
    verify_gone(name, key_name)


def verify_gone(name, key_name):
    """Say, loudly, whether anything of this run is left behind.

    DigitalOcean's list calls are eventually consistent, so a deletion is
    confirmed over a few seconds rather than once.
    """
    for attempt in range(10):
        left = [d["name"] for d in doctl("compute", "droplet", "list", "--tag-name", TAG) or []
                if d.get("name") == name]
        keys = [k["name"] for k in doctl("compute", "ssh-key", "list") or []
                if k.get("name") == key_name]
        if not left and not keys:
            log(f"verified: neither the droplet nor the key named {name} exists")
            return
        time.sleep(5)
    print(f"!! still there after cleanup: droplets={left} keys={keys}", flush=True)


def janitor(minutes):
    """Delete the resources this script leaves behind.

    Two rules, both conservative about what they cannot see:

    * **a droplet** goes when its age is *known* and older than
      `--older-than-minutes`. An age that cannot be read is a lower bound of
      zero, not a licence to delete: without a `created_at` there is no way to
      tell a crashed run from one that is starting, so the droplet stays;
    * **an SSH key** goes with the droplet of the same name, and only then:
      `doctl compute ssh-key list` reports no creation time, so a key whose
      droplet is being created right now (the driver imports the key *before* the
      droplet exists) is indistinguishable from an orphan. A key of a droplet the
      janitor just deleted is deleted with it; any other is left for
      `--cleanup --name`.
    """
    now = dt.datetime.now(dt.timezone.utc)

    def old(created):
        """Whether `created` is *known* to be older than the cutoff."""
        try:
            stamp = dt.datetime.fromisoformat(str(created).replace("Z", "+00:00").rstrip(" "))
        except (AttributeError, ValueError):
            return False
        if stamp.tzinfo is None:
            return False
        return (now - stamp).total_seconds() > minutes * 60

    droplets = doctl("compute", "droplet", "list", "--tag-name", TAG) or []
    deleted = set()
    for droplet in droplets:
        name = droplet.get("name", "")
        if old(droplet.get("created_at", "")):
            log(f"stale droplet {name} ({droplet['id']}, created {droplet.get('created_at')})")
            doctl("compute", "droplet", "delete", str(droplet["id"]), "--force", parse=False,
                  check_rc=False)
            deleted.add(name)
    if not deleted:
        log("no droplet is known to be older than the cutoff")
    for key in doctl("compute", "ssh-key", "list") or []:
        key_name = key.get("name", "")
        if key_name in deleted:
            log(f"the SSH key of the droplet that went: {key_name} ({key['id']})")
            doctl("compute", "ssh-key", "delete", str(key["id"]), "--force", parse=False,
                  check_rc=False)
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
    ap.add_argument("--port", type=int, default=int(os.environ.get("E2E_PORT", "18080")),
                    help="host port Traefik publishes its entry point on")
    ap.add_argument("--tls-port", type=int,
                    default=int(os.environ.get("E2E_TLS_PORT", "18443")),
                    help="host port Traefik publishes its TLS entry point on (E1)")
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
