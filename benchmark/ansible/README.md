# Provisioning the benchmark with Ansible

`setup.yml` creates the two hosts and the managed database the benchmark runs
on; `teardown.yml` destroys everything it made. Both talk to the DigitalOcean API
and to the hosts themselves; nothing is installed on your machine beyond Ansible
and the `community.digitalocean` collection.

```console
$ ansible-galaxy collection install -r requirements.yml

$ export DO_API_TOKEN=...            # read/write; required
$ export DO_REGION=lon1              # optional

$ ansible-playbook setup.yml                    # provision and configure
$ ansible-playbook setup.yml -e run_benchmark=true   # ... and run the grid + compare
$ ansible-playbook teardown.yml                 # destroy it all
```

## What it creates

| Resource | Default | Notes |
|---|---|---|
| server host | `c-16`, `lon1` | Docker; runs PostgreSQL's client side, the server under test and the harness |
| load-generator host | `c-8`, `lon1` | Docker; runs `oha` on the private network |
| managed PostgreSQL | `db-s-1vcpu-1gb`, PG 18 | with a transaction-mode connection pool named `bench` |
| SSH key | one per `prefix` | imported from `BENCH_SSH_PUB_KEY` |

Only the **CPU-Optimized** (`c-*`) and **General Purpose** (`g-*`) families give
you *dedicated* vCPUs, and they are not offered in every region — `ams3`, for
instance, has neither; `lon1`, `nyc1`, `nyc3`, `sfo2` and `blr1` do. Everything
carries the tag `BENCH_PREFIX` (`blastit-bench`), which is how teardown finds it.

## Environment variables

All optional except `DO_API_TOKEN`, all read from the environment.

| Variable | Default | Meaning |
|---|---|---|
| `DO_API_TOKEN` | — | DigitalOcean API token (read/write) |
| `DO_REGION` | `lon1` | region for both hosts and the cluster |
| `DO_VPC_UUID` | region's default VPC | pin a specific VPC |
| `BENCH_PREFIX` | `blastit-bench` | names and the tag teardown matches |
| `BENCH_SERVER_SIZE` | `c-16` | server host size (dedicated CPU) |
| `BENCH_LOADGEN_SIZE` | `c-8` | load-generator size (dedicated CPU) |
| `BENCH_PG_SIZE` | `db-s-1vcpu-1gb` | managed cluster size |
| `BENCH_PG_VERSION` | `18` | PostgreSQL major |
| `BENCH_POOL_SIZE` | `20` | connections in the cluster's pool |
| `BENCH_SERVER_CPUS` | `0-7` | cores pinned to the server (see `benchmark/compose.yaml`) |
| `BENCH_DB_CPUS` | `8-11` | cores for PostgreSQL and the local PgBouncer |
| `BENCH_LOADGEN_CPUS` | `0-7` | cores pinned to the load generator |
| `BENCH_SSH_PUB_KEY` | `~/.ssh/id_rsa.pub` | key imported into the account |
| `BENCH_SSH_KEY_NAME` | `<prefix>-key` | name it is registered under |
| `BENCH_BUILD_WHEEL` | `true` | run `scripts/build-wheel.sh` before shipping |
| `PYO3_PYTHON` | `uv python find 3.13` | interpreter the wheel is built against |
| `BENCH_PORT` | `18090` | port the server host publishes on its private IP |
| `BENCH_CONCURRENCY` | `400` | load-generator connections |
| `BENCH_GRID_DURATION` / `BENCH_GRID_RUNS` | `5` / `1` | per grid point |
| `BENCH_COMPARE_DURATION` / `BENCH_COMPARE_RUNS` | `10` / `3` | per comparison cell |
| `BENCH_RUN` | `false` | same as `-e run_benchmark=true` |

## What each step does

1. **Control machine** — builds the blastit wheel, tars `benchmark/` + `dist/`,
   registers the SSH key, creates the two droplets, creates the managed cluster,
   adds the transaction pool, trusts the server host on the cluster, and derives
   the direct and pooled connection URLs.
2. **Server host** — installs Docker, unpacks the harness at `/root`, builds the
   benchmark image, pulls `postgres:18` (the local-database fallback), writes
   `/root/bench.env` (endpoints, CPU pins), and makes a throwaway key.
3. **Load-generator host** — installs Docker, authorizes that key, installs
   `/root/run-oha.sh`, and pulls the `oha` image.
4. **Run** (only with `run_benchmark=true`) — `grid.py` then `run.py --tuning`,
   and the result JSONs are fetched back into `benchmark/results/`.

## Cost

`c-16` + `c-8` + a `db-s-1vcpu-1gb` cluster is roughly **$0.78/hour**. Nothing
charges you after `teardown.yml`, which deletes the droplets, the cluster and (by
default) the SSH key — pass `-e keep_ssh_key=true` to keep the key registered.

## Notes

* `setup.yml` is safe to re-run: droplets are keyed by name, the SSH key is
  matched by fingerprint, and an existing cluster with the same name is reused
  rather than recreated. The pool and firewall POSTs tolerate `422` (already
  there).
* The local `db` and `pgbouncer` services in `benchmark/compose.yaml` are unused
  when `BENCH_DATABASE_URL` is set (the managed cluster brings its own pool);
  `harness.up_db` skips starting them. Leave those variables unset to run the
  self-contained local-database variant instead.
