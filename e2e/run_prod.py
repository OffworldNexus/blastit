"""The production end-to-end suite: both deployment presets, in real containers.

    uv run --no-project e2e/run_prod.py            # every leg, free ports 28080+

It proves `TESTING_PLAN.md` §6 — the root and rootless deployment stories — with
the shipped artifact, a real PostgreSQL 18, a real PgBouncer, a real Traefik v3
and real PHP workers. Nothing is mocked: every fact below is read from the
kernel (`/proc`), from `docker inspect`, or from the JSON event stream blastit
writes on stdout.

Two legs, one per preset:

* **root** (`--preset digitalocean-paas`, `e2e/apps/prod/Blastfile`): the
  **block** store the DigitalOcean page writes (`DB_HOST`/`DB_PORT`/
  `DB_USERNAME`/`DB_DATABASE`/`DB_PASSWORD`, no URL). Boot stage
  `privilege: root`; per-view OS uids ≥ 20000 read from `process.spawned` *and*
  from `/proc/<pid>/status`; every child holds no capability and has
  `NoNewPrivs: 1`; the view runs under a seccomp filter; no database credential
  in any `/proc/<pid>/environ` (with the scan's own positive control); a view's
  `exec` and outbound `fsockopen` refused by the sandbox while the same code
  paths work through `docker exec`; a trusted `X-Forwarded-Proto` decides the
  scheme; the same statement allowed in one view and refused with SQLSTATE
  `42501` in another; fair queuing answering a second client's fast request
  ahead of a slow client's backlog; and a `SIGTERM` drain that finishes the
  in-flight request and exits `0`.
* **rootless** (`BLASTIT_PRESET=compose-traefik`, `Blastfile.url`): the **URL**
  store the Compose page writes (`DATABASE_URL=postgresql://…@pgbouncer/…`),
  Traefik v3 in front, PgBouncer between dbproxy and PostgreSQL, the seven file
  capabilities on a `0500` `/usr/share/blastit/lib/blastit-init`, no capability
  and `NO_NEW_PRIVS` in every child, uid `10001` refused that copy while `10000`
  runs it, a trusted `X-Forwarded-For`/`X-Forwarded-Proto` believed on a direct
  connection and a forged chain ignored through Traefik, the same refusals and
  sandbox as the root leg, the pool on the path, fair queuing through the
  in-container client, a tampered PHP entry refused before `libphp` opens it,
  and a `SIGTERM` drain of `blastit-init`.

A host fact the suite needs and does not have (Docker, a pullable image, a
binary) is a loud SKIP naming the command that installs it. Stdlib only; the
shared helpers come from `e2e/run_apps.py` so both drivers read the same way.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import shlex
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
APPS = HERE / "apps"
PROD = APPS / "prod"
sys.path.insert(0, str(HERE))
from run_apps import (
    Checks,
    Ports,
    Postgres,
    Summary,
    docker,
    require_docker,
    tail,
)
from universal_binary import LegFailure, Skip, http

WORK_ROOT = Path(os.environ.get("TMPDIR", "/tmp/opencode")) / "blastit-e2e-prod"
PORTS = Ports()

#: The capabilities `compose-traefik` documents, in kernel order (`capabilities(7)`).
CAPABILITIES = [
    "cap_chown",
    "cap_dac_override",
    "cap_fowner",
    "cap_kill",
    "cap_setgid",
    "cap_setuid",
    "cap_setpcap",
]
#: What each preset supplies; `preset_overridden` must stay empty beside it.
PRESET_ROOT = {
    "BLASTIT_CLIENT_IP_HEADER": "do-connecting-ip",
    "BLASTIT_TRUSTED_PROXIES": "*",
    "BLASTIT_FAIR_QUEUING": "on",
    "BLASTIT_LOG_FORMAT": "json",
}
PRESET_ROOTLESS = {
    "BLASTIT_CLIENT_IP_HEADER": "x-forwarded-for",
    "BLASTIT_TRUSTED_PROXIES": "*",
    "BLASTIT_FAIR_QUEUING": "on",
    "BLASTIT_LOG_FORMAT": "json",
}
#: PgBouncer's image, the Traefik version the deploy page pins, and the
#: database; each is pulled only when it is not already local.
POOL_IMAGE = "edoburu/pgbouncer:v1.26.1-p0"
TRAEFIK_IMAGE = "traefik:v3.7.16"
POSTGRES_IMAGE = "postgres:18"

#: One line per container process except init: pid, uid, the five capability
#: sets and `no_new_privs` (ARCHITECTURE §7.2).
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
ZERO = "0000000000000000"
#: The capabilities probe: each path resolved first, because `getcap` on a
#: symlink prints nothing and still exits `0`.
CAPABILITIES_PROBE = (
    'probe() { p=$(readlink -f "$1"); echo "$2: $(stat -c %i "$p" 2>/dev/null || echo MISSING)'
    ' $p $(getcap "$p" 2>/dev/null)"; }; '
    "probe /usr/share/blastit/lib/blastit-init init; "
    "probe /usr/share/blastit/lib/blastit shared; "
    "for shim in /usr/share/blastit/lib/libblastit_*.so"
    " /opt/blastit/shims/libblastit_*.so; "
    'do probe "$shim" shim; done; '
    'probe "$(command -v python3)" python; '
    'stat -c "mode=%a uid=%u" /usr/share/blastit/lib/blastit-init'
)
#: What a tampered entry says, and the decoy that must never run.
ENTRY_CHANGED = "changed since this boot recorded it"
DECOY = (
    '<?php header("Content-Type: application/json");'
    ' echo json_encode(["decoy" => "ran"]), "\\n";' + "\n"
)
TRAEFIK_DYNAMIC = """http:
  routers:
    app:
      rule: PathPrefix(`/`)
      entryPoints: [web]
      service: app
    app-tls:
      rule: PathPrefix(`/`)
      entryPoints: [websecure]
      service: app
      tls: {}
  services:
    app:
      loadBalancer:
        servers:
          - url: http://app:8080
"""


# ---------------------------------------------------------------------------
# containers, events and one HTTP helper
# ---------------------------------------------------------------------------


def exec_(
    name: str,
    script: str,
    user: str = "0",
    env: dict[str, str] | None = None,
    timeout: float = 120,
) -> subprocess.CompletedProcess[str]:
    """`docker exec`: the flags go before the container name, always."""
    arguments = ["exec", "-u", user]
    for key, value in (env or {}).items():
        arguments += ["-e", f"{key}={value}"]
    return docker(*arguments, name, "sh", "-c", script, timeout=timeout)


def start(*arguments: str) -> subprocess.CompletedProcess[str]:
    """`docker run -d`; the caller names every flag it relies on."""
    return docker("run", "-d", *arguments)


def events(name: str) -> list[dict]:
    """Every JSON event the container wrote on stdout, in order."""
    found = []
    for line in docker("logs", name, timeout=60).stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict):
            found.append(event)
    return found


def boot(stream: list[dict], stage: str) -> dict:
    return next(
        (
            event
            for event in stream
            if event.get("event") == "boot" and event.get("stage") == stage
        ),
        {},
    )


def wait_http(
    port: int,
    target: str,
    headers: dict[str, str] | None = None,
    timeout: float = 180,
    expect: int = 200,
) -> tuple[int, bytes]:
    """Polls `target` until it answers `expect`, or fails with the last answer."""
    deadline = time.monotonic() + timeout
    last = "no answer"
    while time.monotonic() < deadline:
        try:
            status, _headers, body = http(port, target, headers=headers, timeout=5)
        except (OSError, LegFailure) as error:
            last = str(error)
            time.sleep(0.25)
            continue
        if status == expect:
            return status, body
        last = f"{status} {body[:160]!r}"
        time.sleep(0.25)
    raise LegFailure(f"http://127.0.0.1:{port}{target} never answered {expect}: {last}")


def document(body: bytes, what: str) -> dict:
    try:
        parsed = json.loads(body)
    except ValueError as error:
        raise LegFailure(f"{what} answered no JSON: {body[:200]!r}") from error
    if not isinstance(parsed, dict):
        raise LegFailure(f"{what} answered {parsed!r}, not an object")
    return parsed


def process_table(name: str) -> list[dict]:
    """Every non-init process's uid, capability sets and `no_new_privs`."""
    rows = []
    for line in exec_(name, PROCESS_PROBE).stdout.splitlines():
        fields = line.split()
        if len(fields) != 8:
            continue
        pid, uid, inh, prm, eff, bnd, amb, nnp = fields
        rows.append(
            {
                "pid": int(pid),
                "uid": int(uid),
                "caps": (inh, prm, eff, bnd, amb),
                "nnp": nnp,
            }
        )
    return rows


def env_hits(name: str, needle: str) -> list[tuple[int, int]]:
    """`(pid, uid)` of every process whose environment contains `needle`.

    `docker exec` inherits the container's configured environment, so the
    variables that carry a secret are cleared for the probe itself: otherwise the
    probe's own shell is the only match and the answer is always "leaked". The
    root container is started with `SYS_PTRACE` (and nothing else) because reading
    a `PR_SET_DUMPABLE(0)` child's environ needs it; the child's own capability
    sets are unaffected.
    """
    script = (
        "for d in /proc/[0-9]*; do pid=${d#/proc/}; "
        f"if tr '\\0' '\\n' < \"$d/environ\" 2>/dev/null | grep -qF -- '{needle}'; then "
        'echo "$pid $(awk \'/^Uid:/ {print $2}\' "$d/status" 2>/dev/null)"; fi; done'
    )
    cleared = {"DATABASE_URL": "", "DATABASE_PASSWORD": "", "DB_PASSWORD": ""}
    rows = []
    for line in exec_(name, script, env=cleared).stdout.splitlines():
        pid, _, uid = line.partition(" ")
        if pid.isdigit() and uid.isdigit():
            rows.append((int(pid), int(uid)))
    return rows


def container_env_has(name: str, secret: str) -> bool:
    """The positive control: the deployment's own configuration carries `secret`."""
    env = docker("inspect", "-f", "{{json .Config.Env}}", name).stdout
    return secret in env


def proc_status(name: str, pid: int) -> dict[str, list[str]]:
    """`Uid`, `Seccomp` and `NoNewPrivs` from a world-readable `/proc/<pid>/status`."""
    out = exec_(name, f"cat /proc/{pid}/status 2>/dev/null").stdout
    fields: dict[str, list[str]] = {}
    for line in out.splitlines():
        key, _, value = line.partition(":")
        if key in ("Uid", "Gid", "Seccomp", "NoNewPrivs"):
            fields[key] = value.split()
    return fields


def proc_state(name: str, pid: int) -> str:
    """The task state from `/proc/<pid>/stat` (`S` is sleeping in `usleep`)."""
    out = exec_(name, f"cat /proc/{pid}/stat 2>/dev/null").stdout
    rest = out.rsplit(")", 1)[-1].split()
    return rest[0] if rest else ""


def proc_wchan(name: str, pid: int) -> str:
    return exec_(name, f"cat /proc/{pid}/wchan 2>/dev/null").stdout.strip()


class Flight:
    """One request in flight on its own thread, its result when it lands."""

    def __init__(self, send, what: str):
        self.send = send
        self.what = what
        self.status = 0
        self.body = b""
        self.error = ""
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _run(self) -> None:
        try:
            self.status, self.body = self.send()
        except (OSError, LegFailure) as error:
            self.error = str(error)

    def start(self) -> Flight:
        self.thread.start()
        return self

    def join(self, timeout: float = 90) -> Flight:
        self.thread.join(timeout=timeout)
        return self


def proxy_request(port: int, target: str, headers: dict[str, str]):
    """A request over the published port (the host client's own connection)."""

    def send() -> tuple[int, bytes]:
        status, _headers, body = http(port, target, headers=headers, timeout=90)
        return status, body

    return send


def direct_http(name: str, target: str, headers: dict[str, str]) -> tuple[int, bytes]:
    """One request from inside the container to blastit's own port.

    The rootless container publishes nothing, so a client that carries its own
    `X-Forwarded-For`/`X-Forwarded-Proto` has to be inside its network namespace.
    """
    lines = [f"GET {target} HTTP/1.0", "Host: app"]
    lines.extend(f"{key}: {value}" for key, value in headers.items())
    payload = "\r\n".join(lines) + "\r\n\r\n"
    php = (
        '$s=@fsockopen("127.0.0.1", 8080, $e, $m, 5); '
        'if (!$s) { echo "ERR $e $m"; exit(0); } '
        f"fwrite($s, {json.dumps(payload)}); "
        "echo (string) stream_get_contents($s); fclose($s);"
    )
    raw = exec_(name, "php -r " + shlex.quote(php), timeout=120).stdout
    # `subprocess(text=True)` translates the response's CRLF to LF, so the head
    # and the body are separated by a blank *line*, not by `\r\n\r\n`.
    head, _, body = raw.partition("\n\n")
    status = 0
    first = head.splitlines()[0] if head else ""
    parts = first.split()
    if len(parts) >= 2 and parts[1].isdigit():
        status = int(parts[1])
    return status, body.encode()


def direct_request(name: str, target: str, headers: dict[str, str]):
    """A [`Flight`] sender for the in-container client."""

    def send() -> tuple[int, bytes]:
        return direct_http(name, target, headers)

    return send


def hits_message(rows: list[tuple[int, int]]) -> str:
    return ", ".join(f"pid {pid} (uid {uid})" for pid, uid in rows)


def denial(checks: Checks, port: int, password: str, target: str, what: str) -> None:
    """One refused statement: `403`, SQLSTATE `42501`, no credential."""
    status, _headers, body = get(port, target)
    checks.that(status == 403, f"{what} is {status}, not the documented 403")
    checks.that(
        document(body, f"GET {target}").get("sqlstate") == "42501",
        f"{what} did not report SQLSTATE 42501: {body[:200]!r}",
    )
    assert_no_secret(checks, body, password, what)


def shell_sandbox(checks: Checks, name: str, port: int) -> None:
    """No process function is callable in the view; `docker exec` can run one."""
    status, _headers, body = get(port, "/shell")
    checks.that(status == 200, f"GET /shell is 200, not {status}")
    checks.that(
        document(body, "GET /shell").get("blocked") is True,
        f"the view started a shell: {body[:200]!r}",
    )
    control = exec_(
        name,
        'php -r \'exec("/usr/bin/id 2>&1", $o, $s); echo implode("\\n", $o);\'',
    )
    checks.that(
        "uid=" in control.stdout,
        f"the positive control could not run `id` ({control.stdout + control.stderr})",
    )


def egress_sandbox(checks: Checks, name: str, port: int, canary: str) -> None:
    """An undeclared TCP peer is refused; the same address is reachable as root."""
    status, _headers, body = get(port, "/egress")
    checks.that(status == 200, f"GET /egress is 200, not {status}")
    egress = document(body, "GET /egress")
    checks.that(
        egress.get("connected") is False,
        f"the view reached an undeclared TCP peer: {body[:200]!r}",
    )
    sentence = f"{egress.get('stream_errstr')} {egress.get('socket_errstr')}".lower()
    checks.that(
        "permitted" in sentence or "denied" in sentence,
        f"the refusal carries no kernel sentence ({egress})",
    )
    checks.that(
        not egress.get("stream_read"),
        f"an allowed connection read the canary's answer ({egress})",
    )
    checks.that(
        b"canary-ok" not in body,
        f"the view read the canary's answer: {body[:200]!r}",
    )

    host, port_text = canary.rsplit(":", 1)
    control_php = (
        f'$s=@fsockopen("{host}", {port_text}, $e, $m, 3); '
        'if (!$s) { echo "ERR $e $m"; exit(0); } '
        'fwrite($s, "GET / HTTP/1.0\\r\\nHost: canary\\r\\n\\r\\n"); '
        "echo (string) stream_get_contents($s); fclose($s);"
    )
    reachable = exec_(name, "php -r " + shlex.quote(control_php))
    checks.that(
        "canary-ok" in reachable.stdout,
        f"the positive control could not reach the canary "
        f"({reachable.stdout + reachable.stderr})",
    )


def assert_no_secret(checks: Checks, body: bytes, password: str, what: str) -> None:
    checks.that(
        password.encode() not in body,
        f"{what}: the database password reached a response",
    )


def get(
    port: int,
    target: str,
    headers: dict[str, str] | None = None,
    timeout: float = 30,
) -> tuple[int, dict[str, str], bytes]:
    try:
        return http(port, target, headers=headers, timeout=timeout)
    except (OSError, LegFailure) as error:
        raise LegFailure(f"GET {target} never answered ({error})") from error


def preset_rows(checks: Checks, stream: list[dict], name: str, settings: dict) -> None:
    """Every boot event after stage 0 names the preset and nothing overrides it."""
    events_after = [
        event
        for event in stream
        if event.get("event") == "boot" and event.get("stage") != "stage0"
    ]
    checks.that(
        bool(events_after),
        "no boot event after stage 0 names the preset at all",
    )
    for event in events_after:
        stage = event.get("stage")
        checks.that(
            event.get("preset") == name,
            f"the {stage} boot event names `preset: {name}` ({event.get('preset')!r})",
        )
        checks.that(
            event.get("preset_settings") == settings,
            f"the {stage} boot event reports the preset's settings "
            f"({event.get('preset_settings')!r})",
        )
        checks.that(
            not event.get("preset_overridden"),
            f"nothing overrides the preset at {stage} "
            f"({event.get('preset_overridden')!r})",
        )


# ---------------------------------------------------------------------------
# the root leg
# ---------------------------------------------------------------------------


def fair_queuing(
    checks: Checks,
    name: str,
    port: int,
    header: str,
    *,
    direct: bool = False,
    count: int = 6,
    ahead_limit: int = 1,
    elapsed_limit: float = 2.2,
    queued_floor_ms: float = 2000,
) -> None:
    """A fast client is served ahead of a slow client's backlog.

    `ahead` alone is satisfied by FIFO *and* by unbounded concurrency, so the
    batch also has to have queued: the slow requests' own `queued_ms` (from the
    `request` events) is the proof they serialized behind each other.
    """
    warm_client = "198.51.100.1"
    slow_client = "198.51.100.10"

    def hit(target: str, client: str) -> tuple[int, bytes]:
        headers = {header: client}
        if direct:
            return direct_http(name, target, headers)
        status, _headers, body = http(port, target, headers=headers, timeout=60)
        return status, body

    hit("/fast", warm_client)
    hit("/slow?ms=40", warm_client)

    order: list[str] = []
    results: dict[str, tuple[int, bytes, float]] = {}
    lock = threading.Lock()

    def slow_flight(label: str) -> None:
        started = time.monotonic()
        try:
            status, body = hit("/slow?ms=700", slow_client)
        except (OSError, LegFailure) as error:
            with lock:
                order.append(label)
                results[label] = (0, str(error).encode(), time.monotonic() - started)
            return
        with lock:
            order.append(label)
            results[label] = (status, body, time.monotonic() - started)

    threads = [
        threading.Thread(target=slow_flight, args=(f"a{index}",), daemon=True)
        for index in range(count)
    ]
    for thread in threads:
        thread.start()
    # The first slow requests are dispatched by now (the queue holds the rest).
    time.sleep(0.4)
    fast_started = time.monotonic()
    try:
        fast_status, fast_body = hit("/fast", "203.0.113.7")
    except (OSError, LegFailure) as error:
        fast_status, fast_body = 0, str(error).encode()
    fast_elapsed = time.monotonic() - fast_started
    with lock:
        # Snapshot before the slow threads are joined: `ahead` is how many of the
        # slow client's requests the scheduler had finished when the *other*
        # client's answer arrived, which is the fair-queuing decision.
        ahead = sum(1 for label in order if label.startswith("a"))
    for thread in threads:
        thread.join(timeout=60)

    checks.that(
        fast_status == 200,
        f"the fast client's request is 200, not {fast_status}: {fast_body[:160]!r}",
    )
    checks.that(
        ahead <= ahead_limit,
        f"fair queuing served the other client after {ahead} of the slow "
        f"client's queued requests ({count} sent, limit {ahead_limit})",
    )
    checks.that(
        fast_elapsed < elapsed_limit,
        f"the fast client waited {fast_elapsed:.2f}s behind a slow view "
        f"({count} x 0.7s of backlog; fair queuing answers in about one request)",
    )
    for label, (slow_status, _slow_body, _took) in results.items():
        checks.that(
            slow_status == 200,
            f"the slow client's request {label} is {slow_status}, not 200",
        )

    bursts = [
        event
        for event in events(name)
        if event.get("event") == "request"
        and event.get("path") == "/slow"
        and event.get("remote_addr") == slow_client
    ]
    checks.that(
        len(bursts) >= count,
        f"only {len(bursts)} of the {count} slow requests reached the scheduler",
    )
    queued = max((event.get("queued_ms") or 0) for event in bursts) if bursts else 0
    checks.that(
        queued >= queued_floor_ms,
        f"the slow batch never queued (max queued_ms {queued:.0f} < "
        f"{queued_floor_ms:.0f}), so this leg cannot tell fair queuing from "
        "unbounded concurrency",
    )


def drain(
    checks: Checks, name: str, port: int, header: str, *, direct: bool = False
) -> None:
    """SIGTERM finishes the in-flight request, then the container exits `0`.

    The request has to be *executing* before the signal lands: a fixed sleep
    proves a thread started, not that a view is inside `usleep`, so the view's
    own `/proc/<pid>/stat` state is the barrier.
    """
    before = [
        event.get("stage") for event in events(name) if event.get("event") == "shutdown"
    ]
    client = "198.51.100.20"
    headers = {header: client}
    send = (
        direct_request(name, "/slow?ms=1500", headers)
        if direct
        else proxy_request(port, "/slow?ms=1500", headers)
    )
    flight = Flight(send, "the drain request").start()

    deadline = time.monotonic() + 12
    found: int | None = None
    state = ""
    while time.monotonic() < deadline:
        spawned = [
            event
            for event in events(name)
            if event.get("event") == "process.spawned" and event.get("route") == "front"
        ]
        if spawned:
            pid = int(spawned[-1]["pid"])
            state = proc_state(name, pid)
            if state:
                found = pid
                if state in ("S", "D"):
                    break
        time.sleep(0.1)
    wchan = proc_wchan(name, found) if found is not None else "?"
    checks.that(
        found is not None and state in ("S", "D"),
        f"no view was executing the drain request when SIGTERM was due "
        f"(pid {found}, state {state!r}, wchan {wchan!r})",
    )

    killed = docker("kill", "--signal=TERM", name)
    checks.that(
        killed.returncode == 0,
        f"`docker kill --signal=TERM {name}` failed: {tail(killed.stderr, 4)}",
    )
    flight.join(timeout=90)
    checks.that(
        flight.status == 200,
        f"the in-flight request was dropped on SIGTERM "
        f"({flight.error or flight.status}): {flight.body[:160]!r}",
    )

    deadline = time.monotonic() + 60
    state_of_container = "running"
    while time.monotonic() < deadline:
        state_of_container = docker(
            "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}}", name
        ).stdout.strip()
        if state_of_container.startswith("exited"):
            break
        time.sleep(0.2)
    checks.that(
        state_of_container == "exited 0",
        f"the container did not exit 0 after SIGTERM ({state_of_container!r})",
    )
    stages = [
        event.get("stage") for event in events(name) if event.get("event") == "shutdown"
    ]
    checks.that(
        stages.count("drain") > before.count("drain")
        and bool(stages)
        and stages[-1] == "stopped",
        f"the shutdown events do not report this drain then stopped "
        f"({before} -> {stages})",
    )


def leg_root(
    binary: Path, php_image: str, pg: Postgres, canary: str, network: str
) -> str:
    """`--preset digitalocean-paas`, one root container, PostgreSQL direct."""
    name = f"blastit-prod-root-{os.getpid()}"
    port = PORTS.take()
    checks = Checks()
    started = start(
        "--name",
        name,
        "--network",
        network,
        "--user",
        "0:0",
        # Only so the harness can read a dumpable view's `/proc/<pid>/environ`
        # (container root has no `SYS_PTRACE` by default); every child still drops
        # every capability, and nothing in the deployment uses this one.
        "--cap-add",
        "SYS_PTRACE",
        "-p",
        f"127.0.0.1:{port}:8080",
        "-v",
        f"{binary}:/usr/share/blastit/lib/blastit:ro",
        "-w",
        "/prod",
        "-e",
        "PORT=8080",
        "-e",
        "DB_HOST=pg",
        "-e",
        "DB_PORT=5432",
        "-e",
        "DB_USERNAME=apps",
        "-e",
        f"DB_PASSWORD={pg.password}",
        "-e",
        "DB_DATABASE=apps",
        "-e",
        "DB_SSLMODE=disable",
        "-e",
        "BLASTIT_MANIFEST=/prod/Blastfile",
        "-e",
        "BLASTIT_MAX_WORKERS=1",
        "-e",
        "BLASTIT_VIEW_MEM_ESTIMATE=140MiB",
        "-e",
        "BLASTIT_MEM_HIGH_WATERMARK=100",
        "-e",
        f"CANARY={canary}",
        php_image,
        "/usr/share/blastit/lib/blastit",
        "--preset",
        "digitalocean-paas",
    )
    if started.returncode != 0:
        raise LegFailure(
            f"the root container did not start: {tail(started.stderr, 20)}"
        )
    try:
        wait_http(port, "/_blastit/health")

        stream = events(name)
        stage0 = boot(stream, "stage0")
        checks.that(
            stage0.get("privilege") == "root",
            f"the boot reports `privilege: root` ({stage0.get('privilege')!r})",
        )
        checks.that(
            stage0.get("seccomp") is True,
            f"the boot reports the seccomp filters on ({stage0.get('seccomp')!r})",
        )
        preset_rows(checks, stream, "digitalocean-paas", PRESET_ROOT)

        # /whoami is the client-address and sandbox subject: it carries the view's
        # own kernel uid and the values blastit passed to PHP. The third row is
        # the positive half of the trusted `X-Forwarded-Proto` rule — a peer in
        # `BLASTIT_TRUSTED_PROXIES` decides the scheme.
        seen = set()
        for client, proto, expected in (
            ("203.0.113.7", None, "off"),
            ("198.51.100.7", None, "off"),
            ("203.0.113.8", "https", "on"),
        ):
            headers = {"Do-Connecting-IP": client}
            if proto is not None:
                headers["X-Forwarded-Proto"] = proto
            status, _headers, body = get(port, "/whoami", headers)
            checks.that(status == 200, f"GET /whoami is 200, not {status}")
            facts = document(body, "GET /whoami")
            checks.that(
                facts.get("remote_addr") == client,
                f"REMOTE_ADDR is the `do-connecting-ip` value ({facts.get('remote_addr')!r}, "
                f"expected {client!r})",
            )
            https = str(facts.get("https", "")).lower()
            if expected == "on":
                checks.that(
                    https in ("on", "1", "https"),
                    f"a trusted `X-Forwarded-Proto: https` did not reach the view "
                    f"({facts.get('https')!r})",
                )
            else:
                checks.that(
                    https in ("off", "0", ""),
                    f"a plaintext request is not `https` to the view ({facts.get('https')!r})",
                )
            uid = facts.get("uid")
            checks.that(
                isinstance(uid, int) and uid >= 20_000,
                f"the view's own uid is at or above 20000 ({uid!r})",
            )
            seen.add(uid)
        checks.that(
            len(seen) == 1,
            f"one route kept one uid across three requests ({sorted(seen)})",
        )

        # The application's own reads, one grant per view.
        status, _headers, body = get(port, "/catalog/products")
        checks.that(status == 200, f"GET /catalog/products is 200, not {status}")
        rows = document(body, "GET /catalog/products").get("rows") or []
        checks.that(
            [row.get("name") for row in rows] == ["alpha", "beta", "gamma"],
            f"the catalogue read is {rows!r}",
        )
        status, _headers, body = get(port, "/catalog/products/2")
        checks.that(status == 200, f"GET /catalog/products/2 is 200, not {status}")
        checks.that(
            document(body, "GET /catalog/products/2").get("rows")
            == [{"id": 2, "name": "beta", "price": "12.00"}],
            f"a parameterised WHERE id = ? read answered another row: {body[:200]!r}",
        )
        status, _headers, body = get(port, "/warehouse/orders")
        checks.that(status == 200, f"GET /warehouse/orders is 200, not {status}")
        checks.that(
            document(body, "GET /warehouse/orders").get("table") == "orders",
            f"the orders read answered another table: {body[:200]!r}",
        )

        # Cross-view isolation: the same app, two policies.
        for target, what in (
            ("/catalog/orders", "the catalogue view reading `orders`"),
            ("/warehouse/products", "the warehouse view reading `products`"),
        ):
            status, _headers, body = get(port, target)
            checks.that(status == 403, f"{what} is {status}, not the documented 403")
            denied = document(body, f"GET {target}")
            checks.that(
                denied.get("sqlstate") == "42501",
                f"{what} did not report SQLSTATE 42501: {body[:200]!r}",
            )
            assert_no_secret(checks, body, pg.password, what)

        status, _headers, body = get(port, "/env")
        checks.that(status == 200, f"GET /env is 200, not {status}")
        facts = document(body, "GET /env")
        checks.that(
            facts.get("database_url") is None,
            f"the block store rendered a DATABASE_URL ({facts.get('database_url')!r})",
        )
        checks.that(
            facts.get("db_password_in_env") is False,
            "DB_PASSWORD reached the worker environment",
        )
        split = facts.get("split") or {}
        checks.that(
            all(
                split.get(name) is True
                for name in ("DB_HOST", "DB_PORT", "DB_USERNAME", "DB_DATABASE")
            )
            and split.get("DB_PASSWORD") is False,
            f"the block store's slots were not rendered as the view sees them ({split})",
        )
        assert_no_secret(checks, body, pg.password, "GET /env")

        shell_sandbox(checks, name, port)
        egress_sandbox(checks, name, port, canary)

        # One live view carries the identity, containment and credential facts:
        # `/proc/<pid>/status` is world-readable, and the environ scan is only
        # worth anything if it can read a dumpable view at all.
        flight = Flight(
            proxy_request(port, "/slow?ms=2500", {"Do-Connecting-IP": "198.51.100.30"}),
            "the identity request",
        ).start()
        spawned: list[dict] = []
        by_route: dict = {}
        live_pid: int | None = None
        live_status: dict[str, list[str]] = {}
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            spawned = [
                event
                for event in events(name)
                if event.get("event") == "process.spawned"
            ]
            by_route = {event.get("route"): event.get("uid") for event in spawned}
            front = [event for event in spawned if event.get("route") == "front"]
            if front and by_route.get("front"):
                candidate = int(front[-1]["pid"])
                live_status = proc_status(name, candidate)
                if live_status:
                    live_pid = candidate
                    break
            time.sleep(0.1)
        child_containment(checks, name)
        readable = env_hits(name, "DB_HOST=")
        leaked = env_hits(name, pg.password)
        flight.join(timeout=60)
        by_route = {
            event.get("route"): event.get("uid")
            for event in events(name)
            if event.get("event") == "process.spawned"
        }

        checks.that(live_pid is not None, "no live view process could be inspected")
        checks.that(
            (live_status.get("Uid") or [None])[0] == str(by_route.get("front")),
            f"the kernel's uid for pid {live_pid} is not the event's "
            f"({live_status.get('Uid')} vs {by_route.get('front')!r})",
        )
        checks.that(
            (live_status.get("Seccomp") or [None])[0] == "2",
            f"the view is not running under a seccomp filter "
            f"(Seccomp: {live_status.get('Seccomp')})",
        )
        checks.that(
            len(by_route) >= 5,
            f"every route runs under a uid of its own ({by_route})",
        )
        checks.that(
            all(isinstance(uid, int) and uid >= 20_000 for uid in by_route.values()),
            f"every view uid is at or above 20000 ({sorted(by_route.values())})",
        )
        checks.that(
            len(set(by_route.values())) == len(by_route),
            f"two routes share a uid ({by_route})",
        )
        checks.that(
            seen == {by_route.get("front")},
            f"`/whoami` reports the uid `process.spawned` gave the front route "
            f"({seen} vs {by_route.get('front')!r})",
        )
        checks.that(
            any(uid >= 20_000 for _pid, uid in readable),
            "the /proc scan could not read a view's environment (SYS_PTRACE did "
            "not take effect), so the credential scan below proves nothing "
            f"(readable: {hits_message(readable)})",
        )
        checks.that(
            container_env_has(name, pg.password),
            "the deployment's own configuration does not carry the password, so "
            "the scan below proves nothing",
        )
        checks.that(
            not leaked,
            f"the database password is in a /proc/<pid>/environ: "
            f"{hits_message(leaked)}",
        )
        checks.that(
            1 not in {pid for pid, _uid in leaked},
            "stage 1's own environment carried the database password past the barrier",
        )

        fair_queuing(checks, name, port, "Do-Connecting-IP")

        if checks.failures:
            print(tail(docker("logs", name).stdout, 80), flush=True)
        drain(checks, name, port, "Do-Connecting-IP")
    finally:
        if sys.exc_info()[0] is not None:
            print(
                tail(docker("logs", name).stdout + docker("logs", name).stderr, 120),
                flush=True,
            )
        docker("rm", "-f", name, timeout=60)
    return checks.done("root preset (digitalocean-paas)")


# ---------------------------------------------------------------------------
# the rootless leg
# ---------------------------------------------------------------------------


def capabilities_probe(checks: Checks, name: str) -> None:
    """`0500` and the seven capabilities on init's copy, none anywhere shared."""
    probe = exec_(name, CAPABILITIES_PROBE)
    lines = probe.stdout.splitlines()
    init = next((line for line in lines if line.startswith("init: ")), "")
    checks.that(
        bool(init), f"the init copy was not probed: {probe.stdout}{probe.stderr}"
    )
    for capability in CAPABILITIES:
        checks.that(
            capability in init,
            f"the init copy lacks {capability} ({init!r})",
        )

    def clean(label: str) -> bool:
        matching = [line for line in lines if line.startswith(f"{label}: ")]
        if not matching:
            return False
        good = []
        for line in matching:
            fields = line.split()
            good.append(
                len(fields) >= 3
                and fields[1].isdigit()
                and fields[2].startswith("/")
                and "cap_" not in line
            )
        return all(good)

    for label, what in (
        ("shared", "the shared binary"),
        ("shim", "a shim"),
        ("python", "the interpreter"),
    ):
        checks.that(
            clean(label),
            f"{what} carries a file capability, which would make every process of "
            f"init's uid an init ({lines})",
        )
    checks.that(
        any("mode=500 uid=10000" in line for line in lines),
        f"init's copy is not mode 0500 owned by 10000 ({probe.stdout})",
    )


def child_containment(checks: Checks, name: str) -> None:
    """No capability and `no_new_privs` in every child worker."""
    layout = [row for row in process_table(name) if row["uid"] >= 20_000]
    checks.that(
        len(layout) >= 3,
        f"the layout runs the three components and a view "
        f"({[row['uid'] for row in layout]})",
    )
    for row in layout:
        checks.that(
            row["caps"] == (ZERO,) * 5 and row["nnp"] == "1",
            f"uid {row['uid']} (pid {row['pid']}) holds "
            f"{row['caps']} with no_new_privs={row['nnp']}",
        )
    uids = {row["uid"] for row in layout}
    checks.that(
        any(uid >= 21_000 for uid in uids),
        f"a view worker was in flight during the scan ({sorted(uids)})",
    )
    for base, what in (
        (1, "http"),
        (2, "scheduler"),
        (3, "dbproxy"),
    ):
        checks.that(
            20_000 + base in uids,
            f"{what} runs under uid {20_000 + base} (saw {sorted(uids)})",
        )


def tls_json(port: int, target: str) -> dict:
    """One JSON document through Traefik's TLS entry point (self-signed)."""
    request = urllib.request.Request(f"https://127.0.0.1:{port}{target}")
    with urllib.request.urlopen(
        request, context=ssl._create_unverified_context(), timeout=30
    ) as response:
        return document(response.read(), f"GET https://127.0.0.1:{port}{target}")


def forwarded_headers(
    checks: Checks, plain: int, secure: int, gateway: str, password: str
) -> None:
    """Traefik decides the scheme and the client address, not the client."""
    forged = {
        "X-Forwarded-For": "203.0.113.99",
        "X-Forwarded-Proto": "https",
        "Do-Connecting-IP": "203.0.113.98",
    }
    status, _headers, body = get(plain, "/whoami", forged)
    checks.that(status == 200, f"GET /whoami through Traefik is 200, not {status}")
    facts = document(body, "GET /whoami through Traefik")
    checks.that(
        facts.get("remote_addr") == gateway,
        f"a forged X-Forwarded-For decided REMOTE_ADDR "
        f"({facts.get('remote_addr')!r}, expected the ingress {gateway!r})",
    )
    checks.that(
        str(facts.get("https", "")).lower() in ("off", "0", ""),
        f"a forged X-Forwarded-Proto decided the scheme ({facts.get('https')!r})",
    )
    assert_no_secret(checks, body, password, "GET /whoami (plain)")

    deadline = time.monotonic() + 60
    facts = None
    while time.monotonic() < deadline:
        try:
            facts = tls_json(secure, "/whoami")
            break
        except (OSError, LegFailure):
            time.sleep(0.3)
    checks.that(facts is not None, "Traefik's TLS entry point never answered")
    if facts is not None:
        checks.that(
            str(facts.get("https", "")).lower() in ("on", "1", "https"),
            f"a TLS-terminated request is not `https` to the view ({facts.get('https')!r})",
        )
        checks.that(
            facts.get("remote_addr") == gateway,
            f"the TLS client address is not the ingress ({facts.get('remote_addr')!r}, "
            f"expected {gateway!r})",
        )


def tampered_entry(checks: Checks, name: str, port: int) -> None:
    """A changed PHP entry is refused before `libphp` opens it."""
    status, _headers, body = get(port, "/whoami")
    checks.that(status == 200, f"GET /whoami before the swap is {status}, not 200")
    checks.that(b"decoy" not in body, "the decoy entry ran before the swap")

    encoded = base64.b64encode(DECOY.encode()).decode()
    swapped = exec_(
        name,
        "cp -p /prod/public/index.php /tmp/index.orig && "
        f"echo {encoded} | base64 -d > /tmp/decoy.php && "
        "mv /tmp/decoy.php /prod/public/index.php",
    )
    checks.that(
        swapped.returncode == 0,
        f"the harness could not install the decoy entry: {swapped.stderr}",
    )

    status, _headers, body = get(port, "/whoami")
    checks.that(
        500 <= status < 600,
        f"the tampered entry answered {status}, not a refusal: {body[:200]!r}",
    )
    checks.that(
        b"decoy" not in body,
        f"the decoy entry ran: {body[:200]!r}",
    )

    # The same inode rewritten in place (what `cp`, `tar -x` and a deploy do).
    rewritten = exec_(name, "cat /tmp/index.orig > /prod/public/index.php")
    checks.that(
        rewritten.returncode == 0,
        f"the harness could not rewrite the entry in place: {rewritten.stderr}",
    )
    status, _headers, body = get(port, "/whoami")
    checks.that(
        500 <= status < 600,
        f"an in-place rewrite answered {status}, not a refusal: {body[:200]!r}",
    )

    restored = exec_(name, "cp -p /tmp/index.orig /prod/public/index.php")
    checks.that(
        restored.returncode == 0,
        f"the harness could not restore the entry: {restored.stderr}",
    )
    status, _headers, _body = get(port, "/whoami")
    checks.that(
        500 <= status < 600,
        f"a restored entry was served without a restart ({status}): the boot's "
        "record is not re-checked",
    )
    checks.that(
        ENTRY_CHANGED in docker("logs", name, timeout=60).stdout,
        f"the refusal does not say `{ENTRY_CHANGED}`",
    )

    docker("restart", name, timeout=120)
    wait_http(port, "/_blastit/health")
    status, _headers, body = get(port, "/whoami")
    checks.that(
        status == 200,
        f"after a restart the recorded entry is not served ({status}): {body[:200]!r}",
    )
    checks.that(b"decoy" not in body, "the decoy ran after the restart")


def pool_leg(checks: Checks, name: str, pool: str, port: int, network: str) -> None:
    """PgBouncer is on the path: no pool, no database."""
    status, _headers, _body = get(port, "/catalog/products")
    checks.that(status == 200, f"GET /catalog/products through the pool is {status}")
    stopped = docker("stop", pool, timeout=120)
    checks.that(
        stopped.returncode == 0,
        f"`docker stop {pool}` failed: {tail(stopped.stderr, 4)}",
    )
    status = 0
    body = b""
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        try:
            status, _headers, body = http(port, "/catalog/products", timeout=20)
        except (OSError, LegFailure):
            status = 0
        if status != 200:
            break
        time.sleep(0.5)
    checks.that(
        status != 200,
        f"the app still answered {status} with PgBouncer stopped, so the pool is "
        f"not on the path: {body[:200]!r}",
    )
    checks.that(
        "pg.refused" in docker("logs", name, timeout=60).stdout,
        "no `pg.refused` event while the pool was down",
    )

    started = docker("start", pool, timeout=120)
    checks.that(
        started.returncode == 0,
        f"`docker start {pool}` failed: {tail(started.stderr, 4)}",
    )
    ready_deadline = time.monotonic() + 120
    while time.monotonic() < ready_deadline:
        ready = docker(
            "run",
            "--rm",
            "--network",
            network,
            POSTGRES_IMAGE,
            "pg_isready",
            "-h",
            "pgbouncer",
            "-p",
            "5432",
            "-U",
            "apps",
            "-d",
            "apps",
            timeout=60,
        )
        if ready.returncode == 0:
            break
        time.sleep(0.5)
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        try:
            status, _headers, body = http(port, "/catalog/products", timeout=20)
        except (OSError, LegFailure):
            status = 0
        if status == 200:
            break
        time.sleep(0.5)
    checks.that(
        status == 200,
        f"the app did not recover after PgBouncer returned ({status}): {body[:200]!r}",
    )


def leg_rootless(
    rootless_image: str,
    pool_image: str,
    traefik_image: str,
    pg: Postgres,
    network: str,
    gateway: str,
    canary: str,
    workspace: Path,
) -> str:
    """`compose-traefik`: Traefik, PgBouncer, capabilities, the URL store."""
    name = f"blastit-prod-app-{os.getpid()}"
    pool = f"blastit-prod-pool-{os.getpid()}"
    proxy = f"blastit-prod-traefik-{os.getpid()}"
    port = PORTS.take()
    secure = PORTS.take()
    checks = Checks()
    dynamic = workspace / "traefik.yml"
    dynamic.write_text(TRAEFIK_DYNAMIC, encoding="utf-8")

    pool_started = start(
        "--name",
        pool,
        "--network",
        network,
        "--network-alias",
        "pgbouncer",
        "-e",
        "DB_HOST=pg",
        "-e",
        "DB_USER=apps",
        "-e",
        f"DB_PASSWORD={pg.password}",
        "-e",
        "DB_NAME=apps",
        "-e",
        "AUTH_TYPE=scram-sha-256",
        "-e",
        "POOL_MODE=transaction",
        "-e",
        "MAX_PREPARED_STATEMENTS=200",
        "-e",
        "DEFAULT_POOL_SIZE=20",
        "-e",
        "MAX_CLIENT_CONN=100",
        "-e",
        "LISTEN_PORT=5432",
        pool_image,
    )
    if pool_started.returncode != 0:
        raise LegFailure(f"PgBouncer did not start: {tail(pool_started.stderr, 20)}")

    app_started = start(
        "--name",
        name,
        "--network",
        network,
        "--network-alias",
        "app",
        "--user",
        "10000:10000",
        "--cap-drop",
        "ALL",
        *[
            argument
            for capability in (
                "CHOWN",
                "DAC_OVERRIDE",
                "FOWNER",
                "KILL",
                "SETGID",
                "SETPCAP",
                "SETUID",
            )
            for argument in ("--cap-add", capability)
        ],
        "--memory",
        "1g",
        "--tmpfs",
        "/run/blastit:mode=0711,uid=10000,gid=10000",
        "-w",
        "/prod",
        "-e",
        "PORT=8080",
        "-e",
        "BLASTIT_MANIFEST=/prod/Blastfile.url",
        "-e",
        "BLASTIT_PRESET=compose-traefik",
        "-e",
        "BLASTIT_MAX_WORKERS=2",
        "-e",
        "BLASTIT_VIEW_MEM_ESTIMATE=140MiB",
        "-e",
        "BLASTIT_MEM_HIGH_WATERMARK=100",
        "-e",
        "BLASTIT_EXEC=/usr/share/blastit/lib/blastit",
        "-e",
        f"DATABASE_URL=postgresql://apps:{pg.password}@pgbouncer:5432/apps?sslmode=disable",
        "-e",
        f"CANARY={canary}",
        rootless_image,
        "/usr/share/blastit/lib/blastit-init",
    )
    if app_started.returncode != 0:
        raise LegFailure(
            f"the rootless container did not start: {tail(app_started.stderr, 20)}"
        )

    traefik_started = start(
        "--name",
        proxy,
        "--network",
        network,
        "-p",
        f"127.0.0.1:{port}:80",
        "-p",
        f"127.0.0.1:{secure}:443",
        "-v",
        f"{dynamic}:/etc/traefik/dynamic.yml:ro",
        traefik_image,
        "--entryPoints.web.address=:80",
        "--entryPoints.websecure.address=:443",
        "--providers.file.filename=/etc/traefik/dynamic.yml",
        "--log.level=INFO",
    )
    if traefik_started.returncode != 0:
        raise LegFailure(f"Traefik did not start: {tail(traefik_started.stderr, 20)}")
    try:
        # The pool must be listening before the boot dials it.
        ready_deadline = time.monotonic() + 120
        while time.monotonic() < ready_deadline:
            ready = docker(
                "run",
                "--rm",
                "--network",
                network,
                POSTGRES_IMAGE,
                "pg_isready",
                "-h",
                "pgbouncer",
                "-p",
                "5432",
                "-U",
                "apps",
                "-d",
                "apps",
                timeout=60,
            )
            if ready.returncode == 0:
                break
            time.sleep(0.5)
        wait_http(port, "/_blastit/health")

        stream = events(name)
        stage0 = boot(stream, "stage0")
        checks.that(
            stage0.get("privilege") == "capabilities",
            f"the boot reports `privilege: capabilities` ({stage0.get('privilege')!r})",
        )
        checks.that(
            stage0.get("capabilities") == CAPABILITIES,
            f"and names exactly the capabilities it holds ({stage0.get('capabilities')!r})",
        )
        checks.that(
            stage0.get("seccomp") is True,
            f"the boot reports the seccomp filters on ({stage0.get('seccomp')!r})",
        )
        preset_rows(checks, stream, "compose-traefik", PRESET_ROOTLESS)
        checks.that(
            not [
                event
                for event in stream
                if event.get("event") == "app.output"
                and "run as uid" in str(event.get("line", ""))
            ],
            "the boot warns that something else runs as init's own uid",
        )

        capabilities_probe(checks, name)

        # Views are forked lazily, so hold two slow requests in flight while the
        # `/proc` scan reads every child: the components alone would prove
        # nothing about a worker.
        held: list[threading.Thread] = []

        def hold() -> None:
            try:
                http(port, "/slow?ms=2500", timeout=60)
            except (OSError, LegFailure):
                pass

        for _ in range(2):
            thread = threading.Thread(target=hold, daemon=True)
            thread.start()
            held.append(thread)
        time.sleep(0.7)
        child_containment(checks, name)
        for thread in held:
            thread.join(timeout=60)

        # The uid that can run the capable copy must not be a shared one...
        refused = exec_(
            name, "/usr/share/blastit/lib/blastit-init --version 2>&1", user="10001"
        )
        checks.that(
            refused.returncode != 0
            and "permission denied" in (refused.stdout + refused.stderr).lower(),
            f"uid 10001 ran the capable copy ({refused.returncode}): "
            f"{(refused.stdout + refused.stderr)[:300]}",
        )
        allowed = exec_(
            name, "/usr/share/blastit/lib/blastit-init --version", user="10000"
        )
        checks.that(
            allowed.returncode == 0 and "blastit" in allowed.stdout,
            f"init's own uid cannot run the capable copy "
            f"({allowed.returncode}): {(allowed.stdout + allowed.stderr)[:300]}",
        )

        forwarded_headers(checks, port, secure, gateway, pg.password)

        # The positive half of the same rule: a direct request (Traefik rewrites
        # the header, so the client has to be inside the container) that names
        # its own client is believed, scheme included.
        status, body = direct_http(
            name,
            "/whoami",
            {"X-Forwarded-For": "198.51.100.9", "X-Forwarded-Proto": "https"},
        )
        checks.that(status == 200, f"the in-container /whoami is {status}, not 200")
        facts = document(body, "in-container GET /whoami")
        checks.that(
            facts.get("remote_addr") == "198.51.100.9",
            f"a trusted X-Forwarded-For was not believed "
            f"({facts.get('remote_addr')!r})",
        )
        checks.that(
            str(facts.get("https", "")).lower() in ("on", "1", "https"),
            f"a trusted X-Forwarded-Proto was not believed ({facts.get('https')!r})",
        )
        assert_no_secret(checks, body, pg.password, "in-container GET /whoami")

        # The URL store the Compose page writes: dbproxy dials `pgbouncer`, and
        # the worker is handed a socket URL with no password.
        status, _headers, body = get(port, "/env")
        checks.that(status == 200, f"GET /env is 200, not {status}")
        facts = document(body, "GET /env")
        checks.that(
            "host=" in (facts.get("database_url") or ""),
            f"the view was not handed a socket URL ({facts.get('database_url')!r})",
        )
        checks.that(
            facts.get("url_has_password") is False, "the URL carries a password"
        )
        checks.that(
            facts.get("db_password_in_env") is False,
            "DB_PASSWORD reached the worker environment",
        )
        split = facts.get("split") or {}
        checks.that(
            not any(split.values()),
            f"the URL store rendered the block store's slots too ({split})",
        )
        assert_no_secret(checks, body, pg.password, "GET /env")

        status, _headers, body = get(port, "/catalog/products")
        checks.that(
            status == 200,
            f"GET /catalog/products through the pool is {status}, not 200: "
            f"{body[:200]!r}",
        )
        rows = document(body, "GET /catalog/products").get("rows") or []
        checks.that(
            [row.get("name") for row in rows] == ["alpha", "beta", "gamma"],
            f"the pooled catalogue read is {rows!r}",
        )
        status, _headers, body = get(port, "/warehouse/orders")
        checks.that(status == 200, f"GET /warehouse/orders is {status}, not 200")

        # The refusal and sandbox halves, on the preset that ships behind the
        # internet.
        denial(
            checks,
            port,
            pg.password,
            "/warehouse/products",
            "the warehouse view reading `products`",
        )
        shell_sandbox(checks, name, port)
        egress_sandbox(checks, name, port, canary)

        pool_leg(checks, name, pool, port, network)

        # Fair queuing and the drain through the in-container client: Traefik
        # rewrites `X-Forwarded-For`, so a second flow only exists on a direct
        # connection (the preset trusts every peer), and the drain must end
        # `blastit-init`, not a `dev` boot.
        fair_queuing(
            checks,
            name,
            port,
            "X-Forwarded-For",
            direct=True,
            count=4,
            ahead_limit=2,
            elapsed_limit=2.6,
            queued_floor_ms=400,
        )
        tampered_entry(checks, name, port)
        drain(checks, name, port, "X-Forwarded-For", direct=True)

        if checks.failures:
            print(tail(docker("logs", name).stdout, 120), flush=True)
    finally:
        if sys.exc_info()[0] is not None:
            print(
                tail(
                    docker("logs", name).stdout + docker("logs", name).stderr,
                    160,
                ),
                flush=True,
            )
        for container in (proxy, name, pool):
            docker("rm", "-f", container, timeout=60)
    return checks.done("rootless preset (compose-traefik)")


# ---------------------------------------------------------------------------
# images, host facts and the run
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


def build_php_image() -> str:
    """The one fixture image both legs share (`debian:trixie` + NTS `libphp`)."""
    image = f"blastit-prod-php:{os.getpid()}"
    built = docker(
        "build",
        "-q",
        "-f",
        str(APPS / "php" / "Dockerfile"),
        "-t",
        image,
        str(ROOT),
        timeout=2400,
    )
    if built.returncode != 0:
        raise LegFailure(f"the fixture image did not build: {tail(built.stderr, 30)}")
    return image


def build_rootless_image(base: str, binary: Path, workspace: Path) -> str:
    """The capable copy, from a staging context that holds the binary alone."""
    image = f"blastit-prod-rootless:{os.getpid()}"
    staging = workspace / "rootless-context"
    staging.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, staging / "blastit")
    built = docker(
        "build",
        "-q",
        "-f",
        str(PROD / "Dockerfile.rootless"),
        "--build-arg",
        f"BASE={base}",
        "-t",
        image,
        str(staging),
        timeout=1800,
    )
    if built.returncode != 0:
        raise LegFailure(
            "the rootless image did not build (the host's Docker must be able to "
            f"store file capabilities in image layers): {tail(built.stderr, 30)}"
        )
    return image


def pull(image: str, why: str) -> None:
    """`image`, pulled only when it is not already local; a Skip when it cannot be."""
    if docker("image", "inspect", image, timeout=30).returncode == 0:
        return
    got = docker("pull", image, timeout=1200)
    if got.returncode != 0:
        raise Skip(
            f"{why} needs `{image}`: run `docker pull {image}` ({tail(got.stderr, 3)})"
        )


def canary_address(image: str, network: str, name: str) -> str:
    """A tiny HTTP server on the app's network, plus its `ip:port`."""
    started = start(
        "--name",
        name,
        "--network",
        network,
        "--network-alias",
        "canary",
        image,
        "sh",
        "-c",
        "mkdir -p /tmp/canary && echo canary-ok > /tmp/canary/index.html && "
        "exec php -S 0.0.0.0:8081 -t /tmp/canary",
    )
    if started.returncode != 0:
        raise LegFailure(f"the canary did not start: {tail(started.stderr, 20)}")
    deadline = time.monotonic() + 60
    address = ""
    while time.monotonic() < deadline:
        inspected = docker(
            "inspect",
            "-f",
            "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
            name,
            timeout=30,
        ).stdout.strip()
        if inspected:
            address = f"{inspected}:8081"
            break
        time.sleep(0.2)
    if not address:
        raise LegFailure(f"{name} has no address on {network}")
    host, port = address.rsplit(":", 1)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        probe = docker(
            "run",
            "--rm",
            "--network",
            network,
            image,
            "php",
            "-r",
            f'$s=@fsockopen("{host}", {port}, $e, $m, 2); '
            'echo $s ? "open" : "ERR $e $m";',
            timeout=60,
        )
        if "open" in probe.stdout:
            return address
        time.sleep(0.3)
    raise LegFailure(f"the canary on {address} never accepted a connection")


def gateway_of(network: str) -> str:
    gateway = docker(
        "network", "inspect", network, "-f", "{{(index .IPAM.Config 0).Gateway}}"
    ).stdout.strip()
    if not gateway:
        raise LegFailure(f"the network {network} has no gateway")
    return gateway


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, help="the blastit artifact under test")
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    arguments = parser.parse_args()

    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix="run-", dir=WORK_ROOT))
    summary = Summary()
    network = f"blastit-prod-net-{os.getpid()}"
    pg_name = f"blastit-prod-pg-{os.getpid()}"
    volume = f"blastit-prod-pgdata-{os.getpid()}"
    canary_name = f"blastit-prod-canary-{os.getpid()}"
    images: list[str] = []
    rootless_ready = False
    try:
        try:
            binary = (
                arguments.binary.resolve() if arguments.binary else binary_argument()
            )
            require_docker()
            pull(POSTGRES_IMAGE, "both production legs")
        except Skip as skipped:
            summary.add("host", "SKIP", str(skipped))
            return summary.report()
        summary.add("binary", "PASS", f"{binary} ({binary.stat().st_size} bytes)")

        try:
            php_image = build_php_image()
        except LegFailure as failure:
            summary.add("fixture image", "FAIL", str(failure))
            return summary.report()
        images.append(php_image)
        summary.add("fixture image", "PASS", php_image)

        try:
            for image, why in (
                (TRAEFIK_IMAGE, "the rootless preset"),
                (POOL_IMAGE, "the rootless preset"),
            ):
                pull(image, why)
            rootless_ready = True
        except Skip as skipped:
            summary.add("rootless host", "SKIP", str(skipped))

        created = docker("network", "create", network)
        if created.returncode != 0:
            raise LegFailure(f"`docker network create` failed: {tail(created.stderr)}")
        docker("volume", "create", volume)
        pg = Postgres(network, pg_name, PORTS.take())
        pg.start()
        pg.apply(APPS / "postgres" / "schema.sql")
        summary.add(
            "postgres", "PASS", f"postgres:18 on 127.0.0.1:{pg.port} (schema applied)"
        )

        canary = canary_address(php_image, network, canary_name)
        summary.add("canary", "PASS", f"an HTTP peer on {canary}")

        try:
            summary.add(
                "root preset",
                "PASS",
                leg_root(binary, php_image, pg, canary, network),
            )
        except Skip as skipped:
            summary.add("root preset", "SKIP", str(skipped))
        except LegFailure as failure:
            summary.add("root preset", "FAIL", str(failure))
        except Exception as error:  # noqa: BLE001 - one leg must not end the run
            summary.add("root preset", "FAIL", f"{type(error).__name__}: {error}")

        if rootless_ready:
            try:
                rootless_image = build_rootless_image(php_image, binary, workspace)
                images.append(rootless_image)
                summary.add("rootless image", "PASS", rootless_image)
                try:
                    summary.add(
                        "rootless preset",
                        "PASS",
                        leg_rootless(
                            rootless_image,
                            POOL_IMAGE,
                            TRAEFIK_IMAGE,
                            pg,
                            network,
                            gateway_of(network),
                            canary,
                            workspace,
                        ),
                    )
                except Skip as skipped:
                    summary.add("rootless preset", "SKIP", str(skipped))
                except LegFailure as failure:
                    summary.add("rootless preset", "FAIL", str(failure))
                except Exception as error:  # noqa: BLE001
                    summary.add(
                        "rootless preset",
                        "FAIL",
                        f"{type(error).__name__}: {error}",
                    )
            except LegFailure as failure:
                summary.add("rootless image", "FAIL", str(failure))
        else:
            summary.add(
                "rootless preset",
                "SKIP",
                f"the host cannot pull {TRAEFIK_IMAGE}/{POOL_IMAGE}",
            )
    except LegFailure as failure:
        summary.add("harness", "FAIL", str(failure))
    except Skip as skipped:
        summary.add("harness", "SKIP", str(skipped))
    finally:
        for container in (canary_name, pg_name):
            docker("rm", "-f", container, timeout=60)
        docker("volume", "rm", "-f", volume, timeout=60)
        docker("network", "rm", network, timeout=60)
        for image in images:
            docker("rmi", "-f", image, timeout=300)
        if not arguments.keep:
            shutil.rmtree(workspace, ignore_errors=True)
    return summary.report()


if __name__ == "__main__":
    sys.exit(main())
