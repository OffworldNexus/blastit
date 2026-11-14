# Settings

Everything is environment variables, plus the manifest. A set `BLASTIT_*`
variable wins over the manifest and over a preset, even when empty, and `PORT`
counts the same. `--preset` is a flag; `--port` belongs to `blastit dev`.

## The app and the manifest

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_APP` | the manifest's `app` | the WSGI callable, `module:attribute`; one of the two is required |
| `BLASTIT_PRELOAD` | the manifest's `preload` | modules imported before the fork, comma-separated |
| `BLASTIT_MANIFEST` | `./blastit.toml` | the manifest path |
| `BLASTIT_SCRIPT_NAME` | — | mount prefix stripped before routing; the app sees `SCRIPT_NAME` |
| `BLASTIT_WARMUP` | — | an optional `module:callable` run before the fork, for an expensive first use |
| `BLASTIT_ENV_STRIP` | — | extra variables hidden from the app, comma-separated |

`app`, `preload`, `BLASTIT_APP`, `BLASTIT_WARMUP` and `BLASTIT_PRELOAD` must be
importable in Blastit's own environment: the project's virtualenv. Blastit reads
the process environment only; `uv run --env-file .env blastit dev` loads a file.

## Port and health

| Setting | Default | What it does |
|---|---|---|
| `PORT` | `8080` | the port init binds on `0.0.0.0`; most platforms set it, and `blastit dev --port` overrides it |
| `SERVER_PORT` | `= PORT` | the port the app is told the server runs on, when a load balancer's differs |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | answered by the scheduler itself, without a view process |
| `BLASTIT_BACKLOG` | `1024` | the TCP listen backlog |
| `BLASTIT_DEV_EXPOSE` | — | `1` is the second acknowledgement `blastit dev --host` needs before it binds a non-loopback address |

The health path takes no worker slot: traffic cannot starve a health check.

## Logs

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_LOG_FORMAT` | `json` | `json` (one event per line) or `pretty` (coloured, for humans); `blastit dev` and `blastit check` default to `pretty`. The boot's first lines wait for it, and a boot whose settings do not parse prints them as JSON |
| `BLASTIT_LOG_LEVEL` | `info` | the minimum level printed |

In server mode stdout is the only output, one event per line. The one exception
is `blastit check --json`: exactly one JSON document on stdout, everything else
on stderr, and exit `2` leaves stdout empty. See [log events](log-events.md).

## Privileges and the uid layout

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_UID_BASE` | `20000` | the uid layout's base: `+1` http, `+2` scheduler, `+3` dbproxy, `+1000+i` for route *i* |
| `BLASTIT_SECCOMP` | `1` | the per-role seccomp filters, the only syscall and egress wall. `0` removes it: honoured only with a loud `boot` warning, and both presets abort the boot on it |
| `BLASTIT_EXEC` | — | standalone installs only: the capability-free copy of the binary that dbproxy runs |

The privilege mode has no variable: the `boot` event reports it — `root`,
`capabilities`, or `unprivileged-dev` under `blastit dev`. Production refuses to
start without root or the capability set above, or without seccomp; `blastit dev`
needs none of them and says so in its banner.

## Admission and threads

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_MAX_WORKERS` | `16` | live request threads across every view; keep it under the database connection limit |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | request-thread cap of one view process; also sizes its `RLIMIT_NPROC` |
| `BLASTIT_QUEUE_SIZE` | `256` | the bounded admission queue; beyond it the answer is `503` |
| `BLASTIT_QUEUE_TIMEOUT` | `10s` | how long a request may wait in that queue before `503` |
| `BLASTIT_REQUEST_TIMEOUT` | `30s` | how long before the client gets a `504` |
| `BLASTIT_STUCK_THREADS` | `4` | overdue threads before a view process is recycled |

## Fair queuing

`BLASTIT_FAIR_QUEUING=1` orders the admission queue by a two-level fair scheduler
instead of by arrival; `0` is plain FIFO, and the `BLASTIT_FQ_*` knobs go unread.
[Fair queuing](../deploy/fair-queuing.md) turns it on safely.

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_FAIR_QUEUING` | `0` | `1` or `on` to enable |
| `BLASTIT_FQ_OUTER_PREFIX_V4` / `BLASTIT_FQ_OUTER_PREFIX_V6` | `24` / `56` | how much of a client address an outer flow keeps |
| `BLASTIT_FQ_OUTER_MAX` | `65536` | outer flows per worker; the rest share one overflow flow |
| `BLASTIT_FQ_INNER_MAX_PER_OUTER` | `1024` | inner flows one outer flow may hold |
| `BLASTIT_FQ_INNER_MAX` | `262144` | inner flows per worker |
| `BLASTIT_FQ_BURST` | `20ms` | what a client may spend before it queues behind the others |
| `BLASTIT_FQ_MAX_DEBT` | `5s` | how far ahead of the queue's virtual time one flow may borrow |
| `BLASTIT_FQ_DEBT_TTL` | `30s` | how long an idle flow with debt is remembered |
| `BLASTIT_FQ_INNER_HEADERS` | `user-agent,accept-language,sec-ch-ua,sec-ch-ua-platform,sec-ch-ua-mobile,authorization` | headers hashed into the inner flow key |
| `BLASTIT_FQ_INNER_COOKIES` | `sessionid,session` | cookies hashed into the inner flow key |
| `BLASTIT_FQ_INNER_INCLUDE_IP` | `1` | whether the client address is part of the inner flow key |

## HTTP

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_MAX_HEADER_BYTES` | `64 KiB` | the largest request head blastit accepts |
| `BLASTIT_MAX_BODY_BYTES` | `100 MiB` | the hard ceiling for one request body; a larger `Content-Length` is rejected before any of it is buffered |
| `BLASTIT_HEADER_TIMEOUT` | `10s` | how long the head may take to arrive (slowloris) |
| `BLASTIT_KEEPALIVE_TIMEOUT` | `75s` | idle time on a kept-alive connection, above a load balancer's |
| `BLASTIT_MAX_CONNECTIONS` | `1024` | client connections served at once |
| `BLASTIT_FRAME_MAX` | `1 MiB` | the largest internal frame; larger bodies travel in several |

Size the body ceiling so the largest body × concurrent uploads fits container
memory: up to `BLASTIT_MAX_WORKERS` uploads can buffer at once.

No TLS or HTTP/2: a load balancer terminates TLS. `Upgrade` gets `501`.

## View process lifecycle and memory

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_VIEW_SPAWN` | `lazy` | `lazy` or `eager`; `prefork = true` overrides it per route |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | the live view-process cap, with LRU retirement |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | the expected cost of one view process; the cap's starting divisor |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | of `total`, the smaller of `MemTotal` and the cgroup limit; the non-reclaimable reading (page cache and reclaimable slab left out) is compared against it. `blastit dev` refuses a boot above it and warns once above the `85%` default; one idle view process per check is retired |
| `BLASTIT_VIEW_IDLE_TIMEOUT` | `600s` | retire a view process idle this long |
| `BLASTIT_VIEW_MAX_REQUESTS` | `0` (off) | retire a view process after this many requests |
| `BLASTIT_IMMORTALIZE` | `auto` | `auto`, `1` or `0`: mark the preloaded heap immortal before the fork |

Sizing the cap: [memory and process caps](../deploy/anything-else.md#memory-and-process-caps).

## Timeouts and shutdown

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_GRACEFUL_TIMEOUT` | `25s` | `SIGTERM`: http finishes in-flight requests this long, then `SIGTERM` → `SIGKILL` |

## Client address

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the header the client address is read from, from a trusted peer only |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | peers whose client-IP header is believable: addresses, CIDR or `*`; a port is an error |
| `BLASTIT_TRUST_FORWARDED` | — | deprecated: `0` is an empty trusted list, `1` is `*`; the list above wins |

`wsgi.url_scheme` follows the same trust decision: `X-Forwarded-Proto` is read
only from a trusted peer, only its last value, and only if it is exactly `http`
or `https`. Both bundled ingresses overwrite it, so a client cannot choose it.
For a chain header, `*` trusts every peer and takes the **rightmost** entry —
what the closest hop added. Naming a network instead compares entries from the
right, and a list that contains every entry falls back to the leftmost, which the
client invented. That is why both presets use `*` and publish no port.

## Proxies

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | upstream connection cap of proxy `<NAME>`, upper-cased: `BLASTIT_PROXY_DB_MAX_CONNECTIONS` |

A proxy's protocol, inputs and outputs live in the
[manifest](manifest.md#proxies).

## Behind a connection pooler

`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` is **per instance**: keep it at the worker
count and let the pool accept `workers × instances` clients (`MAX_CLIENT_CONN`,
100 by default) while it keeps `DEFAULT_POOL_SIZE` (20) server connections. A
managed pool's `size` is its *minimum*; its maximum follows the cluster's RAM
(about 25 connections per GB, 3 reserved), so raise the plan when the pool's
`size` approaches that.

A transaction pool keeps no session state: `SET` and `set_config` do not survive
it. Set the session zone on the role (`ALTER ROLE app SET timezone = 'UTC'`) and
read it back on the **direct** port as the app's own role — a pool's open
backends keep the zone they logged in with until they are recycled
(`server_lifetime`, one hour by default), so restart the pooler after changing
the role ([the rule](../deploy/index.md#pooler)).

Django's PostgreSQL backend sends no prepared statements
(`prepare_threshold=None`). A raw psycopg 3 client prepares after five
executions: `max_prepared_statements` on PgBouncer ≥ 1.22 with libpq ≥ 17 rewrites
them, or connect with `prepare_threshold=None`.

`ca_cert` is a PEM bundle for the upstream TLS leg. It only matters when the URL
asks for verification (`sslmode=verify-full`), and through a pooler it validates
the pooler's certificate, not the origin.

## Presets

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_PRESET` | — | a named set of defaults, such as `digitalocean-paas` |

`--preset NAME` wins over `BLASTIT_PRESET`; an unknown name lists the presets
and exits.

## Not settings, but worth reading

| Name | Where | Meaning |
|---|---|---|
| `blastit.route` | `environ` | the route's name, added to every WSGI request for logging |
| `REMOTE_ADDR` | `environ` | a bare client IP; the peer's port is in `REMOTE_PORT` |
| `BLASTIT_*` | the app's environment | all of them are stripped before the app runs |
