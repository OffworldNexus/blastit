# blastit

**A WSGI server that keeps the blast radius of a security bug small.**

In a typical Python web app, every view runs in the same process, with the
same database credentials and the same rights. One exploitable bug in the
least important page — a template injection, a deserialisation flaw, a
vulnerable dependency — and the attacker holds everything the whole app
holds: every table, every secret in the environment.

blastit serves the same app, unchanged, but splits it up:

* **Each view runs in its own sandbox.** You group your URLs into *routes*;
  every route gets its own process under its own Unix user. A compromised
  route can't read another one's memory, signal it, or borrow its rights.
* **Views use private resources without knowing their secrets.** The real
  database URL never reaches your code. Your app sees an ordinary
  `DATABASE_URL`, which points at a local proxy run by blastit. Only that
  proxy holds the credentials.
* **Each route gets only the privileges it needs.** The proxy lets every route
  run only the SQL statements the manifest grants it. The product page can
  read products; it can't read password hashes, and it can't `DROP TABLE`. A
  route that doesn't need the network can't open an outbound connection.

```mermaid
flowchart LR
    c1(["GET /products/42/"]) --> catalog
    c2(["POST /login/"]) --> login
    subgraph sandboxes["one process and one Unix user per route"]
        catalog["route <b>catalog</b>"]
        login["route <b>login</b>"]
    end
    catalog -- "only the statements<br/>catalog is granted" --> proxy
    login -- "only the statements<br/>login is granted" --> proxy
    proxy["blastit's proxy<br/>holds the real URL"] -- "real credentials" --> pg[("PostgreSQL")]
```

Your framework doesn't know any of this happens. It gets ordinary WSGI
requests and routes them itself. All blastit adds is one file,
`blastit.toml`, which says which URLs belong to which route and what each
route may do.

> **Status: pre-release.** blastit runs a real Django app end to end on
> DigitalOcean App Platform in CI, but it hasn't had a stable release, and the
> manifest format may still change. Linux only.

## What it looks like

A route for the public catalogue, one for logging in, and a catch-all:

```toml
version = 1

# The app's DATABASE_URL is the real one at startup; blastit takes it, and
# hands the app a URL to its own proxy instead.
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/products/', '/products/[0-9]+/']
methods = ["GET", "HEAD"]
[route.db]
queries = [
  'SELECT "shop_product"."id", "shop_product"."name", "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21',
  'SELECT "shop_product"."id", "shop_product"."name", "shop_product"."price" FROM "shop_product" ORDER BY "shop_product"."name" ASC',
]

[[route]]
name = "login"
path = ['/login/']
[route.db]
transaction_control = true
queries = [
  'SELECT "auth_user"."id", "auth_user"."password", "auth_user"."username" FROM "auth_user" WHERE "auth_user"."username" = $1 LIMIT 21',
  'UPDATE "auth_user" SET "last_login" = $1 WHERE "auth_user"."id" = $2',
]

[[route]]
name = "fallback"     # everything else: no database at all
path = ['.*']
```

If the catalogue page is compromised, the attacker can list products. That's
it. Each refusal is logged as one JSON line that names the route and the
statement it was refused:

```json
{"ts":"2026-10-25T14:02:12.007Z","level":"warn","event":"sql.denied","component":"dbproxy","route":"catalog","proxy":"db","kind":"parse","sqlstate":"42501","sql":"SELECT \"auth_user\".\"password\" FROM \"auth_user\""}
```

## Quick start

### 1. Install

blastit isn't on PyPI yet. Wheels (Linux x86_64, CPython 3.13 and 3.14) are
built from every commit to `develop` and published on
[blastit's own package index](https://offworldnexus.github.io/blastit/simple/).
With uv:

```toml
# pyproject.toml
[[tool.uv.index]]
name = "blastit"
url = "https://offworldnexus.github.io/blastit/simple/"
explicit = true

[tool.uv.sources]
blastit = { index = "blastit" }
```

```console
$ uv add --prerelease allow blastit
$ uv run blastit                     # or .venv/bin/blastit, or python -m blastit
```

With pip:

```console
$ pip install --pre blastit --extra-index-url https://offworldnexus.github.io/blastit/simple/
```

Like granian, blastit is a compiled extension plus a `blastit` command written
in Python, so it runs on the interpreter of the environment it's installed in:
your virtualenv and its packages are simply there. To build a wheel yourself,
run `scripts/build-wheel.sh` (Rust 1.88+ and uv).

Configuration is still environment variables only (`BLASTIT_APP`,
`BLASTIT_MANIFEST`, `PORT`, …), and it still wants to be PID 1 in the
container. Two things to know before you start it:

* it has to run as **root**: init binds the port, then drops every other
  process to its own unprivileged user (see the Dockerfile below);
* the project directory — and therefore the `.venv` with blastit and the app's
  dependencies in it — has to stay **readable by the sandbox users**. Routes run
  as `BLASTIT_UID_BASE + 1000 + i`, and blastit drops to `UID_BASE + 3` before
  it `execve`s its own database proxy, so a project inside a `0700` directory
  hides both from those users. `/app`, `/srv/app` or an ordinary checkout are
  fine.

### 2. Django

Two settings make Django's SQL checkable:

```python
# settings.py
DATABASES["default"]["CONN_MAX_AGE"] = 0  # request threads don't outlive their request
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
# Parameters travel separately from the SQL text, so a statement's text is
# the same on every request and can be allowed exactly.
```

Write `blastit.toml` next to `manage.py` (see above), then run blastit as root
in the container. It binds the port and then drops every process to its own
unprivileged user:

```dockerfile
FROM python:3.13-slim
WORKDIR /app
COPY . .
RUN pip install -r requirements.txt

RUN pip install --pre blastit --extra-index-url https://offworldnexus.github.io/blastit/simple/

ENV BLASTIT_APP=myproject.wsgi:application \
    BLASTIT_MANIFEST=/app/blastit.toml \
    BLASTIT_PRELOAD=myproject.urls \
    PORT=8000
CMD ["blastit"]
```

`BLASTIT_PRELOAD` imports your URLconf before the processes are forked, so
that every route shares the same loaded code instead of loading it again on
its first request.

### 3. FastAPI

blastit speaks WSGI. FastAPI is ASGI, so run it through
[a2wsgi](https://github.com/abersheeran/a2wsgi). a2wsgi starts an event-loop
thread, and blastit forks its processes from a single-threaded parent, so
create the adapter lazily, inside each process:

```python
# myproject/wsgi.py
import threading

from a2wsgi import ASGIMiddleware

from myproject.main import app

_lock = threading.Lock()
_wsgi = None


def application(environ, start_response):
    global _wsgi
    if _wsgi is None:
        with _lock:
            if _wsgi is None:
                _wsgi = ASGIMiddleware(app)
    return _wsgi(environ, start_response)
```

With SQLAlchemy, use psycopg 3 (`postgresql+psycopg://`), which sends
parameters separately by default, and don't pool connections across requests
(`poolclass=NullPool`). Then set `BLASTIT_APP=myproject.wsgi:application`, as
for Django. (CI covers the Django path; this one isn't in CI yet.)

### 4. Fill in the grants

You don't write the SQL by hand. Run the app under blastit, use it, and read
the `sql.denied` lines: each one is a statement a route ran and wasn't granted
yet. If it belongs there, copy it into the route. If it doesn't, you just
found a route that does more than you thought.

#### Fill in the grants with your test suite

Denials in production are the truth — that's what actually ran. But each pass
only reveals the *first* denial of each request, because a denied statement
aborts the request, so filling the manifest in that way is one deploy per
statement. Your test suite already makes most of those requests. blastit ships
a pytest plugin that watches it and tells you the difference, **before** the
deploy:

```console
$ uv run pytest --blastit-manifest blastit.toml
...
blastit: 412 requests, 977 statements, 38/41 routes exercised — report in blastit-report.json
blastit: route product-list: 3 statement(s) missing (see blastit-report.json)
blastit: route checkout: needs transaction_control = true
blastit: route search: 2 grant(s) unused
blastit: route queue opens outbound connections (redis:6379, amqp:5672) but has no egress = true
blastit: 4 unrouted request(s): GET /robots.txt, GET /favicon.ico (see blastit-report.json)
blastit: 2 connection-setup statement(s) some routes do not grant: put them in a group attached to every route (see blastit-report.json)
blastit: 2 route(s) never exercised: webhooks, admin
======================= 1513 passed, 12 skipped in 84.20s =======================
```

The options, when the defaults do not fit:

```
--blastit-manifest PATH        the blastit.toml to diff against; without it the
                               plugin does nothing at all (or `blastit_manifest`
                               in pytest.ini / pyproject.toml)
--blastit-report PATH          where the JSON report goes (default
                               blastit-report.json, relative to where pytest runs)
--blastit-proxy ALIAS=PROXY    maps a database alias to a manifest proxy, when
                               the manifest declares more than one postgresql
                               proxy (repeat it per alias)
--blastit-strict               fail the session when anything is missing,
                               unrouted, needing transaction_control or egress,
                               not granted in every route, unreadable, or worth a
                               warning from the plugin itself
```

The plugin is inert unless you ask for it, and it needs
[pytest-django](https://pytest-django.readthedocs.io/) and psycopg 3 with
server-side binding (`DATABASES[...]["OPTIONS"]["server_side_binding"] = True`,
§6). It works under pytest-xdist too: every worker reports what it saw, and the
controller merges them into one report. Set the manifest in `pytest.ini` or
`pyproject.toml` if you would rather not repeat the option:

```ini
[pytest]
blastit_manifest = blastit.toml
```

`blastit-report.json` has the detail the summary only counts, per route:

```json
{
  "name": "product-list",
  "requests": 26,
  "needs_transaction_control": false,
  "egress": {
    "observed": [
      {"target": "redis:6379", "host": "redis", "port": 6379, "family": "AF_INET",
       "count": 26, "tests": ["shop/tests/test_catalog.py::test_on_sale"]}
    ],
    "needed": false
  },
  "proxies": [
    {
      "name": "db",
      "observed": 51,
      "missing": [
        {
          "sql": "SELECT \"shop_product\".\"id\", \"shop_product\".\"name\" FROM \"shop_product\" WHERE \"shop_product\".\"on_sale\" = $? LIMIT 21",
          "count": 9,
          "wire_sql": "SELECT \"shop_product\".\"id\", \"shop_product\".\"name\" FROM \"shop_product\" WHERE \"shop_product\".\"on_sale\" = $1 LIMIT 21",
          "tests": ["shop/tests/test_catalog.py::test_on_sale"],
          "paths": ["/products/on-sale/"]
        }
      ],
      "unused": ["SELECT \"shop_product\".\"id\" FROM \"shop_product\" LIMIT 21"],
      "transaction_control": {"observed": false, "granted": false, "needed": false}
    }
  ]
}
```

and, at the top level, the statements every connection runs:

```json
"connection_setup": {
  "statements": [
    {"sql": "SELECT set_config('TimeZone', $?, false)", "proxy": "db", "count": 26,
     "wire_sql": "SELECT set_config('TimeZone', $1, false)",
     "ungranted_in": ["product-list", "search"], "granted_in": 36}
  ],
  "ungranted": 1
}
```

How to read it:

* **`missing`** — statements the suite executed that no grant allows. `sql` is
  the *normalised* text (the exact form the manifest comparison reads), so you
  can paste it into the route's `queries` as it is; `wire_sql` is one raw
  occurrence with its `$1`, `$2`… placeholders; `tests` and `paths` name up to
  five examples, so you can find the code that sent it. A statement the tests
  never sent can still be missing in production — the report is a lower bound.
  Placeholders are compared as `$?`, and so are the shapes a live query changes
  on every call: an `IN ($1)` grant fits an `IN` list of any length, and one
  grant fits a run of `VALUES (…)` rows (a single row is a shape of its own).
* **`unused`** — grants that nothing matched. Either dead code, or a request
  your suite doesn't make yet.
* **`needs_transaction_control`** — the route ran `BEGIN`/`COMMIT` or a
  savepoint (a nested `transaction.atomic()`, or `ATOMIC_REQUESTS` on the
  alias) and the route's grant does not allow transaction control. Add
  `transaction_control = true`; do *not* try to grant a savepoint, its name
  changes on every request. psycopg's own `BEGIN`/`COMMIT` are captured too, so
  this is seen even when no cursor of the request sends one — a transaction that
  was already open when the request began (pytest-django's `django_db`, an outer
  `atomic()`) is not the route's and is not counted.
* **`unrouted`** — paths blastit would answer `404` for before Django ever saw
  them. Either add a route for them (a catch-all `path = ['.*']` is the usual
  answer, so the app renders its own 404s) or stop requesting them.
* **`unexercised_routes`** — routes nothing hit: their grants are unproven, not
  proven wrong.
* **`egress`** — the outbound connections the route tried to open, with the
  destination, how often and an example test, and `needed` when the route has no
  `egress = true` (§7.3), which is what the seccomp filter would refuse. Mocked
  HTTP clients — pytest-httpx, `responses`, a stub transport — never open a
  socket, so the list is a **lower bound**: a route it says nothing about can
  still need `egress = true` in production.
* **`connection_setup`** — the statements psycopg runs as a connection is
  established: `SET`s of your own (a `connection_created` receiver, a
  middleware), Django's `set_config('TimeZone', …)`, a `TypeInfo.fetch` a
  backend does at connect. None of them goes through a request's cursors, and
  under blastit **every request opens its own connection** (`CONN_MAX_AGE = 0`),
  so they run on every route in production — put them in a group attached to
  every route (`groups = ["connection"]` at the top level). `ungranted_in` names
  the routes that would refuse them today; a route with no grant for that proxy
  gets no listener and never runs them, so it is not listed.
* **`unrecognised`** — statements the plugin could not read, counted rather than
  dropped. psycopg's t-string queries (`t"SELECT {name}"`) have no text before
  they are rendered, so nothing can be compared to a grant; report them to
  whoever wrote the query.
* **`plugin_warnings`** — what the plugin itself could not do, most importantly
  an alias that does not set `server_side_binding = True`: psycopg then inlines
  the parameters and the `$n` text in the report is one the plugin made up,
  not the text dbproxy would check.
* `--blastit-strict` fails the run on any of the above, for a CI job that keeps
  the manifest from rotting. Add the `missing` statements, re-run until it is
  clean, and deploy.

Two statements cannot be granted, and the report cannot hide it:

* a savepoint's name (`SAVEPOINT "s140234_x1"`) is made per request: the fix is
  `transaction_control = true` on the route, not a grant;
* a named cursor — `QuerySet.iterator()`, i.e. Django's `chunked_cursor()` — sends
  `DECLARE "_django_curs_140234_sync_1" CURSOR FOR …`, then `FETCH` and `CLOSE`
  with the same generated name, so no grant can match them. Turn server-side
  cursors off for the routes that use them
  (`DATABASES[...]["DISABLE_SERVER_SIDE_CURSORS"] = True`) or accept that they
  will be denied under blastit.

Read the report, add the grants, repeat until
`blastit: the manifest covers every request that was made`. The denial logs stay
the source of truth in production — the tests only make the requests they make —
but by the time the app is deployed the manifest is already most of the way
there instead of a deploy per statement.

## What you get, and what you don't

| If this is compromised… | …the attacker gets | …and cannot |
|---|---|---|
| one route | that route's granted statements, its scratch directory, the network only if the route has `egress = true` | run any other statement, read the database credentials, touch other routes' processes |
| the HTTP front end | the raw bytes of requests in flight | reach the database, run Python |

blastit doesn't protect against code that runs when your app is *imported*:
that happens before the sandboxes exist. And it only sandboxes what goes
through its proxies; other secrets in the environment (an API key for a
third-party service, say) are still visible to every route.

## Configuration

Everything is configured through environment variables: `BLASTIT_APP`,
`BLASTIT_MANIFEST`, `PORT`, worker budgets, timeouts, log format
(`BLASTIT_LOG_FORMAT=pretty` for humans). The full list is in
[`ARCHITECTURE.md` §8.2](ARCHITECTURE.md#82-configuration-reference).

## Learn more

* [`ARCHITECTURE.md`](ARCHITECTURE.md): the design, the security model, and
  what it relies on.
* [`ROADMAP.md`](ROADMAP.md): what's built and what's next.
* [`e2e/`](e2e/): the Django demo that CI runs under blastit, in Docker and on
  App Platform, to prove all of the above.
* [`docs/spikes/`](docs/spikes/): what was measured on the target platform.

## Development

```console
$ cargo fmt --all -- --check
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo test --workspace
```

`develop` is the only long-lived branch. Each roadmap step lands as one pull
request from a `feature/<step>-<slug>` branch; CI must be green before merging.
