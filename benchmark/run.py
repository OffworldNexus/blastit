"""Compare blastit, granian and gunicorn on one app and one machine.

    scripts/build-wheel.sh
    uv run benchmark/run.py                       # each server at a fixed shape
    uv run benchmark/grid.py                       # search each one's parameters
    uv run benchmark/run.py --tuning results/tuning-*.json   # compare best-of-each

Without `--tuning` every server runs the same shape (`--config`, default
`thr4`). With it, each server runs the shape `grid.py` found best for it on each
scenario — the fair comparison, since an engine asked to run a shape it is bad
at is not being measured, it is being handicapped.

Every server serves the same WSGI app (`app.py`) and reads the same
`DATABASE_URL`; the only difference is the server. Results go to
`benchmark/results/compare-<timestamp>.json` and are printed as tables.

Stdlib only, like the e2e runner.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys

import harness as h

# Fixed shapes for a quick look, before any tuning. The name is the CLI value.
CONFIGS = {
    "thr4": h.config(workers=1, threads=4, worker_class="gthread"),
    "proc4": h.config(workers=4, threads=1, worker_class="sync"),
    "thr16": h.config(workers=1, threads=16, worker_class="gthread"),
}


def load_tuning(path: str) -> dict:
    """Read a `grid.py` tuning file and return its per-(server, scenario) winners."""
    with open(path) as f:
        data = json.load(f)
    # best[server][scenario] = {"config": {...}, "rps": ...}
    return data["best"]


def build_plan(servers, scenarios, args) -> dict:
    """plan[server][scenario] -> (config, source)."""
    plan: dict = {}
    if args.tuning:
        best = load_tuning(args.tuning)
        for server in servers:
            plan[server] = {}
            for scenario in scenarios:
                entry = best.get(server, {}).get(scenario)
                if entry is None:
                    raise SystemExit(f"tuning file has no winner for {server}/{scenario}")
                plan[server][scenario] = (entry["config"], f"tuned ({entry['rps']:.0f} req/s)")
    else:
        cfg = CONFIGS[args.config]
        for server in servers:
            plan[server] = {scenario: (cfg, f"fixed {args.config}") for scenario in scenarios}
    return plan


def run(plan, scenario_paths, args) -> dict:
    """Measure every (server, scenario) cell, restarting a server only when its
    config changes so a fixed shape runs all its scenarios on one instance."""
    results = {"servers": {}}
    for server, per_scenario in plan.items():
        sresults = {"scenarios": {}}
        results["servers"][server] = sresults
        # A server is restarted only when its config changes, so a fixed shape
        # runs every scenario on one instance.
        started_for = None
        for scenario, (cfg, source) in per_scenario.items():
            if started_for != cfg:
                h.stop_server()
                print(f"[{server} {h.shape(server, cfg)}] starting", flush=True)
                h.start_server(server, cfg)
                h.wait_ready()
                started_for = cfg
            print(f"  {scenario}:", end=" ", flush=True)
            summary = h.measure(
                scenario_paths[scenario], args.runs, args.duration, args.warmup, args.concurrency
            )
            print(f"{summary['rps_median']:.0f} req/s  (p99 {summary['latency_ms']['p99']:.1f} ms)", flush=True)
            sresults["scenarios"][scenario] = {
                "config": cfg,
                "shape": h.shape(server, cfg),
                "source": source,
                "summary": summary,
            }
        h.stop_server()
    return results


def print_report(results: dict, scenarios, servers) -> None:
    """Print the throughput and latency tables and the shape each server ran."""
    def cell(scenario, server, fn):
        """Format one table cell, or None when the cell was not measured."""
        sc = results["servers"][server]["scenarios"].get(scenario)
        return fn(sc["summary"]) if sc else None

    h.grid_table(
        "Throughput (median req/s)",
        scenarios, servers,
        lambda s, sv: cell(s, sv, lambda x: f"{x['rps_median']:.0f}"),
    )
    h.grid_table(
        "p99 latency (ms)",
        scenarios, servers,
        lambda s, sv: cell(s, sv, lambda x: f"{x['latency_ms']['p99']:.1f}"),
    )
    h.grid_table(
        "p50 latency (ms)",
        scenarios, servers,
        lambda s, sv: cell(s, sv, lambda x: f"{x['latency_ms']['p50']:.2f}"),
    )
    h.grid_table(
        "Shape each server ran",
        scenarios, servers,
        lambda s, sv: results["servers"][sv]["scenarios"][s]["shape"]
        if results["servers"][sv]["scenarios"].get(s)
        else None,
        col_width=16,
    )


def main() -> int:
    """Parse the flags, seed the database, run the plan, write and print it."""
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--servers", nargs="+", default=h.SERVERS, choices=h.SERVERS)
    ap.add_argument("--scenarios", nargs="+", default=["plain", "json", "db"], choices=list(h.SCENARIOS))
    ap.add_argument("--config", default="thr4", choices=list(CONFIGS), help="shape used without --tuning")
    ap.add_argument("--tuning", help="a grid.py tuning file to run each server at its best")
    ap.add_argument("--runs", type=int, default=3, help="measured runs per cell")
    ap.add_argument("--duration", type=float, default=10.0, help="seconds per run")
    ap.add_argument("--warmup", type=float, default=3.0, help="seconds of discarded warm-up")
    ap.add_argument("--concurrency", type=int, default=50, help="load generator connections")
    ap.add_argument("--skip-build", action="store_true", help="do not rebuild the image")
    args = ap.parse_args()

    if not h.require_wheel():
        return 2
    if not args.skip_build:
        print("building the benchmark image", flush=True)
        h.compose("build")

    print("starting PostgreSQL", flush=True)
    h.up_db()
    print("seeding", flush=True)
    h.seed()
    versions = h.collect_versions()
    print(f"versions: {versions}", flush=True)

    scenario_paths = {s: h.SCENARIOS[s] for s in args.scenarios}
    plan = build_plan(args.servers, args.scenarios, args)
    try:
        results = run(plan, scenario_paths, args)
    finally:
        h.down()

    results.update(
        kind="compare",
        started=dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        versions=versions,
        tuning_source=args.tuning or f"fixed {args.config}",
        params={
            "runs": args.runs,
            "duration_s": args.duration,
            "warmup_s": args.warmup,
            "concurrency": args.concurrency,
        },
    )
    path = h.write_results("compare", results)
    print_report(results, args.scenarios, args.servers)
    print(f"results written to {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
