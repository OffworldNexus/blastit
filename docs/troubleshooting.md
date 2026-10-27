# Troubleshooting

Symptom → fix. Read the [`boot` and `sql.denied` events](reference/log-events.md)
with `BLASTIT_LOG_FORMAT=pretty`.

## It does not start

| Symptom | Fix |
|---|---|
| `blastit: BLASTIT_FOO: …` | a variable that does not parse; the message names it |
| the log stops after `stage0` | the configuration did not parse, so stage 1 never ran |
| `config`, but no `preload` | the manifest, the environment rewrite or the db sockets |
| `must be \`module:attribute\`` | a colon, not Django's dots: the pytest live server converts `WSGI_APPLICATION`, your own `BLASTIT_APP` does not |
| `No module named 'app'` | not importable from blastit's directory, which goes first on `sys.path`: start next to `app.py`, or install the project |
| an unknown key, a bad regex, a duplicate route | the manifest; the message names the key |
| a proxy input variable is missing | `[proxy.db] input` names one the environment lacks |
| `is not a trusted proxy` | an entry is not an address or CIDR; a port is not a network |
| `carries file capabilities` | a rootless start used the shared interpreter: use a [dedicated copy](infra/compose-traefik.md#rootless) |
| a directory `is not readable and traversable` (or `not executable`) | the view uids cannot reach the interpreter, the stdlib, `site-packages`, your project or the dbproxy binary — usually a Python under a private home. Move it (`UV_PYTHON_INSTALL_DIR=/opt/uv-python`), or `chmod o+rx` it and its parents |
| `is writable by another uid` | a directory on the interpreter's `sys.path`, or above one of its entries: a route uid could plant a module another route imports. `chmod go-w` it, or start blastit in the code tree. A sticky directory a view uid owns ("may rename or delete anything in it"): move the project out, or `chown` to root |
| `is the uid the layout hands out` | the service uid collides with `BLASTIT_UID_BASE + 1…3` or a route's uid |

## Requests do not arrive

| Symptom | Fix |
|---|---|
| no `ready` event | the boot stopped before serving; read the last event |
| the health check fails | `PORT` (default `8080`) or `BLASTIT_HEALTH_PATH` changed |
| `404`, and `"route": null` | no route matched: a regex matches the whole `PATH_INFO` |
| `404`, and the path looks right | an earlier route matched first; `rule` is the winner |
| `503` | the queue was full, or the wait timed out (`BLASTIT_QUEUE_SIZE`, `BLASTIT_QUEUE_TIMEOUT`) |
| `504` | `BLASTIT_REQUEST_TIMEOUT`; the slot is held until the thread exits |
| `502` | a view process died or was recycled; the next request spawns one |
| `501` on an `Upgrade` request | WebSockets and HTTP/2 are not implemented |

## Database errors

| Symptom | Fix |
|---|---|
| a `sql.denied` event | not granted to that route; [fix it](../frameworks/day-to-day.md) |
| `No such file or directory` | no grant on that proxy, so no socket; with a grant, the app is not using blastit's `DATABASE_URL` |
| `FATAL 28000` | the credentials did not match the listener: it arrived as another uid |
| `53300` | the upstream cap (`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`) is reached |
| a `SAVEPOINT` refused | the route needs `transaction_control = true` |
| `DECLARE "_django_curs_…"` refused | a named cursor, new per request: turn server-side cursors off |
| everything is denied at once | the app inlines its parameters; [what an app must do](../frameworks/index.md) |
| the statement looks identical to the grant | `$1` compares as `$?`, whitespace collapses; a different column list misses |

## The app sees it differently

| Symptom | Fix |
|---|---|
| `REMOTE_ADDR` is the load balancer's | `BLASTIT_TRUSTED_PROXIES` does not name it; [measure it](infra/anything-else.md#working-it-out-for-your-own-ingress) |
| `REMOTE_ADDR` is forged | the header read is one the ingress passes through; read another |
| `REMOTE_ADDR` carries a port | it is a bare IP; the peer's port is `REMOTE_PORT` |
| `request.is_secure()` is false behind TLS | `X-Forwarded-Proto` is not from a trusted peer, or `SECURE_PROXY_SSL_HEADER` is missing |
| a variable the app expected is gone | `BLASTIT_*`, the proxy inputs and `BLASTIT_ENV_STRIP` are removed |

## Rootless and memory

| Symptom | Fix |
|---|---|
| `"privilege": "root"` when you meant rootless | the container or unit starts blastit as uid 0 |
| `"bounding_set": "kept"` | the platform could not give `cap_setpcap`; the children hold nothing else |
| `permission denied` creating `/run/blastit` | mount a `tmpfs` owned by the service uid, or use `RuntimeDirectory=` |
| the container is OOM-killed | the view-process cap is too high: set `BLASTIT_MAX_VIEW_PROCESSES` or raise `BLASTIT_VIEW_MEM_ESTIMATE` |
| a route's first request is slower | a lazy view process forking: mark hot routes `prefork = true` |
