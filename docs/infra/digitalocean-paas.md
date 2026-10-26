# DigitalOcean App Platform

```console
$ blastit --preset digitalocean-paas
```

## What it sets

| Setting | Value | What the value assumes |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `do-connecting-ip` | the edge **overwrites** it with the real client address |
| `BLASTIT_TRUSTED_PROXIES` | `*` | nothing untrusted can reach the socket |
| `BLASTIT_FAIR_QUEUING` | `on` | one burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | the platform forwards stdout and truncates a line at 2048 bytes |
| *(assumption)* | `root` | the container starts as root; a non-root user gets no capabilities ([why](anything-else.md#root-or-rootless)) |

The platform owns `PORT`; the cap goes in the spec.

## Why the peer is not the client

The socket peer is DO's ingress, not the client: the address comes from a header:

* **`do-connecting-ip`** — the edge overwrites it with the real client address,
  so nothing a client sends survives. The preset reads this one.
* **`X-Forwarded-For`** — the edge appends to it
  (`<client-supplied…>,<real client>,<edge>`), so a forged entry sits next to the
  real client.
* **`X-Forwarded-Proto`** — the edge sets it, and the preset trusts it, so
  `wsgi.url_scheme` is `https` behind TLS.
* **`X-Real-IP`, `Forwarded`, `True-Client-IP`, `X-Client-IP`** — never read.

Trusting every peer is safe only because the read header is one the edge
overwrites; the peers that can reach the container are DO's and this app's.

!!! warning "Keep the assumption true"

    Publish no port for the service: any peer that reaches the container is
    believed. A compromised sibling service in the same app can forge the client
    address too.

## A full example

```dockerfile title="Dockerfile"
FROM python:3.13-slim

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt blastit
COPY . .

ENV PYTHONUNBUFFERED=1 \
    BLASTIT_APP=myshop.wsgi:application \
    BLASTIT_PRELOAD=myshop.urls \
    BLASTIT_MANIFEST=/app/blastit.toml

EXPOSE 8080
CMD ["blastit"]
```

No `USER`: the platform starts the container as root; blastit drops the rest.

```console
$ doctl registry login
$ docker build -t registry.digitalocean.com/yourregistry/myshop:$GIT_SHA .
$ docker push registry.digitalocean.com/yourregistry/myshop:$GIT_SHA
```

```yaml title="app.yaml"
name: myshop
region: ams

databases:
  - { name: db, engine: PG, production: true, cluster_name: myshop-db }

jobs:
  # Migrations run outside blastit, with the real credentials and no manifest.
  - name: migrate
    kind: PRE_DEPLOY
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    instance_size_slug: apps-s-1vcpu-1gb
    run_command: python manage.py migrate -v0
    envs:
      - { key: DATABASE_URL, scope: RUN_TIME, value: "${db.DATABASE_URL}" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_ME" }

services:
  - name: web
    image: { registry_type: DOCR, repository: myshop, tag: "REPLACE_WITH_YOUR_TAG" }
    http_port: 8080
    instance_count: 1
    instance_size_slug: apps-s-1vcpu-1gb
    health_check: { http_path: /_blastit/health, initial_delay_seconds: 20 }
    envs:
      # The proxy's inputs: blastit keeps them and hands the app a local URL.
      - { key: DATABASE_URL, scope: RUN_TIME, value: "${db.DATABASE_URL}" }
      - { key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}" }
      - { key: DJANGO_SECRET_KEY, scope: RUN_TIME, type: SECRET, value: "REPLACE_ME" }
      - { key: BLASTIT_PRESET, scope: RUN_TIME, value: digitalocean-paas }
      - { key: BLASTIT_PROXY_DB_MAX_CONNECTIONS, scope: RUN_TIME, value: "18" }
```

```console
$ doctl apps create --spec app.yaml
```

The manifest names the same inputs:

```toml title="blastit.toml"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

[[route]]
name = "fallback"
path = ['.*']
```

`ca_cert` makes the upstream TLS leg work and never reaches your app.

## Notes for this platform

* **Scale with instances, not workers, until you know the database limit.**
  Workers × instances is the number of connections you may hold.
* **Deploys are cold.** Every deploy recreates `/run/blastit` and re-forks the
  `prefork` routes: keep `initial_delay_seconds` generous.
