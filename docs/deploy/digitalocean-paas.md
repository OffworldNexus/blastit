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

The platform owns `PORT`, so the cap goes in the spec.

`do-connecting-ip` is the header to read here because the edge overwrites it.
`X-Forwarded-For` is *appended* to, so the preset does not read it.

!!! warning "Keep the assumption true"

    Publish no port. Any peer that reaches the container is believed, and a
    compromised sibling service can forge the client address too.

## Image

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

No `USER`: the platform starts the container as root and Blastit drops the rest.
`CMD` needs no flags: Blastit finds `./blastit.toml` and the app and preload
modules are named there.

```console
$ doctl registry login
$ docker build -t registry.digitalocean.com/yourregistry/myshop:$GIT_SHA .
$ docker push registry.digitalocean.com/yourregistry/myshop:$GIT_SHA
```

## Spec

```yaml title="app.yaml"
name: myshop
region: ams

databases:
  - { name: db, engine: PG, production: true, cluster_name: myshop-db }

jobs:
  # Runs outside blastit and the web service, as its own DDL role.
  - name: migrate
    kind: PRE_DEPLOY
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    instance_size_slug: apps-s-1vcpu-1gb
    run_command: python manage.py migrate -v0
    envs:
      - { key: DATABASE_URL, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_MIGRATION_ROLE_URL" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }

services:
  - name: web
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    http_port: 8080
    instance_count: 1
    instance_size_slug: apps-s-1vcpu-1gb
    health_check: { http_path: /_blastit/health, initial_delay_seconds: 20 }
    envs:
      - { key: DATABASE_URL, scope: RUN_TIME, type: SECRET, value: "REPLACE_WITH_APP_ROLE_URL" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_ME" }
      - { key: BLASTIT_PRESET, scope: RUN_TIME, value: digitalocean-paas }
      - { key: BLASTIT_VIEW_MEM_ESTIMATE, scope: RUN_TIME, value: "140MiB" }
      - { key: BLASTIT_PROXY_DB_MAX_CONNECTIONS, scope: RUN_TIME, value: "18" }
```

```console
$ doctl apps create --spec app.yaml
```

The manifest names the same inputs. Replace the `[proxy.db]` block from the
[Django guide](../frameworks/django.md#manifest) with:

```toml
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }
```

`ca_cert` makes the upstream TLS leg work and never reaches your app.

The `migrate` job gets only what `manage.py` needs: the URL of its own DDL role
(`migrator`, which owns the schema) and the CA. `${db.DATABASE_URL}` is the
platform's owner and belongs in neither service. If your settings module needs a
secret key just to import, give the job a throwaway value, never the web
service's.

Create both roles once, as the database owner, before the first deploy. `app`
owns nothing and has no DDL, and inherits DML on the tables `migrator` creates:

```console
$ export OWNER_URL='postgresql://doadmin:PASSWORD@db-host:25060/defaultdb?sslmode=require'
$ psql "$OWNER_URL" -v ddl="$MIGRATOR_PASSWORD" -v app="$APP_PASSWORD" <<'SQL'
CREATE ROLE migrator LOGIN PASSWORD :'ddl' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE app LOGIN PASSWORD :'app' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT CONNECT ON DATABASE defaultdb TO migrator, app;
GRANT migrator TO CURRENT_USER;
ALTER SCHEMA public OWNER TO migrator;
SET ROLE migrator;
GRANT USAGE ON SCHEMA public TO app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT USAGE, SELECT ON SEQUENCES TO app;
RESET ROLE;
SQL
```

Store `postgresql://app:$APP_PASSWORD@db-host:25060/defaultdb?sslmode=require`
as the `web` service's `DATABASE_URL` secret (`REPLACE_WITH_APP_ROLE_URL`), and
`migrator`'s URL as `REPLACE_WITH_MIGRATION_ROLE_URL`. Tighten the default
privileges to the tables and columns your manifest grants once the schema stops
changing: the proxy's role is the backstop, the per-route grants are the wall.

Migrations are a trusted-code boundary: they run unsandboxed, in the app image,
with real credentials, so pin the image and review what it installs.

## Notes for this platform

* **Scale with instances, not workers, until you know the database limit.**
  Workers × instances is the number of connections you may hold.
* **Deploys are cold.** Every deploy recreates the runtime directory and re-forks
  the `prefork` routes: keep `initial_delay_seconds` generous.
