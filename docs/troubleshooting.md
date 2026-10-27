# Troubleshooting

Symptom → fix. Most of it is answerable from the first three `boot` events and the
`sql.denied` events; `BLASTIT_LOG_FORMAT=pretty` makes them readable.

## It does not start

| What you see | Fix |
|---|---|
| `blastit: BLASTIT_FOO: …` | a variable that does not parse; the message names it |
| the log stops after `stage0` | the configuration did not parse, so stage 1 never ran |
| `config`, but no `preload` | the manifest, the environment rewrite or the database sockets |
| `must be \`module:attribute\`` | a colon, not Django's dots: the pytest live server converts `WSGI_APPLICATION` for you, a `BLASTIT_APP` you set yourself does not |
| `No module named 'app'` | not importable from where you started blastit. Its working directory goes first on `sys.path`, so start it next to `app.py` — or install the project |
| an unknown key, an invalid regex, a duplicate route | the manifest; the message names the key |
| a proxy input variable is missing | `[proxy.db] input` names a variable the environment does not have |
| `is not a trusted proxy` | an entry is not an address or CIDR; a port is not a network |
| `carries file capabilities` | a rootless start used the shared interpreter: use a [dedicated copy](infra/compose-traefik.md#rootless) |
| a directory `is not readable and traversable` (or `not executable`) | the view uids cannot reach the interpreter, the stdlib, `site-packages`, your project or the dbproxy binary — a Python under a private home is the usual one. Move it (`UV_PYTHON_INSTALL_DIR=/opt/uv-python`), or `chmod o+rx` the directory named and every one above it |
| `is writable by another uid` | a directory on the interpreter's `sys.path`, or above an entry on it: a route uid could plant a module another route imports, or `rmdir` the entry and put its own there. `chmod go-w` it, or start blastit in the code tree. When the message says "may rename or delete anything in it" — a sticky directory a view uid owns — move the project out from under it, or `chown` it to root |
| `is the uid the layout hands out` | the service uid collides with `BLASTIT_UID_BASE + 1…3` or a route's uid |

## Requests do not arrive

| What you see | Fix |
|---|---|
| no `ready` event | the boot stopped before serving; read the last event printed |
| the health check fails | `PORT` (default `8080`) or `BLASTIT_HEALTH_PATH` was changed |
| `404` and `"route": null` | no route matched: a regex matches the whole `PATH_INFO` |
| `404` and the path looks right | an earlier route matched first; `rule` is the index that won |
| `503` | the queue was full, or the wait timed out (`BLASTIT_QUEUE_SIZE`, `BLASTIT_QUEUE_TIMEOUT`) |
| `504` | `BLASTIT_REQUEST_TIMEOUT`; the slot is held until the thread exits |
| `502` | a view process died or was recycled; the next request spawns one |
| `501` on an `Upgrade` request | WebSockets and HTTP/2 are not implemented |

## The app gets a database error

| What you see | Fix |
|---|---|
| a `sql.denied` event | the statement is not granted to that route; [fix it here](../frameworks/day-to-day.md) |
| `No such file or directory` | the route has no grant on that proxy, so it has no socket |
| the same, with a grant | the app is not using the `DATABASE_URL` blastit rendered |
| `FATAL 28000` | the connection's credentials did not match the listener: it arrived as another uid |
| `53300` | the proxy's upstream cap (`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`) is reached |
| a `SAVEPOINT` refused | the route needs `transaction_control = true` |
| `DECLARE "_django_curs_…"` refused | a named cursor, new per request: turn server-side cursors off |
| everything is denied at once | the app inlines its parameters; see [what an app must do](../frameworks/index.md) |
| the statement looks identical to the grant | `$1` compares as `$?` and whitespace collapses; a different column list does not match |

## The app sees something different

| What you see | Fix |
|---|---|
| `REMOTE_ADDR` is the load balancer's | `BLASTIT_TRUSTED_PROXIES` does not name it; [measure it](infra/anything-else.md#working-it-out-for-your-own-ingress) |
| `REMOTE_ADDR` is forged | the header being read is one the ingress passes through; read another |
| `REMOTE_ADDR` carries a port | it does not: `REMOTE_ADDR` is a bare IP, whoever the client turned out to be. The peer's port is in `REMOTE_PORT` |
| `request.is_secure()` is false behind TLS | `X-Forwarded-Proto` is not arriving from a trusted peer, or `SECURE_PROXY_SSL_HEADER` is missing |
| a variable the app expected is gone | `BLASTIT_*`, proxy inputs and `BLASTIT_ENV_STRIP` are removed |

## Rootless and memory

| What you see | Fix |
|---|---|
| `"privilege": "root"` when you meant rootless | the container or unit still starts blastit as uid 0 |
| `"bounding_set": "kept"` | the platform could not give `cap_setpcap`; the children still hold nothing else |
| `permission denied` creating `/run/blastit` | mount a `tmpfs` owned by the service uid, or use `RuntimeDirectory=` |
| the container is OOM-killed | the view-process cap is too high: set `BLASTIT_MAX_VIEW_PROCESSES` or raise `BLASTIT_VIEW_MEM_ESTIMATE` |
| the first request to a route is slower | the fork of a lazy view process: mark hot routes `prefork = true` |

## Where to look

* the first three `boot` events — mode, preset, seccomp, the pre-fork cost, the cap;
* the failing request's `request` event — `route`, `rule`, `status`, `outcome`;
* the `sql.denied` events around it — `route`, `proxy`, `kind`, `reason`;
* `manifest.warning` — an unrestricted route, or an unattached group.
