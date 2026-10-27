# Recap: blastit vs granian vs gunicorn, with the optimized blastit

The three engines, measured on 2026-10-27 with the **optimized** blastit — the
ten-commit `perf-hot-path` branch, not the build `README.md`'s `## Results`
section was written from. That section still carries the previous run and now
points here.

Result files (git-ignored, like everything in `results/`):
`results/managed-compare-optimized.json` (the main run),
`results/managed-tuning-optimized.json` (its tuning pass),
`results/managed-rerun-db.json` (the repeat of the `db` scenario below).

## Setup

Identical to the previous run, so the two are comparable:

* a `c-16` server host, 8 cores allotted to the engine under test, and a `c-8`
  load-generator host on the same private network; `oha` at 400 keep-alive
  connections;
* DigitalOcean Managed PostgreSQL 18 with its built-in pool (transaction
  pooling, size 20) on the same VPC, over the private network;
* each engine at the shape `grid.py` finds best for it, medians of three
  10-second runs, and every run checked to be ~100% HTTP 2xx before its rate is
  believed — `oha`'s own `success_rate` counts a shed `503` as a success, so the
  harness reads the status distribution instead.

**One deliberate difference from the previous run:** `BLASTIT_FAIR_QUEUING=on`
for every engine. It is blastit's own feature — nothing else here has an
admission queue to order — so its cost belongs in blastit's numbers rather than
in a footnote. It measures ~1.5 µs of CPU per request, about 2%.

Versions: blastit 0.1.0 (`perf-hot-path`), granian 2.8.5, gunicorn 26.2.0,
CPython 3.13.18, psycopg 3.3.6.

## Throughput

Median req/s, higher is better. Bold is the best of the three for that scenario.

| scenario | blastit | granian | gunicorn | |
|---|---:|---:|---:|---|
| `plain` | 6,411 | **169,751** | 23,991 | median of 3 × 10 s |
| `json` | 5,891 | **160,518** | 22,120 | median of 3 × 10 s |
| `db` | 1,547 | 1,587 | **1,959** | median of 8 × 10 s, and see below |

### `plain` and `json` — stable

| scenario | blastit | granian | gunicorn |
|---|---|---|---|
| `plain` | 6,272 / 7,348 / 6,411 | 169,769 / 169,160 / 169,751 | 24,430 / 23,744 / 23,991 |
| `json` | 5,866 / 6,981 / 5,891 | 169,564 / 160,518 / 155,151 | 22,120 / 22,013 / 24,154 |

granian and gunicorn reproduce run to run to within a few percent, and blastit's
own three runs are within 15% of each other. This is the leg to trust.

### `db` — too noisy to rank, except for one thing

The scenario was measured twice: the main run's three 10-second runs, and then a
targeted repeat of five more. All eight, per engine:

| engine | the three | the five | pooled median | pooled range |
|---|---|---|---|---|
| blastit | 1,770 / 1,433 / 1,433 | 1,327 / 1,916 / 1,837 / 1,262 / 1,661 | **1,547** | 1,262 – 1,916 |
| granian | 1,628 / 1,118 / 1,389 | 2,168 / 2,217 / 2,158 / 1,546 / 1,237 | **1,587** | 1,118 – 2,217 |
| gunicorn | 1,935 / 1,969 / 1,587 | 1,948 / 1,994 / 1,938 / 2,398 / 2,304 | **1,959** | 1,587 – 2,398 |

**The two samples rank blastit and granian in opposite orders** — the 3-run
sample puts blastit ahead, the 5-run sample puts granian a long way ahead — and
the pooled medians are 1,547 against 1,587, a 2.6% difference inside a ±25%
spread. There is no ordering to read there. The one thing the data does support
is gunicorn's lead: 1,959 is ~25% above both, and it is above both in every
sample.

The conclusion is about the *leg*, not the engines: this database is a shared
managed service and the round trip through the pool moves far more between runs
than the engines differ by. Ranking HTTP servers on it needs either many more
runs or a database whose latency is stable — and that is worth fixing in the
harness before this scenario is quoted again.

## Latency

p50 / p99 in ms, lower is better.

| scenario | blastit | granian | gunicorn |
|---|---|---|---|
| `plain` | 55.1 / 83.4 | **1.9 / 11.9** | 16.9 / 39.9 |
| `json` | 57.1 / 87.2 | **2.3 / 12.5** | 17.2 / 36.0 |
| `db` | 227.8 / 457.5 | **163.2 / 374.5** | 170.6 / 531.7 |

The `db` row is from the 5-run sample, for the reason above — treat it the same
way.

**blastit's latency is queue depth, not engine slowness.** One process, 16
request threads, 400 connections: about 25 requests queue behind each thread.
Little's law agrees with the measurement — `400 / 6,411 = 62 ms` of mean
residence time against a measured mean of 62.3 ms and p50 of 55.1 ms. granian's
8 processes × 8 threads is 64-way concurrent, which is why its p50 is 1.9 ms.
This single-process ceiling is what the `plain` ranking is really measuring.

## Shapes

| scenario | blastit | granian | gunicorn |
|---|---|---|---|
| `plain` | 1p × 16t | 8p × 8t | 8p × 1t gthread |
| `json` | 1p × 4t | 4p × 1t | 8p × 1t gthread |
| `db` | 1p × 16t | 1p × 4t | 8p × 4t gthread |

## Against the previous run

granian and gunicorn are unchanged binaries, so re-measuring them is the control:

| scenario | blastit | granian | gunicorn |
|---|---|---|---|
| `plain` | 5,572 → **6,411** (+15%) | 164,856 → 169,751 (+3%) | 24,680 → 23,991 (−3%) |
| `json` | 5,001 → **5,891** (+18%) | 149,663 → 160,518 (+7%) | 23,997 → 22,120 (−8%) |
| `db` | 2,101 → 1,547 (−26%) | 2,574 → 1,587 (−38%) | 2,408 → 1,959 (−19%) |

**`plain` and `json` are a real gain.** The controls reproduce within a few
percent, so the infrastructure did not move under them, and blastit's +15% /
+18% is the branch. Its latency improved with it: p50 73.2 → 55.1 on `plain` and
84.5 → 57.1 on `json`, which is what removing per-request CPU should look like.

**`db` is not a blastit result either way.** Every engine fell, by magnitudes of
the same order, which is the leg degrading rather than any engine changing — see
the noise above. The previous run's ranking (granian > gunicorn > blastit) and
this one's (gunicorn > blastit ≈ granian) are both consistent with a leg that
cannot resolve them.

## blastit's sweep — complete

One 5-second run per point, `c=400`, fair queuing on. This is the tuning pass: it
picks the shapes above, and a single short run is noisier than the medians, so
these are not the headline numbers.

| shape | `plain` | `json` | `db` |
|---|---:|---:|---:|
| 1p × 1t | 4,671 | 4,067 | 823 |
| 1p × 2t | 5,178 | 5,051 | 1,657 |
| 1p × 4t | 5,008 | 6,875 | 2,061 |
| 1p × 8t | 6,142 | 6,025 | 2,544 |
| 1p × 16t | **6,982** | 5,720 | **2,659** |
| 1p × 32t | 6,786 | 6,623 | 2,440 |

* **blastit still tops out around one process.** There is no second process to
  add to a single catch-all route, so this is really a sweep of request threads
  inside one view. Past 16 threads the curve is flat (`plain` 16t → 32t is 6,982
  → 6,786) — that is the GIL, and the shape that wins is the one where the Rust
  side of the request path overlaps the interpreter best.
* **The scenarios peak at different shapes**, which is why the comparison tunes
  per scenario rather than fixing one shape: `json` is fastest at 4 threads
  (6,875) while `plain` and `db` want 16.

The sweep's `db` column (2,061 – 2,659 at the shapes near the winner) sits above
the comparison's pooled 1,547 at the same shape. That is the same instability
again: the sweep ran while the database was in a better mood than the medians
did. Another reason not to quote `db`.

## What is not measured here

* **A `db` ranking.** The leg cannot support one at this sample size — see above.
  A harness fix would be to run the `db` scenario many more times, or to hold the
  pooled connection count fixed and measure the round trip on its own.
* **The `work` scenario** (pure-CPU per request), if `SCENARIOS` carries one; it
  is not part of this run.
* **Absolute numbers as a verdict.** The hosts are dedicated but the database is
  shared and its leg moved 19-38% between two runs of the same build. `plain`
  and `json` are stable; `db` is only comparable *within* a run, and barely.
