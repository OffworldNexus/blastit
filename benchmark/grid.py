"""Grid-search each server's parameters, so `run.py --tuning` compares each
engine at its best instead of at a shape somebody picked for it.

    uv run benchmark/grid.py                     # sweep, write results/tuning-*.json
    uv run benchmark/run.py --tuning results/tuning-<timestamp>.json

Every server has a small space of knobs that move its throughput: blastit's
request-thread budget, granian's worker and blocking-thread counts, gunicorn's
worker count, thread count and worker class. For each point the server is
started alone, warmed up, and measured on each scenario; the point with the best
median throughput wins. Both a per-scenario winner and a single best-overall
shape per server are recorded, because the ideal shape can depend on the
workload (a CPU-bound route and a database-bound one need different amounts of
concurrency).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import sys

import harness as h

# The parameter space per server. Sizes are chosen to straddle the pin: the
# server container is given BENCH_SERVER_CPUS cores (four by default), so the
# grid runs from under-subscribed to over-subscribed on both axes.
GRIDS = {
    "blastit": [h.config(workers=1, threads=n) for n in (1, 2, 4, 8, 16, 32)],
    "granian": [h.config(workers=w, threads=t) for w in (1, 2, 4, 8) for t in (1, 4, 8)],
    "gunicorn": (
        [h.config(workers=w, threads=1, worker_class="sync") for w in (1, 2, 4, 8)]
        + [h.config(workers=w, threads=t, worker_class="gthread") for w in (1, 2, 4, 8) for t in (1, 4, 8)]
    ),
}


def sweep(servers, scenarios, args) -> dict:
    """Measure every grid point of every server, on every scenario."""
    results: dict = {"servers": {}}
    for server in servers:
        grid = GRIDS[server][: args.limit] if args.limit else GRIDS[server]
        sresults = {"grid": grid, "points": []}
        results["servers"][server] = sresults
        for i, cfg in enumerate(grid):
            print(f"[{server} {i + 1}/{len(grid)} {h.shape(server, cfg)}] starting", flush=True)
            h.start_server(server, cfg)
            h.wait_ready()
            point = {"config": cfg, "shape": h.shape(server, cfg), "scenarios": {}}
            try:
                for scenario in scenarios:
                    summary = h.measure(
                        h.SCENARIOS[scenario], args.runs, args.duration, args.warmup, args.concurrency
                    )
                    point["scenarios"][scenario] = summary
                    print(f"  {scenario}: {summary['rps_median']:.0f} req/s", flush=True)
            finally:
                h.stop_server()
            sresults["points"].append(point)
    return results


def winners(results: dict, scenarios) -> tuple[dict, dict]:
    """Per-scenario winners and one best-overall shape per server."""
    best: dict = {}
    overall: dict = {}
    for server, sresults in results["servers"].items():
        best[server] = {}
        # The best throughput this server reaches on each scenario, to rank its
        # own points against each other fairly across scenarios of very
        # different speed.
        ceiling = {
            scenario: max(p["scenarios"][scenario]["rps_median"] for p in sresults["points"])
            for scenario in scenarios
        }
        for scenario in scenarios:
            top = max(sresults["points"], key=lambda p: p["scenarios"][scenario]["rps_median"])
            best[server][scenario] = {
                "config": top["config"],
                "shape": top["shape"],
                "rps": top["scenarios"][scenario]["rps_median"],
            }
        # Best single shape: the one with the highest geometric mean of relative
        # throughput, so it cannot win by being excellent on one scenario and
        # useless on another.
        def score(point):
            ratios = [point["scenarios"][s]["rps_median"] / ceiling[s] for s in scenarios]
            return math.prod(ratios) ** (1 / len(ratios))

        top = max(sresults["points"], key=score)
        overall[server] = {"config": top["config"], "shape": top["shape"], "score": score(top)}
    return best, overall


def print_report(results: dict, best: dict, overall: dict, scenarios) -> None:
    """Print each server's grid, its per-scenario winners and its best overall."""
    for server, sresults in results["servers"].items():
        h.grid_table(
            f"{server} — median req/s (* = best for that scenario)",
            [p["shape"] for p in sresults["points"]],
            scenarios,
            lambda shape, scenario, sresults=sresults, server=server: next(
                f"{p['scenarios'][scenario]['rps_median']:.0f}"
                + ("*" if p["shape"] == best[server][scenario]["shape"] else "")
                for p in sresults["points"]
                if p["shape"] == shape
            ),
            col_width=16,
        )
    print("Best per scenario:")
    for server in results["servers"]:
        for scenario in scenarios:
            w = best[server][scenario]
            print(f"  {server:<9} {scenario:<6} {w['shape']:<16} {w['rps']:.0f} req/s")
    print("\nBest single shape (highest geometric mean of relative throughput):")
    for server, w in overall.items():
        print(f"  {server:<9} {w['shape']:<16} score {w['score']:.3f}")


def main() -> int:
    """Parse the flags, seed the database, sweep, write the tuning file, print."""
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--servers", nargs="+", default=h.SERVERS, choices=h.SERVERS)
    ap.add_argument("--scenarios", nargs="+", default=["plain", "db"], choices=list(h.SCENARIOS))
    ap.add_argument("--runs", type=int, default=1, help="measured runs per point")
    ap.add_argument("--duration", type=float, default=5.0, help="seconds per run")
    ap.add_argument("--warmup", type=float, default=1.0, help="seconds of discarded warm-up")
    ap.add_argument("--concurrency", type=int, default=50, help="load generator connections")
    ap.add_argument("--limit", type=int, default=0, help="only the first N points of each grid (0 = all)")
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

    try:
        results = sweep(args.servers, args.scenarios, args)
    finally:
        h.down()

    best, overall = winners(results, args.scenarios)
    results.update(
        kind="tuning",
        started=dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        versions=versions,
        scenarios=args.scenarios,
        best=best,
        overall=overall,
        params={
            "runs": args.runs,
            "duration_s": args.duration,
            "warmup_s": args.warmup,
            "concurrency": args.concurrency,
        },
    )
    path = h.write_results("tuning", results)
    print_report(results, best, overall, args.scenarios)
    print(f"results written to {path}")
    print(f"next: uv run benchmark/run.py --tuning {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
