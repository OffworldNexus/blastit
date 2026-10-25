# blastit

**A WSGI server that keeps the blast radius of a security bug small.**

In a typical Python web app, every view runs in the same process, with the
same database credentials and the same rights. One exploitable bug in the
least important page — a template injection, a deserialisation flaw, a
vulnerable dependency — and the attacker holds everything the whole app
holds: every table, every secret in the environment.

blastit serves the same app, unchanged, but splits it up:

* **Each view runs in its own sandbox.** You group your URLs into *routes*;
  every route gets its own process under its own Unix user. A compromised
  route can't read another one's memory, signal it, or borrow its rights.
* **Views use private resources without knowing their secrets.** The real
  database URL never reaches your code. Your app sees an ordinary
  `DATABASE_URL`, which points at a local proxy run by blastit. Only that
  proxy holds the credentials.
* **Each route gets only the privileges it needs.** The proxy lets every route
  run only the SQL statements the manifest grants it. The product page can
  read products; it can't read password hashes, and it can't `DROP TABLE`. A
  route that doesn't need the network can't open an outbound connection.

```mermaid
flowchart LR
    c1(["GET /products/42/"]) --> catalog
    c2(["POST /login/"]) --> login
    subgraph sandboxes["one process and one Unix user per route"]
        catalog["route <b>catalog</b>"]
        login["route <b>login</b>"]
    end
    catalog -- "only the statements<br/>catalog is granted" --> proxy
    login -- "only the statements<br/>login is granted" --> proxy
    proxy["blastit's proxy<br/>holds the real URL"] -- "real credentials" --> pg[("PostgreSQL")]
```

Your framework doesn't know any of this happens. It gets ordinary WSGI
requests and routes them itself. All blastit adds is one file,
`blastit.toml`, which says which URLs belong to which route and what each
route may do.

> **Status: pre-release.** blastit runs a real Django app end to end on
> DigitalOcean App Platform in CI, but it hasn't had a stable release, and the
> manifest format may still change. Linux only.

## What it looks like

A route for the public catalogue, one for logging in, and a catch-all:

```toml
version = 1

# The app's DATABASE_URL is the real one at startup; blastit takes it, and
# hands the app a URL to its own proxy instead.
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/products/', '/products/[0-9]+/']
methods = ["GET", "HEAD"]
[route.db]
queries = [
  'SELECT "shop_product"."id", "shop_product"."name", "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21',
  'SELECT "shop_product"."id", "shop_product"."name", "shop_product"."price" FROM "shop_product" ORDER BY "shop_product"."name" ASC',
]

[[route]]
name = "login"
path = ['/login/']
[route.db]
transaction_control = true
queries = [
  'SELECT "auth_user"."id", "auth_user"."password", "auth_user"."username" FROM "auth_user" WHERE "auth_user"."username" = $1 LIMIT 21',
  'UPDATE "auth_user" SET "last_login" = $1 WHERE "auth_user"."id" = $2',
]

[[route]]
name = "fallback"     # everything else: no database at all
path = ['.*']
```

If the catalogue page is compromised, the attacker can list products. That's
it. Each refusal is logged as one JSON line that names the route and the
statement it was refused:

```json
{"ts":"2026-10-25T14:02:12.007Z","level":"warn","event":"sql.denied","component":"dbproxy","route":"catalog","proxy":"db","kind":"parse","sqlstate":"42501","sql":"SELECT \"auth_user\".\"password\" FROM \"auth_user\""}
```

## Quick start

### 1. Install

blastit isn't on PyPI yet. Wheels (Linux x86_64, CPython 3.13 and 3.14) are
built from every commit to `develop` and published on
[blastit's own package index](https://offworldnexus.github.io/blastit/simple/).
With uv:

```toml
# pyproject.toml
[[tool.uv.index]]
name = "blastit"
url = "https://offworldnexus.github.io/blastit/simple/"
explicit = true

[tool.uv.sources]
blastit = { index = "blastit" }
```

```console
$ uv add --prerelease allow blastit
$ uv run blastit                     # or .venv/bin/blastit, or python -m blastit
```

With pip:

```console
$ pip install --pre blastit --extra-index-url https://offworldnexus.github.io/blastit/simple/
```

Like granian, blastit is a compiled extension plus a `blastit` command written
in Python, so it runs on the interpreter of the environment it's installed in:
your virtualenv and its packages are simply there. To build a wheel yourself,
run `scripts/build-wheel.sh` (Rust 1.88+ and uv).

Configuration is still environment variables only (`BLASTIT_APP`,
`BLASTIT_MANIFEST`, `PORT`, …), and it still wants to be PID 1 in the
container. Two things to know before you start it:

* it has to run as **root**: init binds the port, then drops every other
  process to its own unprivileged user (see the Dockerfile below);
* the project directory — and therefore the `.venv` with blastit and the app's
  dependencies in it — has to stay **readable by the sandbox users**. Routes run
  as `BLASTIT_UID_BASE + 1000 + i`, and blastit drops to `UID_BASE + 3` before
  it `execve`s its own database proxy, so a project inside a `0700` directory
  hides both from those users. `/app`, `/srv/app` or an ordinary checkout are
  fine.

### 2. Django

Two settings make Django's SQL checkable:

```python
# settings.py
DATABASES["default"]["CONN_MAX_AGE"] = 0  # request threads don't outlive their request
DATABASES["default"].setdefault("OPTIONS", {})["server_side_binding"] = True
# Parameters travel separately from the SQL text, so a statement's text is
# the same on every request and can be allowed exactly.
```

Write `blastit.toml` next to `manage.py` (see above), then run blastit as root
in the container. It binds the port and then drops every process to its own
unprivileged user:

```dockerfile
FROM python:3.13-slim
WORKDIR /app
COPY . .
RUN pip install -r requirements.txt

RUN pip install --pre blastit --extra-index-url https://offworldnexus.github.io/blastit/simple/

ENV BLASTIT_APP=myproject.wsgi:application \
    BLASTIT_MANIFEST=/app/blastit.toml \
    BLASTIT_PRELOAD=myproject.urls \
    PORT=8000
CMD ["blastit"]
```

`BLASTIT_PRELOAD` imports your URLconf before the processes are forked, so
that every route shares the same loaded code instead of loading it again on
its first request.

### 3. FastAPI

blastit speaks WSGI. FastAPI is ASGI, so run it through
[a2wsgi](https://github.com/abersheeran/a2wsgi). a2wsgi starts an event-loop
thread, and blastit forks its processes from a single-threaded parent, so
create the adapter lazily, inside each process:

```python
# myproject/wsgi.py
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

With SQLAlchemy, use psycopg 3 (`postgresql+psycopg://`), which sends
parameters separately by default, and don't pool connections across requests
(`poolclass=NullPool`). Then set `BLASTIT_APP=myproject.wsgi:application`, as
for Django. (CI covers the Django path; this one isn't in CI yet.)

### 4. Fill in the grants

You don't write the SQL by hand. Run the app under blastit, use it, and read
the `sql.denied` lines: each one is a statement a route ran and wasn't granted
yet. If it belongs there, copy it into the route. If it doesn't, you just
found a route that does more than you thought.

## What you get, and what you don't

| If this is compromised… | …the attacker gets | …and cannot |
|---|---|---|
| one route | that route's granted statements, its scratch directory, the network only if the route has `egress = true` | run any other statement, read the database credentials, touch other routes' processes |
| the HTTP front end | the raw bytes of requests in flight | reach the database, run Python |

blastit doesn't protect against code that runs when your app is *imported*:
that happens before the sandboxes exist. And it only sandboxes what goes
through its proxies; other secrets in the environment (an API key for a
third-party service, say) are still visible to every route.

## Configuration

Everything is configured through environment variables: `BLASTIT_APP`,
`BLASTIT_MANIFEST`, `PORT`, worker budgets, timeouts, log format
(`BLASTIT_LOG_FORMAT=pretty` for humans). The full list is in
[`ARCHITECTURE.md` §8.2](ARCHITECTURE.md#82-configuration-reference).

## Learn more

* [`ARCHITECTURE.md`](ARCHITECTURE.md): the design, the security model, and
  what it relies on.
* [`ROADMAP.md`](ROADMAP.md): what's built and what's next.
* [`e2e/`](e2e/): the Django demo that CI runs under blastit, in Docker and on
  App Platform, to prove all of the above.
* [`docs/spikes/`](docs/spikes/): what was measured on the target platform.

## Development

```console
$ cargo fmt --all -- --check
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo test --workspace
```

`develop` is the only long-lived branch. Each roadmap step lands as one pull
request from a `feature/<step>-<slug>` branch; CI must be green before merging.
