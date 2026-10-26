# Log events

stdout is blastit's only output, and it is an **event stream: one event per
line**. Events cover two things: what blastit decided (routing, denials, process
lifecycle) and what the app printed. Nothing is written to disk.

## Formats

| `BLASTIT_LOG_FORMAT` | What it prints |
|---|---|
| `json` (the default) | one JSON object per line, for a log collector and for `blastit-manifest fold` |
| `pretty` | the same events as coloured, aligned lines, for running blastit by hand |

```json
{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"request","component":"scheduler","request_id":"01JB9…","method":"GET","path":"/products/42/","route":"product-detail","rule":0,"status":200,"outcome":"ok","queued_ms":0.1,"duration_ms":18.4,"bytes_in":0,"bytes_out":5120,"cost_ns":18400000,"cpu_ns":9100000,"remote_addr":"203.0.113.7"}
{"ts":"2026-10-25T14:02:12.007Z","level":"warn","event":"sql.denied","component":"dbproxy","route":"product-detail","pid":57,"proxy":"db","kind":"parse","sqlstate":"42501","in_transaction":false,"sql":"SELECT \"shop_product\".\"price\" FROM \"shop_product\" WHERE \"shop_product\".\"id\" = $?","reason":"select on shop_product.price not granted"}
```

```text
14:02:11.384 INFO  request     GET /products/42/ → product-detail  200  18.4ms
14:02:12.007 WARN  sql.denied  product-detail  db  parse  SELECT "shop_product"."price" FROM …
```

Both carry the same fields. `BLASTIT_LOG_LEVEL` sets the minimum level printed.

**Fields common to every event:** `ts` (UTC, with milliseconds), `level`,
`event`, `component` — plus `route` and `request_id` when they apply.

**Line size.** Every line is kept to at most 2048 bytes, because App Platform
truncates forwarded lines and a truncated JSON line is unparseable. Long fields
(`sql`, `path`, `line`, `reason`, `message`, `immortalize_reason`) are cut and
listed in a `"truncated"` array, and long app output is split into several
`app.output` events carrying `part` and `parts`.

**One writer.** init is the only process that writes to stdout, so lines never
interleave: http, the scheduler and dbproxy send structured events to init over
a dedicated pipe, and every child's stdout and stderr go to init over pipes,
where init wraps each line in an `app.output` event. init stamps `component` and
`route` according to *which pipe* the data came from, so a compromised view
cannot forge another component's events — the most it can do is print lines
labelled as its own app output.

## The events

| Event | From | Level | Fields | Emitted when |
|---|---|---|---|---|
| `request` | scheduler | info (warn for 5xx) | `request_id`, `method`, `path`, `route` (null if unmatched), `rule` (index of the matching `path` regex), `status`, `outcome`, `queued_ms`, `duration_ms`, `bytes_in`, `bytes_out`, `cost_ns`, `cpu_ns`, `remote_addr` | once per request, at the end. This is the routing decision |
| `sql.denied` | dbproxy | warn | `route`, `pid` (the view process), `proxy`, `kind` (`query`, `parse`, `function_call`, `copy`, …), `sql` (normalised), `sqlstate`, `in_transaction`, `reason` | a statement was rejected |
| `pg.refused` | dbproxy | warn | `route`, `proxy`, `reason` (`credentials_mismatch`, `too_many_connections`, `upstream_unreachable`, …) | a connection to the proxy was refused |
| `http.rejected` | http | info | `remote_addr`, `status` (`400`, `413`, `431`, `501`…), `reason` | a request was refused before reaching the scheduler |
| `process.spawned` / `process.exited` / `process.retired` | init, scheduler | info (warn for crashes) | `route`, `pid`, `uid`, `fork_ms`, `status` or `signal`, `reason` (`idle`, `max_requests`, `stuck_threads`, `memory_pressure`) | view process lifecycle |
| `process.cap` | scheduler | info | `estimate_bytes` (the measured cost of one view process), `cap` (the live process cap it buys) | the measured cost or the cap changed. Never once per request |
| `app.output` | init | info (stdout) / warn (stderr) | `stream`, `line`, `part`, `parts` | the app or a library printed something |
| `boot`, `ready`, `shutdown` | init | info | see below | lifecycle of the whole server |
| `manifest.warning` | init | warn | `message` | the manifest is usable but has a problem worth reporting, e.g. a route that is `unrestricted` on a proxy, or a group that is attached nowhere |

`outcome` is one of `ok`, `unmatched`, `queue_full`, `queue_timeout`,
`timeout`, `view_died`, `client_gone`.

`cost_ns` is what the request was charged for fair queuing: the view process's
own measurement, clamped between the time from dispatch to the view's first
response frame and the time the scheduler observed to the end of the response —
or that observed time when no report arrived. It is always a number. `cpu_ns` is
the thread CPU time the view measured over the same span, and `null` when the
view had none to report (no cost frame, or no thread clock on its interpreter),
so a reader never has to guess which happened. `duration_ms` still ends at the
last response byte, whether or not the number arrived with it.

## The three `boot` lines

Startup has three phases, and each prints its own `boot` event. A log with all
three is readable on its own, which is the point.

**`stage0`** — printed before anything can fail, so it says the seccomp setting
— read straight from its variable — and nothing about a configuration that has
not been read yet. It carries `privilege` (`root` or `capabilities`),
`bounding_set` (`dropped` or `kept`) and, in rootless mode, the
`capabilities` set.

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"capabilities","capabilities":["cap_chown","cap_dac_override","cap_fowner","cap_kill","cap_setgid","cap_setuid","cap_setpcap"],"bounding_set":"dropped"}
```

**`config`** — emitted as soon as the configuration parsed, and *before* the
manifest is read, so a boot that fails later still says which preset it was
failing with. It carries the mode init detected, the seccomp setting as
configured, and the preset:

| Field | Meaning |
|---|---|
| `preset` | the preset in force, or `null` |
| `preset_settings` | the settings the preset supplied, with **the preset's own values** — never the operator's |
| `preset_overridden` | the names of those settings that an explicit variable overrode. The value that won is never reported |

A `warn` `app.output` line follows this one when the preset's assumptions about
the platform do not hold for this boot (`preset_expects_root`,
`preset_expects_rootless`).

**`preload`** — stage 1's, the one that says what the app cost before the fork:
`immortalize` (`on`/`off`), `immortalize_reason` when it was skipped,
`immortalized` (how many heap objects were marked) and `immortalize_ms`, and the
memory the view-process cap was derived from — `mem_total`, `mem_used` and
`max_view_processes`. Byte counts are integers.

```json
{"event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":1610300,"immortalize_ms":1785.4,"mem_total":1073741824,"mem_used":203423744,"max_view_processes":22}
```

`ready` follows with `stage: "serving"`, the route count, the worker budget, and
the **names** of the output variables per proxy — never their values:

```json
{"event":"ready","component":"init","stage":"serving","routes":4,"workers":16,"outputs":{"db":["DATABASE_URL"]}}
```

`shutdown` reports the phase (`drain`, `stopped`, or `<component>-died`) and the
reason a component went away. If http, the scheduler or dbproxy dies, init kills
everything and exits non-zero, and the platform restarts the container.

## What is never logged

**Query strings, header values, request and response bodies, and SQL parameter
values.** They routinely carry personal data and secrets. `path` is the routed
`PATH_INFO`, without the query string.

This is an API guarantee rather than a convention: those fields cannot be
expressed in the event model, so no future caller can add one by accident. The
one place a value from a header is involved — the fair queue's inner flow key —
is a per-process seeded hash, and neither the values nor the hash are logged.

## Correlation

dbproxy does not see request ids, because the app opens its database connections
itself. `sql.denied` carries the route and the view process `pid`, which together
with `ts` is enough for tooling to line it up with `request` events.

## Reading it

* **`blastit-manifest fold`** reads exactly this stream and turns `sql.denied`
  into grants: [fold the log](../frameworks/day-to-day.md#fold-the-log).
* **`BLASTIT_LOG_FORMAT=pretty`** is what to run when you are debugging by hand
  and do not want to pipe through `jq`.
* **`BLASTIT_LOG_LEVEL=warn`** keeps the steady state to the interesting lines
  without hiding refusals, which are all `warn` or above.

## See also

* [Settings](settings.md#logs) — `BLASTIT_LOG_FORMAT` and `BLASTIT_LOG_LEVEL`.
* [Troubleshooting](../troubleshooting.md) — what a symptom means.
* [`ARCHITECTURE.md` §8.1](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#81-logs) — the specification.
