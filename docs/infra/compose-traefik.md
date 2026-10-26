# Docker Compose behind Traefik

```console
$ blastit --preset compose-traefik
```

## What it sets

| Setting | Value | What the value assumes |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | Traefik **overwrites** it from a peer it does not trust, or appends its own entry |
| `BLASTIT_TRUSTED_PROXIES` | `*` | no untrusted peer can reach blastit's port |
| `BLASTIT_FAIR_QUEUING` | `on` | one burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | stdout is the container's log |
| *(assumption)* | `capabilities` | the rootless deployment: init runs as a dedicated uid with seven capabilities |

## Why `x-forwarded-for`, and why `*`

* **`x-forwarded-for` is the header, not `X-Real-Ip`.** Traefik always adds its
  own observation, so the rightmost entry is the address it saw — whether it
  overwrote the client's value or appended to it.
* **`*` reads the rightmost entry.** A list of the private ranges a Docker network
  uses is worse: when every entry is inside it, the resolver reads the
  **leftmost** one, which the client invented.
* **`X-Forwarded-Proto` is followed.** Traefik overwrites it by default, so a
  client's `https` cannot make a plaintext deployment report HTTPS, and behind the
  TLS entry point `wsgi.url_scheme` is `https`.
* **Keep a client's address out of `forwardedHeaders.trustedIPs`** unless a real
  proxy sits in front of Traefik: Traefik then keeps the client's own `X-Real-Ip`
  and `X-Forwarded-Proto`.

!!! warning "The assumption is the deployment's to keep"

    Any container on the same compose network can reach blastit, and every peer is
    trusted: publish no port for the app, and keep containers you do not trust off
    that network.

## A full example

```yaml title="dynamic.yml"
http:
  routers:
    app:
      rule: Host(`shop.example.com`)
      entryPoints: [web]
      service: app
    app-tls:
      rule: Host(`shop.example.com`)
      entryPoints: [websecure]
      service: app
      tls: {}
  services:
    app:
      loadBalancer:
        servers:
          - url: http://app:8080
```

Mount this as `dynamic.yml` and name it in the provider, because
`/etc/traefik/traefik.yml` is the *static* file and Traefik exits if a dynamic
document is there: `--providers.file.filename=/etc/traefik/dynamic.yml`.

```yaml title="compose.yaml"
name: shop

x-app-env: &app-env
  DATABASE_URL: postgresql://shop:${DB_PASSWORD:?}@db:5432/shop?sslmode=disable
  DJANGO_SECRET_KEY: ${DJANGO_SECRET_KEY:?}
  BLASTIT_PRESET: compose-traefik

services:
  db:
    image: postgres:18
    environment:
      POSTGRES_USER: shop
      POSTGRES_PASSWORD: ${DB_PASSWORD:?}
      POSTGRES_DB: shop
    healthcheck: { test: ["CMD", "pg_isready", "-U", "shop"], retries: 60 }
    volumes:
      - pgdata:/var/lib/postgresql

  # Migrations run outside blastit, as a uid of their own — never init's.
  migrate:
    image: registry.example.com/shop:${TAG:?}
    user: "10001:10001"
    environment: *app-env
    command: ["sh", "-c", "python manage.py migrate -v0"]
    depends_on:
      db: { condition: service_healthy }

  app:
    image: registry.example.com/shop:${TAG:?}
    user: "10000:10000"
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FOWNER, KILL, SETGID, SETPCAP, SETUID]
    tmpfs:
      # /run belongs to root; this is the runtime directory, owned by init's uid.
      - /run/blastit:mode=0700,uid=10000,gid=10000
    environment: *app-env
    command: ["/usr/local/bin/python3.13-init", "-m", "blastit"]
    expose: ["8080"]
    depends_on:
      migrate: { condition: service_completed_successfully }

  traefik:
    image: traefik:v3.7.16
    command:
      - --entryPoints.web.address=:80
      - --entryPoints.websecure.address=:443
      - --providers.file.filename=/etc/traefik/dynamic.yml
      - --log.level=INFO
    volumes:
      - ./dynamic.yml:/etc/traefik/dynamic.yml:ro
    ports: ["80:80", "443:443"]
    depends_on: [app]

volumes:
  pgdata:
```

No `ports:` on the app, and `user:` is a **dedicated** uid — it can run the
capable copy, so nothing else may use it.

```dockerfile title="Dockerfile"
FROM python:3.13-slim

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt blastit
RUN apt-get update && apt-get install -y --no-install-recommends libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
COPY . .

ENV PYTHONUNBUFFERED=1 \
    BLASTIT_APP=shop.wsgi:application \
    BLASTIT_PRELOAD=shop.urls \
    BLASTIT_MANIFEST=/app/blastit.toml

# The dedicated copy, with the capabilities: `chown` clears a file capability, so
# nothing re-owns the file after `setcap`.
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

Docker's `cap_add` only sets the container's *bounding* set: a non-root `user:`
process gets **no** capabilities from it. The image therefore uses file
capabilities on the copy.

```console
$ TAG=$(git rev-parse --short HEAD)
$ docker build -t registry.example.com/shop:$TAG . && docker push registry.example.com/shop:$TAG
$ DB_PASSWORD=… DJANGO_SECRET_KEY=… TAG=$TAG docker compose up -d
$ docker compose logs app | head -1
{"event":"boot","stage":"stage0","seccomp":true,"privilege":"capabilities","bounding_set":"dropped"}
```

## Rootless

Rootless needs these six, plus the seventh when the platform can give it:

| Capability | For |
|---|---|
| `cap_setuid`, `cap_setgid` | `setresuid`/`setresgid`, `setgroups` |
| `cap_chown`, `cap_fowner` | the view directories and sockets, and their modes |
| `cap_dac_override` | opening a view's `0700` directory to bind its socket |
| `cap_kill` | `kill(2)` on a view process |
| `cap_setpcap` | emptying the children's bounding set (wanted, not required) |

Give them to a **dedicated copy** of the file init starts from — mode `0500`,
owned by init's uid, next to the original. A file with capabilities turns any
process of its uid into init, so the boot refuses a deployment where a shared
name carries them, and a standalone install starts the copy and points
`BLASTIT_EXEC` at the capability-free binary.
