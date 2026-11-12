---
description: "A zero-trust WSGI server: one sandbox per route, no database credentials in the app, only the SQL each route needs."
hide:
  - toc
---

![A Eurasian nuthatch in flat poster colours](brand/mascot.svg){ align=right width=200 }

# Small bird. Small blast radius.

**blastit serves your WSGI app unchanged and splits it into one sandbox per
route.** Each route gets its own process, its own uid and its own database
allowlist, so a bug in the least important page is worth that page, not the app.

[Get started](getting-started.md){ .md-button .md-button--primary }
[How it works](how-it-works.md){ .md-button }

## One file, no code changes

```toml title="blastit.toml"
version = 1
app = "myshop.wsgi:application"

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/products/.*']
methods = ["GET", "HEAD"]

[route.db.tables]
products = { select = ["id", "name", "price"] }
```

```console
$ uv run blastit dev                   # your laptop, no root
$ uv run blastit check                 # CI: proves nothing was forgotten
$ blastit --preset digitalocean-paas   # production: real uids, or it refuses
```

Routes, grants and the app's entry point live in this one file, next to your
code. Your framework keeps its own routing. New here? [Getting
started](getting-started.md) builds a smaller app in three steps.

## What you get

| | |
|---|---|
| **One sandbox per route** | its own process and uid; a misrouted request lands in an allowlist that does not fit |
| **No credentials in the app** | the real database URL stays in a proxy; your code gets a local one |
| **Only the SQL it needs** | statements and table/column grants, checked on the wire |
| **One command to prove it** | `blastit check` fails on an unrouted request, a missing or dead grant, an unexercised route, or a body with no declared validation |

## What it costs

* one process per live route, forked from a preloaded parent;
* root or a small capability set at boot — `blastit dev` needs neither;
* one manifest, `blastit.toml`.

## Is it for you?

Yes for WSGI (Django, Flask, SQLAlchemy, psycopg 3), PostgreSQL and Linux.
No for HTTP/2, WebSockets or non-CPython interpreters.

!!! note "Pre-release"

    0.1.1 is the first developer-experience release; the manifest may still
    change before 1.0.
