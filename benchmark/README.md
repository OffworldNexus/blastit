# Benchmark: blastit vs granian vs gunicorn

Three WSGI servers, one app, dedicated-CPU VMs. This measures what blastit's own
front end and sandbox cost per request, next to the two servers a project would
otherwise reach for — with each engine first tuned to its own best shape, so the
comparison is not decided by a configuration somebody picked for it.

**What it is not.** It is not a claim that blastit is *the* fast one — it isn't;
blastit parses every request in a separate process, routes it in another, and
passes database traffic through a proxy that authorises each statement. It is a
measurement of what that security model costs. If you want the fastest way to
serve a WSGI app and you do not need per-view sandboxes, use granian.

## What runs

The load generator, the app and the database are the same for all three servers;
only the server changes.

| Scenario | Request | What it isolates |
|---|---|---|
| `plain` | `GET /` | request/response overhead: parse, dispatch, a 17-byte body |
| `json` | `GET /json` | response encoding with a small JSON body |
| `db` | `GET /db` | one `SELECT` through a pooled connection, and through dbproxy under blastit |

`app.py` is a single raw WSGI callable — no framework — so the numbers are the
server plus WSGI dispatch, not a router's.

The servers:

| Server | Front end | Concurrency model |
|---|---|---|
| blastit | Rust (`http` + `scheduler` processes) | one process per route, one OS thread per request |
| granian | Rust (Rust HTTP server, Python blocking threads for WSGI) | workers × blocking threads |
| gunicorn | Python | workers × threads |

## Tuning before comparing

A server asked to run a shape it is bad at is not being measured, it is being
handicapped. `grid.py` searches each engine's own parameters first:

| Server | Searched |
|---|---|
| blastit | `BLASTIT_MAX_WORKERS` ∈ {1, 2, 4, 8, 16, 32} |
| granian | workers ∈ {1, 2, 4, 8} × blocking-threads ∈ {1, 4, 8} |
| gunicorn | workers ∈ {1, 2, 4, 8} × (sync, or gthread × threads ∈ {1, 4, 8}) |

For each point the server is started alone, warmed up, and measured; the point
with the best throughput wins per scenario, and a single best-overall shape per
server is recorded too (the highest geometric mean of relative throughput, so it
cannot win by being excellent on one scenario and useless on another).
`run.py --tuning <file>` then runs the comparison at each server's winner.

## How it runs

Containers, driven by `benchmark/run.py` and `benchmark/grid.py` over
`benchmark/compose.yaml`. Each server serves `http://server:8080`, and
`ghcr.io/hatoo/oha` generates the load.

**A pooler sits in front of PostgreSQL, two ways.** blastit's dbproxy keeps no
pool — one client connection is one upstream connection — so a pooler is what a
real deploy would use, and it also removes an unfairness: granian and gunicorn
open a pool *per process* (ten connections under eight workers is eighty), while
blastit is a single process. The pooler turns all of that into the same small set
of server connections, so the `db` scenario measures the query and the proxy, not
how many sockets each engine happened to open.

The harness ships a compose PgBouncer for a self-contained run, and can use a
**managed database that brings its own pool** instead — DigitalOcean Managed
PostgreSQL serves PgBouncer on port 25061. Set the endpoints and the compose
`db`/`pgbouncer` services are never started:

```console
export BENCH_DATABASE_URL='postgresql://doadmin:<pw>@private-<cluster>.db.ondigitalocean.com:25061/pool?sslmode=require'
export BENCH_SEED_DATABASE_URL='postgresql://doadmin:<pw>@private-<cluster>.db.ondigitalocean.com:25060/defaultdb?sslmode=require'
```

The direct endpoint seeds; the pooled one serves. The cluster has to share a VPC
with the server host, and the server host has to be a trusted source on it.

**Two hosts, or one.** The load generator can run next to the server (a compose
service on its own cores) or on a second machine, so it never competes with the
server for CPU. For the second case the server publishes on its private IP and
the load generator runs there over SSH:

```console
# on the server host
export BENCH_BIND=<server-private-ip> BENCH_READY_HOST=<server-private-ip> \
       BENCH_TARGET=<server-private-ip>:18090 BENCH_LOADGEN_SSH=root@<loadgen-private-ip>
```

`/root/run-oha.sh` on the load generator host runs the `oha` container pinned to
that host's `BENCH_LOADGEN_CPUS` on the host network.

There is an Ansible playbook pair, [`benchmark/ansible/`](ansible/README.md), that
creates exactly this shape on DigitalOcean — two dedicated-CPU hosts, a managed
PostgreSQL cluster with its pool, both hosts configured, and optionally the run
itself with the results fetched back:

```console
$ ansible-galaxy collection install -r benchmark/ansible/requirements.yml
$ DO_API_TOKEN=... ansible-playbook benchmark/ansible/setup.yml -e run_benchmark=true
$ DO_API_TOKEN=... ansible-playbook benchmark/ansible/teardown.yml
```

CPU is pinned so the sides do not fight for cores: server `0-7`, PostgreSQL and
PgBouncer `8-11`, a local load generator `12-15` (`BENCH_SERVER_CPUS`,
`BENCH_DB_CPUS`, `BENCH_LOADGEN_CPUS`).

```console
$ scripts/build-wheel.sh            # the blastit wheel the images install
$ uv run benchmark/grid.py           # search each engine's parameters
$ uv run benchmark/run.py --tuning results/tuning-<timestamp>.json   # compare
$ uv run benchmark/run.py --runs 1 --duration 5 --scenarios plain    # a quick smoke
```

Useful flags: `--servers`, `--scenarios`, `--runs`, `--duration`, `--concurrency`,
`--warmup`, `--skip-build`; `grid.py` adds `--limit` and `--config` is `run.py`'s
fixed shape for when there is no tuning file. Results are written to
`benchmark/results/<kind>-<timestamp>.json` and printed as tables.

## Fairness notes

* **blastit's sandbox is in the path, and that is the point.** A request crosses
  two more processes than under the others: the Rust `http` process parses it,
  the `scheduler` matches it against the manifest and hands it to a view process.
* **The database leg differs by design.** blastit rewrites the app's
  `DATABASE_URL` to a local Unix socket served by dbproxy, which opens its own
  connection to the pooler and checks each statement against the manifest;
  granian and gunicorn connect to the pooler directly. That difference *is* the
  feature being priced in the `db` scenario.
* **The same connection pool is used in the app** (`psycopg_pool`), with
  `autocommit` and no auto-`PREPARE`, so the scenario measures serving a request
  rather than `connect(2)` or a wire-protocol difference, and so PgBouncer can
  stay in transaction pooling mode.
* **Load-shedding must not score as throughput.** blastit bounds its admission
  queue and answers `503` past it; granian and gunicorn queue without a bound. A
  shed `503` is cheap, so a run that overflows the queue can post a *higher*
  requests-per-second than one that serves everything — and `oha`'s own
  `success_rate` counts a `503` as a success, so it stays at 1.0. The harness
  reads the HTTP status distribution, charges only 2xx responses, and warns when
  a run is not ~all 2xx. blastit's queue is also sized (`BLASTIT_QUEUE_SIZE`, in
  `start.sh`) so it queues like the others rather than shedding at the
  benchmark's client concurrency. Without both, blastit's `plain` and `db`
  numbers are inflated several-fold by rejections.
* **HTTP/1.1 only**, keep-alive on. granian is told `--http 1`; blastit speaks
  nothing else behind a load balancer.
* **The load generator can be the ceiling.** With the generator on another host,
  `throughput ≤ connections / RTT`; when a fast engine flattens at the same rate
  for every shape, that is the client, not the server. Raise `--concurrency`.

## Results

**Setup.** A `c-16` server host (8 cores allotted to the server under test) and a
`c-8` load-generator host on the same private network, `oha` at 400 keep-alive
connections; DigitalOcean Managed PostgreSQL 18 with its built-in pool
(transaction mode, size 20) on the same VPC; each engine run at the shape
`grid.py` found best for it. Medians of three 10-second runs. Every run is
checked to be ~100% HTTP 2xx (see the load-shedding note above), and the reported
rate counts only served requests.

Versions: blastit 0.1.0 (with the `TCP_NODELAY` fix below), granian 2.8.5,
gunicorn 26.2.0, CPython 3.13.18, psycopg 3.3.6.

### Throughput (median req/s, higher is better)

| scenario | blastit | granian | gunicorn | blastit's shape |
|---|---:|---:|---:|---|
| `plain` | 5,572 | **164,856** | 24,680 | 1p × 16t |
| `json` | 5,001 | **149,663** | 23,997 | 1p × 16t |
| `db` | 2,101 | **2,574** | 2,408 | 1p × 16t |

### p50 / p99 latency (ms, lower is better)

| scenario | blastit | granian | gunicorn |
|---|---|---|---|
| `plain` | 73.2 / 81.7 | **2.0 / 13.5** | 15.7 / 35.4 |
| `json` | 84.5 / 97.0 | **2.3 / 13.5** | 17.0 / 32.4 |
| `db` | 161 / 386 | **136 / 379** | 138 / 417 |

### Reading it

**Two regimes.** Without a database the ranking is about using cores. granian's
Rust front end across four to eight processes reaches ~165k; gunicorn's Python
workers reach ~25k; blastit reaches ~5.6k. blastit's *per-request* cost is
actually the best of the three — one blastit process beats one gunicorn worker by
~1.7× on `plain` (6,016 vs 3,520) — but blastit serves a route from a single
process, so seven of the eight cores sit idle. On this budget that is the whole
story: blastit is ~4.4× behind gunicorn and ~30× behind granian.

With a database the picture collapses to one number. All three land at
2.1–2.6k, because the managed pool's round trip (~5 ms under concurrency) — not
the HTTP server — is the bottleneck: at 16 connections all three measure ~2.5k
`db` req/s with p50 ~5 ms, and more clients only add queue. blastit is within 18%
of granian and ahead of gunicorn here, and its p50 is competitive, which is the
point the `plain` numbers hide: blastit's front end is not the expensive part
once real work is in the request.

**Tuning matters less for blastit.** Its grid is nearly flat — 1p × 8t through
1p × 32t all land at 4.5–6k `plain` — because concurrency cannot get past the one
process. granian and gunicorn both climb until four to eight processes and then
flatten. This is why the comparison runs each engine at its own best shape: at a
single fixed shape the gap would be an artifact of the shape, not the server.

### Found by the benchmark: a missing `TCP_NODELAY`

The first run measured blastit at ~1,200 req/s with a p50 of exactly 42 ms,
independent of concurrency. A packet capture showed why: blastit writes the
response head and the response body as two TCP segments, and with Nagle on, the
small body waits for the client's delayed ACK — a fixed ~40 ms on every response
with a small body.

`crates/blastit-http/src/server.rs` accepted the client socket without
`set_nodelay(true)`. Adding that one line took blastit from **24 → 2,630 req/s at
one connection** and p50 from 42 ms to 0.37 ms. It costs no CPU (the delay is in
the kernel, so throughput scales linearly with concurrency), which is why it is
invisible until a server is actually loaded. The numbers above are with the fix;
without it every blastit `plain`/`json` figure here would be roughly a third of
what it is and the `db` figures would be dominated by queueing.

### Caveats

* Latency includes the ~0.9 ms round trip between the two hosts; it applies
  equally to all three, but it makes granian's p50 look larger relative to its
  throughput than it would in-process.
* The server gets 8 of the host's 16 cores. blastit cannot use more than one of
  them for a single route, so a bigger budget widens the `plain`/`json` gap
  rather than closing it.
* `grid.py` maximizes throughput, so a winning shape can be latency-poor:
  blastit's `plain` p50 of 73 ms is 400 connections queued behind 16 threads. The
  `--runs`-and-latency tables are there to read together.
* The `db` scenario is bounded by the pool, so it compares the servers' overhead
  *around* a fixed database cost, not their ability to go fast with a database.

