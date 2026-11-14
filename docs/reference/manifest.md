# The manifest

The manifest is Blastit's routing table and its whole permission model, in one
file. Blastit reads `./blastit.toml`; `BLASTIT_MANIFEST` names another path.
There is no upward search. Parsed strictly at boot, so an unknown key is an
error.

## Top-level keys

| Key | Required | Meaning |
|---|---|---|
| `version` | yes | must be `1` |
| `app` | no* | the WSGI callable, `module:attribute`; required when any route is not static |
| `preload` | no | a module, or a list of them, imported once before the fork |
| `groups` | no | group names attached to **every** route; route-level `groups` is narrower |
| `[proxy.<name>]` | no | a named broker for one external service; a grant on an undeclared proxy aborts the boot |
| `[group.<name>]` | no | grants several routes share; a group grants nothing until attached |
| `[[route]]` | yes | one sandbox: one process, one uid |

\* `BLASTIT_APP` may supply the callable instead, and it wins over `app`. An
all-static manifest needs no `app`: every route is a `[route.static]`. Setting
`app` or `BLASTIT_APP` there is a boot error, because no route could reach it.

```toml
version = 1
app = "myshop.wsgi:application"     # Django; "app:app" for Flask
preload = ["myshop.urls"]           # imported before the fork, shared copy-on-write
```

`BLASTIT_APP` and `BLASTIT_PRELOAD` override the two keys; `BLASTIT_PRELOAD` is
comma-separated, and `preload` takes a string or a list. `version`, `app`,
`preload` and `groups` are top-level. Written after the first `[table]` they are
a parse error: `groups after a [table] is not top-level; move it above the first table`.

## Groups

A group is a named grant several routes share, attached only where a route names
it. The top-level `groups` key attaches one to **every** route, including routes
that never touch the session; prefer the route form.

```toml
[[route]]
name = "account"
path = ['/account/']
methods = ["GET"]
groups = ["session"]

[group.session.db]
transaction_control = true
queries = ["SELECT … FROM django_session WHERE session_key = $?"]
```

A group grants nothing until it is attached, and a group attached nowhere is a
boot warning.

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
| `egress` | no | `true` opens **any** IPv4/IPv6 address and port, including cloud metadata; there is no allowlist |
| `prefork` | no | `true` forks this view at startup instead of on first use |
| `[route.ingress]` | no | how a request body is handled; required for a route that takes one |
| `[route.static]` | no | serve a built front end from a directory; `spa = true` answers a deep URL with its `index` |

## Ingress

A route that receives a body declares what happens to it. A request with a body
to a route with no `[route.ingress]` is a `413`; `blastit check` also fails a
route that took a body without one.

```toml
[[route]]
name = "checkout"
path = ['/checkout/']
methods = ["POST"]

[route.ingress]
body = "json"      # decoded and re-encoded before the app sees it
```

| `body` | What the route gets |
|---|---|
| `json` | only a valid JSON body, re-encoded canonically; anything else is a `400` |
| `form` | only `application/x-www-form-urlencoded`, decoded and re-encoded; anything else is a `400` |
| `none` | no body at all; a request with one is a `413` |
| `raw` | the bytes unchanged — the loud opt-out, flagged at boot |

The declared codec must match what the route really receives: `blastit check`
reports `ingress_mismatch` when the suite sent something else. `raw` also needs
`allow_raw = true` in the same table, or `blastit check` reports `raw_ingress`.
0.1.1 has no multipart codec, so a file upload is `raw` today and the app's
parser sees the original bytes.

## Static

A built front end is one route with a `[route.static]` block. Blastit serves the
files itself: there is no server in front and no rewrite regex to write. Keys are
in [Static assets and SPAs](../frameworks/static-and-spa.md).

```toml
[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"                 # required, relative to blastit.toml
index = "index.html"            # default: a directory or the SPA fallback answers with it
spa = true                      # default false: a deep URL answers `index`
immutable = ["/assets/*"]       # default none: a match gets the year-long cache
```

| Key | Default | Meaning |
|---|---|---|
| `root` | required | the directory served, relative to `blastit.toml`: no leading `/`, no `..` segment, no `\`, no control byte |
| `index` | `index.html` | the file a directory or the SPA fallback answers with, relative to `root`; no dotfile segment |
| `spa` | `false` | a path that is no file answers `index` instead of the app |
| `immutable` | none | `/`-anchored globs matched whole against the answering file's path; `*` stops at `/`; a match gets `Cache-Control: public, max-age=31536000, immutable` |

Only a file from `root` whose path matches a pattern is cached for a year; an
`index` answer is always `no-cache`. A path it does not answer reaches the app
with the route's `methods` and `[route.ingress]`, or is a `404` when the manifest
has no `app`. `..`, NUL and other control bytes, absolute and backslash paths are
`400`; a symlink that leaves `root` and a dotfile are `404`.

The four keys are reserved: `[route.static]` is never a grant table, so
`[route.static] queries = …` is an unknown key.

A static route that can never reach the app — `spa = true`, or a manifest with no
`app` — may not declare `[route.ingress]`, `prefork = true`, a `[route.<proxy>]`
grant or a route-level `groups` list: each is a boot error, because nothing could
ever run it. A front controller omits `spa` and keeps its `[route.ingress]`.

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

`blastit.toml` holds no credentials and no inline literals, so it is safe to
commit.

### Protocol `postgresql`

Input slots: `url` (a libpq URI), or the same split into `host`, `port`, `user`,
`password`, `database`, `sslmode` — not both — plus `ca_cert` (PEM for the
upstream TLS leg).

* **URL query parameters** are allowlisted: `application_name`,
  `channel_binding`, `connect_timeout`, `sslmode`, `sslcompression`,
  `target_session_attrs` and `keepalives*`. Anything else — `options` included —
  aborts the boot, so a URL cannot smuggle session settings or a credential past
  the proxy. Set a session default on the role or the database instead.

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

[route.db]
functions = ["st_distance"]         # beyond the built-in safe set

[route.db.tables]                   # instead of listing every statement
restaurants_restaurant = { select = ["id", "slug", "name"] }
orders_order = { select = "*", update = ["status"], delete = true }
```

| Key | Type | Meaning |
|---|---|---|
| `queries` | list of strings | the statements this grant allows, compared after normalisation |
| `tables` | table | which tables and columns the route may read and write |
| `functions` | list of strings | function calls allowed beyond [the built-in safe set](#the-built-in-safe-function-set) |
| `transaction_control` | bool | allow `BEGIN`/`COMMIT`/`ROLLBACK`/`SAVEPOINT` and friends |
| `unrestricted` | bool | allow **every** statement on this proxy. Inline on a route only |

An exact `queries` entry is trusted as it is: the text is compared and run,
without looking at what it calls. Paste only a statement you have read:
`pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file or network
capabilities, not ordinary queries. A statement that inlines a literal cannot be
granted — bind the value instead.

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
missing proxy input variable, `unrestricted` in a group or next to
`queries`/`tables`/`functions`, an unknown `[route.static]` key or a
`root`/`index`/`immutable` that is not a relative path, is empty, names a dotfile
`index` or a `..` segment, and a static route that can never reach the app
declaring an ingress codec, `prefork`, a grant or inline `groups`.

A static root's own existence is audited by `blastit dev`, `blastit` and
`blastit check` before anything binds, not by validation: a manifest has to
validate on a machine where the build has not run yet. A root that is missing,
not a directory or unlistable stops the boot naming the route and the root.

Warns and runs: a group attached nowhere, a proxy no grant uses, an unrestricted
route, a `raw` ingress, and an `egress` route.

## Writing one

Three tools write the manifest from evidence, instead of from memory:

* **`blastit check --learn`** — runs your suite, records every request and
  statement, and adds the exact `queries` the manifest lacks (a statement that
  inlines a value is refused, not written): [CI](../ci.md#bootstrap-an-existing-project);
* **`blastit dev --learn`** — the same, from clicking through the app:
  [local](../local.md#fill-the-manifest-in-one-pass);
* **`blastit-manifest fold`** — from an existing log's `sql.denied` events:
  [command line](cli.md#blastit-manifest-fold).

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
