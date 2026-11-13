# Django

Four settings, one manifest, one command to fill it in. Nothing here changes
your code. Not on Django? [Flask and other WSGI apps](flask.md).

## Install

```console
$ uv add blastit 'psycopg[binary]' dj-database-url
$ uv add --dev pytest-django
```

`dj-database-url` only turns `DATABASE_URL` into `DATABASES`. `check` also needs
`pytest` (`uv add --dev pytest`) and finds a Django suite by `pytest-django` plus
a `manage.py` or `DJANGO_SETTINGS_MODULE` ([the handshake](../ci.md#your-suite)).

## Settings

```python title="myproject/settings.py"
import os

import dj_database_url

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
ALLOWED_HOSTS = [os.environ.get("ALLOWED_HOST", "localhost")]
SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")
CSRF_TRUSTED_ORIGINS = ["https://" + h for h in ALLOWED_HOSTS if h != "localhost"]
```

`conn_max_age=0` closes the connection with the request, and
`server_side_binding` sends parameters separately from the SQL text, so a
statement's text is the same every time. Those two are what Blastit needs from
any app, not Django features. `ALLOWED_HOSTS` must name the host your ingress
forwards. The last two lines are for TLS: `SECURE_PROXY_SSL_HEADER` makes
`request.is_secure()` true behind the ingress, and `CSRF_TRUSTED_ORIGINS` admits
the `https://` origin on unsafe requests.

## Manifest

Write `blastit.toml` next to `manage.py`. Empty grants are fine: the next step
fills them in.

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
methods = ["GET", "HEAD"]
[route.db]
queries = []

[[route]]
name = "checkout"
path = ['/checkout/']
methods = ["POST"]
[route.db]
transaction_control = true
[route.ingress]
body = "json"

[[route]]
name = "fallback"
path = ['.*']
```

* `app` is the WSGI callable, `module:attribute`. `preload` imports the URLconf
  once, before the fork.
* `input.url = "DATABASE_URL"` names the variable `dj_database_url` reads.
  Blastit gives the app its own local `DATABASE_URL`, so `settings.py` needs no
  branch.
* `transaction_control = true` allows the `BEGIN`, `COMMIT` and savepoints a
  write route sends. A savepoint's name is generated per request, so it can be
  allowed but never listed as a query.

!!! warning "Last resort: an area you cannot enumerate"

    We cannot list statements for Django's own admin. Give it a route of its
    own, with `unrestricted = true` and `body = "form"`, and attach it to
    nothing else:

    ```toml
    [[route]]
    name = "admin"
    path = ['/admin/.*']
    [route.db]
    unrestricted = true
    [route.ingress]
    body = "form"
    ```

    The cost: that route may run **every** statement on the proxy, including
    the ones reading password hashes, and every boot prints a
    `manifest.warning` for it. Use it for admin alone, never for an app route
    you have not enumerated.

## Run it

Migrations run outside Blastit, with the real credentials and no manifest. Any
PostgreSQL works; [one line](../local.md#add-postgresql) starts one.

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ uv run python manage.py migrate
$ uv run blastit dev
```

## Fill in the manifest

```console
$ uv run blastit check --learn
learn: 0 routes, 120 queries added → /srv/shop/blastit.toml
$ uv run blastit check
blastit check: 412 requests, 977 statements, 3/3 routes, clean
```

`blastit check` runs your pytest suite through Blastit: `--learn` records what
the manifest lacks instead of failing, then writes exact `queries` to
`blastit.toml`. Review the diff, then run it again without `--learn` — that is
the gate CI keeps. Django creates `test_<database>` with the real credentials;
the suite then runs through the proxy, never your development database.

Session and user lookups happen on every authenticated request, password hash
included. Put them in a `[group.session.db]` and attach that group to the routes
that authenticate — `login`, `logout`, `notes` — with a route-level `groups`:

```toml
[[route]]
name = "notes"
path = ['/notes/.*']
groups = ["session"]
```

Never write a top-level `groups = ["session"]`: that attaches the hash grant to
every route, anonymous ones too. [The manifest
reference](../reference/manifest.md#groups) has the grant itself.

## Deploy

[Deployments](../deploy/index.md): a preset, the boot line to read, and the
health check.
