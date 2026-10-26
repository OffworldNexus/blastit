# Anything else

Kubernetes, a plain VM, Docker without Traefik, nginx in front, a platform
nobody measured: everything is a combination of
[the settings](../reference/settings.md), and this page is the articulation.

It is ordered the way a deployment gets decided: the client-IP header and the
trusted peers first (getting them wrong makes every log and every rate limit a
lie), then privileges, then the caps, then logs, then the uid layout, then four
worked examples.

## Client IP and trusted proxies

Two variables, and one rule between them.

| Variable | Default | What it is |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the one header the client address is read from, and only from a trusted peer |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | the peers whose client-IP header may be believed: IP addresses, CIDR networks, or `*` |

**The peer decides whether anything may be believed.** A peer that is not in
`BLASTIT_TRUSTED_PROXIES` **is** the client, whatever its headers say. Only a
trusted peer's chain can name somebody else.

From a trusted peer, the chain is read **from the right**:

1. an entry that is itself a trusted peer is skipped — it is infrastructure;
2. the first entry that is not a trusted peer is the client;
3. if every entry is trusted, the **leftmost** is the client.

With `*` — every peer trusted — the rule is simply the **rightmost** entry.
That is deliberate, and it is the difference from granian's scheduler, which
takes `chain[0]` when every peer is trusted: wherever the edge *appends* to a
header the client already sent, the leftmost entry is the client's own
invention, and the rightmost is what the hop closest to blastit added — the one
thing that hop cannot be lying about.

Three more details that decide real deployments:

* **An unreadable entry makes the whole header worthless.** A chain entry is a
  bare address (`1.2.3.4`), or an IPv6 literal in brackets (`[::1]`). Anything
  else — `1.2.3.4:5678`, `example.com`, `unknown`, `for=1.2.3.4` — and the
  header is ignored and the peer is the client. Reading half of a value we
  cannot read is how a forgery becomes an address.
* **No header, or an empty one**, means the peer is the client.
* **A peer that is not an address at all** (an unnamed Unix socket) is not
  trusted.

<!-- E2b -->
`REMOTE_ADDR` is a bare IP address in both cases: an address a trusted header
named, or the peer's own. The peer's port, when it arrived over a socket, is in
`REMOTE_PORT`, so code that compares `REMOTE_ADDR` to a list of addresses does
not have to strip anything.

`BLASTIT_TRUSTED_PROXIES` **refuses what it cannot read** rather than dropping
it: `127.0.0.1:8080` is a boot error, not an entry. A trusted-proxy list that
silently trusts nothing when the operator meant it to trust something is a
deployment that believes a forged header from the very peer it meant to
distrust. An empty list (`BLASTIT_TRUSTED_PROXIES=`) trusts nobody.

`wsgi.url_scheme` follows the same trust decision: `X-Forwarded-Proto` is read
only from a trusted peer, and only the last value, and only if it is exactly
`http` or `https`. **That is a rule, not a guarantee.** It holds only where the
ingress overwrites the header (or where its own observation is the one that ends
up last): measured for Traefik v3, unmeasured for DO's edge — see
[the App Platform caveat](digitalocean-paas.md#what-it-sets). If a client can
send its own `X-Forwarded-Proto` and the peer is trusted, the value is the
client's.

### Working it out for your own ingress

Three questions, in this order.

**1. What does my ingress do with the header?** Two behaviours decide
everything:

* **it overwrites** the header it manages (Traefik by default, DO's edge with
  `do-connecting-ip`): a client cannot put anything in it, and either the
  rightmost or the only entry is the truth;
* **it appends** to a header the client may have sent (nginx's
  `$proxy_add_x_forwarded_for`, Traefik with `forwardedHeaders.trustedIPs`):
  the chain then starts with whatever the client invented, and the **rightmost**
  entry is this hop's own observation.

Test it from outside with this throwaway route and look at what comes back:

```python title="whoami.py"
import json


def application(environ, start_response):
    body = json.dumps(
        {
            "remote_addr": environ.get("REMOTE_ADDR"),
            "scheme": environ.get("wsgi.url_scheme"),
            "http_x_forwarded_for": environ.get("HTTP_X_FORWARDED_FOR"),
            "http_do_connecting_ip": environ.get("HTTP_DO_CONNECTING_IP"),
        },
        indent=2,
    ).encode()
    start_response("200 OK", [("Content-Type", "application/json")])
    return [body]
```

```toml title="blastit.toml"
version = 1

[[route]]
name = "whoami"
path = ['/whoami']
```

```console
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/whoami
```

If `remote_addr` comes back as `6.6.6.6`, you are reading a header your ingress
passes through forged, and **which header that is has just been measured** — do
not ship it.

**2. Which header should blastit read?** The one your ingress *overwrites*, if
there is one. Otherwise the one it appends to — read rightmost, which is what
`BLASTIT_TRUSTED_PROXIES=*` does, and what a list does as long as the closest
hop's own address is a trusted entry and the client's is not.

**3. Which peers are trusted?** The addresses your ingress connects *from*, and
nothing else:

| Deployment | `BLASTIT_TRUSTED_PROXIES` |
|---|---|
| nginx on the same host as blastit | `127.0.0.1,::1` |
| nginx in another container, same network | that container's address on the network, or the network's CIDR |
| Kubernetes ingress controller | the node or pod CIDR that reaches the pod — measure it with the route above; the peer is often a node address after SNAT |
| Traefik in the same compose network | `*`, because the app publishes no port (see [`compose-traefik`](compose-traefik.md#why-x-forwarded-for-and-why)) |
| App Platform | `*`, because the app is only reachable through DO's load balancer (see [`digitalocean-paas`](digitalocean-paas.md#why-the-peer-is-not-the-client)) |

!!! danger "Do not put a range that can hold your clients in the list"

    If the client's own address is inside `BLASTIT_TRUSTED_PROXIES`, then every
    entry of the chain can look like infrastructure — and the resolver then
    takes the **leftmost**, which is the entry a client appended. `10/8`, the
    docker bridge network and `100.64.0.0/10` are all traps for this reason.
    List the *ingress*, not the neighbourhood.

If the list is wrong in the other direction — too narrow — the peer is reported
as the client, and `REMOTE_ADDR` becomes your ingress's address. That is
visible, and harmless; the dangerous direction is the one above.

**`BLASTIT_TRUST_FORWARDED` is deprecated.** `0` is `BLASTIT_TRUSTED_PROXIES`
empty, `1` is `*` — which is what it always meant: the rightmost entry of
anybody's `X-Forwarded-For`. Either way it is reported once as a warn
`app.output` line at startup, and `BLASTIT_TRUSTED_PROXIES` wins when both are
set.

## Root or rootless

blastit asks the kernel which privileges it has before it does anything, so the
mode is **detected, not configured**, and reported in the first event.

| Mode | When you get it | What it needs |
|---|---|---|
| `root` | the container or unit starts blastit as uid 0 | the container's own capability set: `SETUID`, `SETGID`, `CHOWN`, `FOWNER`, `DAC_OVERRIDE`, `KILL`. Docker's defaults minus `NET_RAW` are enough |
| `capabilities` | anything else | the six it requires, and only those — `cap_setuid`, `cap_setgid`, `cap_chown`, `cap_fowner`, `cap_dac_override`, `cap_kill` — plus `cap_setpcap` when the platform can give it |

Pick `root` when your platform gives the container root and you have not been
asked to do better: it is what App Platform and most Kubernetes namespaces do,
and it is one less moving part. Pick `capabilities` when the platform or your
own policy will not have a root process — and read
[the rootless section of the compose page](compose-traefik.md#rootless), which
is the recipe: the capability belongs on a *dedicated copy* of the file init
starts from, that uid becomes a dedicated identity, and `/run/blastit` has to
belong to it.

Three things make rootless more work, and all three are worth knowing before
you choose it:

* **Docker's `--cap-add` does not give a `user:` process anything.** It only
  limits the container's *bounding* set. Measured on Docker 29.5 (compose v5):
  `CapPrm`, `CapEff` and `CapAmb` are all zero for a non-root `user:` with
  `cap_add`. Two mechanisms do work: file capabilities on a dedicated copy, and
  ambient capabilities set by a launcher that starts as root
  (`setpriv --inh-caps … --ambient-caps …`), which is also the way on a platform
  that sets `no_new_privs`, where the kernel ignores file capabilities.
* **A platform that sets `no_new_privs`** (`allowPrivilegeEscalation: false` on
  Kubernetes, App Platform's containers) ignores file capabilities entirely, and
  Kubernetes cannot set the ambient set for you. So the rootless route there is
  a launcher that starts as root and drops the uid — which is root mode with one
  extra step.
* **The dedicated uid.** With file capabilities, the uid that can run the capable
  copy must not run anything else: running the copy is becoming init. A worker, a
  migration, or an operator's shell sharing that uid is init. Ambient
  capabilities do not have this problem — nothing carries the capability, so any
  process of that uid inherits it only from whatever started it.

`cap_setpcap` is wanted rather than required. Without it, the children's other
sets are still cleared and `no_new_privs` still stops an `execve` from granting
what the bounding set allows, and the boot says `"bounding_set": "kept"` with
one warning.

Nothing else is needed: blastit only ever *lowers* an `rlimit`, and binding
`$PORT` needs no privilege at all.

## Memory and process caps

Two independent limits, and they are about different resources.

```mermaid
flowchart LR
    subgraph clients["incoming requests"]
        q["admission queue<br/>BLASTIT_QUEUE_SIZE"]
    end
    subgraph budget["BLASTIT_MAX_WORKERS = 16"]
        t1["request thread"]
        t2["request thread"]
        t3["…"]
    end
    subgraph procs["BLASTIT_MAX_VIEW_PROCESSES (derived from memory)"]
        v1["view `catalog`"]
        v2["view `login`"]
    end
    q --> t1
    q --> t2
    q --> t3
    t1 --> v1
    t2 --> v2
    t3 --> v1
```

| Variable | Default | What it limits |
|---|---|---|
| `BLASTIT_MAX_WORKERS` | `16` | **request threads alive across every view**, globally. One worker slot is one in-flight request |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | request threads in one view process. It also sizes that uid's `RLIMIT_NPROC` (`+16` for the interpreter's own threads) |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | **live view processes.** When a new one is needed, the least-recently-used *evictable* one is retired |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | the starting divisor of the derived cap, and the floor of the measured one |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | above this much of `total` memory, one idle view process is retired per 2 s check |
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | upstream connections of one proxy, upper-cased (`BLASTIT_PROXY_DB_MAX_CONNECTIONS`) |

**Size `BLASTIT_MAX_WORKERS` ≤ the database plan's connection limit**, because
each request thread can hold one upstream connection. A managed 1 GiB
PostgreSQL plan allows about 20; blastit's default of 16 fits, `BLASTIT_MAX_WORKERS=64`
does not.

**The view-process cap is derived from memory, and then measured.** The
`preload` boot event reports what it worked out:

```json
{"event":"boot","stage":"preload","mem_total":1057259520,"mem_used":211812352,"max_view_processes":23,"immortalize":"on","immortalized":148213}
```

`total` is `min(MemTotal, cgroup limit)`; the reserve is `max(64 MiB, 10 % of
total)`, kept for the kernel, the platform's agents and the next fork. After each
view process reports `READY`, the scheduler samples aggregate usage again and
re-derives the cap from what one view really costs on *your* app — which is where
the number usually moves a long way: a Django app under gVisor copies about
140 MiB per view process, against the 32 MiB default, and a cap of 45 derived
from the default bounds nothing. That measurement replaces only the *derived*
default; `BLASTIT_MAX_VIEW_PROCESSES` still wins, and `process.cap` is logged
when the measured cost or the cap changes.

Two consequences worth planning for:

* **`BLASTIT_VIEW_SPAWN=lazy` (the default)** means only the routes that are
  actually hit cost memory. A route marked `prefork = true` in the manifest is
  forked at startup and skips the first-hit fork latency — 12–60 ms measured on
  App Platform, 0.5–7 ms on plain Linux. Mark the two or three hottest routes,
  not all of them.
* **Retiring is never allowed to fail a request.** The cap only retires a
  process that is `READY`, has nothing in flight, and has nothing queued for its
  route; when nothing is evictable the request waits in the queue instead.

`BLASTIT_VIEW_IDLE_TIMEOUT` (default `600s`) retires a view process that has
been idle that long, and `BLASTIT_VIEW_MAX_REQUESTS` (default `0`, off) retires
one after that many requests — useful as a slow leak valve, at the cost of a
fork per N requests.

## Fair queuing

`BLASTIT_FAIR_QUEUING=on` orders the admission queue fairly instead of by
arrival, so one client's burst cannot starve the others. Both presets turn it
on; on a single small instance it is the difference between one slow client and
everybody.

Off (`0`, the default) is exactly a FIFO, and the `BLASTIT_FQ_*` knobs are then
never read. With it on, the defaults are granian's:

| Variable | Default | What it does |
|---|---|---|
| `BLASTIT_FQ_OUTER_PREFIX_V4` / `_V6` | `24` / `56` | how much of a client address an outer flow keeps (a /24, a /56) |
| `BLASTIT_FQ_BURST` | `20ms` | what a client may spend before it starts queueing behind the others |
| `BLASTIT_FQ_MAX_DEBT` | `5s` | how far ahead of the queue's virtual time one flow may borrow |
| `BLASTIT_FQ_DEBT_TTL` | `30s` | how long an idle flow with debt is remembered |
| `BLASTIT_FQ_INNER_HEADERS` | `user-agent,accept-language,sec-ch-ua,sec-ch-ua-platform,sec-ch-ua-mobile,authorization` | headers hashed into the inner flow key |
| `BLASTIT_FQ_INNER_COOKIES` | `sessionid,session` | cookies hashed into the inner flow key |
| `BLASTIT_FQ_INNER_INCLUDE_IP` | `1` | whether the full client address is part of the inner key |
| `BLASTIT_FQ_OUTER_MAX`, `BLASTIT_FQ_INNER_MAX_PER_OUTER`, `BLASTIT_FQ_INNER_MAX` | `65536`, `1024`, `262144` | table sizes; a flow that does not fit shares one overflow flow |

An **outer** flow is a network (/24, /56): one network cannot starve another. An
**inner** flow is one browser behind a NAT, keyed on a per-process hash of the
headers and cookies above: one client behind a NAT cannot starve its neighbours.
The header and cookie *values* are never logged, and neither is the hash.

A knob that is out of range is **clamped, not refused** — a prefix cannot be
longer than the address it masks, a table cannot hold zero flows — and the boot
says so. Fair queuing keys on the client address, so it is only as honest as
[your trusted proxies](#client-ip-and-trusted-proxies).

## Logs

stdout is blastit's only output, and it is an **event stream: one event per
line**. Nothing is written to disk.

| Variable | Default | |
|---|---|---|
| `BLASTIT_LOG_FORMAT` | `json` | `json` for a log collector, `pretty` for a human on a terminal |
| `BLASTIT_LOG_LEVEL` | `info` | the minimum level printed |

* **`json`** is the one to ship: one JSON object per line, which is what
  `blastit-manifest fold` reads back.
* **`pretty`** prints the same events as coloured, aligned lines — the child
  events (`request`, `sql.denied`) included. It is for
  running blastit by hand, and for a `systemd` journal you actually read.
* **Every line is kept under 2048 bytes**, because App Platform truncates
  forwarded lines and a truncated JSON line is unparseable: long fields are cut
  and named in a `"truncated"` array, and long app output is split into several
  `app.output` events carrying `part` and `parts`.
* **init is the only writer.** Children send structured events over a pipe and
  their stdout/stderr over pipes, and init stamps `component` and `route`
  according to which pipe the data came from — so a compromised view cannot
  forge another component's events. The most it can do is print lines labelled
  as its own app output.
* **What is never logged** is query strings, header values, request and response
  bodies, and SQL parameter values. This is an API guarantee, not a convention:
  those fields cannot be expressed in the event model.

See [log events](../reference/log-events.md) for every event and field.

## The uid layout

`BLASTIT_UID_BASE` defaults to `20000` and everything else is derived from it:

| Process | uid |
|---|---|
| init | `0`, or the uid it was started as |
| http | `UID_BASE + 1` |
| scheduler | `UID_BASE + 2` |
| dbproxy | `UID_BASE + 3` |
| view for route *i* | `UID_BASE + 1000 + i` |

* **Nobody needs an `/etc/passwd` entry**: `setuid()` only needs `CAP_SETUID`,
  which is measured working on App Platform.
* Every process gets `gid == uid` and an empty supplementary group list.
* The uids stay below 65536 for portability, although App Platform maps the
  full 32-bit range.
* **`i` is the route's index in the manifest**, so reordering routes renumbers
  them. That is fine: the uids live as long as the container.
* **init may not run as a uid the layout hands out.** The boot refuses
  `UID_BASE + 1…3` or any route's uid: that process would share init's identity.
  This matters when you pick a service uid for a rootless deployment — `10000`
  is a comfortable choice.
* Change the base if `20000–24000` collides with something in your container or
  on your host (`/etc/passwd` entries you care about, another sandbox runtime).

## Worked examples

### Kubernetes

blastit is one container, and it wants to be PID 1 in it, which is the default.
The part that needs thinking about is `securityContext`.

```yaml title="deployment.yaml"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: shop
spec:
  replicas: 2
  selector:
    matchLabels: { app: shop }
  template:
    metadata:
      labels: { app: shop }
    spec:
      containers:
        - name: shop
          image: registry.example.com/shop:1.2.3
          ports:
            - { name: http, containerPort: 8080 }
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef: { name: shop-db, key: url }
            - name: DATABASE_CA_CERT
              valueFrom:
                secretKeyRef: { name: shop-db, key: ca_cert }
            - name: BLASTIT_FAIR_QUEUING
              value: "on"
            # The ingress is the only peer that may name a client. Measure which
            # address it connects from (see above) rather than guessing; the pod
            # CIDR is the usual answer.
            - name: BLASTIT_TRUSTED_PROXIES
              value: "10.244.0.0/16"
          readinessProbe:
            httpGet: { path: /_blastit/health, port: http }
            initialDelaySeconds: 15
          livenessProbe:
            httpGet: { path: /_blastit/health, port: http }
            periodSeconds: 10
          securityContext:
            # blastit binds the port and then drops every other process to its
            # own unprivileged user, so the container itself starts as root.
            runAsUser: 0
            capabilities:
              drop: ["ALL"]
              add: ["SETUID", "SETGID", "CHOWN", "FOWNER", "DAC_OVERRIDE", "KILL"]
          resources:
            requests: { cpu: 250m, memory: 512Mi }
            limits: { memory: 1Gi }
```

* **`readinessProbe` on `/_blastit/health`** (the default
  `BLASTIT_HEALTH_PATH`): the scheduler answers it itself, so the probe covers
  http → scheduler without depending on any view process. Raising
  `initialDelaySeconds` after a cold start is normal.
* **`resources.limits.memory`** becomes `total` for the derived view-process
  cap when the cgroup limit is lower than the machine's memory, and the `preload`
  boot event reports which one it used.
* **`BLASTIT_TRUSTED_PROXIES`** has to name the ingress, not a big range. If the
  ingress controller is in the cluster and the pod CIDR is `10.244.0.0/16`, use
  that. Do not use `0.0.0.0/0`: that is `*` with extra steps, and it is only
  safe when nothing untrusted can reach the pod — which in Kubernetes means
  there is a NetworkPolicy that says so.
* **`capabilities.add` here only sets the bounding set**, which is what a *root*
  process needs to have in its permitted/effective set — Kubernetes adds them
  for a root container. For a rootless Deployment you would need
  `runAsNonRoot: true` **and** `allowPrivilegeEscalation: true` (so that file
  capabilities on the copy are honoured), which the restricted Pod Security
  Standard forbids. Under `restricted`, blastit does not run today: it needs
  either root or a capability set the standard will not grant. Under `baseline`
  or an unlabelled namespace, the manifest above works.

Scale with `replicas`, not with `BLASTIT_MAX_WORKERS`, until you are sure about
the database's connection limit: workers × replicas is the number of connections
blastit may hold.

### Plain Docker

```console
$ docker run -d --name shop \
    -e DATABASE_URL='postgresql://shop:secret@db:5432/shop?sslmode=disable' \
    -e DATABASE_CA_CERT="$(cat ca.pem)" \
    -e BLASTIT_TRUSTED_PROXIES=172.18.0.2 \
    -p 127.0.0.1:8080:8080 \
    --health-cmd 'python -c "import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:8080/_blastit/health\")"' \
    --health-interval 10s --health-start-period 15s \
    registry.example.com/shop:1.2.3
```

* **No `--user`.** Docker's default capability set is enough for root mode;
  `--cap-drop ALL` without adding `SETUID`, `SETGID`, `CHOWN`, `FOWNER`,
  `DAC_OVERRIDE` and `KILL` is not.
* **`BLASTIT_TRUSTED_PROXIES=172.18.0.2` is a placeholder**, and the one value
  here you have to work out: it is the address your ingress connects *from*, as
  this container sees it (the address of the nginx container, not of the host or
  of the client). Add a route for
  [the `/whoami` probe](#working-it-out-for-your-own-ingress), ask through the
  ingress, and read the address the container reports.
* **The health check runs inside the image**, so it may only use what the image
  has. `python` is always there — the line above uses only the standard library
  — while `curl` and `wget` are not part of `python:3.13-slim`, which is why the
  check is written as a `python -c` one-liner.
* **Publish on loopback only** (`127.0.0.1:8080:8080`) and let an ingress
  reach it, or make sure nothing else can: with `BLASTIT_TRUSTED_PROXIES=*` a
  published port hands the client-IP header to whoever can reach it.
* **`--tmpfs /run/blastit:mode=0700,uid=1000,gid=1000`** when you run rootless:
  `/run` belongs to root.
* `--read-only` works — nothing is written outside `/run/blastit` and its
  private view directories — but the project directory and its virtualenv have
  to stay readable by the view uids.

### systemd

Rootless, with systemd providing both the capabilities and the runtime
directory — the least amount of `setcap` a deployment can get away with:

```ini title="/etc/systemd/system/shop.service"
[Unit]
Description=shop (blastit)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=shop
Group=shop
WorkingDirectory=/srv/shop
EnvironmentFile=/etc/shop/blastit.env
ExecStart=/srv/shop/.venv/bin/blastit
Restart=always
RestartSec=2

# The six capabilities the boot needs, plus the one that lets it empty its
# children's bounding set. As *ambient* capabilities, so nothing carries them
# on a file and the interpreter stays capability-free.
AmbientCapabilities=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP

# /run/blastit, owned by User=, which is what a rootless init cannot make itself.
RuntimeDirectory=blastit
RuntimeDirectoryMode=0700

# Nothing blastit starts may gain privileges it did not hold.
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/run/blastit

[Install]
WantedBy=multi-user.target
```

```ini title="/etc/shop/blastit.env"
DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
DJANGO_SECRET_KEY=…
BLASTIT_APP=shop.wsgi:application
BLASTIT_PRELOAD=shop.urls
BLASTIT_MANIFEST=/srv/shop/blastit.toml
PORT=8080

# The client address, from whatever is in front: nginx on this host, say.
BLASTIT_CLIENT_IP_HEADER=x-forwarded-for
BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1
```

```console
$ sudo install -d -o shop -g shop /srv/shop /etc/shop
$ sudo install -m 0600 -o shop -g shop /dev/null /etc/shop/blastit.env   # then edit it
$ sudo systemctl daemon-reload && sudo systemctl enable --now shop
$ journalctl -u shop -f -o cat      # the event stream, one line per event
```

Notes:

* **`EnvironmentFile`** is where every setting lives — the same file the
  [Django page](../frameworks/django.md#run-it-locally) uses for a local run,
  which is the point of keeping it out of the unit. `systemctl show shop -p
  Environment` prints what systemd resolved, secrets included, so treat it like
  any other file with a password in it (`0600`, owned by the service user).
* **`AmbientCapabilities`** is what makes this work without a `setcap` copy:
  systemd sets the inheritable and ambient sets before it changes the uid, and
  ambient capabilities survive the exec of an unprivileged file. `User=shop`
  must not be a uid the layout hands out (`20000`–`24000` with the default
  `BLASTIT_UID_BASE`), and the boot refuses it if it is.
* **`RuntimeDirectory=blastit`** creates `/run/blastit`, mode `0700`, owned by
  `User=`, and removes it on stop — the same thing the compose file's `tmpfs`
  does.
* **`NoNewPrivileges=true` is fine here**, because the capabilities were granted
  before the uid change rather than by a file attribute.
* Logs go to the journal because blastit writes to stdout, and `journalctl`
  reads them as JSON with `-o json` if you want a collector to have them.

### nginx in front

nginx appends to `X-Forwarded-For` by default with
`proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for`, and it must be
told what scheme the client used, because nginx does not overwrite
`X-Forwarded-Proto` for you:

```nginx title="/etc/nginx/sites-enabled/shop"
server {
    listen 443 ssl;
    server_name shop.example.com;

    ssl_certificate     /etc/letsencrypt/live/shop.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/shop.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;

        # `$proxy_add_x_forwarded_for` (nginx's default) keeps whatever the
        # client sent and appends nginx's own peer address after it, so the
        # client's invention stays in the chain. Stating the value instead makes
        # the chain exactly one entry long.
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Host $host;

        # blastit does not speak WebSocket or Upgrade at all (it answers 501),
        # so do not try to upgrade through it.
    }
}
```

Those are two lines in the service's environment, whichever way you run it —
the [systemd unit](#systemd) above has them in `/etc/shop/blastit.env`:

```ini
BLASTIT_CLIENT_IP_HEADER=x-forwarded-for
BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1
```

`proxy_set_header X-Forwarded-For $remote_addr` **states** the value rather than
appending to whatever the client sent — the shortest way to make the chain one
entry long and forged-proof. If you keep nginx's default
`$proxy_add_x_forwarded_for` instead, believe the rightmost entry: that is what
a single trusted peer in `BLASTIT_TRUSTED_PROXIES` does, because the entry nginx
added is at the end.

Two things nginx must not do here:

* **Terminate TLS and leave `$scheme` at `http`.** Then `wsgi.url_scheme` is
  `http` in your app, and Django's `SECURE_PROXY_SSL_HEADER`,
  `request.is_secure()` and every absolute URL it builds disagree with the
  browser. `proxy_set_header X-Forwarded-Proto $scheme` fixes it.
* **Be the second hop of two.** If Cloudflare or another proxy sits in front of
  nginx, that one's address is what nginx reports, and you have to decide which
  entry of a two-entry chain to believe — this is the case where you write
  `BLASTIT_TRUSTED_PROXIES` as the two hops and let the resolver walk from the
  right, rather than trusting everything.

## See also

* [The settings reference](../reference/settings.md) — every variable, with
  defaults, including the HTTP limits, timeouts and health path.
* [Troubleshooting](../troubleshooting.md) — what a symptom means.
