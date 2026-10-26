# Troubleshooting

Symptom → cause → fix, in the order things go wrong. Almost everything here is
answerable from the first three lines of the log and one `sql.denied` event, so
start there.

```console
$ BLASTIT_LOG_FORMAT=pretty sudo -E .venv/bin/blastit
```

## It does not start

The whole container exits non-zero on any startup failure. A half-started
sandbox is worse than a crash loop the platform can see, so nothing degrades
quietly.

| What you see | Cause |
|---|---|
| `blastit: BLASTIT_FOO: …` | a variable that does not parse. The message names the variable, never its value |
| `blastit: BLASTIT_APP: must be \`module:attribute\`` | `BLASTIT_APP` has no `:` — a blank start with nothing named, or the pytest live server handing over Django's dotted `WSGI_APPLICATION`. See the row lower down |
| `configuration document: …` | a stage 1 problem — a document stage 0 did not write. Report it |
| `blastit: … is not `--key=value`` | an internal argument was mistyped. See [the command line](reference/cli.md#internal-arguments) |
| `… is only for stage 0` | `--preset` was passed with `--stage=1` or `--role=…` |
| an unknown preset name | the message lists every preset. `blastit --help` does too |
| `… is not a trusted proxy: …` | a `BLASTIT_TRUSTED_PROXIES` entry is not an address or a CIDR network. **A port is an error**: `127.0.0.1:8080` is not a network |
| an unknown key, a duplicate route name, an invalid regex, an unknown group | the manifest. Validation is strict, and the message names the key by path |
| `path: expected an array` | `path = ".*"` instead of `path = ['.*']` |
| `unrestricted grants take no queries` | `unrestricted = true` next to `queries` (or `tables`, or `functions`) in the same grant |
| `unrestricted` in a group | it is accepted inline on a route only |
| a proxy input variable is missing | `[proxy.db] input = { url = "DATABASE_URL" }` and no `DATABASE_URL` in the environment |
| `the file init starts from carries capabilities` | a rootless deployment gave them to the **shared** interpreter or binary. The message names the recipe: `setcap` on a dedicated copy, mode `0500`, owned by init's uid. See [rootless](infra/compose-traefik.md#rootless) |
| missing-capability names and a `setcap` line | a rootless start that is missing one of the six required capabilities. The message names exactly the ones that are missing |
| `… is the uid the layout hands out` | a rootless service uid collides with `BLASTIT_UID_BASE + 1`, `+2`, `+3` or a route's uid. Pick one below the base |
| `BLASTIT_EXEC` refused | its target is not owned by root or by init's uid, is writable by group or others, or sits in a directory that is. `/tmp` is refused by name |
| `--role=dbproxy runs no interpreter` | `--role=dbproxy` from a wheel install. That is stage 0's job, and the wheel ships the Python-free binary it uses |
| `python: …` output on stderr with a non-zero exit | importing `BLASTIT_APP`, `BLASTIT_PRELOAD` or `BLASTIT_WARMUP` raised. It is printed as `app.output` events, not as a traceback on stderr of its own |
| `BLASTIT_APP (app:app): ModuleNotFoundError: No module named 'app'` | the project is not on the interpreter's `sys.path`: stage 1 re-executes with `-I`, and blastit restores only the *console script's* path, whose first entry is its own `bin` directory. Set `PYTHONPATH=$PWD` (or install the project) |
| `BLASTIT_APP: must be \`module:attribute\`` | `module` and `attribute` are separated by a **colon**. Django's `WSGI_APPLICATION` uses dots, and the pytest live server hands that setting to blastit as it stands — pass `--blastit-live-app myproject.wsgi:application` |
| `ModuleNotFoundError: No module named 'stringprep'`, or `'encodings.idna'`, or `'subprocess'`, from a view | the **interpreter's own standard library** is not readable by the view uids. A Python installed under a private home (`uv python install` puts one in `~/.local/share/uv/python`, and `~` is often `0750`) is reachable by your shell and by no sandbox uid, so only the modules compiled into `libpython` import. Use an interpreter with traversable paths |
| a `__pycache__` directory owned by root appears in the project or its `site-packages` | expected: stage 1 runs `python -I` **as root**, which ignores `PYTHONDONTWRITEBYTECODE`, and compiles the modules it imports before the fork into bytecode there. That is also why a wheel serves an application directory it may write to |
| the log stops after `stage0` | the configuration did not parse; see the row above |
| the log reaches `config` but not `preload` | the failure is in the manifest, the environment rewrite, or binding the database sockets — all of which happen between the two |

## It starts, but requests do not arrive

The `ready` event means the server is serving: init bound the port, forked http
and the scheduler, and is supervising.

| What you see | Cause |
|---|---|
| no `ready` at all | see above. The `boot` events stop at the phase that failed |
| the platform's health check fails | the port is wrong (`PORT`, default `8080`), or the health path was changed. `BLASTIT_HEALTH_PATH` defaults to `/_blastit/health` and is answered by the scheduler, so it does not depend on any route |
| `404` from blastit, and `"route": null` in the `request` event | no route matched the path. Regexes are matched against the **whole** of `PATH_INFO`, so `/products/` does not match `/products/1/`. Add a route, or a catch-all `path = ['.*']` so the app renders its own 404s |
| `404` and the path *looks* right | the regex is not anchored where you think, or a **previous** route matched first. Routes are tried in manifest order and the first match wins; `rule` in the event is the index of the one that matched |
| `503` | the admission queue is full (`BLASTIT_QUEUE_SIZE`) or the wait exceeded `BLASTIT_QUEUE_TIMEOUT`. More workers, or a smaller per-request cost |
| `504` | `BLASTIT_REQUEST_TIMEOUT` expired. The worker slot stays held until the thread really exits, so a few slow routes eat the budget: check `BLASTIT_STUCK_THREADS` and the `process.retired` events |
| `502` | a view process died, or was recycled for stuck threads. Its other in-flight requests get `502` and the next request spawns a fresh process |
| `501` on an `Upgrade` request | WebSockets and HTTP/2 are not implemented: the load balancer terminates TLS and speaks plaintext HTTP/1.1 |
| nothing at all | you are not looking at the right port, or the container is not PID 1. blastit writes to stdout only — no file, no syslog |

## The app gets a database error

This is the interesting class, because it means the sandbox is working.

| What you see | Cause |
|---|---|
| a `sql.denied` event and an error in the app | the statement is not granted to that route. Read `reason`: `select on shop_product.price not granted` is a table grant missing a column, `the statement is not granted` is an exact `queries` entry that no grant matched. [How to fix it](frameworks/day-to-day.md#a-denial-in-development) |
| `could not connect to server: No such file or directory` | the route has **no grant at all** for that proxy, so it has no socket. A route with no `[route.db]` and no group that grants `db` cannot reach the database — which is usually what you want, and is the strongest thing in a manifest |
| `No such file or directory` and the route *does* have a grant | the connection is not going to the socket blastit made. The app must use the `DATABASE_URL` blastit rendered, and not rebuild one from its own settings. Do not override the host/port in code |
| `FATAL 28000` (`credentials_mismatch`) | the connection arrived on a listener whose view uid does not match the kernel-attached credentials of the startup packet. Something is connecting as a uid that is not the route's — an operator's shell running the app as root, or code dialling another route's socket |
| `53300` (too many connections) | the proxy's upstream cap, `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` (default `BLASTIT_MAX_WORKERS`). Raise `BLASTIT_MAX_WORKERS` only up to the database plan's limit |
| `DECLARE "_django_curs_…" CURSOR FOR …` refused | a named cursor. Turn server-side cursors off for that route (`DATABASES[...]["DISABLE_SERVER_SIDE_CURSORS"] = True`), because the name is generated per request and no grant can match it |
| `SAVEPOINT "s140234_x1"` refused | the route needs `transaction_control = true`. Do not try to grant a savepoint |
| the statement looks identical to the grant | check the *normalised* text: trimmed, no trailing `;`, whitespace collapsed, `$1`/`$2` compared as `$?`. Two lines of Python that concatenate into one SQL statement are fine; a newline inside the SQL is not the same text as a space |
| everything is denied at once | the app inlines its parameters: psycopg2, or psycopg 3 with client-side binding. See [what blastit needs from an app](frameworks/index.md#send-parameters-separately-from-the-sql-text) |

## Something is different from the app's point of view

| Symptom | Cause |
|---|---|
| `REMOTE_ADDR` is the load balancer's address | `BLASTIT_TRUSTED_PROXIES` does not include it. Default is `127.0.0.1,::1`; a non-loopback proxy has to be named. Read [working it out for your own ingress](infra/anything-else.md#working-it-out-for-your-own-ingress) |
| `REMOTE_ADDR` carries a port | expected when the address came from the socket peer: it keeps the `ip:port` shape it arrived with. An address a trusted header named is a bare IP. Code that compares `REMOTE_ADDR` to a list of addresses should handle both |
| `REMOTE_ADDR` is a forged value | the header being read is one the ingress passes through. Measure it with the throwaway route in that same section |
| `request.is_secure()` is false behind TLS | `X-Forwarded-Proto` is not arriving from a trusted peer (or is not exactly `https`). Django also needs `SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")` |
| `environ["SCRIPT_NAME"]` is empty but the app is mounted under a path | set `BLASTIT_SCRIPT_NAME`; blastit strips it before routing and passes it as `SCRIPT_NAME` |
| a variable the app expected is gone | every `BLASTIT_*` variable, every proxy **input**, and everything in `BLASTIT_ENV_STRIP` is removed from the app's environment. That is the feature: a proxy input is a secret. Re-declare it as a proxy output if the app needs a non-secret version |
| a request the app used to handle now 404s | it matches no route. Add it, or widen an existing route's regex |

## The log

| What you see | Cause |
|---|---|
| nothing but `request` events, and 5xx ones are `warn` | normal. Raise `BLASTIT_LOG_LEVEL` to `warn` for a quieter steady state |
| an `app.output` event wrapping your print statements | normal: init is the only writer to stdout, so every child's output is wrapped and stamped with the `component` of the pipe it came from |
| `"truncated": ["sql"]` | the line hit 2048 bytes and a field was cut. The event is still valid JSON |
| a long print statement split into several events | `app.output` re-chunks long lines and carries `part` and `parts` |
| the JSON is one line per event but hard to read | `BLASTIT_LOG_FORMAT=pretty` |
| a log line that looks like a Python traceback | that is `app.output` on stderr: the framework's own error handling, forwarded |

## Memory

| What you see | Cause |
|---|---|
| the container is OOM-killed | the view-process cap is too high, or the estimate was never measured. The `preload` boot event reports `mem_total`, `mem_used` and `max_view_processes`; `process.cap` reports each re-derivation. Set `BLASTIT_MAX_VIEW_PROCESSES` explicitly, or raise `BLASTIT_VIEW_MEM_ESTIMATE` so the derived cap starts closer to reality |
| the first request to a route takes 12–60 ms longer than later ones | that is the fork of a lazy view process. Mark the hot routes `prefork = true`, or `BLASTIT_VIEW_SPAWN=eager` |
| one warning saying the machine's memory cannot be read | there is no derived cap and the configured or default one is used. Set `BLASTIT_MAX_VIEW_PROCESSES` yourself |
| a `boot` event with `"immortalize": "off"` and a reason | the interpreter is one whose immortal marker blastit does not know (32-bit, big-endian, free-threaded, `Py_TRACE_REFS`, or outside 3.12–3.14). Memory use will be higher; nothing else changes |

## Rootless

| What you see | Cause |
|---|---|
| `boot` says `"privilege": "root"` when you meant rootless | the container or unit is still starting blastit as uid 0. Which uid it runs as is the deployment's declaration, not a setting |
| `"bounding_set": "kept"` plus one warning | the platform could not give `cap_setpcap`, so the children's bounding set is not emptied. Everything else is still cleared and `no_new_privs` still applies; the boot runs on purpose |
| the boot refuses a missing capability | the message names the missing ones and the `setcap` line that fixes it |
| a warning that another process runs as init's uid | with file capabilities, that uid can become init. Move the worker, the migration or the debug shell to another uid |
| `permission denied` creating `/run/blastit` | `/run` belongs to root. Mount a `tmpfs` there owned by the service uid, or use `RuntimeDirectory=` under systemd |
| a preset warning about the privilege mode | the preset was written for the other mode. It runs either way — root where a preset expected capabilities is simply more authority than intended |

## Where to look

* **the first three `boot` events** — mode, preset, seccomp, what the app cost
  before the fork, and the view-process cap it bought;
* **the `request` event of the failing request** — `route`, `rule`, `status`,
  `outcome`, `queued_ms`, `duration_ms`;
* **the `sql.denied` events around it** — `route`, `proxy`, `kind`, `reason`;
* **`manifest.warning`** — an unrestricted route, or an unattached group.

If you are opening an issue, those four things plus what you expected to happen
are most of the report. `e2e/README.md` and `python/tests/README.md` describe how
to reproduce against a real PostgreSQL, which is the fastest way to tell a
manifest problem from a blastit one.

## See also

* [Log events](reference/log-events.md) — every field above.
* [Day-to-day life](frameworks/day-to-day.md) — the denial loop, and folding a
  production log.
* [Anything else](infra/anything-else.md#client-ip-and-trusted-proxies) — the
  client-address rules.
