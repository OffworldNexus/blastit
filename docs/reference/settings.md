# Settings

Everything is environment variables, plus the manifest. A variable that is set
wins over a preset, even when it is empty, and `PORT` counts the same. `--preset`
is the only [flag](cli.md).

## The app and the manifest

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_APP` | — | the WSGI callable, `module:attribute`; required |
| `BLASTIT_PRELOAD` | — | modules imported before the fork, comma-separated |
| `BLASTIT_MANIFEST` | `/app/blastit.toml` | the manifest path |
| `BLASTIT_SCRIPT_NAME` | — | mount prefix stripped before routing; the app sees it as `SCRIPT_NAME` |
| `BLASTIT_WARMUP` | — | an optional `module:callable` run before the fork, for an expensive first use |
| `BLASTIT_ENV_STRIP` | — | extra variables hidden from the app, comma-separated |

`BLASTIT_APP`, `BLASTIT_WARMUP` and `BLASTIT_PRELOAD` must be importable in
blastit's own environment: the project's virtualenv.

## Port and health

| Setting | Default | What it does |
|---|---|---|
| `PORT` | `8080` | the port init binds on `0.0.0.0`; set by most platforms |
| `SERVER_PORT` | `= PORT` | the port the app is told the server runs on, when a load balancer's differs |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | answered by the scheduler, covering http → scheduler without a view process |
| `BLASTIT_BACKLOG` | `1024` | the TCP listen backlog |

The health path takes no worker slot: traffic cannot starve a health check.

## Logs

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_LOG_FORMAT` | `json` | `json` (one event per line) or `pretty` (coloured, for humans) |
| `BLASTIT_LOG_LEVEL` | `info` | the minimum level printed |

stdout is the only output. See [log events](log-events.md).

## Privileges and the uid layout

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_UID_BASE` | `20000` | the uid layout's base: `+1` http, `+2` scheduler, `+3` dbproxy, `+1000+i` for route *i* |
| `BLASTIT_SECCOMP` | `1` | the per-role seccomp filters; the only lever for egress control |
| `BLASTIT_EXEC` | — | standalone installs only: the capability-free copy of the binary that dbproxy runs |

The privilege mode has no variable: the `boot` event reports it.

## Admission and threads

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_MAX_WORKERS` | `16` | live request threads across every view; size it at or below the database connection limit |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | request-thread cap of one view process; also sizes its `RLIMIT_NPROC` |
| `BLASTIT_QUEUE_SIZE` | `256` | the bounded admission queue; beyond it the answer is `503` |
| `BLASTIT_QUEUE_TIMEOUT` | `10s` | how long a request may wait in that queue before `503` |
| `BLASTIT_REQUEST_TIMEOUT` | `30s` | how long before the client gets a `504` |
| `BLASTIT_STUCK_THREADS` | `4` | overdue threads before a view process is recycled |

## Fair queuing

`BLASTIT_FAIR_QUEUING=1` orders the admission queue by a two-level fair scheduler
instead of by arrival; `0` is plain FIFO, and the `BLASTIT_FQ_*` knobs go unread.

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
| `BLASTIT_MAX_BODY_BYTES` | `100 MiB` | the largest request body |
| `BLASTIT_HEADER_TIMEOUT` | `10s` | how long the head may take to arrive (slowloris) |
| `BLASTIT_KEEPALIVE_TIMEOUT` | `75s` | idle time on a kept-alive connection, above a load balancer's |
| `BLASTIT_MAX_CONNECTIONS` | `1024` | client connections served at once |
| `BLASTIT_FRAME_MAX` | `1 MiB` | the largest internal frame; larger bodies travel in several |

No TLS or HTTP/2: a load balancer terminates TLS and speaks plaintext HTTP/1.1.
`Upgrade` gets `501`.

## View process lifecycle and memory

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_VIEW_SPAWN` | `lazy` | `lazy` or `eager`; `prefork = true` overrides it per route |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | the live view-process cap, with LRU retirement |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | the expected cost of one view process, the starting divisor of the cap |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | above it, one idle view process per check is retired |
| `BLASTIT_VIEW_IDLE_TIMEOUT` | `600s` | retire a view process idle this long |
| `BLASTIT_VIEW_MAX_REQUESTS` | `0` (off) | retire a view process after this many requests |
| `BLASTIT_IMMORTALIZE` | `auto` | `auto`, `1` or `0`: mark the preloaded heap immortal before the fork |

Sizing the cap: [memory and process caps](../infra/anything-else.md#memory-and-process-caps).

## Timeouts and shutdown

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_GRACEFUL_TIMEOUT` | `25s` | `SIGTERM`: http finishes in-flight requests this long, then `SIGTERM` → `SIGKILL` |

## Client address

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the header the client address is read from, and only from a trusted peer |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | peers whose client-IP header is believable: addresses, CIDR or `*`; a port is an error |
| `BLASTIT_TRUST_FORWARDED` | — | deprecated: `0` is an empty trusted list, `1` is `*`; the list above wins |

`wsgi.url_scheme` follows the same trust decision: `X-Forwarded-Proto` is read
only from a trusted peer. The full resolution rule is in
[client IP and trusted proxies](../infra/anything-else.md#client-ip-and-trusted-proxies).

## Proxies

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | upstream connection cap of proxy `<NAME>`, upper-cased: `BLASTIT_PROXY_DB_MAX_CONNECTIONS` |

A proxy's protocol, inputs and outputs live in the
[manifest](manifest.md#proxies).

## Presets

| Setting | Default | What it does |
|---|---|---|
| `BLASTIT_PRESET` | — | a named set of defaults, such as `digitalocean-paas` or `compose-traefik` |

`--preset NAME` wins over `BLASTIT_PRESET`; an unknown name lists the presets
and exits.

## Not settings, but worth reading

| Name | Where | Meaning |
|---|---|---|
| `blastit.route` | `environ` | the route's name, added to every WSGI request for logging |
| `REMOTE_ADDR` | `environ` | a bare client IP; the peer's port is in `REMOTE_PORT` |
| `BLASTIT_*` | the app's environment | all of them are stripped before the app runs |
