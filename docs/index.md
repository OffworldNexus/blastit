---
description: "A zero-trust application harness: one sandbox per view, no database credentials in the app, only the SQL each view needs."
hide:
  - toc
---

![A Eurasian nuthatch in flat poster colours](brand/mascot.svg){ align=right width=200 }

# Small bird. Small blast radius.

**blastit serves your Python WSGI app, PHP front controller or static site unchanged and splits it into one sandbox per
view.** Each view gets its own process, its own uid and its own database
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

Views, grants and the app's entry point live in this one file, next to your
code. Your framework keeps its own routing. New here? [Getting
started](getting-started.md) builds a smaller app in three steps.

## The trust boundary

```mermaid
flowchart LR
    client(["client"]) --> edge["edge · TLS"]
    subgraph box["one blastit"]
        ingress["ingress · validates the body"]
        sched["scheduler · fair queue"]
        view["view · one uid per view"]
        dbproxy["dbproxy · the only credentials"]
    end
    db[("PostgreSQL")]

    edge --> ingress --> sched --> view
    view -- "local socket" --> dbproxy -- "TCP" --> db
```

No view holds a database password, and no view can open a TCP socket unless its
own block says `egress any`. [Security model](security.md) has the full argument.

## What you get

| | |
|---|---|
| **One sandbox per view** | its own process and uid; a misrouted request lands in an allowlist that does not fit |
| **No credentials in the app** | the real database URL stays in a proxy; your code gets a local one |
| **Only the SQL it needs** | statements and table/column grants, checked on the wire |
| **One command to prove it** | `blastit check` fails on an unrouted request, a missing or dead grant, an unexercised view, or a body with no declared validation |

!!! note "Same file, three runtimes"

    `entrypoint python "module:attr"` starts a WSGI callable, `entrypoint php
    "public/index.php"` runs a front controller on the host's embedded NTS
    `libphp`, and no `entrypoint` at all with a `static` mount serves built
    assets. Every runtime declares itself once; the views below it are identical.

## What it costs

* one process per live view, forked from a preloaded parent;
* root or a small capability set at boot — `blastit dev` needs neither;
* one manifest, `Blastfile`.

## Is it for you?

Yes for Python WSGI (Django, Flask, SQLAlchemy, psycopg 3), embedded PHP 8.3–8.5
(Symfony, Laravel, PSR-15), static sites and SPAs, PostgreSQL and Linux (plus
`blastit-lsp` on Linux, macOS and Windows).
No for HTTP/2, WebSockets or an upload codec: a body is `json`, `form`, `none`
or `raw`, and `raw` is the one the app parses itself.

!!! note "Pre-release"

    0.2.0 is a pre-1.0 release; surfaces may still evolve before 1.0. [Changelog](changelog.md)
    lists what each release carries.
