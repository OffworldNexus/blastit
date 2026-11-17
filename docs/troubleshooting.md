# Troubleshooting

Symptom → fix. Read the [`boot` and `sql.denied` events](reference/log-events.md);
`blastit dev` prints them as readable lines already.

## It does not start

| Symptom | Fix |
|---|---|
| `init runs as uid … and is missing cap_…` | production, unprivileged: run `blastit dev` locally, or give the deployment the [capabilities](deploy/anything-else.md#root-or-rootless) |
| `BLASTIT_SECCOMP=0` | it removes the syscall and egress wall: the boot warns, and both presets abort the boot on it. Keep it at `1` |
| `./Blastfile: no such file or directory` | there is no `Blastfile` in this directory: `cd` to the project, or set `BLASTIT_MANIFEST` |
| `BLASTIT_APP: the WSGI callable … is required` | the file exists but names no callable: write `entrypoint python "module:attribute"`, or set `BLASTIT_APP` |
| `BLASTIT_FOO: …` | a variable that does not parse; the message names it |
| the log stops after `stage0` | the configuration did not parse, so stage 1 never ran |
| `config`, but no `preload` | the `Blastfile`, a store's variable or the db sockets: the message names it |
| `must be \`module:attribute\`` | a colon, not Django's dots: `myshop.wsgi:application` |
| `No module named 'app'` | not importable from Blastit's directory: start next to `app.py`, or install the project |
| an unknown keyword, a bad regex, a duplicate view | the `Blastfile`; the one-line message names the token |
| `body raw … without the acknowledgement` | the production boot refuses an unacknowledged `raw` view before it binds a listener: write `body raw unvalidated` ([view directives](reference/blastfile.md#view-directives)) |
| a store's variable is missing | its `$IDENT` names one the environment lacks |
| `is not a trusted proxy` | an entry is not an address or CIDR; a port is not a network |
| `carries file capabilities` | a rootless start used the shared interpreter: use a [dedicated copy](deploy/compose-traefik.md#rootless) |
| a directory `is not readable and traversable` | production only: a view uid cannot reach the interpreter, the stdlib or your project. Keep developing with `blastit dev`; for production, install Python under a world-readable prefix (`UV_PYTHON_INSTALL_DIR=/opt/uv-python` before `uv python install`) |
| `is writable by another uid` | a directory on the import path belongs to someone else: your route uids could plant a module. `chmod go-w` it, or start Blastit in the code tree |
| `is the uid the layout hands out` | the service uid collides with `BLASTIT_UID_BASE + 1…3` or a route's uid |
| `memory: … is above BLASTIT_MEM_HIGH_WATERMARK` | `blastit dev` stops rather than serve `503`s: the memory the kernel cannot reclaim is over the knob. Free memory, or raise `BLASTIT_MEM_HIGH_WATERMARK` |

## `blastit check` fails

| Symptom | Fix |
|---|---|
| a statement is not granted | add `query '<sql>'` to that view's `postgres` block, or rerun with `--learn` |
| a setup statement (`SELECT set_config(…)`, `SET …`, `SHOW …`) is not granted | it runs on whichever view opens the connection: one trait serves them all — `trait connection { postgres { … } }`, `use connection` in every view that connects ([traits](reference/blastfile.md#traits)) |
| a grant is never used | delete it, or test the code path it covers |
| a view is never exercised | add a test, or delete the view |
| a view took a body and declares no `body` | write `body json`, `form`, `none`, or `raw unvalidated` |
| `blastit check` reports `raw_ingress` | the view declares `body raw` without `unvalidated`: acknowledge it knowingly, or pick `json` or `form` — the production boot refuses the same view |
| exit `2` | the `Blastfile` or the configuration did not parse; run `blastit dev` once to see the message |
| exit `3` and the suite failed | fix the suite: Blastit reports no audit for a run that never got there |
| pytest exits `5`, "no tests ran" | nothing was collected, so `blastit check` exits `3` with no audit: add a test, or bootstrap with `blastit dev --learn` |

## Requests do not arrive

| Symptom | Fix |
|---|---|
| no `ready` event | the boot stopped before serving; read the last event |
| the health check fails | `PORT` (default `8080`) or `BLASTIT_HEALTH_PATH` changed |
| nothing answers from another machine | `blastit dev` binds loopback: add `--host 0.0.0.0` **and** `BLASTIT_DEV_EXPOSE=1` |
| `--host` is refused | set `BLASTIT_DEV_EXPOSE=1` as the second acknowledgement; the boot line then names the address and your uid |
| `413` on a view you expect to take a body | it has no `body`: declare `body json`, `form` or `none`, or `body raw unvalidated` |
| `413` on a large upload | the `Content-Length` is over `BLASTIT_MAX_BODY_BYTES` (`100 MiB` default); size it so the largest body × concurrent uploads fits memory |
| `404`, and `"route": null` | no view matched: a quoted pattern matches the whole `PATH_INFO` |
| `404`, and the path looks right | an earlier view matched first; `rule` is the winner |
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
| a `SAVEPOINT` refused | the view needs `transactions` in its `postgres` block |
| `DECLARE "_django_curs_…"` refused | a named cursor, new per request: turn server-side cursors off |
| everything is denied at once | the app inlines its parameters; [what an app must do](frameworks/python/index.md#bind-parameters-server-side) |
| the statement looks identical to the grant | `$1` compares as `$?`, whitespace collapses; a different column list misses |

## The app sees it differently

| Symptom | Fix |
|---|---|
| `REMOTE_ADDR` is the load balancer's | `BLASTIT_TRUSTED_PROXIES` does not name it; [measure it](deploy/index.md#measure-your-ingress) |
| `REMOTE_ADDR` is forged | the header read is one the ingress passes through; read another |
| `REMOTE_ADDR` carries a port | it is a bare IP; the peer's port is `REMOTE_PORT` |
| `request.is_secure()` is false behind TLS | `BLASTIT_TRUSTED_PROXIES` does not name the edge |
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
