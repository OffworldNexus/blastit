# Flask, SQLAlchemy and raw WSGI

Everything here applies to any WSGI app; the syntax is Flask's.

## Install and app

```console
$ uv init --app shop && cd shop
$ uv add flask 'psycopg[binary]' blastit
$ uv add sqlalchemy            # only if you use it
$ uv add --dev pytest
```

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

## Manifest

```toml title="blastit.toml"
version = 1
app = "app:app"

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "products"
path = ['/products/']
methods = ["GET", "HEAD"]
[route.db]
queries = []

[[route]]
name = "fallback"       # everything else: no database at all
path = ['.*']
```

The empty `queries` list is the point: every statement is denied until a grant
allows it. `uv run blastit check --learn` fills it in from your tests. Without a
suite, `uv run blastit dev --learn` and click through the app.

A `queries` entry is the statement as it goes on the wire, normalised: trimmed,
whitespace collapsed, binds written `$?`.

## Run it

Any PostgreSQL works; [one line](../local.md#add-postgresql) starts one.

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ uv run blastit dev
```

`app:app` resolves from the directory you started in, on your project's
interpreter.

## SQLAlchemy

```python
from sqlalchemy import create_engine
from sqlalchemy.pool import NullPool

url = os.environ["DATABASE_URL"].replace("postgresql://", "postgresql+psycopg://", 1)
engine = create_engine(url, poolclass=NullPool)
```

* The `postgresql+psycopg://` driver binds parameters server-side. Blastit hands
  the app a plain `postgresql://` URL, hence the replacement.
* `NullPool` gives each request its own connection.
* SQLAlchemy sends `BEGIN` itself, so its routes need
  `transaction_control = true`.
* Its first connection runs `select pg_catalog.version()` before any request
  statement. `--learn` records it; a `[group.<name>]` is where it belongs.

Flask-SQLAlchemy takes the same two: `SQLALCHEMY_ENGINE_OPTIONS =
{"poolclass": NullPool}`, and the same `postgresql+psycopg://` replacement for
`SQLALCHEMY_DATABASE_URI`.

## Any other WSGI app

Serve a plain callable the same way: set `app = "wsgi:application"`, write the
routes, and let `blastit check` fill the grants. Nothing is imported from
Blastit.

## Next

[Local development](../local.md) · [CI/CD](../ci.md) ·
[anywhere it deploys](../deploy/index.md).
