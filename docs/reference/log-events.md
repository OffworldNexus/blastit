# Log events

In server mode stdout is the only output, and it is an event stream: one event
per line. Nothing is written to disk. `blastit check --json` is the one
exception: exactly one JSON document on stdout, everything else on stderr, and
exit `2` leaves stdout empty.

## Formats

| `BLASTIT_LOG_FORMAT` | What it prints |
|---|---|
| `json` (the default) | one JSON object per line, for a collector and for `blastit-manifest fold` |
| `pretty` | the same events as coloured, aligned lines, for debugging by hand; `blastit dev` and `blastit check` use it by default |

Every event carries `ts`, `level`, `event` and `component`, plus `route` and
`request_id` when they apply. `BLASTIT_LOG_LEVEL` sets the minimum level
printed. Lines are cut at 2048 bytes, and the cut fields are listed in
`truncated`; a long app line becomes several `app.output` events carrying `part`
and `parts`.

```json
{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"request","component":"scheduler","request_id":"01JB9…","method":"GET","path":"/products/42/","route":"product-detail","rule":0,"status":200,"outcome":"ok","queued_ms":0.1,"duration_ms":18.4,"bytes_in":0,"bytes_out":5120,"cost_ns":18400000,"cpu_ns":9100000,"remote_addr":"203.0.113.7"}
```

`pretty` puts the same event in aligned columns, naming the mount that answered
a file and leaving `(unmatched)` for a path nothing claimed:

```console
<ts> INFO  request     GET /css/app.css → static  304  0.2ms
```

## The events

| Event | When | Key fields |
|---|---|---|
| `request` | once per request, at the end — the routing decision | `request_id`, `method`, `path`, `route` (the view's name; `static` when a static mount answered; null when neither did), `rule`, `status`, `outcome`, `queued_ms`, `duration_ms`, `bytes_in`, `bytes_out`, `cost_ns`, `cpu_ns`, `remote_addr` |
| `sql.denied` | a statement was rejected | `route`, `pid`, `proxy`, `kind`, `sql` (normalised), `sqlstate`, `in_transaction`, `reason` |
| `pg.refused` | a connection to a proxy was refused | `route`, `proxy`, `reason` (`credentials_mismatch`, `upstream_unreachable`, …) |
| `http.rejected` | a request was refused before the scheduler | `remote_addr`, `status` (`400`, `413`, `431`, `501`…), `reason` |
| `process.spawned` / `process.exited` / `process.retired` | view process lifecycle | `route`, `pid`, `uid`, `fork_ms`, `status` or `signal`, `reason` (`idle`, `max_requests`, `stuck_threads`, `memory_pressure`, `reload`) |
| `process.cap` | the measured cost or the live cap changed | `estimate_bytes`, `cap` |
| `app.output` | the app or a library printed something | `stream`, `line`, `part`, `parts` |
| `boot`, `ready`, `shutdown` | the server's lifecycle | see below |
| `runtime` | what the entrypoint resolved to, at boot and after a change that alters it | `entrypoint` (`php`, `python`, `static`), and only the fields its row lists — see below |
| `manifest.warning` | the manifest is usable but has a problem | `message` |
| `dev.reload` | `blastit dev` replaced every view after a save | `routes` (all of them), `files` (what changed), `reason` (`loaded`, `preload`, `blastfile`, `overflow`) |
| `dev.reload.unavailable` | `blastit dev` could not watch anything at boot; the server serves | `errno`, `filesystem` |

`dev.reload` is `info`: one reload of every view, never a subset. On any tracked
change `routes` lists every route serving after the reload, `files` the absolute
paths that triggered it, and `reason` is `loaded` (a file the app loaded), `preload`
(a `preload` module), `blastfile` (the `Blastfile`) or `overflow` (changes were
dropped, so it reloads all). `dev.reload.unavailable` is a `warn` at boot:
watching is a dev convenience, so the server serves without it; `errno` is the
symbolic name (`"EPERM"`), `filesystem` the filesystem type (`"overlayfs"`).
`--no-reload` emits neither, `blastit check` and `blastit test` never watch, and
production has no reload at all.

`outcome` is one of `ok`, `unmatched`, `queue_full`, `queue_timeout`, `timeout`,
`view_died`, `bailed`, `client_gone`. `bailed` — the handler exited, died or
fataled out of a `5xx` it had answered: it answered before it went away, which
is not a death mid-request.
`cost_ns` is what the request was charged for fair queuing; `cpu_ns` is `null`
when the view had no thread clock to report. A request the scheduler answered
itself is charged the time it held a worker — dispatch to completion, never the
wait it spent in the queue — which is `0` for one that never reached a worker
at all, like a `503` refusal or a queue timeout. A `5xx` an app answered itself
is charged like any other answer; only a `5xx` a handler **bailed out of**
(`bailed`) is charged its whole wall time, arrival to last byte.

## The three `boot` lines

Startup prints one `boot` event per phase, so a log with all three is readable on
its own.

* **`stage0`** — before anything can fail: the `seccomp` setting as read, the
  `privilege` mode (`root`, `capabilities`, or `unprivileged-dev` under
  `blastit dev`), the `bounding_set`, and the rootless `capabilities`. `preset`
  is always `null` here. Written in the format `BLASTIT_LOG_FORMAT` names — this
  line waits for the settings — or as JSON when the settings do not parse.
* **`config`** — as soon as the configuration parsed, before the manifest:
  `preset` in force (or `null`), `preset_settings` with the preset's own values,
  and `preset_overridden`, the names an explicit variable overrode.
* **`preload`** — what the app cost before the fork: `build` (the binary's
  version), `immortalize`, its `immortalize_reason` when skipped,
  `immortalize_ms`, and the memory the view-process cap came from — `mem_total`,
  `mem_used`, `max_view_processes`.

`ready` follows with `stage: "serving"`, the route count, the worker budget, and
the **names** of the output variables per proxy, never their values. `shutdown`
reports the phase (`drain`, `stopped`, or `<component>-died`) and why a component
went away.

## The `runtime` line

`runtime` is `info` (`component: "init"`), one line each time the entrypoint
resolves: at boot in `blastit dev` and in server mode, and in `blastit dev` again
after a `Blastfile` change that alters it, so the latest line names what is
serving. A resolved runtime is not a problem.

| `entrypoint` | Fields |
|---|---|
| `php` | `version` (`"8.5.4"`), `libphp` (the resolved path), `php_ini` (the file opened, or `null` when none is) |
| `python` | `version` (`"3.14.0"`), `environment` (`VIRTUAL_ENV` when set, else `sys.prefix`) |
| `static` | `entrypoint` only: no language runtime is loaded |

A field a row does not list is absent from the object, never `null`. Blastit
strips `PHPRC` and `PHP_INI_SCAN_DIR` from the view's environment, so `php_ini`
is the file actually opened. `pretty` prints the entrypoint and `version`, then
the labelled fields `libphp`, `php.ini` and `environment`, two spaces apart; a
`null` `php_ini` prints `php.ini none`, and `static` is followed by
`serving static assets only`.

`runtime` is never `warn`: `warn` is a separate event and means something needs
attention — `manifest.warning` (`unrestricted`, `egress any`, a `postgres` store
that no view or trait reaches, a `raw` body — both shapes in `blastit dev`, only
the acknowledged `body raw unvalidated` in production — an unused `trait`, an
`entrypoint php` that no view reaches) and `dev.reload.unavailable`.

## What is never logged

**Query strings, header values, request and response bodies, and SQL parameter
values.** `path` is the routed `PATH_INFO`, without the query string. This is an
API guarantee: those fields cannot be expressed in the event model.

## Correlation

dbproxy never sees a request id, because the app opens its own database
connections. `sql.denied` carries the route and the view `pid`, which together
with `ts` line it up with `request` events.
