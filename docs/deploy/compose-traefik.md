# Docker Compose behind Traefik

```console
$ blastit --preset compose-traefik
```

## What it sets

| Setting | Value | Why |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | Traefik always adds its own observation; the rightmost entry is what it saw |
| `BLASTIT_TRUSTED_PROXIES` | `*` | no untrusted peer can reach Blastit's port |
| `BLASTIT_FAIR_QUEUING` | `on` | one burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | stdout is the container's log |
| *(assumption)* | `capabilities` | the rootless deployment: a dedicated uid with seven capabilities |

`*` reads the **rightmost** entry; [client address](../reference/settings.md#client-address)
says why a private-range list is worse. Traefik overwrites `X-Forwarded-Proto`,
so `wsgi.url_scheme` is `https` behind its TLS entry point.

!!! warning "The assumption is the deployment's to keep"

    Publish no port for the app, and keep containers you do not trust off that
    network: every peer is trusted.

## Traefik

```yaml title="dynamic.yml"
http:
  routers:
    app:
      rule: Host(`shop.example.com`)
      entryPoints: [web]
      service: app
      middlewares: [redirect-to-tls]
    app-tls:
      rule: Host(`shop.example.com`)
      entryPoints: [websecure]
      service: app
      tls: {}
  middlewares:
    redirect-to-tls:
      redirectScheme: { scheme: https }
  services:
    app:
      loadBalancer:
        servers:
          - url: http://app:8080
```

Mount this as `dynamic.yml` and name it in the provider:
`--providers.file.filename=/etc/traefik/dynamic.yml`. (`traefik.yml` is the
static file; Traefik exits if a dynamic document is there.) The `tls: {}` router
serves Traefik's self-signed certificate: add a `certificatesResolvers` block for
ACME, or mount your own certificate before exposing it. `shop.example.com` is
your host: change it in both rules **and** in `ALLOWED_HOST`;
`registry.example.com` is your registry, here and in every `image:` line.

## Values

Create `.env` and git-ignore it before the first build; every `${...:?}` is in
it:

```dotenv title=".env"
DB_OWNER_PASSWORD=REPLACE_WITH_OWNER_PASSWORD
DB_PASSWORD=REPLACE_WITH_APP_PASSWORD
DJANGO_SECRET_KEY=REPLACE_WITH_50_CHAR_SECRET
TAG=REPLACE_WITH_GIT_SHA
```

`openssl rand -hex 16` twice fills the passwords, `openssl rand -base64 50` the
key; `TAG` is `git rev-parse --short HEAD`. Load the file into your shell
(`set -a; . ./.env; set +a`) so the build and `up` share it. Never commit it.

## Compose

The manifest is the [Django guide](../frameworks/django.md#manifest)'s:
`[proxy.db] input = { url = "DATABASE_URL" }` (`ca_cert` only for a network
database, with `sslmode=verify-full`).

```yaml title="compose.yaml"
name: shop

x-app-env: &app-env
  # The pool's host, not `db`.
  DATABASE_URL: postgresql://app:${DB_PASSWORD:?}@pgbouncer:5432/shop?sslmode=disable
  # Must equal `Host(...)` in dynamic.yml, or Django answers 400.
  ALLOWED_HOST: shop.example.com
  DJANGO_SECRET_KEY: ${DJANGO_SECRET_KEY:?}
  BLASTIT_PRESET: compose-traefik
  BLASTIT_VIEW_MEM_ESTIMATE: 140MiB  # one view; ~7 in 1 GB

services:
  db:
    image: postgres:18
    environment:
      POSTGRES_USER: owner
      POSTGRES_PASSWORD: ${DB_OWNER_PASSWORD:?}
      POSTGRES_DB: shop
    healthcheck: { test: ["CMD", "pg_isready", "-U", "owner"], retries: 60 }
    volumes:
      - pgdata:/var/lib/postgresql

  # The pool. App -> here; migrations -> db, because DDL and session state do
  # not survive transaction pooling.
  pgbouncer:
    image: edoburu/pgbouncer:v1.26.1-p0
    environment:
      DB_HOST: db
      DB_USER: app
      DB_PASSWORD: ${DB_PASSWORD:?}
      DB_NAME: shop
      # md5 (the image default) fails against PostgreSQL 18's SCRAM passwords.
      AUTH_TYPE: scram-sha-256
      POOL_MODE: transaction
      MAX_PREPARED_STATEMENTS: "200"
      DEFAULT_POOL_SIZE: "20"
      MAX_CLIENT_CONN: "100"
    healthcheck:
      test: ["CMD", "pg_isready", "-h", "127.0.0.1", "-p", "5432", "-U", "app", "-d", "shop"]
      retries: 60
    depends_on:
      db: { condition: service_healthy }

  # The image ships no HEALTHCHECK; `pg_isready` on the pool's own port is the
  # portable probe, and `app` never dials a pool that is not listening.

  # The schema owner, used by migrations only: its own DATABASE_URL, never the
  # web service's. Trusted code, run outside blastit.
  migrate:
    image: registry.example.com/shop:${TAG:?}
    # Not init's uid: a migration bug cannot run the capable copy.
    user: "10001:10001"
    environment:
      DATABASE_URL: postgresql://owner:${DB_OWNER_PASSWORD:?}@db:5432/shop?sslmode=disable
      DJANGO_SECRET_KEY: ${DJANGO_SECRET_KEY:?}
    command: ["sh", "-c", "python manage.py migrate -v0"]
    depends_on:
      db: { condition: service_healthy }

  # Creates the non-owner `app` role and grants it the union of your manifest's
  # grants, once the tables exist.
  grant:
    image: postgres:18
    environment:
      OWNER_URL: postgresql://owner:${DB_OWNER_PASSWORD:?}@db:5432/shop?sslmode=disable
      APP_PASSWORD: ${DB_PASSWORD:?}
    command: ["sh", "-c", "psql \"$$OWNER_URL\" -v ON_ERROR_STOP=1 -v pw=\"$$APP_PASSWORD\" -f /grants.sql"]
    volumes: ["./db/grants.sql:/grants.sql:ro"]
    depends_on:
      migrate: { condition: service_completed_successfully }

  app:
    image: registry.example.com/shop:${TAG:?}
    user: "10000:10000"
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FOWNER, KILL, SETGID, SETPCAP, SETUID]
    mem_limit: 1g
    tmpfs:
      # /run belongs to root; this is the runtime directory, owned by init's uid.
      - /run/blastit:mode=0700,uid=10000,gid=10000
    environment: *app-env
    command: ["/usr/local/bin/python3.13-init", "-m", "blastit"]
    expose: ["8080"]
    depends_on:
      grant: { condition: service_completed_successfully }
      pgbouncer: { condition: service_healthy }

  traefik:
    image: traefik:v3.7.16
    command:
      - --entryPoints.web.address=:80
      - --entryPoints.websecure.address=:443
      - --providers.file.filename=/etc/traefik/dynamic.yml
    volumes:
      - ./dynamic.yml:/etc/traefik/dynamic.yml:ro
    ports: ["80:80", "443:443"]
    depends_on: [app]

volumes:
  pgdata:
```

```sql title="db/grants.sql"
-- One GRANT per table and verb your blastit.toml grants; add columns when the
-- grant is narrower than the table. `grant` runs with ON_ERROR_STOP=1 and `app`
-- waits for it: replace `shop_product` (a table this repo does not create) or
-- the job aborts the deploy before the app starts. Re-run it after migrations
-- that add a table.
DO $$ BEGIN
  CREATE ROLE app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
ALTER ROLE app PASSWORD :'pw';
-- Every pooled session inherits this; a connect-time set_config would not.
ALTER ROLE app SET timezone = 'UTC';
GRANT CONNECT ON DATABASE shop TO app;
GRANT USAGE ON SCHEMA public TO app;
GRANT SELECT, INSERT, UPDATE, DELETE ON shop_product TO app;
```

No `ports:` on the app, and `user:` is a **dedicated** uid: it can run the
capable copy below, so nothing else may use it. The app's `DATABASE_URL` names
`pgbouncer`; `migrate` and `grant` name `db`. Never swap the two. `ALLOWED_HOST`
must equal the `Host(...)` in `dynamic.yml`, or Django answers `400`.

`db/grants.sql` sets `ALTER ROLE app SET timezone = 'UTC'`, so Django sends no
`set_config` and the `connection` group's grant for it is a `dead_grant` here
([the zone rule](index.md#pooler)). `sslmode=disable` is plaintext on the
compose bridge: for a network database, set `sslmode=verify-full` **and** a
`ca_cert` ([pooling and TLS](../reference/settings.md#behind-a-connection-pooler)).

## Image

Docker's `cap_add` only sets the container's *bounding* set: a non-root `user:`
process gets **no** capabilities from it. The image therefore puts file
capabilities on a dedicated copy of the interpreter.

```dockerfile title="Dockerfile"
FROM python:3.13-slim

WORKDIR /app
COPY requirements.txt .
RUN apt-get update && apt-get install -y --no-install-recommends libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir -r requirements.txt blastit
COPY . .

ENV PYTHONUNBUFFERED=1

# `chown` clears a file capability, so nothing re-owns the file after `setcap`.
ARG BLASTIT_UID=10000
RUN useradd --uid "$BLASTIT_UID" --user-group --create-home --shell /usr/sbin/nologin blastit \
    && interpreter="$(readlink -f /usr/local/bin/python3.13)" \
    && cp "$interpreter" "${interpreter%/*}/python3.13-init" \
    && chown "$BLASTIT_UID:$BLASTIT_UID" "${interpreter%/*}/python3.13-init" \
    && chmod 0500 "${interpreter%/*}/python3.13-init" \
    && setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
         "${interpreter%/*}/python3.13-init"

EXPOSE 8080
```

Blastit refuses a deployment where a shared interpreter carries those
capabilities; [the security model](../security.md#the-mechanisms) says why.

`--env-file .env` supplies all four values, so none reaches your shell history;
the compose file reads each with `${VAR:?}`.

```console
$ docker build -t registry.example.com/shop:$TAG . && docker push registry.example.com/shop:$TAG
$ docker compose --env-file .env up -d
$ docker compose logs app | head -1
{"event":"boot","stage":"stage0","seccomp":true,"privilege":"capabilities","bounding_set":"dropped"}
```

## Rootless

A file with capabilities turns any process of its uid into init;
[the security model](../security.md#the-mechanisms) says why.
