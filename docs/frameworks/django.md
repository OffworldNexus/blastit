# Django

Two settings, one manifest, and a test suite that fills the grants in. Nothing
here changes your code.

Not on Django? [Flask and other WSGI apps](flask.md).

## Two settings

```python title="myproject/settings.py"
import dj_database_url

DATABASES = {"default": dj_database_url.config(conn_max_age=0)}
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
```

`conn_max_age=0` closes the connection with the request, and
`server_side_binding` sends parameters separately from the SQL text, so a
statement's text is the same every time. Both are what blastit needs from any
app, not Django features.

## Install

```console
$ uv init && uv add blastit 'psycopg[binary]' dj-database-url
```

Run `uv` as yourself, and keep the interpreter it installs where the route uids
can read it — [Getting started, step
1](../getting-started.md#1-a-throwaway-directory-the-route-uids-can-read).

`dj-database-url` just parses `DATABASE_URL` into `DATABASES`, and it is what
the [end-to-end demo](https://github.com/OffworldNexus/blastit/tree/develop/e2e/demo)
uses.

## First manifest

Write `blastit.toml` next to `manage.py`, with the routes and empty grants: the
test suite fills the grants in.

```toml title="blastit.toml"
version = 1
groups = ["session"]

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }     # the real URL stays here, never in the app

[group.session.db]
transaction_control = true
queries = []

[[route]]
name = "catalog"
path = ['/products/', '/products/[0-9]+/']
methods = ["GET", "HEAD"]
[route.db]
queries = []

[[route]]
name = "admin"                       # too much SQL to enumerate
path = ['/admin/.*']
[route.db]
unrestricted = true

[[route]]
name = "fallback"
path = ['.*']                        # everything else: no database at all
```

Four Django-specific points:

* `input.url = "DATABASE_URL"` is the variable `dj_database_url` reads; blastit
  hands the app a `DATABASE_URL` of its own, so `settings.py` needs no branch.
* `groups = ["session"]` — session and user lookups happen on every
  authenticated request, so they belong in a group attached to every route.
* `transaction_control = true` — Django opens a transaction around writes, and
  psycopg sends the `BEGIN`/`COMMIT` itself. A savepoint's name is generated per
  request, so it cannot be granted, only allowed.
* `unrestricted = true` on `/admin/.*` keeps "too much SQL to enumerate" out of
  every other route. It is flagged at boot.

## Run it locally

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

`.venv/bin/blastit` re-executes *your* interpreter, so Django is importable in
every view process; a bare `sudo blastit` would find root's interpreter instead.
`BLASTIT_PRELOAD` imports the URLconf once, before the fork, so every route
shares one copy.

The `boot` events say what blastit decided:

```json
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root"}
{"event":"ready","component":"init","stage":"serving","routes":4,"workers":16}
```

## Fill in the manifest

### With the pytest plugin

```console
$ uv add --dev pytest-django
$ uv run pytest --blastit-manifest blastit.toml
blastit: 412 requests, 977 statements, 38/41 routes exercised
blastit: route catalog: 3 statement(s) missing
blastit: route checkout: needs transaction_control = true
blastit: route search: 2 grant(s) unused
```

Set it once instead of on every command line:

```ini title="pytest.ini"
[pytest]
DJANGO_SETTINGS_MODULE = myproject.settings
python_files = tests.py test_*.py
blastit_manifest = blastit.toml
```

The plugin records every request Django handles and the SQL it sent, then diffs
that against the manifest with blastit's own matching. It needs pytest-django
and psycopg 3 with server-side binding. `--blastit-strict` fails the session on
anything the report finds: that is the CI gate.

!!! warning "`--blastit-strict` only fails on what the suite reached"

    A session that collected no routed request reports clean. Check
    `routes_exercised` in the report before you trust a green run.

### The live server

A suite that drives a browser or a real HTTP client makes its traffic in another
process. `blastit_live_server` runs the real server with your manifest and your
test database:

```python
from urllib.request import urlopen


def test_notes_page(blastit_live_server):
    with urlopen(f"{blastit_live_server.url}/notes/") as response:
        assert response.status == 200
```

`--blastit-live-server` makes pytest-django's `live_server` be blastit's, tests
untouched. It needs root or passwordless sudo, and skips with the reason
otherwise. On teardown its log is folded into the report, so a grant the front
end needed shows up like any other missing statement.

Read the report, add the grants, repeat until nothing is missing.

## Next

[Day-to-day life](day-to-day.md) and [fit it to your infra](../infra/index.md).
