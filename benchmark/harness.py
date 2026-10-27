"""Machinery shared by `run.py` and `grid.py`.

`run.py` compares the servers; `grid.py` searches each server's own parameters
first and writes a tuning file that `run.py` can then use. Both drive the same
compose file, the same load generator and the same reporting, and both live
here so the two scripts stay about their own question.

A **config** is a dict `{workers, threads, worker_class}`: the shape a server is
asked for. `blastit` serves a route from one process, so it is given
`workers*threads` request threads and the `worker_class` goes unread; granian and
gunicorn take all three. Every report says which shape a server actually ran.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import statistics
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
COMPOSE_FILE = os.path.join(HERE, "compose.yaml")
RESULTS_DIR = os.path.join(HERE, "results")
PORT = os.environ.get("BENCH_PORT", "18090")
# Readiness is polled on the host that runs the server: loopback locally, the
# private IP when the server publishes there for a remote load generator.
READY_HOST = os.environ.get("BENCH_READY_HOST", "127.0.0.1")
READY_URL = f"http://{READY_HOST}:{PORT}/"

# Where the load generator sends requests. `server:8080` is the compose service
# on its network; over a private network it is `<private-ip>:<BENCH_PORT>`.
TARGET = os.environ.get("BENCH_TARGET", "server:8080")
# When set (`user@host`), the load generator runs on that other machine through
# `/root/run-oha.sh` instead of in a container next to the server, so it cannot
# contend with the server for cores.
LOADGEN_SSH = os.environ.get("BENCH_LOADGEN_SSH", "")

SERVERS = ["blastit", "granian", "gunicorn"]

# name -> path the app serves.
SCENARIOS = {"plain": "/", "json": "/json", "db": "/db", "work": "/work"}


def config(workers: int = 1, threads: int = 1, worker_class: str = "gthread"):
    """A concurrency shape."""
    return {"workers": workers, "threads": threads, "worker_class": worker_class}


def shape(server: str, cfg: dict) -> str:
    """How a server actually runs a config, for the reports."""
    conc = cfg["workers"] * cfg["threads"]
    if server == "blastit":
        # One process, whatever the config's process count would suggest.
        return f"1p x {conc}t"
    if server == "gunicorn" and cfg["worker_class"] == "sync":
        return f"{cfg['workers']}p sync"
    if server == "gunicorn":
        return f"{cfg['workers']}p x {cfg['threads']}t gthread"
    return f"{cfg['workers']}p x {cfg['threads']}t"


# --- process control -------------------------------------------------------


def compose(*args, capture: bool = False, check: bool = True):
    """Run `docker compose` for the benchmark project and return the result."""
    env = {**os.environ, "BENCH_PORT": PORT}
    return subprocess.run(
        ["docker", "compose", "-f", COMPOSE_FILE, *args],
        text=True,
        capture_output=capture,
        env=env,
        check=check,
    )


def start_server(server: str, cfg: dict) -> None:
    """Bring the `server` service up alone, at the shape `cfg` describes.

    The shape is exported into the process environment because compose reads its
    interpolation from there (`compose.yaml`), and `start.sh` in the image turns
    it into the server's own flags.
    """
    os.environ.update(
        BENCH_SERVER=server,
        BENCH_WORKERS=str(cfg["workers"]),
        BENCH_THREADS=str(cfg["threads"]),
        BENCH_WORKER_CLASS=cfg["worker_class"],
    )
    compose("up", "-d", "--force-recreate", "--no-deps", "server", capture=True)


def stop_server() -> None:
    """Remove the running server container, so the next config starts clean."""
    compose("rm", "-sf", "server", capture=True, check=False)


def dump_server_logs(why: str) -> None:
    """Write the server's log tail to stderr, for when a start failed."""
    print(f"  !! {why}; server logs:", file=sys.stderr)
    compose("logs", "--no-color", "--tail", "40", "server", check=False)


def up_db() -> None:
    """Start the compose database, unless an external one is configured.

    With BENCH_DATABASE_URL set — a managed cluster that brings its own pool —
    there is nothing local to start: the servers connect straight to it.
    """
    if os.environ.get("BENCH_DATABASE_URL"):
        return
    compose("up", "-d", "--wait", "--quiet-pull", "db", "pgbouncer")


def seed() -> None:
    """Run the one-shot `seed` service: create and fill the benchmark table."""
    compose("run", "--rm", "-T", "--no-deps", "seed")


def down() -> None:
    """Tear the project's containers, network and volumes down."""
    compose("down", "--volumes", "--remove-orphans", capture=True, check=False)


def wait_ready(timeout: float = 120.0) -> None:
    """Poll the server's published port until it answers, or give up."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(READY_URL, timeout=2) as resp:
                if resp.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            time.sleep(0.2)
    raise SystemExit(f"server never answered {READY_URL}")


# --- load generation -------------------------------------------------------


def _oha_json(stdout: str) -> dict:
    """The JSON object in oha's stdout, which is otherwise wrapped in noise."""
    start = stdout.find("{")
    end = stdout.rfind("}")
    if start < 0 or end < 0:
        raise ValueError(f"no JSON in oha output:\n{stdout[:2000]}")
    return json.loads(stdout[start : end + 1])


def oha(path: str, duration: float, concurrency: int) -> dict:
    """One oha run against `path`: in a local container, or on a remote host."""
    args = [
        "--no-tui", "--output-format", "json",
        # The status-code distribution, so a run that shed load with 5xx is
        # visible: oha's own success_rate counts those as successes.
        "--stats-success-breakdown",
        "-z", f"{duration}s", "-c", str(concurrency),
        f"http://{TARGET}{path}",
    ]
    if LOADGEN_SSH:
        # A separate machine: run the same oha container there. The script pins
        # it to that host's BENCH_LOADGEN_CPUS.
        proc = subprocess.run(
            ["ssh", "-o", "StrictHostKeyChecking=accept-new",
             "-o", "UserKnownHostsFile=/root/.ssh/known_hosts",
             "-o", "BatchMode=yes", LOADGEN_SSH, "/root/run-oha.sh", *args],
            text=True, capture_output=True, check=False,
        )
        return _oha_json(proc.stdout)
    proc = compose(
        "run", "--rm", "-T", "--no-deps", "loadgen", *args,
        capture=True,
        # oha exits non-zero when any request failed; the run still has numbers
        # worth keeping, and success_rate travels with them.
        check=False,
    )
    return _oha_json(proc.stdout)


def _ok_ratio(run: dict) -> float | None:
    """The fraction of responses that were 2xx, from oha's status distribution.

    oha's `success_rate` is not that: it counts a 503 the server shed in order to
    keep up as a success, which would let load-shedding look like throughput.
    """
    dist = run.get("statusCodeDistribution") or {}
    total = sum(dist.values())
    if total == 0:
        return None
    ok = sum(n for code, n in dist.items() if str(code).startswith("2"))
    return ok / total


def summarize(runs: list[dict]) -> dict:
    """Reduce several oha runs to one cell: median rate, latency and statuses.

    The rate counts only served (2xx) responses, so a run that shed load with
    503s reports what it served rather than a cheap rejection rate.
    """
    rps = []
    ratios = []
    codes: dict = {}
    for r in runs:
        ratio = _ok_ratio(r)
        base = r["metrics"]["requests_per_sec"]
        # Charge only what was actually served.
        rps.append(base * (ratio if ratio is not None else 1.0))
        ratios.append(ratio)
        for code, n in (r.get("statusCodeDistribution") or {}).items():
            codes[str(code)] = codes.get(str(code), 0) + n
    order = sorted(range(len(runs)), key=lambda i: rps[i])
    median_run = runs[order[len(order) // 2]]
    lat = median_run["metrics"]["latency_ms"]
    known = [x for x in ratios if x is not None]
    # oha reports success_rate as null for a run with no comparable requests (a
    # run whose requests were all still in flight at the deadline); keep that as
    # null instead of crashing on min() over None.
    rates = [r["metrics"].get("success_rate") for r in runs]
    rates = [x for x in rates if x is not None]
    return {
        "rps": rps,
        "rps_median": statistics.median(rps),
        "rps_min": min(rps),
        "rps_max": max(rps),
        "success_rate": min(rates) if rates else None,
        "success_ratio": min(known) if known else None,
        "status_codes": codes,
        "latency_ms": {"p50": lat["p50"], "p95": lat["p95"], "p99": lat["p99"], "mean": lat["mean"]},
    }


def measure(path: str, runs: int, duration: float, warmup: float, concurrency: int) -> dict:
    """Warm up, then take `runs` samples, and summarize them."""
    oha(path, warmup, concurrency)
    samples = [oha(path, duration, concurrency) for _ in range(runs)]
    summary = summarize(samples)
    ratio = summary["success_ratio"]
    if ratio is not None and ratio < 0.995:
        print(
            f"    !! only {ratio * 100:.2f}% of responses were 2xx "
            f"({summary['status_codes']}): this run shed load, it did not serve it",
            flush=True,
        )
    return summary


def collect_versions() -> dict:
    """The server image's tool versions, so a result file names what produced it."""
    proc = compose(
        "run", "--rm", "-T", "--no-deps",
        "--entrypoint", "python", "server", "-c",
        "import importlib.metadata as m, sys;"
        "print('python', sys.version.split()[0]);"
        "[print(n, m.version(n)) for n in ('blastit','granian','gunicorn','psycopg')]",
        capture=True,
        check=False,
    )
    versions = {}
    for line in proc.stdout.splitlines():
        parts = line.split()
        if len(parts) == 2:
            versions[parts[0]] = parts[1]
    return versions


# --- reporting -------------------------------------------------------------


def write_results(kind: str, payload: dict) -> str:
    """Write a result JSON under `benchmark/results/` and return its path."""
    os.makedirs(RESULTS_DIR, exist_ok=True)
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    path = os.path.join(RESULTS_DIR, f"{kind}-{stamp}.json")
    with open(path, "w") as f:
        json.dump(payload, f, indent=2, sort_keys=True)
    return path


def grid_table(title: str, rows, cols, cell, col_width: int = 14) -> None:
    """Print a small fixed-width table. `rows`/`cols` are lists of labels and
    `cell(row, col)` returns the string for one cell (None becomes an em dash)."""
    print(f"\n{title}")
    header = f"{'':>{col_width}} " + " ".join(f"{c:>{col_width}}" for c in cols)
    print(header)
    print("-" * len(header))
    for r in rows:
        cells = []
        for c in cols:
            v = cell(r, c)
            cells.append(f"{v:>{col_width}}" if v is not None else f"{'—':>{col_width}}")
        print(f"{r:>{col_width}} " + " ".join(cells))
    print()


def require_wheel() -> bool:
    """True if a blastit wheel is in `dist/`, with a message to stderr if not."""
    dist = os.path.join(ROOT, "dist")
    if os.path.isdir(dist) and any(f.endswith(".whl") for f in os.listdir(dist)):
        return True
    print("no wheel in dist/: run scripts/build-wheel.sh first", file=sys.stderr)
    return False
