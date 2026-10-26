# Fit it to your framework

blastit is a WSGI server. That is the whole of its integration story, and it is
deliberately short.

## No framework integration

blastit loads **one standard WSGI callable** (`BLASTIT_APP`, written
`module:attribute`) and calls it. It knows nothing about your framework's
routing, your middleware, your settings module or your test runner. Which
process serves a request comes from blastit's own routing table in
`blastit.toml`, not from your URL configuration.

So there is nothing to install into the framework and nothing to import:

* no plugin, no middleware, no `settings.py` hook, no `manage.py` command;
* the app does its own routing exactly as it would behind any other WSGI
  server, and gets ordinary WSGI requests;
* the only thing blastit adds to the `environ` is `blastit.route` — the route
  name — for logging.

| Framework | `BLASTIT_APP` |
|---|---|
| Django | `myproject.wsgi:application` |
| Flask | `app:app` (or whatever your module calls it) |
| Pyramid, Bottle, Falcon, a hand-written callable | `module:attribute` |

## What blastit needs from an app

None of this is something blastit can check: it does not look inside your app,
and a violation shows up as a `sql.denied` event rather than as a startup
error.

### Send parameters separately from the SQL text

blastit matches statements by their text, so the text of a statement has to be
the same on every request. A client that inlines literals into the SQL —
psycopg2, or psycopg 3 in its default client-side binding mode — sends a
different string every time, and no allowlist could ever match it.

| Client | What to do |
|---|---|
| Django with psycopg 3 | `DATABASES["default"]["OPTIONS"]["server_side_binding"] = True` |
| Django with psycopg2 | not usable: it has no server-side binding. |
| SQLAlchemy with psycopg 3 | `postgresql+psycopg://`, which binds server-side by default |
| SQLAlchemy with psycopg2 | not usable, for the same reason |

### Don't keep connections across requests

Under blastit every request opens its own connection, and request threads are
thrown away afterwards, so a connection that outlives its request is orphaned.

* Django: `DATABASES["default"]["CONN_MAX_AGE"] = 0`
* SQLAlchemy: `poolclass=NullPool`
* Flask-SQLAlchemy: `SQLALCHEMY_ENGINE_OPTIONS = {"poolclass": NullPool}`

If you want pooling, put it in front of the real database — PgBouncer, or
DigitalOcean's connection pools.

### Load lazily-loaded code before the fork

Anything imported only after the fork is private to one process: it costs
memory per route instead of being shared, and the first request to each route
pays for it. List such modules in `BLASTIT_PRELOAD` (comma-separated), or touch
them in a `BLASTIT_WARMUP` callable, which blastit runs before the fork.

For Django that means at least the root URLconf, and otherwise whatever your
project loads on first request. For any framework: your view modules, your
serialisers, anything with an expensive first import.

## Routes and URL paths

A route's `path` is a list of regular expressions. Each one is matched against
the **whole** of `PATH_INFO` — the percent-decoded path the app will see, not
the raw request target and not the query string. The routes are tried in
manifest order and the first match wins, so ordering matters and a catch-all
belongs last.

```toml
[[route]]
name = "product-detail"
path = ['/products/[0-9]+/']        # `/products/42/` matches; `/products/42/reviews/` does not
methods = ["GET", "HEAD"]           # optional; without it, any method
```

* **Syntax**: Rust's [`regex`](https://docs.rs/regex/latest/regex/) crate, which
  guarantees linear-time matching, so a hostile URL cannot stall the scheduler.
  Anchors are unnecessary — the match is already anchored at both ends.
* **Methods**: `HEAD` is never implied by `GET`; list both if you want both.
  Without `methods`, the route answers any method.
* **Query strings** are not part of `PATH_INFO`, so a route cannot tell
  `/search/?q=a` from `/search/?q=b`. Route on the path, and let the app decide
  the rest.
* **`BLASTIT_SCRIPT_NAME`** strips a mount prefix before routing and passes it
  on as `SCRIPT_NAME`, the way WSGI says. Set it when the app is mounted under
  a path rather than at the root.
* **A catch-all** — `path = ['.*']` as the last route — lets the app render its
  own 404s. Without one, a path no route matches gets blastit's own `404` and
  no Python runs.

Your framework's URL patterns do not have to line up one-to-one with blastit's
routes: a route is a *sandbox* and a permission set, so the question is "which
URLs may run which statements", not "which view is this". Splitting a blueprint
across two routes because one of its pages reads a table the others don't is
normal, and a single route may cover a dozen URL patterns.

**Getting it wrong is safe, if annoying.** The sandbox comes from uid
privileges, not from code isolation: all app code is mapped into every view
process, so a misrouted request runs view A's code with view B's allowlist. If
the regex and the app's routing disagree, the request lands in a process whose
allowlist does not fit, and its queries are denied and logged. It fails closed;
it never gains a privilege.

## ASGI and FastAPI

!!! warning "WSGI only"

    blastit speaks WSGI and HTTP/1.1. It does not speak ASGI, and an ASGI app
    needs an adapter — [a2wsgi](https://github.com/abersheeran/a2wsgi), for
    instance. The Django path is what CI covers; the ASGI path is not in CI at
    all, so treat it as untested.

An adapter is not hard, but it has one trap. a2wsgi starts an event-loop
thread, and blastit forks its processes from a single-threaded parent, so the
adapter has to be created lazily — inside each process, on the first request —
not at import time:

```python title="myproject/wsgi.py"
import threading

from a2wsgi import ASGIMiddleware

from myproject.main import app

_lock = threading.Lock()
_wsgi = None


def application(environ, start_response):
    global _wsgi
    if _wsgi is None:
        with _lock:
            if _wsgi is None:
                _wsgi = ASGIMiddleware(app)
    return _wsgi(environ, start_response)
```

Then `BLASTIT_APP=myproject.wsgi:application`, as for Django. With SQLAlchemy,
use psycopg 3 and turn pooling off, as
[above](#dont-keep-connections-across-requests). And remember that the pytest
plugin does not apply: see
[Flask and other WSGI apps](flask.md#no-pytest-plugin-for-flask).

## Next

<div class="grid cards" markdown>

- :fontawesome-brands-python: **[Django](django.md)**

    Install, first manifest, run locally, and fill in the grants from your test
    suite.

- :material-flask: **[Flask and other WSGI apps](flask.md)**

    The same, and what to do where the Django tooling does not apply.

- :material-sync: **[Day-to-day life](day-to-day.md)**

    Adding a view, a denial, folding a production log, upgrading.

</div>
