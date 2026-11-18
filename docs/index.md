---
description: "A zero-trust application harness: one sandbox per route, no database credentials in the app, only the SQL each route needs."
hide:
  - toc
---

![A Eurasian nuthatch in flat poster colours](brand/mascot.svg){ align=right width=200 }

# Small bird. Small blast radius.

**blastit serves your Python WSGI app, PHP front controller or static site unchanged and splits it into one sandbox per
route.** Each route gets its own process, its own uid and its own database
allowlist, so a bug in the least important page is worth that page, not the app.

[Get started](getting-started.md){ .md-button .md-button--primary }
[How it works](how-it-works.md){ .md-button }

## One file, no code changes

```text title="Blastfile"
app "myshop"

entrypoint python "myshop.wsgi:application"

postgres $DATABASE_URL

view catalog GET "/products/*" {
    postgres {
        select products(id, name, price)
    }
}
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
* one manifest, `Blastfile`.

## Is it for you?

Yes for Python WSGI (Django, Flask, SQLAlchemy, psycopg 3), embedded PHP 8.3–8.5
(Symfony, Laravel, PSR-15), static sites and SPAs, PostgreSQL and Linux (plus
`blastit-lsp` on Linux, macOS and Windows).
No for HTTP/2 or WebSockets.

!!! note "Pre-release"

    0.2.0 is a pre-1.0 release; surfaces may still evolve before 1.0.
