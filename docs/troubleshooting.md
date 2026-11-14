# Troubleshooting

Symptom → fix. Read the [`boot` and `sql.denied` events](reference/log-events.md);
`blastit dev` prints them as readable lines already.

## It does not start

| Symptom | Fix |
|---|---|
| `init runs as uid … and is missing cap_…` | production, unprivileged: run `blastit dev` locally, or give the deployment the [capabilities](deploy/anything-else.md#root-or-rootless) |
| `BLASTIT_SECCOMP=0` | it removes the syscall and egress wall: the boot warns, and both presets abort the boot on it. Keep it at `1` |
| `blastit.toml: no such file` | there is no manifest in this directory: `cd` to the project, or set `BLASTIT_MANIFEST` |
| `BLASTIT_APP: the WSGI callable … is required` | the manifest exists but declares no `app`: write `app = "module:attribute"`, or set `BLASTIT_APP` |
| `BLASTIT_FOO: …` | a variable that does not parse; the message names it |
| the log stops after `stage0` | the configuration did not parse, so stage 1 never ran |
| `config`, but no `preload` | the manifest, a proxy input variable or the db sockets: the message names it |
| `must be \`module:attribute\`` | a colon, not Django's dots: `myshop.wsgi:application` |
| `No module named 'app'` | not importable from Blastit's directory: start next to `app.py`, or install the project |
| an unknown key, a bad regex, a duplicate route | the manifest; the message names the key |
| a proxy input variable is missing | `[proxy.db] input` names one the environment lacks |
| `is not a trusted proxy` | an entry is not an address or CIDR; a port is not a network |
| `carries file capabilities` | a rootless start used the shared interpreter: use a [dedicated copy](deploy/compose-traefik.md#rootless) |
| a directory `is not readable and traversable` | production only: a view uid cannot reach the interpreter, the stdlib or your project. Keep developing with `blastit dev`; for production, install Python under a world-readable prefix (`UV_PYTHON_INSTALL_DIR=/opt/uv-python` before `uv python install`) |
| `is writable by another uid` | a directory on the import path belongs to someone else: your route uids could plant a module. `chmod go-w` it, or start Blastit in the code tree |
| `is the uid the layout hands out` | the service uid collides with `BLASTIT_UID_BASE + 1…3` or a route's uid |
| `memory: … is above BLASTIT_MEM_HIGH_WATERMARK` | `blastit dev` stops rather than serve `503`s: the memory the kernel cannot reclaim is over the knob. Free memory, or raise `BLASTIT_MEM_HIGH_WATERMARK` |

## `blastit check` fails

| Symptom | Fix |
|---|---|
| a statement is not granted | add it to `queries`, or rerun with `--learn` |
| a setup statement (`SELECT set_config(…)`, `SET …`, `SHOW …`) is not granted | it runs on whichever route opens the connection: one group serves them all — `[group.connection.db]` with the statement, and top-level `groups = ["connection"]` ([groups](reference/manifest.md#groups)) |
| a grant is never used | delete it, or test the code path it covers |
| a route is never exercised | add a test, or delete the route |
| a route took a body and declares no ingress | add `[route.ingress]` with `body = "json"`, `"form"`, `"none"`, or `"raw"` plus `allow_raw = true` |
| `blastit check` reports `raw_ingress` | the route declares `body = "raw"` without `allow_raw = true`: acknowledge it knowingly, or pick `json` or `form` |
| exit `2` | the manifest or the configuration did not parse; run `blastit dev` once to see the message |
| exit `3` and the suite failed | fix the suite: Blastit reports no audit for a run that never got there |
| pytest exits `5`, "no tests ran" | nothing was collected, so `blastit check` exits `3` with no audit: add a test, or bootstrap with `blastit dev --learn` |

## Requests do not arrive

| Symptom | Fix |
|---|---|
| no `ready` event | the boot stopped before serving; read the last event |
| the health check fails | `PORT` (default `8080`) or `BLASTIT_HEALTH_PATH` changed |
| nothing answers from another machine | `blastit dev` binds loopback: add `--host 0.0.0.0` **and** `BLASTIT_DEV_EXPOSE=1` |
| `--host` is refused | set `BLASTIT_DEV_EXPOSE=1` as the second acknowledgement; the boot line then names the address and your uid |
| `413` on a route you expect to take a body | it has no `[route.ingress]`: declare `body`, or `raw` with `allow_raw = true` |
| `413` on a large upload | the `Content-Length` is over `BLASTIT_MAX_BODY_BYTES` (`100 MiB` default); size it so the largest body × concurrent uploads fits memory |
| `404`, and `"route": null` | no route matched: a regex matches the whole `PATH_INFO` |
| `404`, and the path looks right | an earlier route matched first; `rule` is the winner |
| `503` | the queue was full, or the wait timed out (`BLASTIT_QUEUE_SIZE`, `BLASTIT_QUEUE_TIMEOUT`) |
| `504` | `BLASTIT_REQUEST_TIMEOUT`; the slot is held until the thread exits |
| `502` | a view process died or was recycled; the next request spawns one |
| `501` on an `Upgrade` request | WebSockets and HTTP/2 are not implemented |

## Database errors

| Symptom | Fix |
|---|---|
| a `sql.denied` event | not granted to that route; [fix it](local.md#a-denial) |
| `No such file or directory` | no grant on that proxy, so no socket; with a grant, the app is not using Blastit's `DATABASE_URL` |
| `FATAL 28000` | the credentials did not match the listener: it arrived as another uid |
| `53300` | the upstream cap (`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`) is reached |
| a `SAVEPOINT` refused | the route needs `transaction_control = true` |
| `DECLARE "_django_curs_…"` refused | a named cursor, new per request: turn server-side cursors off |
| everything is denied at once | the app inlines its parameters; [what an app must do](frameworks/index.md#bind-parameters-server-side) |
| the statement looks identical to the grant | `$1` compares as `$?`, whitespace collapses; a different column list misses |

## The app sees it differently

| Symptom | Fix |
|---|---|
| `REMOTE_ADDR` is the load balancer's | `BLASTIT_TRUSTED_PROXIES` does not name it; [measure it](deploy/index.md#measure-your-ingress) |
| `REMOTE_ADDR` is forged | the header read is one the ingress passes through; read another |
| `REMOTE_ADDR` carries a port | it is a bare IP; the peer's port is `REMOTE_PORT` |
| `request.is_secure()` is false behind TLS | `X-Forwarded-Proto` is not from a trusted peer, or `SECURE_PROXY_SSL_HEADER` is missing |
| a variable the app expected is gone | `BLASTIT_*`, the proxy inputs and `BLASTIT_ENV_STRIP` are removed |

## Rootless and memory

| Symptom | Fix |
|---|---|
| `"privilege": "root"` when you meant rootless | the container or unit starts Blastit as uid 0 |
| `"bounding_set": "kept"` | the platform could not give `cap_setpcap`; the children hold nothing else |
| `permission denied` creating the runtime directory | mount a `tmpfs` owned by the service uid, or use `RuntimeDirectory=` |
| the container is OOM-killed | the view-process cap is too high: set `BLASTIT_MAX_VIEW_PROCESSES` or raise `BLASTIT_VIEW_MEM_ESTIMATE` |
| a `memory:` WARN, then it serves | the reading is above the `85%` default and you raised `BLASTIT_MEM_HIGH_WATERMARK` past it; the line names the knob, the measured value and the cache it left out. Free memory to quieten it |
| a route's first request is slower | a lazy view process forking: mark hot routes `prefork = true` |
