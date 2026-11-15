# Django

A Django project is a WSGI callable: blastit needs one manifest and a few settings,
and no plugin. Not Django? [Flask and any WSGI app](flask.md).

## 1. Executor { #manifest }

```toml title="blastit.toml"
version = 1
app = "myshop.wsgi:application"
preload = ["myshop.urls"]

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/products/.*']
[route.db]
queries = []

[[route]]
name = "checkout"
path = ['/checkout/']
[route.db]
transaction_control = true      # the BEGIN and savepoints a write route sends
[route.ingress]
body = "json"

[[route]]
name = "fallback"
path = ['.*']
```

`app` is `module:attribute` and `preload` imports the URLconf once before the fork;
Django still routes — blastit picks the sandbox by `PATH_INFO` and passes the request
through. The last route takes what nothing else claimed, so Django renders its 404s.
`uv add blastit 'psycopg[binary]' dj-database-url` supplies the rest, then
`uv run blastit dev` serves the app; a savepoint's name is generated per request, so only
`transaction_control` can allow it.

## 2. Static assets

```toml
[[route]]
name = "static"
path = ['/static/.*']

[route.static]
root = "staticfiles"
immutable = ["/static/*", "/static/*/*", "/static/*/*/*"]
```

Add it above the catch-all, and set `STATIC_ROOT = BASE_DIR / "staticfiles"` beside
`STATIC_URL = "/static/"` so `collectstatic` writes there — the root must exist before
`blastit dev` starts. List `immutable` only for fingerprinted output, one pattern per
depth: `collectstatic` nests twice under `admin/`.

## 3. Database

```python title="myshop/settings.py"
import dj_database_url

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
```

`conn_max_age=0` closes the connection with the request and `server_side_binding` sends
parameters apart from the statement text. The app's `DATABASE_URL` may name a pool;
`manage.py migrate` needs the direct URL. Sessions are read on every authenticated
request: define `[group.session.db]` once with those statements, then attach it with
`groups = ["session"]` where you authenticate ([groups](../reference/manifest.md#groups)).

Django's own admin cannot be enumerated as `queries`: give it a route of its own with
`[route.db.tables]` for its tables, and `unrestricted = true` only if you cannot list
them — that route may then run every statement, and every boot warns about it.

## 4. Ingress

`json` for an API route, `form` for a POST, `raw` with `allow_raw = true` for an upload
— [the manifest](../reference/manifest.md#ingress). Behind TLS set
`SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")`, list the origin in
`CSRF_TRUSTED_ORIGINS`, and name the host in `ALLOWED_HOSTS`.

## 5. Environment and secrets

`dj_database_url` reads the local `DATABASE_URL` blastit rendered, and `SECRET_KEY`
comes from the same environment. Never copy the database URL into a log, a template or
a child process: it is the app's only way to the database.

## 6. Health

Do not add `/_blastit/health` to `urls.py`: the scheduler answers it before routing, so
a view for it never runs.

## 7. Migrations

Run `uv run python manage.py migrate` outside blastit, before the new code boots.

## 8. Learn and check

```console
$ uv run blastit check
```

`pytest-django` plus a `manage.py` (or `DJANGO_SETTINGS_MODULE`) is how `check` finds your
suite, and why it can create `test_<database>` instead of touching your development
database ([CI/CD](../ci.md#your-suite)). A statement `check` reports as not granted is
closed by pasting it into that route's `queries` — bind any value it inlined first.

## 9. Deployment notes

Keep `preload` short: it is imported once and shared copy-on-write, so each view process
starts with the URLconf already in memory.
