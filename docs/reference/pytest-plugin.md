# pytest plugin

The plugin diffs what your test suite really did against the manifest, before
you deploy — so the grants do not have to be found in production, one deploy per
statement.

!!! warning "Django only"

    It defines "a request" as Django's `BaseHandler.get_response`, so it applies
    to a Django project and nothing else. For Flask and other WSGI frameworks,
    see [Flask and other WSGI apps](../frameworks/flask.md#no-pytest-plugin-for-flask)
    — `blastit-manifest fold` is the answer there.

## It is inert unless you ask for it

```console
$ uv run pytest --blastit-manifest blastit.toml
```

Without `--blastit-manifest` (and without `blastit_manifest` in the
configuration) the plugin does nothing at all: nothing is patched, nothing is
written, and no test sees a difference. Its seven options are one group in
`--help`.

It needs [pytest-django](https://pytest-django.readthedocs.io/) and psycopg 3
with server-side binding
(`DATABASES[...]["OPTIONS"]["server_side_binding"] = True`).

## Options

| Option | Default | What it does |
|---|---|---|
| `--blastit-manifest PATH` | — | the `blastit.toml` to diff against. Without it the plugin does nothing |
| `--blastit-report PATH` | `blastit-report.json` | where the JSON report goes, relative to where pytest runs |
| `--blastit-proxy ALIAS=PROXY` | — | maps a Django database alias to a manifest proxy. Repeatable; needed only when the manifest declares more than one `postgresql` proxy |
| `--blastit-strict` | off | fail the session when anything is missing, unrouted, needing `transaction_control`, opening connections without `egress = true`, not granted in every route, unreadable, or worth a warning from the plugin itself |
| `--blastit-prefer-tables` | off | print the table-grant snippet of a route with missing statements, instead of only naming the statements |
| `--blastit-live-server` | off | serve pytest-django's `live_server` through blastit itself (needs root, or passwordless sudo) |
| `--blastit-live-app MODULE:ATTR` | — | the WSGI callable that server serves. Default: the Django setting `WSGI_APPLICATION`, or `<package>.wsgi:application` derived from `DJANGO_SETTINGS_MODULE` |

The manifest can be set once in the pytest configuration instead of on every
command line:

```ini title="pytest.ini"
[pytest]
blastit_manifest = blastit.toml
```

`--blastit-proxy`, `--blastit-strict`, `--blastit-prefer-tables`,
`--blastit-live-server` and `--blastit-live-app` are an error without a
manifest, rather than being silently ignored.

## What it records

* **every request Django handles** — `django.test.Client`, `live_server` in its
  own thread, or a view called directly, as long as it goes through
  `BaseHandler.get_response` — with its method, its `PATH_INFO`, and the pytest
  node id of the test that made it;
* **every SQL statement executed during that request**, on every database alias,
  **as it goes on the wire**: psycopg's own converter turns `%s`/`%(name)s` into
  `$n`, exactly as it does with server-side binding. That includes what psycopg
  sends by itself — `SET`s while a connection opens, a `TypeInfo.fetch`, and the
  `BEGIN`/`COMMIT` of `_exec_command` — and `ServerCursor`'s `DECLARE`;
* **the outbound connections each request tried to open**, via
  `socket.socket.connect`, with the destination and an example test. A Unix
  socket is not egress: under blastit the database itself is one. Mocked HTTP
  clients — pytest-httpx, `responses`, a stub transport — never open a socket,
  so this list is a **lower bound**;
* **statements executed outside a request** — fixture setup, teardown, a
  `manage.py`-style command — are ignored, because no route is responsible for
  them.

The plugin does not patch `django.test.Client`: it patches
`BaseHandler.get_response`, so anything that ends up in Django is seen, whatever
called it.

It works under **pytest-xdist**: every worker reports what it saw, the controller
merges them into one report, and only files that say which worker wrote them
*and* carry a version this build knows are merged.

### Transaction control

pytest-django runs each test inside a transaction, so a request never sends its
own `BEGIN`/`COMMIT`: what a nested `atomic()` sends is a `SAVEPOINT`, and that
is what the report looks for. A route whose requests ran one without the grant
allowing it is reported as `needs_transaction_control`. When the alias itself has
`ATOMIC_REQUESTS = True` the request is marked as needing a transaction even if
the test's own atomic swallowed the savepoint, because under blastit there is no
test transaction in the way.

A transaction that was already open when the request began (pytest-django's
`django_db`, an outer `atomic()`) is not the route's and is not counted.

## The summary

At the end of the session, the report is written as JSON and summarised on the
terminal:

```text
blastit: 412 requests, 977 statements, 38/41 routes exercised — report in blastit-report.json
blastit: route product-list: 3 statement(s) missing (see blastit-report.json)
blastit: route checkout: needs transaction_control = true
blastit: route search: 2 grant(s) unused
blastit: route queue opens outbound connections (redis:6379, amqp:5672) but has no egress = true
blastit: 4 unrouted request(s): GET /robots.txt, GET /favicon.ico (see blastit-report.json)
blastit: 2 connection-setup statement(s) some routes do not grant: put them in a group attached to every route (see blastit-report.json)
blastit: 2 route(s) never exercised: webhooks, admin
```

## The report

`blastit._blastit.audit` — the Rust module — is the schema; these are the stable
keys.

```json
{
  "version": 1,
  "manifest": {"routes": 41, "proxies": {"db": "postgresql"}, "warnings": []},
  "routes": [
    {
      "name": "product-list",
      "path": ["/products/"],
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
          "transaction_control": {"observed": false, "granted": false, "needed": false},
          "table_grants": {
            "tables": {"shop_product": {"select": ["id", "name", "on_sale"]}},
            "functions": [],
            "toml": "[route.db.tables]\n…",
            "unanalysable": []
          }
        }
      ]
    }
  ],
  "unrouted": [{"method": "GET", "path_info": "/robots.txt", "count": 1, "statements": 0, "tests": ["shop/tests/test_pages.py::test_robots"]}],
  "unexercised_routes": ["webhooks", "admin"],
  "unused_global": {"statements": [], "tables": [], "count": 0},
  "connection_setup": {
    "statements": [
      {"sql": "SELECT set_config('TimeZone', $?, false)", "proxy": "db", "count": 26,
       "wire_sql": "SELECT set_config('TimeZone', $1, false)",
       "ungranted_in": ["product-list", "search"], "granted_in": 36}
    ],
    "ungranted": 1
  },
  "unrecognised": {"statements": [], "count": 0},
  "plugin_warnings": [],
  "totals": {"requests": 412, "routed": 408, "unrouted": 4, "routes": 41,
             "routes_exercised": 38, "statements": 977, "missing": 3,
             "missing_distinct": 3, "unused": 2, "unused_global": 0,
             "needs_transaction_control": ["checkout"], "connection_setup": 2,
             "connection_setup_ungranted": 1, "egress": 1, "unrecognised": 0,
             "plugin_warnings": 0, "clean": false}
}
```

How to read it:

* **`missing`** — statements the suite executed that no grant allows. `sql` is
  the *normalised* text — the exact form the manifest comparison reads — so it
  can be pasted into the route's `queries` as it is. `wire_sql` is one raw
  occurrence with its `$1`, `$2`… placeholders, for a human. `tests` and `paths`
  name up to five examples, so you can find the code that sent it. **A statement
  the tests never sent can still be missing in production: the report is a lower
  bound.**
* **`unused`** — grants nothing matched. Either dead code, or a request your
  suite does not make yet.
* **`unused_global`** — the same idea for grants a **global** group (`groups =
  […]` at the top level) hands to every route at once. A statement such a group
  grants is not one route's grant, so counting it as unused per route would
  repeat one dead grant once per route; it is reported here, once, and only when
  no route matched it. Same for a global group's `tables` grant.
* **`needs_transaction_control`** — the route ran `BEGIN`/`COMMIT` or a savepoint
  and its grant does not allow transaction control. Add
  `transaction_control = true`; do not try to grant a savepoint, its name changes
  on every request.
* **`unrouted`** — paths blastit would answer `404` for before Django ever saw
  them. Either add a route (a catch-all `path = ['.*']` is the usual answer, so
  the app renders its own 404s) or stop requesting them.
* **`unexercised_routes`** — routes nothing hit: their grants are unproven, not
  proven wrong.
* **`egress`** — the outbound connections the route tried to open, and `needed`
  when the route has no `egress = true`, which is what the seccomp filter would
  refuse.
* **`connection_setup`** — the statements psycopg runs as a connection is
  established. None of them goes through a request's cursors, and under blastit
  **every request opens its own connection**, so they run on every route in
  production: put them in a group attached to every route
  (`groups = ["connection"]` at the top level). `ungranted_in` names the routes
  that would refuse them today; a route with no grant for that proxy gets no
  listener and never runs them, so it is not listed.
* **`unrecognised`** — statements the plugin could not read, counted rather than
  dropped. psycopg's t-string queries (`t"SELECT {name}"`) have no text before
  they are rendered, so nothing can be compared to a grant.
* **`plugin_warnings`** — what the plugin itself could not do, most importantly
  an alias that does not set `server_side_binding = True`: psycopg then inlines
  the parameters and the `$n` text in the report is one the plugin made up, not
  the text dbproxy would check.
* **`table_grants`** — the smallest `[route.<proxy>.tables]` grant covering
  **every** statement the pair ran, with the TOML to paste in, so a route can be
  rewritten from exact queries to table grants in one step. A statement a table
  grant cannot describe at all (a `SET`, a `COPY`, a statement this build does
  not understand) is listed in `unanalysable` with the same reason a
  `sql.denied` event carries.

`--blastit-strict` fails the run on any of the above, for a CI job that keeps the
manifest from rotting. Add the `missing` statements, re-run until it is clean,
and deploy.

## The live server

The plugin sees the requests *your test process* makes. A suite that drives a
live server — Playwright against `live_server`, a browser, `requests` to a URL —
makes its traffic somewhere else, in a server process of its own, and the paths
nobody asserts on are exactly the ones that surprise a deployment.

`blastit_live_server` starts the real `blastit` binary on a free port, with your
manifest and your *test* database, and hands you the URL:

```python
from urllib.request import urlopen


def test_notes_page(blastit_live_server):
    with urlopen(f"{blastit_live_server.url}/notes/") as response:
        assert response.status == 200
```

A suite already written against pytest-django's `live_server` does not have to
change:

```console
$ uv run pytest --blastit-live-server --blastit-manifest blastit.toml
```

or, in `conftest.py`:

```python
from blastit.pytest_plugin import blastit_live_server as live_server
```

On teardown, the server's log is folded into the same observations the report is
built from: every `request` it served and every `sql.denied` it wrote, so a grant
the front end needs and your tests never exercised shows up as a missing
statement like any other. A denial carries a route and no request id, so it is
attached to the first request the log shows for it.

Two consequences of that:

* it needs what blastit needs — **root, or passwordless sudo** (view uids,
  listeners, `/run/blastit`). Without either it skips, with the reason, rather
  than failing your suite;
* a route only the live server saw **reports no unused grants**. The log records
  what was *denied*, never what *ran*, so no grant of that route can be called
  dead.

It preloads your root URLconf and serves your project where it lives when the
view uids can read it there; when they cannot — pytest's own temporary
directories are private — it serves a copy in a temporary directory it creates
and removes, and changes no mode of your checkout.

## See also

* [Django](../frameworks/django.md#fill-in-the-manifest) — the loop this fits
  into.
* [The manifest](manifest.md#writing-one) — what you are filling in.
* [`ARCHITECTURE.md` §6](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#6-the-manifest) — what "granted" means exactly.
