# Any WSGI app

Blastit loads one callable and calls it. `app` names it as
`<module>:<callable>`. There is no package to import, no `manage.py` command and
no framework integration: Django, Flask, SQLAlchemy, raw psycopg and a
hand-written WSGI app all look the same from here.

```toml title="Django"
app = "myshop.wsgi:application"
```

```toml title="Flask"
app = "app:app"
```

```toml title="anything else"
app = "wsgi:application"
```

Requests are routed by `blastit.toml`, not by your URLconf: `PATH_INFO` picks a
route, and each route is a process and a uid with its own grants. `preload`
imports modules once, before the fork, so every route shares one copy.

## What Blastit needs from the app

### Bind parameters server-side

Statements are matched by text, so the text must be identical on every request.
A client that inlines literals sends a different string every time.

| Client | What to do |
|---|---|
| psycopg 3, directly | nothing: parameters travel as `$1` |
| Django | `OPTIONS["server_side_binding"] = True` |
| SQLAlchemy | use `postgresql+psycopg://` |
| psycopg2 | not usable: it cannot bind server-side; install `psycopg[binary]` |

### Open the connection per request

Each request runs on a thread that is thrown away afterwards, so a connection
that outlives the request is orphaned.

| Client | What to do |
|---|---|
| psycopg 3, directly | close it in a teardown |
| Django | `CONN_MAX_AGE = 0` |
| SQLAlchemy | `poolclass=NullPool` |
| Flask-SQLAlchemy | `SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}` |

For pooling, put PgBouncer in front of the real database.

## Routing rules

* `path` is a list of [regexes](https://docs.rs/regex/latest/regex/) matched
  against the **whole** of `PATH_INFO`, in manifest order; the first match wins,
  so a catch-all goes last.
* `methods = ["GET", "HEAD"]` is optional. `HEAD` is not implied by `GET`.
* A query string is not part of `PATH_INFO`.
* A catch-all at the end lets your app render its own `404`s.

## Check it

`blastit check` is framework-agnostic: it runs your pytest suite through
Blastit's brokers and checks every request and statement against the manifest.

## Next

[Django](django.md) · [Flask, SQLAlchemy and raw WSGI](flask.md) ·
[local development](../local.md) · [CI/CD](../ci.md).
