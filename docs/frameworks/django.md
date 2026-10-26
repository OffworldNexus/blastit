# Django

The whole lifecycle, from an existing project: two settings, an install, the
first `blastit.toml`, running it locally, and then filling the grants in from
your test suite. Nothing here changes your code.

Not on Django? [Flask and other WSGI apps](flask.md) is the same lifecycle, with
what changes; [the section index](index.md) explains what blastit needs from any
app.

## Two settings

Django's SQL only becomes checkable when two things are true, and both are
Django settings rather than blastit ones:

```python title="myproject/settings.py"
import dj_database_url

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
# Connect from DATABASE_URL, and don't keep the connection past the request:
# under blastit every request opens its own connection, and its thread is
# thrown away afterwards.

DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
# Requires psycopg 3. Parameters then travel separately from the SQL text, so a
# statement's text is the same on every request and can be allowed exactly.
```

Why each one matters is in
[what blastit needs from an app](index.md#what-blastit-needs-from-an-app).

## Install

```console
$ uv init && uv add blastit       # or: pip install blastit
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

[proxy.db]                               # a named broker for one service
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

### What is Django-specific here

Four things in that file, and the rest is the manifest format, which is the same
for every app:

* **`[proxy.db]`** — `input.url = "DATABASE_URL"` is the variable
  `dj_database_url` reads, and blastit hands the app back a `DATABASE_URL` of its
  own, so `settings.py` needs no branch for "under blastit". The rest of the
  proxy declaration is in [the manifest reference](../reference/manifest.md#proxies).
* **`groups = ["session"]`** — Django's session and user lookups happen on
  *every* authenticated request, whichever URL it is, so they belong in a group
  attached to every route rather than copied into each one.
* **`transaction_control = true`** on that group — Django opens a transaction
  around writes, `ATOMIC_REQUESTS` opens one around the whole request, and
  psycopg sends the `BEGIN`/`COMMIT` itself; a savepoint's name is generated per
  request, so it cannot be granted, only allowed. See
  [grants](../reference/manifest.md#grants).
* **`unrestricted = true` on `/admin/.*`** — Django's back-office runs too many
  statement shapes to enumerate, and giving it a route of its own keeps that
  fact out of every other route. It is inline on a route only, and it is flagged
  at boot: [unrestricted](../reference/manifest.md#unrestricted).

Everything else — exact `queries` versus `tables`, how a statement is matched,
what a group does — is in [the manifest](../reference/manifest.md), once, for
every framework.

!!! tip "Start smaller than this"

    You do not have to write the statements by hand, and you should not. Write
    the routes and an empty `queries = []`, then
    [let the test suite fill them in](#fill-in-the-manifest), or read the
    `sql.denied` events from a run.

## Run it locally

A file for the environment is the tidiest way to keep this out of your shell
history — and it is the same file the
[systemd unit](../infra/anything-else.md#systemd) reads:

```ini title="/etc/shop/blastit.env"
DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
BLASTIT_APP=myproject.wsgi:application
BLASTIT_PRELOAD=myproject.urls
BLASTIT_MANIFEST=/srv/shop/blastit.toml
PORT=8080
```

```console
$ uv run python manage.py migrate

$ sudo env $(grep -v '^#' /etc/shop/blastit.env | xargs) .venv/bin/blastit
```

`.venv/bin/blastit` is *this* project's console script: stage 1 re-executes the
same interpreter, so your project's packages — Django included — are importable
in every view process. A bare `sudo blastit` would find whatever interpreter
root's `PATH` points at instead. `BLASTIT_PRELOAD=myproject.urls` imports the
URLconf before the fork, so every route shares one copy of it instead of loading
it again on its own first request.

In another terminal:

```console
$ curl -sS -o /dev/null -w '%{http_code}\n' localhost:8080/products/
$ curl -sS -o /dev/null -w '%{http_code}\n' localhost:8080/nothing-here/
```

The `boot` events say what blastit decided before the first request. There are
three, one per phase of the startup:

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped","preset":null}
{"event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":null}
{"event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":148213,"immortalize_ms":412.7,"mem_total":1057259520,"mem_used":211812352,"max_view_processes":23}
{"event":"ready","component":"init","stage":"serving","routes":4,"workers":16,"outputs":{"db":["DATABASE_URL"]}}
```

and, when the manifest has something worth saying, a warning of its own — this is
the real text for the `admin` route above:

```json
{"event":"manifest.warning","component":"init","message":"route \"admin\" may run any statement on proxy \"db\" (unrestricted = true)"}
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

Set it once instead of on every command line:

```ini title="pytest.ini"
[pytest]
DJANGO_SETTINGS_MODULE = myproject.settings
python_files = tests.py test_*.py
blastit_manifest = blastit.toml
```

`DJANGO_SETTINGS_MODULE` is what pytest-django needs in any case;
`python_files` matters because Django's own convention is a `tests.py` per app,
which pytest does not collect by default.

It records every request Django handles — `django.test.Client`, `live_server` in
its own thread, a view called through `BaseHandler.get_response` — with the SQL
each one sent **as it goes on the wire**, and diffs that against the manifest
with blastit's own routing and SQL matching. It needs
[pytest-django](https://pytest-django.readthedocs.io/) and psycopg 3 with
server-side binding. `--blastit-strict` fails the session on anything the report
finds, which is what keeps a manifest from rotting in CI.

!!! warning "`--blastit-strict` only fails on what the suite reached"

    A session that collected no routed request reports clean — there is nothing
    to be missing. Check `routes_exercised` in the report before you take a green
    run as proof:
    [the plugin reference](../reference/pytest-plugin.md#the-report) explains the
    number and what it does not cover.

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

<!-- E2b -->
`blastit_live_server` works with the layout `django-admin startproject` writes,
too: it reads Django's own dotted `WSGI_APPLICATION` (`myshop.wsgi.application`),
it puts the project root on the server's import path so a packaged settings
module resolves, and it finds the manifest whether the path you gave it was
relative or absolute. Nothing to configure.

Read the report, add the grants, and repeat until the plugin says the manifest
covers every request that was made.

## Next

Once it is installed, the loop is
[adding a view, a new query, and reading a denial](day-to-day.md). Where it runs
is [fit it to your infra](../infra/index.md).
