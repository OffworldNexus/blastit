# DigitalOcean App Platform

```console
$ blastit --preset digitalocean-paas
```

or, for a process you do not start yourself:

```console
$ BLASTIT_PRESET=digitalocean-paas blastit
```

The preset covers a deployment with one App Platform component and a managed
PostgreSQL in the same app. Every value it supplies is a default: set the
variable yourself and yours wins.

## What it sets

| Setting | Value | What the value assumes |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `do-connecting-ip` | the one header DO's edge **overwrites** with the real client address, so a client cannot put anything in it |
| `BLASTIT_TRUSTED_PROXIES` | `*` | **nothing untrusted can reach the socket.** A service is only reachable through DO's load balancer and the app's own internal services |
| `BLASTIT_FAIR_QUEUING` | `on` | one client's burst must not starve the others on a single small instance |
| `BLASTIT_LOG_FORMAT` | `json` | the platform forwards stdout to its own log service and truncates a line at 2048 bytes |

and two assumptions it does not turn into settings:

| | | |
|---|---|---|
| privilege mode it assumes | `root` | App Platform starts the container as root, and cannot grant capabilities to a non-root user — so a rootless deployment is not available here ([why](anything-else.md#root-or-rootless)) |
| deliberately not set | `PORT`, `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | the platform owns the port; a connection cap names a proxy only the manifest knows |

## Why the peer is not the client

The socket peer blastit sees on App Platform is **DigitalOcean's ingress** —
`10.244.x.x` on one cluster, `100.127.x.x` on another — not the client. So the
client address has to come from a header, and the whole question is which one
may be believed.

Measured on App Platform ([§10](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#10-platform-constraints-digitalocean-app-platform)):

| Header | What arrives |
|---|---|
| `do-connecting-ip` | **overwritten by the edge** with the real client address |
| `X-Real-IP` | stripped |
| `X-Forwarded-For` | appended to: `<client-supplied…>,<real client>,<Cloudflare edge>` |
| `Forwarded`, `True-Client-IP`, `X-Client-IP` | pass through **forged** |

`do-connecting-ip` is therefore the one header an operator can believe: the
edge replaces it, so nothing a client sends can survive in it. It is not
`x-forwarded-for`, because the edge *appends* to that one — a forged leftmost
entry would sit in the chain right next to the real client.

**Trusting every peer is safe here for one reason, and it is not the peer
list.** What makes it safe is that the header being read is one the edge
overwrites. The peer list only has to cover the hops that can reach the
container, and every one of them is DO's or this app's — the load balancer, and
the app's own internal services. Trusting every peer is then the simplest rule
that also covers service-to-service calls, with no second rule for "internal".

!!! warning "The assumption is the deployment's to keep"

    Two things break it:

    * **Exposing blastit's port another way.** If anything outside DO's load
      balancer and the app's internal network can open a connection to the
      container's port, that peer is trusted, and it can claim any client
      address. Do not publish the port, do not add a proxy in front that you do
      not control.
    * **A component you do not trust, on the app's network.** Every other
      service in the same app can reach this one and is believed — that is what
      makes internal calls work without a second rule. It also means a
      compromised sibling service can forge the client address as seen by this
      one.

Nothing in blastit can detect either; the preset's job stops at saying what it
assumed, and it does.

`X-Real-IP`, `Forwarded`, `True-Client-IP`, `X-Client-IP` and `CF-Connecting-IP`
are **not read at all** — blastit reads exactly one header,
`BLASTIT_CLIENT_IP_HEADER`, and only from a peer it trusts. That is the whole
of the rule; there is no fallback list.

One more consequence of reading an overwritten header: `100.64.0.0/10` is also
carrier-grade NAT client space, so a real client can arrive *from* that range.
That is fine now, and it is not treated as a proxy range anywhere: the address
in `REMOTE_ADDR` is simply the address the edge last wrote, which is the
client's.

## Platform constraints that matter

| Constraint | What it means for you |
|---|---|
| gVisor, kernel reported as `4.4.0`, some syscalls missing | blastit's internal traffic is Unix sockets only; the seccomp filters are measured working here |
| the container runs as **root, PID 1**, with Docker's default capabilities minus `NET_RAW` | exactly what blastit needs. Your image must **not** set a non-root `USER` |
| `no_new_privs` is set on every container | `sudo` refuses to start; `USER root` + `CMD ["blastit"]` is the way. blastit only ever *drops* privileges, which `no_new_privs` allows |
| no `CAP_SYS_ADMIN` / `CAP_NET_ADMIN` | no namespaces, no cgroups of our own, no per-uid firewall. Isolation is uids, file modes, rlimits and seccomp |
| `/proc/<pid>/environ` is readable across uids | real credentials are never in `environ`: they go to the database proxy over a pipe, and it is `execve`'d with an empty environment |
| root filesystem is a writable 9p overlay; `/tmp` and `/dev/shm` are tmpfs | blastit's runtime directories live under `/run/blastit`, which is disk, not RAM, and is recreated on every start |
| forwarded log lines are truncated at 2048 bytes | blastit keeps every event under 2048 bytes and re-chunks long app output |
| amd64 only | one image, one architecture |
| `MemTotal` is the instance size, cgroup v1 accounting is available, `cpu_count()` overstates | the view-process cap is derived from memory at boot, and never from the CPU count. It is in the `preload` boot event |
| managed PostgreSQL plans allow about 20 connections | set `BLASTIT_PROXY_DB_MAX_CONNECTIONS` (see [memory and process caps](anything-else.md#memory-and-process-caps)) |

## A full example

A Django project, one web service, a pre-deploy job for migrations, and a
managed database. Replace `myshop` with your app's name.

### The Dockerfile

```dockerfile title="Dockerfile"
FROM python:3.13-slim

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# blastit is on PyPI: a compiled extension plus a `blastit` command, installed
# like any other dependency.
RUN pip install --no-cache-dir blastit

COPY . .

ENV PYTHONUNBUFFERED=1 \
    BLASTIT_APP=myshop.wsgi:application \
    BLASTIT_PRELOAD=myshop.urls \
    BLASTIT_MANIFEST=/app/blastit.toml

EXPOSE 8080

# No `USER`: App Platform starts the container as root, and blastit drops every
# other process to its own unprivileged user. A non-root `USER` would leave
# blastit with nothing to drop.
CMD ["blastit"]
```

Build it with the repository as the context, exactly as any other App Platform
image:

```console
$ doctl registry login
$ docker build -t registry.digitalocean.com/yourregistry/myshop:$GIT_SHA .
$ docker push registry.digitalocean.com/yourregistry/myshop:$GIT_SHA
```

### The app spec

```yaml title="app.yaml"
name: myshop
region: ams

databases:
  - name: db
    engine: PG
    production: true
    cluster_name: myshop-db

jobs:
  # Schema changes run outside blastit, with the real credentials: migrations
  # have no manifest and should not be granted one.
  - name: migrate
    kind: PRE_DEPLOY
    image:
      registry_type: DOCR
      repository: myshop
      tag: "REPLACE_WITH_YOUR_TAG"
    instance_size_slug: apps-s-1vcpu-1gb
    run_command: python manage.py migrate -v0
    envs:
      - key: DATABASE_URL
        scope: RUN_TIME
        value: ${db.DATABASE_URL}
      - key: DATABASE_CA_CERT
        scope: RUN_TIME
        value: ${db.CA_CERT}
      - key: DJANGO_SECRET_KEY
        scope: RUN_TIME
        type: SECRET
        value: "REPLACE_WITH_A_LONG_RANDOM_STRING"

services:
  - name: web
    image:
      registry_type: DOCR
      repository: myshop
      tag: "REPLACE_WITH_YOUR_TAG"
    http_port: 8080
    instance_count: 1
    instance_size_slug: apps-s-1vcpu-1gb
    health_check:
      # The scheduler answers this one itself, so a health check covers
      # http -> scheduler without depending on any view process.
      http_path: /_blastit/health
      initial_delay_seconds: 20
    envs:
      # `DATABASE_URL` and `DATABASE_CA_CERT` are the proxy's *inputs*: blastit
      # takes them, keeps them, and hands the app a local URL instead.
      - key: DATABASE_URL
        scope: RUN_TIME
        value: ${db.DATABASE_URL}
      - key: DATABASE_CA_CERT
        scope: RUN_TIME
        value: ${db.CA_CERT}
      - key: DJANGO_SECRET_KEY
        scope: RUN_TIME
        type: SECRET
        value: "REPLACE_WITH_A_LONG_RANDOM_STRING"
      # The preset is the deployment's identity, so it belongs in the spec.
      - key: BLASTIT_PRESET
        scope: RUN_TIME
        value: digitalocean-paas
      - key: BLASTIT_PROXY_DB_MAX_CONNECTIONS
        scope: RUN_TIME
        value: "18"
```

```console
$ doctl apps create --spec app.yaml
```

Then the first three lines of the service's log should say which mode and which
preset are in force:

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped"}
{"event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":"digitalocean-paas","preset_settings":{…},"preset_overridden":[]}
```

### The manifest, for that deployment

```toml title="blastit.toml"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

[[route]]
name = "products"
path = ['/products/.*']
methods = ["GET", "HEAD"]
[route.db.tables]
shop_product = { select = "*" }

[[route]]
name = "fallback"
path = ['.*']
```

`ca_cert` is optional, and DO's managed databases want it: the proxy uses it for
the upstream TLS leg. It never reaches your app.

## Notes for this platform

* **`instance_size_slug` and `instance_count`.** One instance and
  `apps-s-1vcpu-1gb` are enough to start, and the preset's fair queuing keeps a
  small instance fair between clients. Each instance is its own blastit, with
  its own uid layout; nothing is shared between them, and
  `BLASTIT_MAX_WORKERS` is per instance.
* **Scale by adding instances, not workers, until you are sure about the
  database.** `BLASTIT_MAX_WORKERS` × instances must stay under the plan's
  connection limit, which is what
  `BLASTIT_PROXY_DB_MAX_CONNECTIONS` is for.
* **Console output is your only log.** Nothing is written to disk. `doctl apps
  logs --type run` (or the dashboard) gives you the event stream; `pretty`
  formatting is for humans on a terminal, not for the platform.
* **Deploys are cold.** Every deploy recreates `/run/blastit`, re-forks the
  view processes marked `prefork = true` and re-derives the memory cap — so the
  first requests after a deploy pay for one fork each (12–60 ms measured here),
  and the health check's `initial_delay_seconds` should stay generous.

## Next

If your ingress is not App Platform, read
[the parameters, and how they articulate](anything-else.md).
