# Flask, SQLAlchemy and raw WSGI

Flask is a decorator around a WSGI callable, a bare WSGI app drops it, SQLAlchemy changes two
lines, and the [connector](index.md) has the runtime mechanics.

```mermaid
flowchart LR
    client(["client"]) --> ingress["ingress"]
    ingress --> sched["scheduler"]
    sched --> products["view products<br/>body form · use connection"]
    sched --> fallback["view fallback<br/>no store"]
    products -- "socket" --> dbproxy["dbproxy"]
    dbproxy --> db[("PostgreSQL")]
```

## Install

```console
$ uv add flask 'psycopg[binary]' blastit
$ uv add --dev pytest
```

## The Blastfile

```text title="Blastfile"
app "shop"

entrypoint python "app:app"

postgres $DATABASE_URL

trait connection {
    postgres {
        transactions
        query 'select pg_catalog.version()'
        query 'select current_schema()'
        query 'show standard_conforming_strings'
        query 'show transaction isolation level'
    }
}

view products POST "/products/" {
    body form
    use connection
    postgres { insert products(name, price) }
}

view fallback "/*" {}
```

`app:app` is the module attribute `entrypoint python` names: `app.py` in the project root exposes
`app`. The `connection` trait carries the transaction control and the setup statements every view
that opens the engine needs; `view products` adds the trait and the one `insert` its handler runs,
and the catch-all shows "no database here" — it is not the shape for the app's other routes.

## The callable you name

`entrypoint python` names a module attribute, so an app built by `create_app()` needs a module-level
`app = create_app()` (a two-line `wsgi.py`) for the file to name.

## One connection per request

A connection closed in `teardown_appcontext` cannot outlive the request that opened it.

## SQLAlchemy

`postgresql+psycopg://` selects psycopg 3, `NullPool` gives each request its own connection, and
the lowercase `select pg_catalog.version()` probe and the `transactions` that `engine.begin()` needs
belong in the one `connection` trait every view `use`s. `engine.begin()` sends `BEGIN`, the
savepoint pair psycopg wraps it in, and `COMMIT`, which is what `transactions` allows.

```python
url = os.environ["DATABASE_URL"].replace("postgresql://", "postgresql+psycopg://", 1)
engine = create_engine(url, poolclass=NullPool)
```

The reference has [the connection-setup trait](../../reference/blastfile.md#connection-setup).

## Proxy headers

Nothing to configure: Blastit sets `REMOTE_ADDR` and `wsgi.url_scheme` from a trusted peer, so the
app must not re-read `X-Forwarded-*` — `ProxyFix` would let a client choose both.

## Tests

`uv run blastit check` runs `pytest` from the project root, so the suite must import the same
callable the file names.

## Migrations

`uv run flask db upgrade` applies what Flask-Migrate wrote into `migrations/`.

## Where next

[Django](django.md), and [static files](../static-and-spa.md) for a built front end.
