# Flask and other WSGI apps

Everything here applies to any WSGI framework; the syntax is Flask's.

## Install and first manifest

```console
$ uv init --python 3.13 shop && cd shop
$ uv add flask 'psycopg[binary]' blastit
```

Run `uv` as yourself, and keep the interpreter it installs where the route uids
can read it — [Getting started, step
1](../getting-started.md#1-a-throwaway-directory-the-route-uids-can-read).

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
        cursor.execute('SELECT "shop_product"."name" FROM "shop_product"')
        return {"products": [row[0] for row in cursor.fetchall()]}
```

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
queries = ['SELECT "shop_product"."name" FROM "shop_product"']

[[route]]
name = "fallback"       # everything else: no database at all
path = ['.*']
```

A `queries` entry is the statement as it goes on the wire, normalised: trimmed,
whitespace collapsed, binds written `$?`.

## Run it locally

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ sudo env BLASTIT_APP=app:app BLASTIT_MANIFEST=$PWD/blastit.toml \
           PORT=8080 .venv/bin/blastit
```

`app:app` resolves from the directory you started in, on your own interpreter.

## Database clients

**Bind parameters server-side.** psycopg 3 always sends them separately, so
`cursor.execute("… WHERE id = %s", (42,))` puts `$1` on the wire and the SQL
text is the same for every row. psycopg2 inlines the literal, which no grant
could match. With SQLAlchemy, use `postgresql+psycopg://`.

**Close the connection with the request.** Its thread is thrown away afterwards,
so a connection that outlives the request is orphaned. A teardown, as in
`app.py`, is enough. With SQLAlchemy: one engine, no pool.

```python
from sqlalchemy import create_engine
from sqlalchemy.pool import NullPool

engine = create_engine(os.environ["DATABASE_URL"], poolclass=NullPool)
```

SQLAlchemy sends `BEGIN` itself, so those routes need
`transaction_control = true`, and its first connection runs
`select pg_catalog.version()` before any request statement — which belongs in a
group attached to every route:

```toml
groups = ["connection"]

[group.connection.db]
queries = ["select pg_catalog.version()"]
```

Flask-SQLAlchemy takes the same thing through its own setting:
`SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}`. For pooling, put
PgBouncer in front of the real database.

## No pytest plugin for Flask

The plugin is Django-only. It defines a request as Django's
`BaseHandler.get_response`, and it knows Django's database aliases and
`ATOMIC_REQUESTS`; asked for a manifest in a project without Django, it stops
with `ERROR: blastit: the plugin needs Django in the test environment to record
requests`.

Fill the manifest in by hand, in this order:

1. **Table grants.** Say which tables and columns each route may touch, and let
   PostgreSQL's own parser check every statement. It is usually the fastest
   honest start, and stronger than `unrestricted = true`:

    ```toml
    [[route]]
    name = "products"
    path = ['/products/']
    [route.db.tables]
    shop_product = { select = ["name"] }
    ```

2. **Read the denials.** Run it with `BLASTIT_LOG_FORMAT=pretty` and click
   through the app: each `sql.denied` line is a statement a route ran and was
   not granted yet.

3. **Fold a log.** `blastit-manifest fold app.log --manifest blastit.toml`
   prints the grants a log is missing. `--write` puts them in the file, and
   `--prefer-tables` writes the smallest table grant instead of one statement
   per denial.

## Next

[Day-to-day life](day-to-day.md) — adding a view, reading a denial, folding a
log — and [fit it to your infra](../infra/index.md).
