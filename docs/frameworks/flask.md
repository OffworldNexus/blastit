# Flask and other WSGI apps

Everything on this page applies to any WSGI framework: Flask, Pyramid, Bottle,
Falcon, or a hand-written callable. The *syntax* of the examples is Flask's; the
lifecycle is identical.

The install, the shape of the manifest and the day-to-day loop are the same as
for [Django](django.md). What differs is how you fill the grants in, because
[the pytest plugin does not apply](#no-pytest-plugin-for-flask).

## Install and first manifest

```console
$ uv init --python 3.13 shop && cd shop
$ uv add flask 'psycopg[binary]'
$ uv add --prerelease allow blastit
```

If `uv add` cannot find blastit, add the package index first, as in
[Getting started](../getting-started.md#1-install-it).

```python title="app.py"
import os

import psycopg
from flask import Flask, g

app = Flask(__name__)


def connection():
    if "db" not in g:
        g.db = psycopg.connect(os.environ["DATABASE_URL"], autocommit=True)
    return g.db


@app.teardown_appcontext
def close_connection(_exception=None):
    db = g.pop("db", None)
    if db is not None:
        db.close()


@app.get("/products/")
def products():
    with connection().cursor() as cursor:
        cursor.execute(
            'SELECT "shop_product"."id", "shop_product"."name"'
            ' FROM "shop_product" ORDER BY "shop_product"."name" ASC'
        )
        rows = cursor.fetchall()
    return {"products": [{"id": row[0], "name": row[1]} for row in rows]}
```

The manifest is the same shape as Django's: a proxy, a route per permission
set, and a catch-all.

```toml title="blastit.toml"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "products"
path = ['/products/']
methods = ["GET", "HEAD"]
[route.db]
queries = [
  'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" ORDER BY "shop_product"."name" ASC',
]

[[route]]
name = "fallback"          # everything else: no database at all
path = ['.*']
```

Note the shape of that `queries` entry: it is the statement *as it goes on the
wire* after normalisation — trimmed, no trailing `;`, whitespace collapsed, and
every bind placeholder written `$?`. The two Python string literals above are
concatenated by Python into one line, so the SQL text on the wire is one line
too. That matters: a statement split across lines in *the Python source* is
fine, because the text is concatenated before it is sent, but a statement with
a newline in it is not the same text as the same statement with a space.

## Run it locally

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ export BLASTIT_APP=app:app
$ export BLASTIT_MANIFEST=$PWD/blastit.toml
$ export PORT=8080
$ sudo -E .venv/bin/blastit
```

There is no `BLASTIT_PRELOAD` here because a small Flask app has nothing
expensive to load on first request. Once it does — a serialiser library, an
ORM's model registry, a big template — list it, so that every route shares one
copy instead of loading it again in its own process:

```console
$ export BLASTIT_PRELOAD=app,shop.serializers
```

## Database clients

Two things break exact matching, and both are about the client rather than the
framework.

### Use a client that binds parameters server-side

psycopg 3 (the `psycopg` package) always sends parameters separately, so
`cursor.execute("… WHERE id = %s", (42,))` puts `$1` on the wire and the SQL
text is the same for every row. psycopg2 inlines the literal instead, and no
grant could ever match it.

* **psycopg 3** — fine as it is.
* **SQLAlchemy** — use `postgresql+psycopg://`, not `postgresql+psycopg2://`.
* **psycopg2** — not usable.

The pytest plugin can tell you which one you have; for a non-Django app, check
it yourself: run the query with `BLASTIT_LOG_FORMAT=pretty`, or turn on
`PostgreSQL`'s `log_statement = 'all'` and look at what arrived.

### Close the connection with the request

Under blastit every request opens its own connection, and its thread is thrown
away afterwards. A connection that outlives its request is orphaned.

=== "psycopg directly"

    Close it in a teardown, as `app.py` above does:

    ```python
    @app.teardown_appcontext
    def close_connection(_exception=None):
        db = g.pop("db", None)
        if db is not None:
            db.close()
    ```

=== "SQLAlchemy"

    One engine, no pool:

    ```python
    from sqlalchemy import create_engine
    from sqlalchemy.pool import NullPool

    engine = create_engine(os.environ["DATABASE_URL"], poolclass=NullPool)
    ```

=== "Flask-SQLAlchemy"

    Same idea, through the extension's own setting:

    ```python
    SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}
    ```

If you want pooling, put it in front of the real database — PgBouncer, or
DigitalOcean's connection pools. blastit's own proxy keeps no pool either: one
inbound connection is one upstream connection.

## No pytest plugin for Flask

The plugin is **Django-only**, and it is worth being precise about why rather
than discovering it from an empty report.

It defines "a request" as `django.core.handlers.base.BaseHandler.get_response`.
Everything else — which route the path belongs to, which test made the request,
which statements belong to it — hangs off that. It also knows Django's database
aliases, Django's `ATOMIC_REQUESTS`, and pytest-django's `live_server`.

A Flask app has no `BaseHandler`. The plugin's psycopg-level capture still sees
your statements, but with no request to attach them to they are treated as
fixture or setup traffic and dropped, so `pytest --blastit-manifest
blastit.toml` reports nothing useful about a Flask project.

### Filling the manifest without the plugin

Three ways, in the order most projects use them.

**1. Write the routes and use table grants.** If you would rather not enumerate
statements at all, say which tables and columns each route may touch, and let
PostgreSQL's own parser check each statement:

```toml
[[route]]
name = "products"
path = ['/products/']
[route.db.tables]
shop_product = { select = ["id", "name"] }

[[route]]
name = "checkout"
path = ['/checkout/']
[route.db.tables]
orders_order = { select = "*", insert = "*", update = ["status", "accepted_at"] }
orders_line = { select = "*", insert = "*" }
[route.db]
functions = ["unaccent"]
```

That is usually the fastest honest start, and it is strictly stronger than
`unrestricted = true`. See
[the manifest reference](../reference/manifest.md#grants) for what a table
grant does and does not check.

**2. Run it and read the denials.** Start blastit with
`BLASTIT_LOG_FORMAT=pretty`, click through the app, and read the `sql.denied`
lines: each one is a statement a route ran and was not granted yet. Copy the
ones that belong there. This is the same loop as Django's, only without the
test-suite shortcut.

**3. Fold a log.** Once you have a log — from a development run, a staging run,
or production — `blastit-manifest fold` turns every denial into the grant the
manifest is missing, with its route and proxy:

```console
$ blastit-manifest fold app.log --manifest blastit.toml
$ blastit-manifest fold app.log --manifest blastit.toml --write
$ blastit-manifest fold app.log --manifest blastit.toml --prefer-tables
```

`--prefer-tables` writes the smallest table grant covering the denials instead
of one exact statement per denial, which is what a Flask project usually wants.
It is all in
[Day-to-day life](day-to-day.md#fold-the-log).

**4. Bring your own capture.** If you want that report for a non-Django app,
the piece to write is small: record, per request, the `PATH_INFO`, the HTTP
method and every statement psycopg sends, then diff that against the manifest
with blastit's own matching. Nothing in blastit assumes Django; only the plugin
does.

!!! note "Why the asymmetry"

    The plugin was written for the pilot project, which is Django, and the
    roadmap's D3b/D3c steps say so. A framework-agnostic recorder is not built
    yet.

## Next

[Day-to-day life](day-to-day.md) — adding a view, a denial, folding a log,
upgrading — and [fit it to your infra](../infra/index.md).
