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

## Image

The image copies `blastit.toml`, so edit it before building: replace the
`[proxy.db]` block from the [Django guide](../frameworks/django.md#manifest) with
this one — same inputs, plus the CA. `ca_cert` is a PEM bundle; it only matters
with `sslmode=verify-full` and never reaches your app.

```toml
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }
```

```dockerfile title="Dockerfile"
FROM python:3.13-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt blastit
COPY . .
ENV PYTHONUNBUFFERED=1
EXPOSE 8080
CMD ["blastit"]
```

```console
$ doctl registry login
$ GIT_SHA=$(git rev-parse --short HEAD)
$ docker build -t registry.digitalocean.com/yourregistry/myshop:$GIT_SHA .
$ docker push registry.digitalocean.com/yourregistry/myshop:$GIT_SHA
```

`REPLACE_WITH_YOUR_TAG` below is this `$GIT_SHA`; `yourregistry` is your DOCR
name, and `repository: myshop` is the image name inside it.

## Cluster, roles and the pooler

Create the cluster once (`doctl databases create myshop-db --engine pg --region
ams3 --size db-s-1vcpu-1gb`, or the console); every `doctl` step takes its id:
`CLUSTER_ID=$(doctl databases list --format ID,Name --no-header | awk
'$2=="myshop-db"{print $1}')`. Create `app` as a **database user the platform
manages** (**Users & Databases → Add user**, or `doctl databases user create
"$CLUSTER_ID" app`): its password is the pool's. `OWNER_URL` is the owner's URI
(`doadmin`, port 25060) from the console or `doctl databases connection
"$CLUSTER_ID" --format URI --no-header --show`:

```console
$ export OWNER_URL='postgresql://doadmin:<password>@<host>:25060/defaultdb?sslmode=require'
$ export MIGRATOR_PASSWORD='choose-a-long-password'
$ psql "$OWNER_URL" -v ddl="$MIGRATOR_PASSWORD" <<'SQL'
CREATE ROLE migrator LOGIN PASSWORD :'ddl' NOSUPERUSER NOCREATEDB NOCREATEROLE;
ALTER ROLE app SET timezone = 'UTC';
GRANT CONNECT ON DATABASE defaultdb TO migrator, app;
GRANT migrator TO CURRENT_USER;
ALTER SCHEMA public OWNER TO migrator;
SET ROLE migrator;
GRANT USAGE ON SCHEMA public TO app;
-- Broad backstop: narrow it to one GRANT per table and verb.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT USAGE, SELECT ON SEQUENCES TO app;
RESET ROLE;
SQL
```

The role default is `UTC`, so Django sends no `TimeZone` statement and the
`connection` group's `set_config` grant is a `dead_grant` here (only CI needs it).

Create the pool: **Databases → myshop-db → Connection Pools → Create pool**,
name `shop-pool` (it must match `${db.shop-pool…}` below), user `app`, database
`defaultdb`, size `18` backend connections. Transaction mode is the default and
**cannot be changed later**: an app that prepares statements or holds advisory
locks needs `--mode session` at creation ([pool modes](https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/)). `doctl databases pool create
"$CLUSTER_ID" shop-pool --db defaultdb --size 18 --user app` does the same; add
`--mode session` for those apps. `doctl databases pool list "$CLUSTER_ID"
--format URI --no-header --show` prints the pool's URL.

The pool listens on **25061**, the cluster on **25060**: the app uses the pool
and `migrate` the cluster, because DDL and session state do not survive
transaction pooling. Django prepares nothing (`prepare_threshold=None`).

## Spec

```yaml title="app.yaml"
name: myshop
region: ams  # App Platform slug; the cluster is ams3

databases:
  - { name: db, engine: PG, production: true, cluster_name: myshop-db }

jobs:
  # Outside blastit and the web service, as its own DDL role, on the direct port.
  - name: migrate
    kind: PRE_DEPLOY
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    instance_size_slug: apps-s-1vcpu-1gb
    run_command: python manage.py migrate -v0
    envs:
      # Direct cluster port as the DDL role; never the pool bindable.
      - { key: DATABASE_URL, scope: RUN_TIME, type: SECRET, value: "postgresql://migrator:REPLACE_WITH_MIGRATOR_PASSWORD@REPLACE_WITH_DB_HOST:25060/defaultdb?sslmode=require" }
      # Only if your settings module imports one; never the web service's.
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_THROWAWAY_SECRET" }

services:
  - name: web
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    http_port: 8080
    instance_count: 1
    instance_size_slug: apps-s-1vcpu-1gb
    health_check: { http_path: /_blastit/health, initial_delay_seconds: 20 }
    envs:
      # The pool's URL (port 25061). `${db.DATABASE_URL}` is the direct one.
      - { key: DATABASE_URL, scope: RUN_TIME, value: "${db.shop-pool.DATABASE_URL}" }
      - { key: ALLOWED_HOST, scope: RUN_TIME, value: "${APP_DOMAIN}" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_50_CHAR_SECRET" }
      - { key: BLASTIT_PRESET, scope: RUN_TIME, value: digitalocean-paas }
      - { key: BLASTIT_VIEW_MEM_ESTIMATE, scope: RUN_TIME, value: "140MiB" }  # one view; ~7 in 1 GB
```

Fill the placeholders before deploying: `REPLACE_WITH_50_CHAR_SECRET`
(`openssl rand -base64 50`), the job's throwaway key, and the migrator URL's
password and host with `$MIGRATOR_PASSWORD` and the host from `OWNER_URL`.
`app.yaml` now holds secrets — keep it out of git. `apps create` fails once the
name exists; later edits use `doctl apps update <app-id> --spec app.yaml`.

```console
$ doctl apps create --spec app.yaml
```

## Notes for this platform

* **Deploys are cold.** Every deploy recreates the runtime directory and re-forks
  the `prefork` routes: keep `initial_delay_seconds` generous.
