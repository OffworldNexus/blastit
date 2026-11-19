<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/brand/header-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/brand/header-light.svg">
  <img src=".github/brand/header-light.svg" alt="blastit — Small bird. Small blast radius." width="960">
</picture>

<p align="center">
  <a href="https://offworldnexus.github.io/blastit/">Documentation</a> · <a href="ARCHITECTURE.md">Architecture</a> · <a href="LICENSE">Apache 2.0</a>
</p>

**Blastit is a Linux application server that sits behind your cloud ingress.**
Traefik, DigitalOcean App Platform, Cloudflare or nginx terminate TLS and HTTP/2;
blastit runs what they forward and splits it into one sandbox per route: each
`view` gets its own process, its own Unix user and its own SQL allowlist, so a bug
in the least important page is worth that page, not the app.

Linux. Python (WSGI), PHP (front controllers on the host's NTS `libphp` 8.3, 8.4,
8.5) or static files. `blastit-lsp` runs natively on Linux, macOS and Windows; on a
macOS or Windows laptop, run `blastit` in Docker/Compose or a Linux VM.

```console
$ uv init --app && uv add blastit     # Python (WSGI)
$ uv run blastit dev                  # your machine, no root
$ uv run blastit check                # CI: proves nothing was forgotten

$ curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz
$ tar -xzf blastit-*.tar.gz && export PATH="$PWD/blastit-x86_64-unknown-linux-gnu:$PATH"
$ blastit dev                         # PHP (host libphp) and static
```

```mermaid
flowchart LR
    client(["client"]) --> edge["cloud ingress · TLS, HTTP/2"]
    edge --> ingress["ingress · validates"] --> sched["scheduler · fair queue"]
    sched --> view["view · own uid"]
    view -- "socket" --> dbproxy["dbproxy · credentials"]
    dbproxy --> db[("PostgreSQL")]
```

> [!NOTE]
> `blastit dev` runs every process as you: the credential barrier and the grants
> are real, the uid split is not. Production `blastit` refuses to boot without
> root or the capabilities a preset names.

## Try it

```python title="hello.py"
def application(environ, start_response):
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"hello from blastit\n"]
```

```text title="Blastfile"
app "hello"

entrypoint python "hello:application"

view hello GET "/*" {
}
```

```console
$ uv run blastit dev
  blastit dev · single-uid simulation · http://127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
  view  hello  GET, HEAD  /.*
```

```console
$ curl localhost:8080/whatever
hello from blastit
```

The server keeps the terminal; `curl` goes in another one. One line swaps the
interface: `entrypoint php "public/index.php"` runs a PSR-7/PSR-15 front controller
on the host's embedded NTS `libphp`, and a file with no `entrypoint` plus a
`static` mount serves a built front end with no interpreter at all.

## What `blastit check` prints

`check` runs your suite through the real route proxies and names every gap in the
`Blastfile`'s own words: the `view`, the statement, and the line that fixes it.

```text title="a failing run — three of its six findings"
blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies
  view catalog: 1 statement not granted on proxy db
    SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?
      fix: add it to view catalog postgres
  view upload: takes a body and declares no ingress validation; observed application/x-www-form-urlencoded
      fix: write `body form`
  proxy analytics: declared, 0 statements observed
      fix: remove `proxy analytics` or add the call that uses it
blastit check: 6 problems · `--json` for the machine-readable list
```

A clean run is one line — `blastit check: 412 requests, 977 statements, 3/3
views, clean` — and that is the deploy gate. `--json` is the same document for an
agent.

## Where it sits

| Layer | What it does |
|---|---|
| your ingress | TLS, certificates, HTTP/2, edge cache, DDoS — Traefik, DigitalOcean, Cloudflare, nginx |
| blastit | isolates every route, holds the database credentials, enforces each `view`'s SQL |
| your app | its own framework routing, unchanged |

## Three places it has to work

| Where | Command | What it gives you |
|---|---|---|
| Your laptop | `blastit dev` | the real process topology, unprivileged |
| CI | `blastit check` | proof no request, grant or view was forgotten |
| Production | `blastit` / `--preset …` | real uids and capabilities, or it refuses to start |

This documentation site is served the same way: `make docs-serve` builds it with
Zensical and hands it to a freshly built `blastit dev` (`PORT=8080`).

**Read the documentation →**
[Local](https://offworldnexus.github.io/blastit/local/) ·
[CI/CD](https://offworldnexus.github.io/blastit/ci/) ·
[Deployments](https://offworldnexus.github.io/blastit/deploy/) ·
[Reference](https://offworldnexus.github.io/blastit/reference/cli/)

## License

Apache 2.0 — see [`LICENSE`](LICENSE).
