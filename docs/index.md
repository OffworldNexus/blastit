---
description: "A Linux zero-trust application server for Python WSGI, embedded PHP and static sites: one sandbox per route, no database credentials in the app."
hide:
  - toc
---

![A Eurasian nuthatch in flat poster colours](brand/mascot.svg){ align=right width=200 }

# Small bird. Small blast radius.

**Blastit is a Linux application server that runs behind your ingress.** Traefik,
DigitalOcean App Platform, Cloudflare or nginx terminate TLS and HTTP/2; blastit
isolates what they forward — one `view` per route, with its own process, uid and
SQL allowlist — so a bug in the least important page is worth that page.

[Get started](getting-started.md){ .md-button .md-button--primary }
[How it works](how-it-works.md){ .md-button }

```console
$ uv init --app && uv add blastit     # Python; PHP and static use the archive
$ uv run blastit dev                  # your machine, no root
$ uv run blastit check                # CI: proves nothing was forgotten
```

!!! note "Linux, and one file for every interface"

    `entrypoint python "module:attr"` starts a WSGI callable; `entrypoint php
    "public/index.php"` runs a PSR-7/PSR-15 front controller on the host's embedded
    NTS `libphp` (8.3, 8.4, 8.5); no `entrypoint` plus a `static` mount serves a
    built front end. `blastit-lsp` runs natively on Linux, macOS and Windows; on a
    macOS or Windows laptop, run the server in Docker/Compose or a Linux VM.

## One file, no code changes

=== "Python (WSGI)"
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
    $ uv run blastit check                 # CI: nothing forgotten
    $ uv run blastit --preset digitalocean-paas   # production: real uids
    ```

=== "PHP"
    ```text title="Blastfile"
    app "myshop"

    entrypoint php "public/index.php"

    postgres $DATABASE_URL

    view api POST "/api/*" {
        body json
        postgres {
            insert orders(customer_id, total)
        }
    }

    view app GET "/*" {
    }
    ```

    ```console
    $ blastit dev                          # archive + host libphp
    $ blastit --preset digitalocean-paas
    ```

=== "Static & SPA"
    ```text title="Blastfile"
    static "/" {
        root "public"
        fallback "index.html"
        immutable "/assets/*"
    }
    ```

    ```console
    $ blastit dev                          # the release archive: no language runtime
    $ blastit check                        # audits every root
    ```

Views, grants and the entry point live in one file; your framework keeps its own
routing, and views are tried in the order written. Every served file carries an
`ETag`, so a deploy invalidates cached copies. [Getting started](getting-started.md)
is the small one.

## The trust boundary

```mermaid
flowchart LR
    client(["client"]) --> edge["cloud ingress · TLS, HTTP/2"]
    edge --> ingress["ingress · validates"] --> sched["scheduler · fair queue"] --> view["view · one uid per view"]
    view -- "socket" --> dbproxy["dbproxy · the only credentials"]
    dbproxy --> db[("PostgreSQL")]
```

No view holds a database password, and no view opens a TCP socket unless its own
block says so. [Security model](security.md) has the argument.

## What you get

| | |
|---|---|
| **One sandbox per view** | own process and uid; a misrouted request lands in an allowlist that does not fit |
| **No credentials in the app** | the real URL stays in a proxy; your code gets a local one |
| **Only the SQL it needs** | statements and table/column grants, checked on the wire |
| **One command to prove it** | `blastit check` fails on an unrouted request, a missing or dead grant, an unexercised view, or an undeclared body |

??? note "Platforms"

    Linux x86_64 and aarch64, glibc and musl: wheels for CPython 3.13 and 3.14, and
    standalone archives with the PHP SAPI shim beside the binary. Static sites need no
    language runtime.

## Where next

* [Getting started](getting-started.md) · [The Blastfile](blastfile.md)
* [Local](local.md) · [CI/CD](ci.md) · [Deployments](deploy/index.md) · [Security](security.md)
