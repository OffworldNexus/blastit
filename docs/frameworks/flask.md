# Flask, SQLAlchemy and raw WSGI

One callable, one manifest. The code below is Flask's; a bare WSGI app is the same file
without the decorator, and SQLAlchemy changes two lines.

## 1. Executor

```toml title="blastit.toml"
version = 1
app = "app:app"

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "products"
path = ['/products/']
[route.db]
queries = []
[route.ingress]                 # 4. ingress: json, form, none, raw
body = "form"                   # the search form posts URL-encoded

[[route]]
name = "fallback"       # everything else: no database at all
path = ['.*']
```

```console
$ uv add flask 'psycopg[binary]' blastit
$ uv add --dev pytest
```

`app:app` must be importable from the directory you start in, on your project's
interpreter; add `uv add sqlalchemy` if you use it, and `uv run blastit dev` serves it.

## 2. Static assets

A WSGI app has no static story of its own: `[route.static]` serves the files itself and
forwards everything else to the app — a route above the catch-all, or a page of its own
([static assets and SPAs](static-and-spa.md)).

## 3. Database

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
def close(_error=None):
    if (db := g.pop("db", None)) is not None:
        db.close()
```

One connection per request, closed in the teardown: a connection that outlives the
request is orphaned when its thread ends. psycopg 3 binds parameters server-side
(`%s` becomes `$1`), so a statement's text is the same every time.

With SQLAlchemy, two lines and one key:

```python
url = os.environ["DATABASE_URL"].replace("postgresql://", "postgresql+psycopg://", 1)
engine = create_engine(url, poolclass=NullPool)
```

The `postgresql+psycopg://` driver keeps parameters out of the text, `NullPool` gives
each request its own connection, and SQLAlchemy's own `BEGIN` needs
`transaction_control = true`. Its first connection runs `select pg_catalog.version()`
before any request statement: put it in `[group.connection.db]` and attach
`groups = ["connection"]` wherever the app connects ([groups](../reference/manifest.md#groups)).
Flask-SQLAlchemy takes `SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}` and the same
URL replacement for `SQLALCHEMY_DATABASE_URI`.

## 4. Ingress

The `products` route declares `body = "form"`, so a URL-encoded POST reaches the view;
declare the codec each route that takes a body receives, and a body of the wrong shape is
a `400` before Flask sees it.

## 5. Environment and secrets

`os.environ["DATABASE_URL"]` is the local socket URL blastit rendered. Read every other
secret from the environment the same way, and never write the database URL into a
response, a log line or a subprocess.

## 6. Health

Do not add a view for `/_blastit/health`: the scheduler answers it before your routes are
consulted, so it never runs.

## 7. Migrations

Run Flask-Migrate's `uv run flask db upgrade` outside blastit, before the new code boots.

## 8. Learn and check

`check` runs your pytest suite, so keep `app.py` importable from the project root: the
suite imports the same callable the manifest names
([CI/CD](../ci.md#what-it-fails-on)).

## 9. Deployment notes

A catch-all route is one process for the whole app; give a hot or risky path its own
`[[route]]` above it to get its own process, uid and grants.
