# Local development

`blastit dev` runs your app the way production does, without root. Everything
runs as you, so nothing needs `sudo`, and a misconfiguration fails on your machine
— for an honest program — instead of in production.

One uid means it is **not a sandbox**. The credential barrier still keeps the
database URL out of the app's `environ` and `argv`, but any process running as you
can read `.env` and your `/proc` entries. Use a throwaway local database; never
point `blastit dev` at production data.

## Start

```console
$ uv run blastit dev
```

It reads `./blastit.toml` from the current directory (no parent search), binds
`127.0.0.1:8080` only, and prints pretty logs. (The reference default is `json`;
`dev` and `check` choose `pretty`, and the variable still wins.) The banner stays
up for the whole run.

The manifest is read once. Restart after editing a route or a grant.

| Flag | What it does |
|---|---|
| `--port N` | listen on `N` instead of `8080` |
| `--host ADDR` | expose the server on another address for a VM or phone; also set `BLASTIT_DEV_EXPOSE=1`, and the boot line then names the address and your uid |
| `--learn` | record instead of denying; see [below](#fill-the-manifest-in-one-pass) |

`--host` on its own is refused; `BLASTIT_DEV_EXPOSE=1` is the second
acknowledgement. Exposed, there is no authentication — anyone who reaches it
drives every route as you — so use it only with the throwaway database above,
never with production data or an `unrestricted` route.

## Add PostgreSQL

Any PostgreSQL works. No server? One line starts one on loopback only, if you
have Docker (skip it if you already have PostgreSQL):

```console
$ docker run --rm -d -p 127.0.0.1:5432:5432 \
    -e POSTGRES_USER=shop -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=shop postgres:18
```

```toml title="blastit.toml"
version = 1
app = "app:app"

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "products"
path = ['/products/.*']

[route.db]
queries = []
```

```console
$ export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
$ uv run blastit dev
```

Blastit reads the process environment only. `uv run --env-file .env blastit dev`
loads a file; keep it to the throwaway database above.

Your app keeps reading `DATABASE_URL`. Blastit reads the real one, hands the app
a local URL, and connects upstream itself. The password never enters the app's
environment, and `blastit.toml` holds no credentials or inline literals, so it is
safe to commit.

A route with no `[route.db]` gets no listener at all. Here `queries = []` gives
it a listener that denies every statement (`42501`) — what the next step fixes.

## Fill the manifest in one pass

With a test suite, one command records the whole app:

```console
$ uv run blastit check --learn
learn: 2 routes, 38 queries added → /home/you/shop/blastit.toml
$ git diff blastit.toml
$ uv run blastit check
blastit check: 412 requests, 977 statements, 3/3 routes, clean
```

Without a suite, click through the app instead. Stop it with ++ctrl+c++ when you
are done:

```console
$ uv run blastit dev --learn
…
learn: 2 routes, 3 queries added → /home/you/shop/blastit.toml
```

Learn mode allows what the manifest does not cover and records it, so one pass
sees every route and statement instead of stopping at the first denial. On exit
it adds exact `queries` and the routes it saw to the manifest, and prints the
resolved path. It names a route from the first path segment and turns numeric and
UUID segments into `[0-9]+` and `[^/]+`; review the diff and tighten anything it
guessed too widely. A statement that inlines a value is refused, not written:
bind the value and learn again. Then run `blastit check` without `--learn`.

## A denial

Outside learn mode, a statement a route is not granted fails the request with
PostgreSQL's permission error (`42501`); uncaught, that is a `500`. One line says
why:

```console
15:41:02.118 WARN  sql.denied  products  db  parse  SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?
```

`sql` is the statement as normalised, with quoted literals replaced by `…`:
exactly what a grant has to contain, and safe to paste into a ticket. The JSON
form of the same event adds `reason`, which separates a missing table or column
from a statement nobody listed. Two answers:

* **It belongs there.** Replace the empty list in that route's `[route.db]`
  block with the normalised statement:

  ```toml
  [[route]]
  name = "products"
  path = ['/products/.*']

  [route.db]
  queries = ['SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?']
  ```

  An exact grant is trusted as written: paste only a statement you have read.
  `pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file or network
  capabilities, not ordinary queries.

* **It does not.** You have found a route that does more than you thought. Give
  the code path its own route, or narrow this route's `path` so it is
  unreachable from here.

Do not reach for `unrestricted = true` first: it gives away every statement on
that proxy, including the ones reading password hashes.

## Before you deploy

Production refuses to start on one uid. To exercise the real uid and capability
path locally, use the [rootless recipe](deploy/anything-else.md#root-or-rootless).
Otherwise, [CI](ci.md) is the next gate.
