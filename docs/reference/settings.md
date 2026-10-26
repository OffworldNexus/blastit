# Settings

Everything is configured through environment variables, plus one manifest file.
There is no configuration file for blastit itself and no flags except
[`--preset`](../infra/index.md#picking-a-preset).

A variable that is **set** wins over a preset, even when it is empty. `PORT` is
the platform's and is treated the same way.

## The app and the manifest

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_APP` | — | the WSGI callable, `module:attribute`. Required |
| `BLASTIT_PRELOAD` | — | extra modules imported before the fork (comma-separated) |
| `BLASTIT_MANIFEST` | `/app/blastit.toml` | the manifest path |
| `BLASTIT_SCRIPT_NAME` | — | a mount prefix stripped before routing, passed to the app as `SCRIPT_NAME` |
| `BLASTIT_WARMUP` | — | an optional `module:callable`, run before the fork, for code whose first use is expensive |
| `BLASTIT_ENV_STRIP` | — | extra variables hidden from the app (comma-separated), on top of every `BLASTIT_*` variable and every proxy input |

`BLASTIT_APP`, `BLASTIT_WARMUP` and `BLASTIT_PRELOAD` must be importable in the
environment blastit runs in — that is, the project's virtualenv.

## Port and health

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8080` | the port init binds on `0.0.0.0`. Set by most platforms |
| `SERVER_PORT` | `= PORT` | the port the *app* is told the server runs on, when that is not the port blastit listens on (a load balancer's port) |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | answered by the scheduler itself, so a health check covers http → scheduler without depending on any view process. Must be an absolute path |
| `BLASTIT_BACKLOG` | `1024` | the TCP listen backlog init binds with |

The health path takes no worker slot and never enters the admission queue: a
health check cannot be starved by traffic, and cannot starve it.

## Logs

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_LOG_FORMAT` | `json` | `json` (one event per line) or `pretty` (coloured, for humans) |
| `BLASTIT_LOG_LEVEL` | `info` | the minimum level printed (`info`, `warn`, …) |

stdout is the only output. Every line is kept under 2048 bytes. See
[log events](log-events.md).

## Privileges and the uid layout

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_UID_BASE` | `20000` | the base of the uid layout: `+1` http, `+2` scheduler, `+3` dbproxy, `+1000+i` for route *i*. Also the reason a boot refuses to run as a uid the layout hands out |
| `BLASTIT_SECCOMP` | `1` | the per-role seccomp filters. This is the only lever for egress control, and what `egress = true` buys |
| `BLASTIT_EXEC` | — | **standalone installs only**: the capability-free copy of the binary that `--role=dbproxy` runs. Checked at boot: owned by root or by init's uid, writable by neither group nor others, in a directory that is not either |

There is deliberately **no variable for the privilege mode**. It is read from
`/proc/self/status` at boot and reported in the `boot` event, because a
deployment that believes it granted a capability has no other way to find out
that it did not.

## Admission and threads

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_MAX_WORKERS` | `16` | the global number of live request threads across every view. Size it at or below the database plan's connection limit |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | the request-thread cap of one view process. It also sizes that uid's `RLIMIT_NPROC` (`+16`) |
| `BLASTIT_QUEUE_SIZE` | `256` | the bounded admission queue. Beyond it the answer is `503` with `Retry-After` |
| `BLASTIT_QUEUE_TIMEOUT` | `10s` | how long a request may wait in that queue before `503` |
| `BLASTIT_REQUEST_TIMEOUT` | `30s` | how long before the client gets a `504`. The slot stays held until the thread exits |
| `BLASTIT_STUCK_THREADS` | `4` | overdue threads before a view process is recycled (its other in-flight requests get `502`) |

A slot is released when the view reports `DONE`, meaning the thread has actually
exited — so stuck threads eat into the budget instead of piling up without
limit.

## Fair queuing

`BLASTIT_FAIR_QUEUING=1` orders the admission queue by a two-level fair
scheduler instead of by arrival. `0` (the default) is exactly the FIFO, and the
`BLASTIT_FQ_*` knobs are then never read. The [`compose-traefik`](../infra/compose-traefik.md)
and [`digitalocean-paas`](../infra/digitalocean-paas.md) presets turn it on.

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_FAIR_QUEUING` | `0` | `1` or `on` to enable |
| `BLASTIT_FQ_OUTER_PREFIX_V4` / `_V6` | `24` / `56` | how much of a client address an outer flow keeps. Read as `32` / `128` when longer |
| `BLASTIT_FQ_OUTER_MAX` | `65536` | outer flows per worker; the rest share one overflow flow. Read as `1` when `0` |
| `BLASTIT_FQ_INNER_MAX_PER_OUTER` | `1024` | inner flows one outer flow may hold |
| `BLASTIT_FQ_INNER_MAX` | `262144` | inner flows per worker |
| `BLASTIT_FQ_BURST` | `20ms` | what a client may spend before it starts queueing behind the others |
| `BLASTIT_FQ_MAX_DEBT` | `5s` | how far ahead of the queue's virtual time one flow may borrow |
| `BLASTIT_FQ_DEBT_TTL` | `30s` | how long an idle flow with debt is remembered; read as `1s` when shorter |
| `BLASTIT_FQ_INNER_HEADERS` | `user-agent,accept-language,sec-ch-ua,sec-ch-ua-platform,sec-ch-ua-mobile,authorization` | headers hashed into the inner flow key (an empty value hashes none) |
| `BLASTIT_FQ_INNER_COOKIES` | `sessionid,session` | cookies hashed into the inner flow key |
| `BLASTIT_FQ_INNER_INCLUDE_IP` | `1` | whether the full client address is part of the inner flow key |

Out-of-range values are clamped, not refused, and the boot says so: a prefix
cannot be longer than the address it masks, and a table cannot hold zero flows.

## HTTP

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_MAX_HEADER_BYTES` | `64 KiB` | the largest request head blastit accepts |
| `BLASTIT_MAX_BODY_BYTES` | `100 MiB` | the largest request body |
| `BLASTIT_HEADER_TIMEOUT` | `10s` | how long the head may take to arrive (slowloris) |
| `BLASTIT_KEEPALIVE_TIMEOUT` | `75s` | idle time on a kept-alive connection, deliberately above a load balancer's |
| `BLASTIT_MAX_CONNECTIONS` | `1024` | client connections served at once; sizes http's `RLIMIT_NOFILE` |
| `BLASTIT_FRAME_MAX` | `1 MiB` | the largest internal frame. Larger bodies travel in several frames, so this is a memory/latency trade, not a limit on uploads |

There is no TLS here and no HTTP/2: a load balancer terminates TLS and speaks
plaintext HTTP/1.1 to blastit. `Upgrade` is answered with `501`.

## View process lifecycle and memory

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_VIEW_SPAWN` | `lazy` | `lazy` or `eager`. `prefork = true` in the manifest overrides it per route |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | the live view-process cap, with LRU retirement. See the [sizing section](../infra/anything-else.md#memory-and-process-caps) |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | the expected cost of one view process: the starting divisor of the derived cap, and the floor of the measured one |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | of `total` (the smaller of `MemTotal` and the cgroup limit). Above it, one idle view process per 2 s check is retired |
| `BLASTIT_VIEW_IDLE_TIMEOUT` | `600s` | retire a view process that has been idle this long |
| `BLASTIT_VIEW_MAX_REQUESTS` | `0` (off) | retire a view process after this many requests |
| `BLASTIT_IMMORTALIZE` | `auto` | `auto`, `1` or `0`: mark the preloaded heap immortal before the fork. On 3.14 that needs the version's own marker, and it is skipped with the reason in the `boot` event on any interpreter whose marker blastit does not know (32-bit, big-endian, free-threaded, `Py_TRACE_REFS`, or outside 3.12–3.14) |

## Timeouts and shutdown

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_GRACEFUL_TIMEOUT` | `25s` | `SIGTERM` to PID 1: http stops accepting and finishes in-flight requests for this long, then everyone gets `SIGTERM` → `SIGKILL` |

## Client address

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the one header the client address is read from, and only from a trusted peer. A chain entry is a bare or bracketed address, so `1.2.3.4:5678` makes the whole header unreadable and the peer is the client |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | the peers whose client-IP header may be believed: IP addresses, CIDR networks or `*`. A port is a **boot error**, and an empty list trusts nobody |
| `BLASTIT_TRUST_FORWARDED` | — | **deprecated**: `0` is `BLASTIT_TRUSTED_PROXIES` empty, `1` is `*`. Reported once as a warn `app.output` line at startup, and `BLASTIT_TRUSTED_PROXIES` wins when both are set |

The resolution rule, and how to work it out for your own ingress, is in
[client IP and trusted proxies](../infra/anything-else.md#client-ip-and-trusted-proxies).
`wsgi.url_scheme` follows the same trust decision: `X-Forwarded-Proto` is read
only from a trusted peer, only its last value, and only if it is exactly `http`
or `https`.

## Proxies

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | the upstream connection cap of proxy `<NAME>`, upper-cased: `BLASTIT_PROXY_DB_MAX_CONNECTIONS` for `[proxy.db]` |

The proxy's own configuration — its protocol, its inputs, its outputs — lives in
the manifest, because it is versioned with the code.
`BLASTIT_PROXY_DB_MAX_CONNECTIONS` is not settable by a preset, since only the
manifest knows which proxies exist.

## Presets

| Variable | Default | Meaning |
|---|---|---|
| `BLASTIT_PRESET` | — | a named set of defaults, e.g. `digitalocean-paas` or `compose-traefik` |

`--preset NAME` (or `--preset=NAME`) sets the same thing and wins over
`BLASTIT_PRESET` when the two disagree, with a warning. The name is trimmed, and
a value that is only whitespace is unset. An unknown name is a startup error
that lists the presets. Every variable that is set, and `PORT`, wins over the
preset — see [what a preset is](../infra/index.md#what-a-preset-is).

## Not settings, but worth reading

| Name | Where | Meaning |
|---|---|---|
| `blastit.route` | `environ` | the route's name, added to every WSGI request for logging. The only thing blastit adds to a request |
| `BLASTIT_*` | the app's environment | **all** of them are stripped before the app runs, whatever they are |

## See also

* [The manifest](manifest.md) — what a route may do, and how it is checked.
* [Log events](log-events.md) — what `preset_overridden` and the rest mean.
* [`ARCHITECTURE.md` §8.2](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#82-configuration-reference) — the specification these defaults come from.
