# Django

The whole lifecycle, from an existing project: two settings, an install, the
first `blastit.toml`, running it locally, and then filling the grants in from
your test suite. Nothing here changes your code.

## Two settings

Django's SQL only becomes checkable when two things are true, and both are
Django settings rather than blastit ones:

```python title="myproject/settings.py"
DATABASES["default"]["CONN_MAX_AGE"] = 0
# Request threads don't outlive their request: under blastit every request opens
# its own connection.

DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
# Requires psycopg 3. Parameters then travel separately from the SQL text, so a
# statement's text is the same on every request and can be allowed exactly.
```

Why each one matters is in
[what blastit needs from an app](index.md#what-blastit-needs-from-an-app).

## Install

```console
$ uv add blastit                  # or: pip install blastit
```

Then, in the environment where you run `manage.py`:

```console
$ uv add 'psycopg[binary]' dj-database-url
```

`dj-database-url` is not required — it is just the shortest way to parse
`DATABASE_URL` into Django's `DATABASES`, and it is what the
[end-to-end demo](https://github.com/OffworldNexus/blastit/tree/develop/e2e/demo)
uses.

## First manifest

Write `blastit.toml` next to `manage.py`. This one is for a small shop app: a
public catalogue, a login, Django's admin, and a catch-all.

```toml title="blastit.toml"
version = 1
groups = ["session"]                     # attached to every route

[proxy.db]                               # a named broker for one service (§5.1)
protocol = "postgresql"
input = { url = "DATABASE_URL" }         # the real URL stays here, never in the app

# Statements several routes share.
[group.session.db]
transaction_control = true
queries = [
  'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21',
  'SELECT "auth_user"."id", "auth_user"."password", "auth_user"."username" FROM "auth_user" WHERE "auth_user"."id" = $1 LIMIT 21',
]

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
methods = ["GET", "POST"]
[route.db]
queries = [
  'SELECT "auth_user"."id", "auth_user"."password", "auth_user"."username" FROM "auth_user" WHERE "auth_user"."username" = $1 LIMIT 21',
  'UPDATE "auth_user" SET "last_login" = $1 WHERE "auth_user"."id" = $2',
]

[[route]]
name = "admin"                           # Django's back-office: too much SQL to list
path = ['/admin/.*']
[route.db]
unrestricted = true                      # flagged at boot, and see below

[[route]]
name = "fallback"                        # everything else
path = ['.*']
```

### Proxies, grants, groups, `unrestricted`

**`[proxy.db]`** declares one external service. `input` names the environment
variables that hold the real configuration; blastit takes them, removes them
from the app's environment, and hands the app a URL to its own local proxy
instead. The default output is the same variable name, so the app keeps reading
`DATABASE_URL` and never knows.

**A route's grant** is the union of four things: its own inline `queries`, its
groups' `queries`, the global groups' `queries`, and the same for `tables`.
`transaction_control` is on if any of them turns it on. A route with no grant at
all for a proxy gets **no listener** for that proxy: the `fallback` route above
has no `[fallback.db]`, so it cannot reach the database even by accident.

**`queries` are exact** after normalisation — trimmed, a trailing `;` removed,
whitespace collapsed, and every bind placeholder `$1`/`$2`/… compared as `$?`
(see [how a statement matches](../reference/manifest.md#how-a-statement-matches)).
`tables` is the other way: name which tables and columns a route may read and
write, and each statement is parsed with PostgreSQL's own parser and allowed
when everything it touches is granted.

| Use `queries` when | Use `tables` when |
|---|---|
| the route runs a handful of known statements | the route runs many shapes against a few tables |
| you want the exact text in the file, reviewable | you would rather review "`orders_order`: select/insert, `status` updatable" than forty statements |

Both can live in the same grant; a statement that matches no exact query is
checked against the table grant.

**Groups** are named sets of the same grants, for statements several routes
share. A group grants nothing until it is attached — to some routes
(`groups = ["session"]` inside a `[[route]]`), or to all of them (`groups =
[…]` at the top of the file). Above, `session` is global because every
authenticated request loads a session.

**`unrestricted = true`** means exactly what it says: that route may run any
statement on that proxy, transaction control included. It is accepted **inline
on a route only** — a group attached to every route would turn the whole
manifest into one sandbox — and every unrestricted (route, proxy) pair is
reported as a `manifest.warning` at boot. What remains for such a route is the
sandbox itself: its own uid, its own listener, and the fact that the process
never sees the credentials. Django's admin is the usual example: its SQL is too
varied to enumerate, and it is worth giving it its own route so that the rest of
the app is not paying for that.

!!! tip "Start smaller than this"

    You do not have to write the statements by hand, and you should not. Write
    the routes and an empty `queries = []`, then
    [let the test suite fill them in](#fill-in-the-manifest), or read the
    `sql.denied` events from a run.

## Run it locally

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ uv run python manage.py migrate

$ export BLASTIT_APP=myproject.wsgi:application
$ export BLASTIT_PRELOAD=myproject.urls   # import the URLconf before the fork
$ export BLASTIT_MANIFEST=$PWD/blastit.toml
$ export PYTHONPATH=$PWD                  # so `myproject` is importable
$ export PORT=8080
$ sudo -E .venv/bin/blastit
```

`sudo -E` keeps your `BLASTIT_*` variables, and `.venv/bin/blastit` is *this*
project's console script: stage 1 re-executes the same interpreter, so your
project's packages — Django included — are importable in every view process. A
bare `sudo blastit` would find whatever interpreter root's `PATH` points at
instead.

In another terminal:

```console
$ curl -sS -o /dev/null -w '%{http_code}\n' localhost:8080/products/
$ curl -sS -o /dev/null -w '%{http_code}\n' localhost:8080/nothing-here/
```

The `boot` events say what blastit decided before the first request. There are
three, one per phase of the startup:

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped"}
{"event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":null}
{"event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":148213,"immortalize_ms":412.7,"mem_total":1057259520,"mem_used":211812352,"max_view_processes":23}
{"event":"ready","component":"init","stage":"serving","routes":4,"workers":16,"outputs":{"db":["DATABASE_URL"]}}
```

and, when the manifest has something worth saying, a warning of its own:

```json
{"event":"manifest.warning","component":"init","message":"route `admin` is unrestricted on proxy `db`"}
```

## Fill in the manifest

Denials in production are the truth — that is what actually ran. But each pass
only reveals the *first* denial of each request, because a denied statement
aborts the request, so filling the manifest in that way is one deploy per
statement. Your test suite already makes most of those requests.

### With the pytest plugin

blastit ships a pytest plugin that watches your suite and tells you the
difference, before the deploy. It is inert unless you ask for it:

```console
$ uv add --dev pytest-django
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

Set the manifest once instead of on every command line:

```ini title="pytest.ini"
[pytest]
blastit_manifest = blastit.toml
```

It records every request Django handles — `django.test.Client`, `live_server` in
its own thread, a view called through `BaseHandler.get_response` — with the SQL
each one sent **as it goes on the wire**, and diffs that against the manifest
with blastit's own routing and SQL matching. It needs
[pytest-django](https://pytest-django.readthedocs.io/) and psycopg 3 with
server-side binding. `--blastit-strict` fails the session on anything the report
finds, which is what keeps a manifest from rotting in CI.

The options, and what the JSON report contains, are in
[the plugin's reference](../reference/pytest-plugin.md).

### The live server

The plugin sees what your *test process* does. A suite that drives a live server
— Playwright against `live_server`, a browser, `requests` to a URL — makes its
traffic in a server process of its own, and the paths nobody asserts on are
exactly the ones that surprise a deployment.

```python
from urllib.request import urlopen


def test_notes_page(blastit_live_server):
    with urlopen(f"{blastit_live_server.url}/notes/") as response:
        assert response.status == 200
```

`blastit_live_server` starts the real `blastit` binary on a free port, with your
manifest and your *test* database, and hands you the URL. On teardown its log is
folded into the report: every request it served and every `sql.denied` it wrote,
so a grant the front end needs and your tests never exercised shows up as a
missing statement like any other.

A suite already written against `live_server` does not have to change:

```console
$ uv run pytest --blastit-live-server --blastit-manifest blastit.toml
```

`--blastit-live-server` makes `live_server` *be* blastit's, tests untouched — or
alias it in `conftest.py`:
`from blastit.pytest_plugin import blastit_live_server as live_server`.

It runs blastit, so it needs what blastit needs: **root, or passwordless
sudo**. Without either it skips, with the reason, rather than failing your
suite. It preloads your root URLconf and serves your project where it lives when
the view uids can read it there; when they cannot — pytest's own temporary
directories are private — it serves a copy in a temporary directory it creates
and removes, and changes no mode of your checkout.

!!! warning "Three things the live server needs, and one it does not do yet"

    This fixture is the newest and least forgiving part of the plugin. Check
    all three before you conclude your manifest is wrong.

    1. **`WSGI_APPLICATION` has to be spelled `module:attribute`.** The fixture
       hands that setting to `BLASTIT_APP` as it stands, and Django defines it
       as a *dotted* path — `django-admin startproject` writes
       `myshop.wsgi.application`, which Django accepts and blastit does not. The
       boot then fails with ``BLASTIT_APP: must be `module:attribute` ``. Pass
       blastit's spelling explicitly:

        ```console
        $ uv run pytest --blastit-live-app myproject.wsgi:application --blastit-live-server
        ```

       (The plugin's own test project writes `WSGI_APPLICATION = "wsgi:application"`
       for this reason.)

    2. **The manifest path has to be absolute.** The server runs in another
       directory, so a relative `--blastit-manifest blastit.toml` (or the same
       value in `pytest.ini`) is not found:

        ```console
        $ uv run pytest --blastit-manifest "$PWD/blastit.toml" --blastit-live-app myproject.wsgi:application
        ```

    3. **The WSGI callable has to be importable from the directory that holds
       your settings module**, because that directory is the server's working
       directory and its `PYTHONPATH`. With the `django-admin startproject`
       layout — `myshop/settings.py`, so the settings module is `myshop.settings`
       — that puts `myshop/` itself on the path, and `import myshop.wsgi` fails
       with `ModuleNotFoundError: No module named 'myshop'`. A project whose
       settings module is a plain file next to `wsgi.py` (`settings.py`,
       `DJANGO_SETTINGS_MODULE=settings`) is what the fixture was written for.

    Neither of the last two is something the docs can paper over, and both are
    reported in this pull request.

    What *does* work today, end to end: a suite that drives
    `blastit_live_server` (or `--blastit-live-server`) in a project laid out
    like the plugin's own `python/tests/live_project/`, with
    `WSGI_APPLICATION` in blastit's spelling and an absolute
    `--blastit-manifest`. Everything else about the manifest loop — the report,
    `--blastit-strict`, the denied statements — works in any layout.

Read the report, add the grants, and repeat until the plugin says the manifest
covers every request that was made.

## Next

Once it is installed, the loop is
[adding a view, a new query, and reading a denial](day-to-day.md). Where it runs
is [fit it to your infra](../infra/index.md).
