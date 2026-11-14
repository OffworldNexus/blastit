# DigitalOcean App Platform

```console
$ blastit --preset digitalocean-paas
```

## What it sets
| Setting | Value | Why |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `do-connecting-ip` | the edge **overwrites** it with the real client address |
| `BLASTIT_TRUSTED_PROXIES` | `*` | nothing untrusted can reach the socket |
| `BLASTIT_FAIR_QUEUING` | `on` | one burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | the platform forwards stdout and truncates a line at 2048 bytes |
| *(assumption)* | `root` | the container starts as root; a non-root user gets no capabilities |

## Image and manifest
In the [Django guide](../frameworks/django.md#manifest)'s `[proxy.db]`, change only the `input` line to
`input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }` — the CA is read only for `verify-full`, not for DO's `require`.

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
$ docker build --platform linux/amd64 -t registry.digitalocean.com/yourregistry/myshop:$GIT_SHA . && docker push registry.digitalocean.com/yourregistry/myshop:$GIT_SHA
```

`REPLACE_WITH_YOUR_TAG` below is this `$GIT_SHA`; `yourregistry` is your DOCR, and App Platform runs `linux/amd64`.

## Cluster, roles and the pooler
A cluster, three roles and a pool. The app connects to the **pool** (25061) and the migrator to the
**cluster** (25060): DDL and session state do not survive transaction pooling, and the pool logs in as `app` only.

| Name | Connects to | Port | URL from | Used by |
|---|---|---|---|---|
| `doadmin` | cluster | 25060 | Connection Details | you, once, with `psql` |
| `migrator` | cluster | 25060 | `app.yaml` literal | the `migrate` job |
| `app` | pool `shop-pool` | 25061 | `${db.shop-pool.DATABASE_URL}` | the `web` service |

### 1. Create the cluster
```console
$ doctl databases create myshop-db --engine pg --region ams3 --size db-s-1vcpu-1gb --wait
$ export CLUSTER_ID=<id>   # the id the create command printed above
```

Console: **Databases → Create → PostgreSQL**; `doctl databases list` prints the id again, and every step below takes `"$CLUSTER_ID"`.

### 2. Create the app role
```console
$ doctl databases user create "$CLUSTER_ID" app
```

A user **the platform manages** (**Users & Databases → Add user**); DigitalOcean generates its password and the pool carries it.

### 3. Owner, migrator, grants
`migrator` runs DDL so `app` never owns what it reads; `OWNER_URL` is on **Connection Details**, and the
password is hex (`openssl rand -hex 24`) because it goes into a URL.

```console
$ export OWNER_URL='postgresql://doadmin:<password>@<host>:25060/defaultdb?sslmode=require'
$ export MIGRATOR_PASSWORD=<hex>
$ psql "$OWNER_URL" -v ON_ERROR_STOP=1 -v ddl="$MIGRATOR_PASSWORD" -f grants.sql
```

```sql title="grants.sql"
-- The DDL role: it owns the schema, and `app` never does.
CREATE ROLE migrator LOGIN PASSWORD :'ddl' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT CONNECT ON DATABASE defaultdb TO migrator, app;
GRANT migrator TO CURRENT_USER;
ALTER SCHEMA public OWNER TO migrator;
SET ROLE migrator;
GRANT USAGE ON SCHEMA public TO app;
-- Broad backstop: narrow it to one GRANT per table and verb as routes grow.
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO app;
RESET ROLE;
```

`ALTER ROLE app SET timezone = 'UTC'` is a no-op on DO, whose roles already default to `UTC`; Django then sends no
`set_config('TimeZone', …)`, so the `connection` group's grant for it is a `dead_grant` here ([the zone rule](index.md#pooler)).
Keep it for CI, whose Postgres says `Etc/UTC`.

### 4. Create the pool
#### Why a pooler, and what transaction mode cannot do
- Each instance opens 16 client connections, and a 1 GB cluster allows about 22 backends: a second
  instance would exhaust it, so the pool holds 18 server connections and shares them.
- A transaction pool returns the session after each transaction. Prepared statements, advisory locks,
  `LISTEN`/`NOTIFY`, temp tables and `SET` do not survive it; an app that needs them wants **Session**
  mode, fixed at creation. Django sets `prepare_threshold=None`.

**Databases → myshop-db → Connection Pools → Create pool**: name `shop-pool` (the spec's
`${db.shop-pool.DATABASE_URL}` must match), user `app`, db `defaultdb`, size `18`, mode **Transaction**.

```console
$ doctl databases pool create "$CLUSTER_ID" shop-pool --db defaultdb --size 18 --user app --mode transaction
```

### 5. Wire the two URLs
`web.DATABASE_URL` takes the pool bindable `${db.shop-pool.DATABASE_URL}` (25061) and `${db.CA_CERT}`;
`migrate.DATABASE_URL` is the cluster's, as `migrator`. The [spec](#spec) below carries both.

## Spec
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
      # The pool's URL (25061). `${db.DATABASE_URL}` is the cluster as `doadmin`.
      - { key: DATABASE_URL, scope: RUN_TIME, value: "${db.shop-pool.DATABASE_URL}" }
      - { key: ALLOWED_HOST, scope: RUN_TIME, value: "${APP_DOMAIN}" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_50_CHAR_SECRET" }
      - { key: BLASTIT_PRESET, scope: RUN_TIME, value: digitalocean-paas }
      - { key: BLASTIT_VIEW_MEM_ESTIMATE, scope: RUN_TIME, value: "140MiB" }  # one view; ~7 in 1 GB
```

`REPLACE_WITH_DB_HOST` is the private host on **Connection Details**; `REPLACE_WITH_MIGRATOR_PASSWORD`
is the password from step 3, pasted literally (YAML does not expand it); the two keys are
`openssl rand -base64 50`. Keep `app.yaml` out of git; `doctl apps list` prints the id for updates.

```console
$ doctl apps create --spec app.yaml
```
