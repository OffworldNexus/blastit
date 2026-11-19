# DigitalOcean App Platform

App Platform starts your container as root, an edge overwrites the client
address, and PostgreSQL is a managed cluster with its own pool.

```console
$ blastit --preset digitalocean-paas
```

That is the boot, not a print: without root or the capabilities it exits `1`,
naming the missing `cap_*` and pointing at `blastit dev`. Your deploy file sets
`BLASTIT_PRESET` instead.

## What the preset sets

| Setting | Value | Why |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `do-connecting-ip` | the edge **overwrites** it with the real client address |
| `BLASTIT_TRUSTED_PROXIES` | `*` | nothing untrusted can reach the socket |
| `BLASTIT_FAIR_QUEUING` | `on` | one burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | already the default; the platform truncates a line at 2048 bytes |
| *(assumption)* | `root` | the container starts as root; a non-root user gets no capabilities |

## The topology

```mermaid
flowchart TB
    client(["client"])
    edge["DigitalOcean edge · TLS"]
    subgraph app["App Platform service · root"]
        ingress["ingress"]
        sched["scheduler"]
        view["view · one uid per route"]
        dbproxy["dbproxy"]
    end
    migrate["PRE_DEPLOY job"]
    pool["pool · 25061"]
    db[("cluster · 25060")]

    client --> edge --> ingress --> sched --> view
    view -- "socket" --> dbproxy
    dbproxy -- "TCP + TLS" --> pool --> db
    migrate -. "direct, no pool" .-> db
```

Everything inside the box is the [model](index.md#the-topology) unchanged.

## Preset defaults

| Thing | Value | Why |
|---|---|---|
| instance | `apps-s-1vcpu-1gb`, `BLASTIT_VIEW_MEM_ESTIMATE=140MiB` | one Django view costs about 140 MiB, so ~7 live views fit in 1 GB |
| cluster | `db-s-1vcpu-1gb` in `ams3` | a 1 GB cluster allows about 22 backends |
| roles | `app` logs in; `migrator` owns the schema | `app` never owns what it reads |
| pool | `shop-pool`, user `app`, size `18`, transaction mode | each instance opens 16 client connections and a second one would exhaust the cluster's ~22 backends, so the pool holds 18 server connections and shares them |
| URLs | web → `25061` (pool); `migrate` → `25060` (cluster) | DDL and session state do not survive transaction pooling; the pool logs in as `app` only |
| TLS | the pool leg is `verify-full` with the cluster's CA | an unverified leg would accept any certificate the pool presented |
| health | `/_blastit/health`, `initial_delay_seconds: 60` | the scheduler answers it without a view, and the first boot is slow |

## The steps

### 1. Image and manifest

Start from the [Django guide](../frameworks/python/django.md#manifest)'s `Blastfile` and
change two things: the store, and the grants — the guide's `postgres {}` denies every
statement. Only the split store carries a CA, and the platform hands out the pool as
separate bindables, so name each slot the spec fills:

```text title="Blastfile: the store block"
postgres {
    host $DATABASE_HOST
    port $DATABASE_PORT
    user $DATABASE_USERNAME
    password $DATABASE_PASSWORD
    database $DATABASE_DATABASE
    sslmode $DATABASE_SSLMODE
    ca_cert $DATABASE_CA_CERT
}
```

A block gives the app `DATABASE_HOST` (its socket directory), `DATABASE_PORT`,
`DATABASE_USERNAME` and `DATABASE_DATABASE` — never the password — and no `DATABASE_URL`,
so `settings.py` builds `DATABASES` from those four (`sslmode` is `disable` there) and the
`migrate` job keeps reading its own `DATABASE_URL`. `settings.py` reads the host the spec
passes down: `ALLOWED_HOSTS = [os.environ["ALLOWED_HOST"]]` — Django does not read
`ALLOWED_HOST` on its own.

Deploy only a file `blastit check` passes ([CI/CD](../ci.md#what-it-fails-on)).

```dockerfile title="Dockerfile"
FROM python:3.13-slim
WORKDIR /app
COPY . .
RUN pip install --no-cache-dir -r requirements.txt blastit
CMD ["blastit"]
```

```console
$ doctl registry login   # once: doctl auth init, doctl registry create <name>
$ GIT_SHA=$(git rev-parse --short HEAD)
$ docker build --platform linux/amd64 -t registry.digitalocean.com/REPLACE_WITH_REGISTRY/myshop:$GIT_SHA . && docker push registry.digitalocean.com/REPLACE_WITH_REGISTRY/myshop:$GIT_SHA
```

### 2. The cluster

```console
$ doctl databases create myshop-db --engine pg --region ams3 --size db-s-1vcpu-1gb --wait
$ export CLUSTER_ID=REPLACE_WITH_CLUSTER_ID   # the id the command above printed
```

Console: **Databases → Create → PostgreSQL**. Steps 3 and 4 take `"$CLUSTER_ID"`.

### 3. Roles and grants

`migrator` runs DDL so `app` never owns what it reads. `app` is a user **the
platform manages** (**Users & Databases → Add user**, or the first command below):
DigitalOcean generates its password and the pool carries it. `OWNER_URL` is
`doadmin`'s, copied from **Connection Details**; `MIGRATOR_PASSWORD` is yours,
hex because it is pasted into a URL.

```console
$ doctl databases user create "$CLUSTER_ID" app
$ export OWNER_URL='postgresql://doadmin:REPLACE_WITH_OWNER_PASSWORD@REPLACE_WITH_DB_HOST:25060/defaultdb?sslmode=require'
$ export MIGRATOR_PASSWORD=REPLACE_WITH_HEX   # openssl rand -hex 24
$ psql "$OWNER_URL" -v ON_ERROR_STOP=1 -v ddl="$MIGRATOR_PASSWORD" -f grants.sql
```

```sql title="grants.sql"
-- The DDL role: it owns the schema, and `app` never does.
CREATE ROLE migrator LOGIN PASSWORD :'ddl' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT CONNECT ON DATABASE defaultdb TO migrator, app;
GRANT migrator TO CURRENT_USER;
ALTER SCHEMA public OWNER TO migrator;
-- Already the default on DigitalOcean; pinned so a later role edit cannot drift.
-- As the owner: `migrator` below may not alter another role.
ALTER ROLE app SET timezone = 'UTC';
SET ROLE migrator;
GRANT USAGE ON SCHEMA public TO app;
-- Broad backstop: narrow it to one GRANT per table and verb as routes grow.
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO app;
RESET ROLE;
```

### 4. The pool

Transaction mode drops the session after each transaction: migrations and any
`SET` stay on the direct port, and an app that needs prepared statements, advisory
locks or `LISTEN` wants **Session** mode, fixed at creation. Django sets
`prepare_threshold=None`.

**Databases → myshop-db → Connection Pools → Create pool**: name `shop-pool` (every
`${db.shop-pool.*}` in the spec is this name), user `app`, db `defaultdb`,
size `18`, mode **Transaction**.

```console
$ doctl databases pool create "$CLUSTER_ID" shop-pool --db defaultdb --size 18 --user app --mode transaction
```

### 5. The spec

The web service's host, port, user, password and database are the pool's bindables (25061),
so it logs in as `app` through the pool; `DATABASE_SSLMODE` is the literal `verify-full` and
`DATABASE_CA_CERT` is the cluster's `${db.CA_CERT}` — the only value read from the cluster
itself. `migrate.DATABASE_URL` is the cluster's direct port (25060), as `migrator`.
`REPLACE_WITH_YOUR_TAG` below is this `$GIT_SHA`; `REPLACE_WITH_MIGRATOR_PASSWORD`
is the hex from step 3, pasted literally (YAML does not expand it);
`REPLACE_WITH_DB_HOST` is the private host on **Connection Details**; the two keys
are `openssl rand -base64 50`. Keep `app.yaml` out of git.

```yaml title="app.yaml"
name: myshop
region: ams  # App Platform slug; the cluster is ams3

databases:
  - { name: db, engine: PG, production: true, cluster_name: myshop-db }

jobs:
  - name: migrate
    kind: PRE_DEPLOY  # before the first web container of each deploy
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    instance_size_slug: apps-s-1vcpu-1gb
    run_command: python manage.py migrate -v0
    envs:
      # Direct cluster port as the DDL role; never the pool bindable.
      - { key: DATABASE_URL, scope: RUN_TIME, type: SECRET, value: "postgresql://migrator:REPLACE_WITH_MIGRATOR_PASSWORD@REPLACE_WITH_DB_HOST:25060/defaultdb?sslmode=require" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_THROWAWAY_SECRET" }  # any key only this job reads

services:
  - name: web
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    http_port: 8080
    instance_size_slug: apps-s-1vcpu-1gb
    health_check: { http_path: /_blastit/health, initial_delay_seconds: 60 }
    envs:
      # The pool's bindables (25061) fill the store's slots: `app`, never `doadmin`.
      - { key: DATABASE_HOST, scope: RUN_TIME, value: "${db.shop-pool.HOSTNAME}" }
      - { key: DATABASE_PORT, scope: RUN_TIME, value: "${db.shop-pool.PORT}" }
      - { key: DATABASE_USERNAME, scope: RUN_TIME, value: "${db.shop-pool.USERNAME}" }
      - { key: DATABASE_PASSWORD, scope: RUN_TIME, value: "${db.shop-pool.PASSWORD}" }
      - { key: DATABASE_DATABASE, scope: RUN_TIME, value: "${db.shop-pool.DATABASE}" }
      - { key: DATABASE_SSLMODE, scope: RUN_TIME, value: verify-full }
      # The one thing read from the cluster: the CA that signs the pool's certificate.
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: ALLOWED_HOST, scope: RUN_TIME, value: "${APP_DOMAIN}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_50_CHAR_SECRET" }
      - { key: BLASTIT_PRESET, scope: RUN_TIME, value: digitalocean-paas }
      - { key: BLASTIT_VIEW_MEM_ESTIMATE, scope: RUN_TIME, value: "140MiB" }  # one view; ~7 in 1 GB
```

### 6. Create it

```console
$ doctl apps create --spec app.yaml
```

`doctl apps list` prints the id for later updates.

## What this platform does not do for you

1. **Keep `*` honest.** A service is reachable only through DigitalOcean's edge,
   which is why `*` is safe here: expose it another way and every peer is trusted.
2. **Make the numbers yours.** `BLASTIT_MAX_WORKERS` × instances must fit the
   pool's client limit, and the app's role must not own the tables it reads
   ([the pooler](anything-else.md#pooler)).
3. **Read the ingress decision once it is live.** A header name is a guess until
   one real request has been through the edge
   ([measure it](index.md#measure-your-ingress)).

## Where next

Verify it: [measure the client address](index.md#measure-your-ingress) on the
first real request, then [fair queuing](fair-queuing.md) for the knobs. Splitting
the same app across platforms? [Docker Compose](compose-traefik.md).
