# Log events

stdout is the only output, and it is an event stream: one event per line.
Nothing is written to disk.

## Formats

| `BLASTIT_LOG_FORMAT` | What it prints |
|---|---|
| `json` (the default) | one JSON object per line, for a collector and for `blastit-manifest fold` |
| `pretty` | the same events as coloured, aligned lines, for debugging by hand |

Every event carries `ts`, `level`, `event` and `component`, plus `route` and
`request_id` when they apply. `BLASTIT_LOG_LEVEL` sets the minimum level
printed. Lines are cut at 2048 bytes, and the cut fields are listed in
`truncated`; a long app line becomes several `app.output` events carrying `part`
and `parts`.

```json
{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"request","component":"scheduler","request_id":"01JB9…","method":"GET","path":"/products/42/","route":"product-detail","rule":0,"status":200,"outcome":"ok","queued_ms":0.1,"duration_ms":18.4,"bytes_in":0,"bytes_out":5120,"cost_ns":18400000,"cpu_ns":9100000,"remote_addr":"203.0.113.7"}
```

## The events

| Event | When | Key fields |
|---|---|---|
| `request` | once per request, at the end — the routing decision | `request_id`, `method`, `path`, `route` (null if unmatched), `rule`, `status`, `outcome`, `queued_ms`, `duration_ms`, `bytes_in`, `bytes_out`, `cost_ns`, `cpu_ns`, `remote_addr` |
| `sql.denied` | a statement was rejected | `route`, `pid`, `proxy`, `kind`, `sql` (normalised), `sqlstate`, `in_transaction`, `reason` |
| `pg.refused` | a connection to a proxy was refused | `route`, `proxy`, `reason` (`credentials_mismatch`, `upstream_unreachable`, …) |
| `http.rejected` | a request was refused before the scheduler | `remote_addr`, `status` (`400`, `413`, `431`, `501`…), `reason` |
| `process.spawned` / `process.exited` / `process.retired` | view process lifecycle | `route`, `pid`, `uid`, `fork_ms`, `status` or `signal`, `reason` (`idle`, `max_requests`, `stuck_threads`, `memory_pressure`) |
| `process.cap` | the measured cost or the live cap changed | `estimate_bytes`, `cap` |
| `app.output` | the app or a library printed something | `stream`, `line`, `part`, `parts` |
| `boot`, `ready`, `shutdown` | the server's lifecycle | see below |
| `manifest.warning` | the manifest is usable but has a problem | `message` |

`outcome` is one of `ok`, `unmatched`, `queue_full`, `queue_timeout`, `timeout`,
`view_died`, `client_gone`. `cost_ns` is what the request was charged for fair
queuing; `cpu_ns` is `null` when the view had no thread clock to report.

## The three `boot` lines

Startup prints one `boot` event per phase, so a log with all three is readable on
its own.

* **`stage0`** — before anything can fail: the `seccomp` setting as read, the
  `privilege` mode (`root` or `capabilities`), the `bounding_set`, and the
  rootless `capabilities`. `preset` is always `null` here. Written in the format
  `BLASTIT_LOG_FORMAT` names — this line waits for the settings — or as JSON
  when the settings do not parse.
* **`config`** — as soon as the configuration parsed, before the manifest:
  `preset` in force (or `null`), `preset_settings` with the preset's own values,
  and `preset_overridden`, the names an explicit variable overrode.
* **`preload`** — what the app cost before the fork: `immortalize`, its
  `immortalize_reason` when skipped, `immortalize_ms`, and the memory the
  view-process cap came from — `mem_total`, `mem_used`, `max_view_processes`.

`ready` follows with `stage: "serving"`, the route count, the worker budget, and
the **names** of the output variables per proxy, never their values. `shutdown`
reports the phase (`drain`, `stopped`, or `<component>-died`) and why a component
went away.

## What is never logged

**Query strings, header values, request and response bodies, and SQL parameter
values.** `path` is the routed `PATH_INFO`, without the query string. This is an
API guarantee: those fields cannot be expressed in the event model.

## Correlation

dbproxy never sees a request id, because the app opens its own database
connections. `sql.denied` carries the route and the view `pid`, which together
with `ts` line it up with `request` events.
