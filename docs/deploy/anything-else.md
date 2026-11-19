# Anywhere else

Kubernetes, a plain VM, Docker without Traefik: the same [model](index.md) with your own values. The
two presets are default sets, not modes. This page is the table of levers, the sections that need
more than a row, then three recipes.

## The levers

| Lever | Default | Change it when | If it is wrong |
|---|---|---|---|
| `PORT` | `8080`, bound on `0.0.0.0` | your platform assigns a port | the platform forwards to a port nothing listens on; a port published directly hands every peer the trust `*` gives |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | your platform requires a fixed path | a probe hits a route instead, and 404s restart a healthy app |
| `BLASTIT_MAX_WORKERS` | `16` | the database's or pool's client limit is lower | each worker holds one upstream connection: too high exhausts the pool, too low queues |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | one route should not hold every thread | one hot view takes the whole budget |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | your framework costs more (Django: about `140MiB`) | the derived process cap overcommits and memory pressure retires the views you needed |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | you know better than the divisor | too high is memory pressure; too low and a new route waits for a retirement |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` of `total` | your app is legitimately large | idle views are retired under pressure, and a container already above it answers `503` |
| `BLASTIT_MAX_BODY_BYTES` | `100 MiB` | you take uploads | the largest body × concurrent uploads has to fit inside the limit |
| `BLASTIT_FAIR_QUEUING` | `off` | any client you do not control | one scraper owns the FIFO and everyone waits behind its backlog |
| `BLASTIT_QUEUE_SIZE` / `BLASTIT_QUEUE_TIMEOUT` | `256` / `10s` | your latency budget differs | past the queue the answer is `503` with `Retry-After` |
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | your edge writes another header | every client looks like the load balancer: fair queuing is one flow, and logs lie |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | your ingress connects from another address | an unlisted ingress is ignored and the header discarded; `*` trusts whoever can reach the port |
| `BLASTIT_PROXY_DB_MAX_CONNECTIONS` | `= MAX_WORKERS` | the pool has fewer clients to give | the proxy waits for a connection instead of opening one, and requests slow down under load |
| `BLASTIT_UID_BASE` | `20000` | the range collides inside the container | two roles share a uid and the boundaries between them collapse |
| `BLASTIT_SECCOMP` | `1` | only on an exotic kernel, for debugging | `0` removes the syscall and egress wall, and both presets abort the boot on it |
| `egress any` | off, per route | that route calls an API | without it `socket(AF_INET)` fails `EPERM`; with it, every address and port, cloud metadata included |
| `BLASTIT_VIEW_SPAWN` | `lazy` | first-request latency matters | `eager` forks every view at boot; `prefork` does it per route |

[Settings](../reference/settings.md) lists the rest.

## Root or capabilities { #root-or-rootless }

| Mode | When you get it | What it needs |
|---|---|---|
| `root` | the container or unit starts Blastit as uid 0 | `SETUID`, `SETGID`, `CHOWN`, `FOWNER`, `DAC_OVERRIDE`, `KILL` |
| `capabilities` | anything else | those six plus `SETPCAP`, on a dedicated copy of the file init starts from |

The mode is detected, not configured: the first `boot` event says which one is in
force, and production refuses to start when it can get neither.

Pick `root` when your platform gives the container root: one less moving part. Pick `capabilities`
when a root process is not allowed: Docker's `--cap-add` gives a non-root `user:` process
**nothing**, and `no_new_privs` ignores file capabilities, so the way there is a launcher that drops
the uid itself:

```console
$ setpriv --reuid=1000 --regid=1000 --init-groups \
      --inh-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      --ambient-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      blastit
```

Without `SETPCAP` the boot still runs, but no child's bounding set is emptied:
the `boot` event says `"bounding_set": "kept"` and warns.

## The client address { #client-ip-header }

Blastit believes `BLASTIT_CLIENT_IP_HEADER` only from a peer in `BLASTIT_TRUSTED_PROXIES`. For a
chain header, `*` takes the **rightmost** entry — what the closest hop added. Narrowing the list to
a network is worse: if every entry is in it, Blastit falls back to the leftmost one, which the
client invented ([client address](../reference/settings.md#client-address)).

A header name is a guess until one request has been through the real ingress.
Send it carrying a value your edge must overwrite:

```console
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/products/
```

Then read `remote_addr` in the `request` event; the sample is trimmed (`ts`,
`level`, `component` and `request_id` elided):

```json
{"event":"request","method":"GET","path":"/products/","route":"catalog","status":200,"remote_addr":"203.0.113.7"}
```

It must be your public address. `6.6.6.6` means the ingress trusted a
client-supplied entry; a `10.x` or `172.x` address means the header names the
load balancer. Either way `BLASTIT_CLIENT_IP_HEADER` must name the header your
edge **sets**, and `BLASTIT_TRUSTED_PROXIES` the address it connects from.

## The pooler { #pooler }

A pool is a connection multiplier, not a tuning knob, and a deployment concern: no `Blastfile`
directive declares one. `BLASTIT_MAX_WORKERS` × instances is the client count it must accept, and
`DEFAULT_POOL_SIZE` (PgBouncer) the server connections it keeps. A managed pool's `size` is its
**minimum**, and its maximum follows the cluster's RAM — about 25 connections per GB, 3 reserved
([behind a connection pooler](../reference/settings.md#behind-a-connection-pooler)).

Only the app's URL names the pool. DDL and session state do not survive
transaction pooling, so migrations and grants connect to the database's direct
port; on a managed cluster the two URLs differ by port, and the pool's URI has
the pool's name as its path. A pool in **session** mode keeps `SET`, prepared
statements, advisory locks, `LISTEN`/`NOTIFY` and temp tables — at the cost of
one server connection per client, which is exactly what a pool was for.

### Session state through a transaction pool { #session-state }

An open pooled backend keeps the session settings it logged in with, so a
connect-time `SET` is not yours to rely on:

- **Put the zone on the role.** `ALTER ROLE app SET timezone = 'UTC'` — both
  recipes do it (DigitalOcean's roles already default to `UTC`), and every pooled
  session inherits it.
- **A URL cannot do it.** `options=-c timezone=UTC` is refused: the proxy
  allowlists the startup parameters, so a URL cannot set session state or smuggle
  a credential ([postgresql stores](../reference/blastfile.md#stores)).
- **Read it back as the app's own role on the direct port, never the pool.** It
  must print exactly `UTC`; if it does not, set it on the role as the owner and
  read again.

```console
$ psql 'postgresql://app:REPLACE_WITH_APP_PASSWORD@REPLACE_WITH_DB_HOST:5432/REPLACE_WITH_DB' -A -t -c 'SHOW TimeZone'
```
- **Changed a live role? Restart the pooler.** Open backends keep the zone they logged in with
  until they recycle (an hour by default). On App Platform the pool is created after the role, so
  the order matters.
- **A `UTC` database makes the trait grant a `dead_grant`.** Django then sends no
  `set_config('TimeZone', …)`: remove that grant where `check` runs against such a database, and
  keep it where the database says otherwise ([what `check` fails on](../ci.md#what-it-fails-on)).

## Migrations { #migrations }

Schema changes run outside Blastit, with the real credentials, as their own role on the database's
direct URL — a job before the first container, a task in your pipeline. Grants cover what a request
needs: a route that could also run `ALTER TABLE` could rewrite its own permission model. Both
recipes default-grant for tables a later migration adds; narrow that as routes grow.

## Egress { #egress }

A view opens no TCP connection unless its route says `egress any`, and then it reaches **any**
IPv4/IPv6 address and port, cloud metadata included. `postgresql` reaches the network through
`dbproxy` instead, which holds the credential and enforces the grants.

## The edge and TLS { #edge-and-tls }

Your cloud ingress terminates TLS and HTTP/2 and forwards plain HTTP/1.1 to Blastit. It sets
`X-Forwarded-Proto`, which Blastit reads from a trusted peer only, so `wsgi.url_scheme` is right.
Every health check goes to `BLASTIT_HEALTH_PATH`: the scheduler answers it, from no view.

## Memory and process caps { #memory-and-process-caps }

- **Give the container a memory limit.** `total` is the smaller of `MemTotal` and the cgroup limit;
  `BLASTIT_MEM_HIGH_WATERMARK` is a percentage of it, and `BLASTIT_VIEW_MEM_ESTIMATE` divides it
  into a process cap. On Kubernetes that is `resources.limits.memory`.
- **Retiring never fails a request.** Idle views go after `BLASTIT_VIEW_IDLE_TIMEOUT` (`600s`);
  `BLASTIT_VIEW_MAX_REQUESTS` (`0`, off) retires one after that many.
- **Size the body ceiling for the worst case**, not the average: the largest upload × how many
  are in flight has to fit.

## Plain Docker

One image per runtime, each with its own `HEALTHCHECK`; the `docker run` below is the same for all
three:

=== "Python (WSGI)"
    ```dockerfile
    FROM python:3.13-slim
    WORKDIR /app
    COPY . .
    RUN pip install --no-cache-dir -r requirements.txt blastit
    HEALTHCHECK CMD python3 -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080/_blastit/health')"
    CMD ["blastit"]
    ```

=== "PHP"
    ```dockerfile
    FROM debian:trixie-slim
    RUN apt-get update && apt-get install -y --no-install-recommends \
            ca-certificates curl libphp-embed php-pgsql \
        && rm -rf /var/lib/apt/lists/*
    WORKDIR /app
    COPY . .
    RUN curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz \
        && tar -xzf blastit-*.tar.gz -C /usr/local --strip-components=1 \
        && rm blastit-*.tar.gz
    HEALTHCHECK CMD curl -fsS http://127.0.0.1:8080/_blastit/health
    CMD ["blastit"]
    ```

=== "Static & SPA"
    ```dockerfile
    FROM debian:trixie-slim
    RUN apt-get update && apt-get install -y --no-install-recommends \
            ca-certificates curl \
        && rm -rf /var/lib/apt/lists/*
    WORKDIR /app
    COPY . .
    RUN curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz \
        && tar -xzf blastit-*.tar.gz -C /usr/local --strip-components=1 \
        && rm blastit-*.tar.gz
    HEALTHCHECK CMD curl -fsS http://127.0.0.1:8080/_blastit/health
    CMD ["blastit"]
    ```

```console
$ docker run -d --name shop --env-file .env \
    -p 127.0.0.1:8080:8080 --memory 1g \
    registry.example.com/shop:1.2.3
```

No `--health-cmd`: each image carries its own `HEALTHCHECK`, so the runtime it probes never matters
to the command line. No `--user` either: Docker's default set is what root mode needs. `.env` holds
`DATABASE_URL` and `BLASTIT_TRUSTED_PROXIES=172.18.0.2` (the address your ingress connects from), so
neither is in your shell history. `-p 127.0.0.1:` keeps the *only* listener on loopback, behind your
ingress.

## systemd

```ini title="/etc/systemd/system/shop.service"
[Unit]
Description=shop (blastit)
After=network-online.target

[Service]
User=shop
Group=shop
WorkingDirectory=/srv/shop
EnvironmentFile=/etc/shop/blastit.env
ExecStart=/srv/shop/.venv/bin/blastit
Restart=always

# The six capabilities the boot needs, plus the one that empties the children's
# bounding set — as *ambient* capabilities, so no file carries them.
AmbientCapabilities=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP
NoNewPrivileges=true

RuntimeDirectory=blastit
RuntimeDirectoryMode=0700
ReadWritePaths=/run/blastit
ProtectSystem=strict

[Install]
WantedBy=multi-user.target
```

```ini title="/etc/shop/blastit.env"
DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
DJANGO_SECRET_KEY=…
PORT=8080
BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1
```

`WorkingDirectory` is where Blastit finds `./Blastfile`. `User=shop` must not
be a uid the layout hands out. The environment file holds secrets: mode `0600`,
owned by the service user.

## Kubernetes

Blastit is one container and wants to be PID 1 in it, which is the default; the
part to decide is `securityContext`.

```yaml title="deployment.yaml"
apiVersion: apps/v1
kind: Deployment
metadata: { name: shop }
spec:
  replicas: 2
  selector: { matchLabels: { app: shop } }
  template:
    metadata: { labels: { app: shop } }
    spec:
      containers:
        - name: shop
          image: registry.example.com/shop:1.2.3
          ports: [{ name: http, containerPort: 8080 }]
          env:
            - name: DATABASE_URL
              valueFrom: { secretKeyRef: { name: shop-db, key: url } }
            - name: BLASTIT_TRUSTED_PROXIES
              value: "*"
          readinessProbe:
            httpGet: { path: /_blastit/health, port: http }
            initialDelaySeconds: 15
          livenessProbe:
            httpGet: { path: /_blastit/health, port: http }
          securityContext:
            runAsUser: 0
            capabilities:
              drop: ["ALL"]
              add: ["SETUID", "SETGID", "CHOWN", "FOWNER", "DAC_OVERRIDE", "KILL", "SETPCAP"]
          resources:
            limits: { memory: 1Gi }
```

```yaml title="networkpolicy.yaml"
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: shop }
spec:
  podSelector: { matchLabels: { app: shop } }
  ingress:
    # Name your ingress controller's namespace: without this policy every pod
    # in the cluster can reach :8080 and forge the client address.
    - from:
        - namespaceSelector:
            matchLabels: { kubernetes.io/metadata.name: ingress-nginx }
      ports: [{ protocol: TCP, port: 8080 }]
```

The scheduler answers the health path itself, so a probe does not depend on any
view. `resources.limits.memory` becomes `total` for the derived cap; under the
`restricted` Pod Security Standard Blastit does not run.

## Where next

[Back to the model](index.md) · [the pooler settings](../reference/settings.md#behind-a-connection-pooler) · [fair queuing](fair-queuing.md).
