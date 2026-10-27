# The manifest

The manifest is blastit's routing table and its whole permission model, in one
file. Default `/app/blastit.toml` or `BLASTIT_MANIFEST`; parsed strictly at boot,
so an unknown key is an error.

## Top-level keys

| Key | Required | Meaning |
|---|---|---|
| `version` | yes | must be `1` |
| `groups` | no | group names attached to **every** route |
| `[proxy.<name>]` | no | a named broker for one external service; a grant on an undeclared proxy aborts the boot |
| `[group.<name>]` | no | grants several routes share; a group grants nothing until attached |
| `[[route]]` | yes | one sandbox: one process, one uid |

`version` and `groups` come before any `[table]`.

## Routes

```toml
[[route]]
name = "product-detail"
path = ['/products/[0-9]+/']        # full match against PATH_INFO
methods = ["GET", "HEAD"]           # optional
groups = ["catalog"]                # shared grants
prefork = true                      # fork at startup
egress = true                       # may open TCP connections
```

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | the route's name, in the log and in `environ["blastit.route"]`; must match `[a-z0-9][a-z0-9_-]*`, and duplicates abort the boot |
| `path` | yes | a list of regexes matched against the whole of `PATH_INFO`, in order; one regex may be a bare string (`path = ".*"` is `path = ['.*']`) |
| `methods` | no | the methods it answers; without it, any method. `HEAD` is never implied by `GET` |
| `groups` | no | groups whose grants this route takes |
| `egress` | no | `true` lets the process open outbound TCP connections |
| `prefork` | no | `true` forks this view at startup instead of on first use |

## Proxies

A proxy declares its protocol, the variables it owns, and the ones the app sees
instead.

```toml
[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }
[proxy.analytics.output]
ANALYTICS_DB_HOST = "{{ socket_dir }}"
ANALYTICS_DB_PORT = "{{ port }}"
```

* **Inputs** — `input` maps input slots to variable names. A missing input
  variable aborts the boot.
* **Outputs** — `output` maps names to templates. Without an `output` table,
  each input variable is rewritten in place: `DATABASE_URL` in, `DATABASE_URL`
  out, pointing at the local endpoint.
* **Templates** are tiny: `{{ field }}` inserts a field and `{{ field | urlencode }}`
  percent-encodes it. They render once, at boot; an unknown field or filter
  aborts the boot.
* **Secrets cannot be templated**: templates see the local endpoint and a few
  non-secret facts.
* An output may not overwrite a variable it does not own, and no two proxies may
  produce the same variable.

`blastit.toml` holds no secrets and is safe to commit.

### Protocol `postgresql`

Input slots: `url` (a libpq URI), or the same split into `host`, `port`, `user`,
`password`, `database`, `sslmode` — not both — plus `ca_cert` (PEM for the
upstream TLS leg).

Template fields: the default `url` is
`postgresql://{{ user }}@/{{ database }}?host={{ socket_dir }}&port={{ port }}` —
the socket directory in a **query parameter**, the one form libpq,
`dj-database-url` and SQLAlchemy all resolve — where `socket_dir` is
`/proc/self/cwd`, each view's own socket directory, and `port` is `5432 + k`,
*k* = index among `postgresql` proxies sorted by name. `user` is `blastit`,
which the proxy ignores.

Default outputs: every input variable is rewritten in place; `sslmode` becomes
`disable`; `password` and `ca_cert` are removed. The local leg is a Unix socket,
so libpq connects to `<host>/.s.PGSQL.<port>`.

Grant keys for this protocol: `queries`, `tables`, `functions`,
`transaction_control`, `unrestricted`.

## Grants

A grant is per proxy; for `postgresql` it says which statements a route may run.

```toml
[group.session.db]                  # grants several routes share
transaction_control = true
queries = ["SET TIME ZONE 'UTC'"]

[[route]]
name = "restaurants"
path = ['/restaurants/.*']
[route.db.tables]                   # instead of listing every statement
restaurants_restaurant = { select = ["id", "slug", "name"] }
orders_order = { select = "*", update = ["status"], delete = true }
functions = ["st_distance"]         # beyond the built-in safe set
```

| Key | Type | Meaning |
|---|---|---|
| `queries` | list of strings | the statements this grant allows, compared after normalisation |
| `tables` | table | which tables and columns the route may read and write |
| `functions` | list of strings | function calls allowed beyond [the built-in safe set](#the-built-in-safe-function-set) |
| `transaction_control` | bool | allow `BEGIN`/`COMMIT`/`ROLLBACK`/`SAVEPOINT` and friends |
| `unrestricted` | bool | allow **every** statement on this proxy. Inline on a route only |

An exact `queries` entry is trusted as it is: the text is compared and run,
without looking at what it calls.

### Table and column grants

`[route.<db>.tables]` has one entry per table; `select`, `insert` and `update`
take columns or `"*"`, and `delete` takes `true`.

A statement no exact query lists is parsed with **PostgreSQL's own parser**
(`libpg_query`) and allowed when every table, column and function it touches is
granted.

What decides real deployments:

* reading a column **anywhere** — a `WHERE`, an `ORDER BY`, a join condition — is
  reading it;
* every base table a statement names must be granted, even when no column of it
  is read;
* `count(*)` reads no column;
* `pg_catalog` and `information_schema` are never readable this way: statements
  on the catalogue stay exact `queries`;
* a row lock (`FOR UPDATE`, `FOR SHARE`, …) needs an `update` entry on every base
  table it locks, on any one column;
* a function call must be in [the built-in safe set](#the-built-in-safe-function-set)
  or in `functions` (`pg_catalog.x` is `x`); casts to `reg*` and other types need
  an entry.

### `unrestricted`

```toml
[[route]]
name = "admin"
path = ['/admin/.*']
[route.db]
unrestricted = true
```

That route may run any statement on that proxy, transaction control included.

* Inline on a route only: `unrestricted` in a group is a boot error.
* Every unrestricted (route, proxy) is a `manifest.warning` at boot.
* The sandbox stays: the listener, the uid check, the other proxies' grants and
  the credential barrier.
* `unrestricted` with `queries`, `tables` or `functions` is a boot error.

### Effective grants

A route's allowlist for a proxy is the **union** of its own grant, its groups'
grants and the global groups'; `transaction_control` is on if any turns it on.
**A route with no grant for a proxy gets no listener**, so a catch-all fallback
cannot reach the database.

## How a statement matches

Matching is exact, on normalised text: trimmed, a trailing `;` removed,
whitespace collapsed outside quotes. The *shape* is normalised too:

* `$1`, `$2`… become `$?`, so one template is one grant;
* a list of nothing but placeholders after `IN` becomes `IN ($?, ...)` at any
  length, so `__in` matches one grant;
* identical rows after `VALUES` collapse to one row and `...`, so `bulk_create`
  matches one grant.

A matching `Parse` authorises the `Bind`/`Execute` that follow; simple query
messages get the same check. `COPY`, `LISTEN` and the function-call
sub-protocol are denied unless listed.

Two things cannot be granted: a savepoint's name (use `transaction_control`) and
a named cursor (`QuerySet.iterator()`).

## Routing

* Routes are tried in order; the first match of `path` (and `methods`) wins.
* A regex must match the whole of `PATH_INFO`.
* They use Rust [`regex`](https://docs.rs/regex/latest/regex/) syntax, matched in
  linear time.
* `BLASTIT_SCRIPT_NAME` strips a mount prefix first.
* Nothing matches: the scheduler answers `404`, no Python runs. A catch-all last
  lets the app render its own 404s.
* Every decision is a `request` event, with `route` and `rule`.

## Validation at boot

Aborts the boot: an unknown key or group, a `version` that is not `1`, a grant
on an undeclared proxy, a duplicate route name, an invalid regex or method, a
missing proxy input variable, and `unrestricted` in a group or next to
`queries`/`tables`/`functions`.

Warns and runs: a group attached nowhere, a proxy no grant uses, and an
unrestricted route.

## Writing one

Two tools write grants from evidence, instead of from memory:

* **the pytest plugin** — Django: it diffs what your test suite ran against the
  manifest and prints the table grant to paste: [the plugin](pytest-plugin.md);
* **`blastit-manifest fold`** — anything, from `sql.denied` events:
  [fold the log](../frameworks/day-to-day.md#fold-the-log).

`e2e/demo/blastit.toml` is a working example.

## The built-in safe function set

A `tables` grant accepts these names; anything else must be named in the grant's
`functions`.

Aggregates: `array_agg`, `avg`, `bool_and`, `bool_or`, `count`, `json_agg`,
`json_build_object`, `jsonb_agg`, `jsonb_build_object`, `max`, `min`,
`string_agg`, `sum`. Conditions: `coalesce`, `greatest`, `least`, `nullif`.
Strings: `btrim`, `concat`, `left`, `length`, `lower`, `ltrim`, `overlay`,
`position`, `replace`, `right`, `rtrim`, `similar_to_escape`, `substr`,
`substring`, `trim`, `upper`. Numbers: `abs`, `ceil`, `floor`, `round`. Dates
and times: `age`, `date_trunc`, `extract`, `make_interval`, `now`, `timezone`,
`to_char`. Sets and arrays: `array_length`, `cardinality`, `generate_series`,
`unnest`. Window functions: `dense_rank`, `rank`, `row_number`, `cume_dist`,
`first_value`, `lag`, `last_value`, `lead`, `mode`, `nth_value`, `ntile`,
`percent_rank`, `percentile_cont`, `percentile_disc`, `stddev`, `stddev_pop`,
`stddev_samp`, `var_pop`, `var_samp`, `variance`. JSON and full text:
`json_extract_path_text`, `jsonb_array_length`, `jsonb_extract_path_text`,
`plainto_tsquery`, `to_tsvector`, `ts_rank`, `websearch_to_tsquery`.

`pg_catalog.` is dropped before the lookup. `unaccent` reads a rule file, so a
grant must name it. The list is `SAFE_FUNCTIONS` in
`crates/blastit-sqlpolicy/src/analysis.rs`.
